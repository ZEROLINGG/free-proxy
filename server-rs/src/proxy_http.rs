// server-rs：HTTP/HTTPS 流式加密代理（/api/{version}/{target}）。
use axum::body::{Body, Bytes};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::extract::{Path, Request, State};
use futures_util::stream::{LocalBoxStream, Stream};
use futures_util::StreamExt;
use worker::send::IntoSendFuture;
use worker::Fetch;

use lib::algo::{decode_chunk, encode_chunk, ProxyAead, ProxyCompressor};
use lib::frames::{make_frame, Frame, FrameCache};
use lib::http::{parse_head, UrlBuilder};

use crate::app::{status_text, AppState};
use crate::error;

/// wasm 单线程场景下安全：绕过 axum/tower 对 Future: Send 的编译期要求。
/// SAFETY: 仅可用于 Cloudflare Workers 这种单线程 Wasm 运行时，
/// 若移植到多线程原生后端将引发数据竞争，禁止跨环境复用。
struct SendStream<T>(T);
unsafe impl<T> Send for SendStream<T> {}
unsafe impl<T> Sync for SendStream<T> {}
impl<T: Stream + Unpin> Stream for SendStream<T> {
    type Item = T::Item;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.0).poll_next(cx)
    }
}

fn pack_frame(
    raw: &[u8],
    compressor: ProxyCompressor,
    aead: ProxyAead,
    key16: &[u8],
    key32: &[u8],
) -> Result<Vec<u8>, String> {
    let enc = encode_chunk(raw, compressor, aead, key16, key32).map_err(|e| e.to_string())?;
    Ok(make_frame(&enc))
}

#[worker::send]
pub(crate) async fn proxy(
    State(state): State<AppState>,
    Path((version, target)): Path<(String, String)>,
    req: Request,
) -> Result<Response, (StatusCode, String)> {
    let key16 = state.keys.key16;
    let key32 = state.keys.key32;

    // 算法映射与客户端共享（lib::algo），URL 契约有往返单测兜底
    let compressor = ProxyCompressor::from_version(&version)
        .map_err(|_| error!(BAD_REQUEST, "unsupported version: {}", version))?;
    let aead = ProxyAead::from_target(&target)
        .map_err(|_| error!(BAD_REQUEST, "unsupported api: {}", target))?;

    // ---------- 阶段 1：读取头帧（head + https 标志位，末尾字节） ----------
    // 请求体以帧流形式到达：第一帧是头帧，之后是 body 帧，零长帧 = EOS。
    let mut incoming = req.into_body().into_data_stream();
    let mut parser = FrameCache::new();
    let head_frame = loop {
        match parser.try_pop() {
            Ok(Frame::Frame(f)) => break f.to_vec(),
            Ok(Frame::Eos) => {
                return Err(error!(BAD_REQUEST, "empty request (EOS before head frame)"))
            }
            Ok(Frame::None) => {}
            Err(e) => return Err(error!(BAD_REQUEST, "frame error: {}", e)),
        }
        match incoming.next().await {
            Some(Ok(b)) => parser.push(&b),
            Some(Err(_)) => {
                return Err(error!(BAD_REQUEST, "request body read error"))
            }
            None => {
                return Err(error!(BAD_REQUEST, "request body truncated (no head frame)"))
            }
        }
    };

    let data = decode_chunk(&head_frame, compressor, aead, &key16, &key32)
        .map_err(|_| error!(BAD_REQUEST, "Unprocessable Request"))?;

    if data.is_empty() {
        return Err(error!(BAD_REQUEST, "Unprocessable Request"));
    }
    let protocol = if data[data.len() - 1] % 2 == 0 {
        "http"
    } else {
        "https"
    };
    // 轻量借用解析：只提取 method/target/host/headers，其余语义一概不碰
    let head = parse_head(&data[..data.len() - 1])
        .map_err(|_| error!(BAD_REQUEST, "Unprocessable Request"))?;

    // HTTP/1.1 请求必须携带 Host；absolute-form 的 request-target 自带权威信息
    let is_absolute_form =
        head.target.starts_with("http://") || head.target.starts_with("https://");
    if head.host.is_none() && !is_absolute_form {
        return Err(error!(BAD_REQUEST, "Missing Host header"));
    }

    // ---------- 阶段 2：榨干已缓冲的 body 帧 ----------
    let mut initial_frames: Vec<Vec<u8>> = Vec::new();
    let mut saw_eos = false;
    loop {
        match parser.try_pop() {
            Ok(Frame::Frame(f)) => initial_frames.push(f.to_vec()),
            Ok(Frame::Eos) => {
                saw_eos = true;
                break;
            }
            Ok(Frame::None) => break,
            Err(e) => return Err(error!(BAD_REQUEST, "frame error: {}", e)),
        }
    }

    // ---------- 构造并发出上游请求 ----------
    let mut init = worker::RequestInit::new();

    let method = match head.method {
        "GET" => worker::Method::Get,
        "POST" => worker::Method::Post,
        "PUT" => worker::Method::Put,
        "DELETE" => worker::Method::Delete,
        "PATCH" => worker::Method::Patch,
        "HEAD" => worker::Method::Head,
        "OPTIONS" => worker::Method::Options,
        _ => worker::Method::Get,
    };
    init.with_method(method.clone());
    // 重定向改为 manual：不跟随（否则无 cookie jar 的 fetch 会把
    // 302→sorry→302 之类的 cookie 依赖型重定向循环跟满 20 次后报
    // "Too many redirects"），3xx 响应连同 Location/Set-Cookie 原样
    // 转发给浏览器，由浏览器按原生语义处理重定向与 cookie。
    init.with_redirect(worker::RequestRedirect::Manual);

    let fetch_headers = worker::Headers::new();
    for (k, v) in head.headers.iter() {
        // body 长度/编码由 fetch 接管（流式 body 未知长度），Expect 不再转发
        if ["host", "content-length", "transfer-encoding", "expect"]
            .iter()
            .any(|&h| k.eq_ignore_ascii_case(h))
        {
            continue;
        }
        fetch_headers
            .append(k, *v)
            .map_err(|_| error!(BAD_REQUEST, "Invalid Header"))?;
    }
    init.with_headers(fetch_headers);

    if method != worker::Method::Get && method != worker::Method::Head {
        // ---------- 流式 body：解密的帧字节直接喂给上游 fetch（pull 驱动） ----------
        // 客户端边传我们边解帧边转发，EOS 前上游请求体一直处于"未完成"状态。
        let upstream_body = async_stream::stream! {
            for f in initial_frames {
                match decode_chunk(&f, compressor, aead, &key16, &key32) {
                    Ok(raw) => yield Ok(raw),
                    Err(e) => {
                        yield Err(format!("{e}"));
                        return;
                    }
                }
            }
            if !saw_eos {
                loop {
                    match incoming.next().await {
                        Some(Ok(b)) => {
                            parser.push(&b);
                            loop {
                                match parser.try_pop() {
                                    Ok(Frame::Frame(f)) => {
                                        match decode_chunk(&f, compressor, aead, &key16, &key32) {
                                            Ok(raw) => yield Ok(raw),
                                            Err(e) => {
                                                yield Err(format!("{e}"));
                                                return;
                                            }
                                        }
                                    }
                                    Ok(Frame::Eos) => return, // 请求体正常结束
                                    Ok(Frame::None) => break,
                                    Err(e) => {
                                        yield Err(format!("frame error: {e}"));
                                        return;
                                    }
                                }
                            }
                        }
                        Some(Err(_)) => {
                            yield Err("request body read error".to_string());
                            return;
                        }
                        // EOF 而未收到 EOS = 截断：让上游 fetch 报错，客户端得到 502
                        None => {
                            yield Err("request body truncated (no EOS)".to_string());
                            return;
                        }
                    }
                }
            }
        };
        let body = worker::Body::from_stream(upstream_body)
            .map_err(|e| error!(INTERNAL_SERVER_ERROR, "build upstream body stream failed: {:?}", e))?;
        if let Some(stream) = body.into_inner() {
            init.with_body(Some(stream.into()));
        }
    }

    // 直构上游 URL：absolute-form 直接用 request-target；否则 Host 头 + origin-form
    let full_url = if is_absolute_form {
        head.target.to_string()
    } else {
        UrlBuilder::new()
            .scheme(protocol)
            .host(head.host)
            .path(head.target)
            .build()
            .map_err(|_| error!(BAD_REQUEST, "Unprocessable Request"))?
    };

    let outbound = worker::Request::new_with_init(&full_url, &init)
        .map_err(|e| error!(INTERNAL_SERVER_ERROR, "build upstream request failed: {:?}", e))?;

    // Fetch不像cloudflare:sockets 的 connect受到限制无法连接使用了cf服务的站点，能够解锁更多内容
    let upstream_resp = Fetch::Request(outbound)
        .send()
        .into_send()
        .await
        .map_err(|e| error!(BAD_GATEWAY, "fetch upstream failed: {:?}", e))?;

    let status: u16 = upstream_resp.status_code();
    let resp_headers: Vec<(String, String)> = upstream_resp.headers().entries().collect();
    let is_head = method == worker::Method::Head;

    // 上游响应体可能为空（HEAD / 204 / 304 / 1xx 等）：此时 `web_sys::Response.body()`
    // 为 null，worker::Response::stream() 会报 "body is not streamable" 并让整个请求 500。
    // 按 ResponseBody 分派：Stream → 原样转发；Body(vec) → 单块流；Empty → 空流。
    let body_stream: LocalBoxStream<'static, Result<Vec<u8>, worker::Error>> =
        match upstream_resp.into_parts().1 {
            worker::ResponseBody::Stream(s) => Box::pin(worker::ByteStream::from(s)),
            worker::ResponseBody::Body(bytes) => Box::pin(futures_util::stream::once(async move {
                Ok(bytes)
            })),
            worker::ResponseBody::Empty => Box::pin(futures_util::stream::empty()),
        };

    // 枚举为 Copy，可直接移入流闭包
    let compressor = compressor;
    let aead = aead;

    // ============================================================
    // 核心：流式把 "status line + headers + (chunked) body" 逐段打包
    // 每个帧都是独立可解密/解压的最小单元，帧间无分隔符、EOF 即完成。
    // 客户端解包后直接把明文字节按顺序写入 TCP 即可，无需理解 HTTP 语义。
    // ============================================================
    let stream = async_stream::stream! {
        let mut body_stream = body_stream;

        // ---------- 1. status line + headers ----------
        // 上游的 framing/编码信息经 Workers Fetch 后已不再成立：
        // 因此统一丢弃，body 一律用 chunked 重新分帧。
        let body_allowed = !is_head && status != 204 && status != 304;
        let use_chunked = body_allowed;

        let mut head = Vec::new();
        head.extend_from_slice(
            format!("HTTP/1.1 {} {}\r\n", status, status_text(status)).as_bytes(),
        );
        for (k, v) in resp_headers.iter() {
            if ["proxy-authenticate","proxy-authorization", "te", "trailer",
                "transfer-encoding", "upgrade", "content-length", "content-encoding"]
            .iter().any(|&h| k.eq_ignore_ascii_case(h))
            {
                continue;
            }
            head.extend_from_slice(format!("{}: {}\r\n", k, v).as_bytes());
        }
        if use_chunked {
            head.extend_from_slice(b"Transfer-Encoding: chunked\r\n");
        }
        head.extend_from_slice(b"\r\n");

        match pack_frame(&head, compressor, aead, &key16, &key32) {
            Ok(frame) => yield Ok(Bytes::from(frame)),
            Err(e) => {
                yield Err(std::io::Error::other(format!("pack frame failed: {e}")));
                return;
            }
        }

        // ---------- 2. body ----------
        if !is_head {
            loop {
                let next = body_stream.next().await;
                let bytes = match next {
                    Some(Ok(b)) => b,
                    Some(Err(e)) => {
                        yield Err(std::io::Error::other(format!("{e:?}")));
                        return;
                    }
                    None => break,
                };

                if bytes.is_empty() {
                    continue;
                }

                let framed: Vec<u8> = if use_chunked {
                    let mut v = format!("{:x}\r\n", bytes.len()).into_bytes();
                    v.extend_from_slice(&bytes);
                    v.extend_from_slice(b"\r\n");
                    v
                } else {
                    bytes
                };

                match pack_frame(&framed, compressor, aead, &key16, &key32) {
                    Ok(frame) => yield Ok(Bytes::from(frame)),
                    Err(e) => {
                        yield Err(std::io::Error::other(format!("pack frame failed: {e}")));
                        return;
                    }
                }
            }

            if use_chunked {
                let tail = b"0\r\n\r\n".to_vec();
                match pack_frame(&tail, compressor, aead, &key16, &key32) {
                    Ok(frame) => yield Ok(Bytes::from(frame)),
                    Err(e) => {
                        yield Err(std::io::Error::other(format!("pack frame failed: {e}")));
                        return;
                    }
                }
            }
        }

        // 零长帧 = EOS 结束标记（客户端以此区分正常结束与截断）
        yield Ok(Bytes::from(make_frame(b"")));
    };

    let stream = SendStream(Box::pin(stream));
    let resp = Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(stream))
        .map_err(|e| error!(INTERNAL_SERVER_ERROR, "build response failed: {}", e))?;
    Ok(resp)
}