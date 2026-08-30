// server-rs：HTTP/HTTPS 流式加密代理（/api/{version}/{target}）。
use axum::body::{Body, Bytes};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::extract::{Path, Request, State};
use futures_util::stream::{LocalBoxStream, Stream};
use futures_util::StreamExt;
use std::io::Write;
use worker::send::IntoSendFuture;
use worker::Fetch;

use js_sys::Uint8Array;

use lib::algo::{decode_chunk, encode_chunk, ProxyAead, ProxyCompressor};
use lib::frames::{make_frame, Frame, FrameCache};
use lib::http::{parse_head, UrlBuilder};

use crate::app::{status_text, AppState};
use crate::error;

/// wasm 单线程场景下安全：绕过 axum/tower 对 Future: Send 的编译期要求。
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
    lib::debug!("start");

    let experimental = state.env.var("experimental")
        .map(|v| v.to_string())
        .map_or(false, |v| {
            let s = v.trim();
            s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("1") ||
                s.eq_ignore_ascii_case("use") || s.eq_ignore_ascii_case("enable")
        });


    let key16 = state.keys.key16;
    let key32 = state.keys.key32;

    // 算法映射与客户端共享（lib::algo），URL 契约有往返单测兜底
    let compressor = ProxyCompressor::from_version(&version)
        .map_err(|_| error!(BAD_REQUEST, "unsupported version: {}", version))?;
    let aead = ProxyAead::from_target(&target)
        .map_err(|_| error!(BAD_REQUEST, "unsupported api: {}", target))?;

    // ---------- 阶段 1：读取头帧（head + https 标志位，末尾字节） ----------
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
            Some(Err(_)) => return Err(error!(BAD_REQUEST, "request body read error")),
            None => return Err(error!(BAD_REQUEST, "request body truncated (no head frame)")),
        }
    };

    let data = decode_chunk(&head_frame, compressor, aead, &key16, &key32)
        .map_err(|_| error!(BAD_REQUEST, "Unprocessable Request"))?;

    if data.is_empty() {
        return Err(error!(BAD_REQUEST, "Unprocessable Request"));
    }
    let protocol = if data[data.len() - 1] % 2 == 0 { "http" } else { "https" };

    let head = parse_head(&data[..data.len() - 1])
        .map_err(|_| error!(BAD_REQUEST, "Unprocessable Request"))?;

    // HTTP/1.1 请求必须携带 Host；absolute-form 的 request-target 自带权威信息
    let is_absolute_form = head.target.starts_with("http://") || head.target.starts_with("https://");
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

    // ---------- 定长缓冲判定 ----------
    let mut content_length: Option<u64> = None;
    let mut transfer_encoding_chunked = false;
    for (k, v) in head.headers.iter() {
        if k.eq_ignore_ascii_case("content-length") {
            content_length = v.trim().parse::<u64>().ok();
        } else if k.eq_ignore_ascii_case("transfer-encoding") {
            transfer_encoding_chunked |= v
                .split(',')
                .any(|t| t.trim().eq_ignore_ascii_case("chunked"));
        }
    }

    const BUFFER_THRESHOLD: u64 = 8 * 1024 * 1024;
    let buffer_mode = !transfer_encoding_chunked
        && content_length.is_some_and(|cl| cl < BUFFER_THRESHOLD)
        && experimental;

    // ---------- 构造并发出上游请求 ----------
    let mut init = worker::RequestInit::new();
    lib::debug!("{:<7} {:<15.30} {:<15.30} content_length:{:?} buffer_mode:{buffer_mode}", head.method, head.host.unwrap_or("unknown"), head.target, content_length);

    let method = match head.method {
        "GET" => worker::Method::Get,
        "POST" => worker::Method::Post,
        "PUT" => worker::Method::Put,
        "DELETE" => worker::Method::Delete,
        "PATCH" => worker::Method::Patch,
        "HEAD" => worker::Method::Head,
        "OPTIONS" => worker::Method::Options,
        other => return Err(error!(METHOD_NOT_ALLOWED, "unsupported method: {other}")),
    };
    init.with_method(method.clone());
    init.with_redirect(worker::RequestRedirect::Manual);

    let fetch_headers = worker::Headers::new();
    for (k, v) in head.headers.iter() {
        if [
            "host", "content-length", "transfer-encoding", "expect",
            "connection", "keep-alive", "proxy-connection", "proxy-authorization",
            "te", "trailer", "upgrade",
            "via", "forwarded", "x-forwarded-for", "x-forwarded-host", "x-forwarded-proto",
            "cf-worker"
        ]
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

    if method != worker::Method::Head && method != worker::Method::Get  {
        if buffer_mode {
            let expect = content_length.unwrap_or(0) as usize;
            let mut buf: Vec<u8> = Vec::with_capacity(expect);
            for f in initial_frames {
                let raw = decode_chunk(&f, compressor, aead, &key16, &key32)
                    .map_err(|e| error!(BAD_REQUEST, "decode body frame failed: {}", e))?;
                buf.extend_from_slice(&raw);
            }
            if !saw_eos {
                'drain: loop {
                    match incoming.next().await {
                        Some(Ok(b)) => {
                            parser.push(&b);
                            loop {
                                match parser.try_pop() {
                                    Ok(Frame::Frame(f)) => {
                                        let raw = decode_chunk(&f, compressor, aead, &key16, &key32)
                                            .map_err(|e| error!(BAD_REQUEST, "decode body frame failed: {}", e))?;
                                        buf.extend_from_slice(&raw);
                                    }
                                    Ok(Frame::Eos) => break 'drain,
                                    Ok(Frame::None) => break,
                                    Err(e) => return Err(error!(BAD_REQUEST, "frame error: {}", e)),
                                }
                            }
                        }
                        Some(Err(_)) => return Err(error!(BAD_REQUEST, "request body read error")),
                        None => return Err(error!(BAD_GATEWAY, "request body truncated (no EOS)")),
                    }
                }
            }
            if buf.len() != expect {
                return Err(error!(
                    BAD_GATEWAY,
                    "body length mismatch: got {}, expected {}",
                    buf.len(),
                    expect
                ));
            }
            let arr = Uint8Array::new_with_length(buf.len() as u32);
            arr.copy_from(&buf);
            init.with_body(Some(arr.into()));
        } else {
            let upstream_body = async_stream::stream! {
                for f in initial_frames {
                    match decode_chunk(&f, compressor, aead, &key16, &key32) {
                        Ok(raw) => yield Ok(raw),
                        Err(e) => { yield Err(format!("{e}")); return; }
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
                                                Err(e) => { yield Err(format!("{e}")); return; }
                                            }
                                        }
                                        Ok(Frame::Eos) => return,
                                        Ok(Frame::None) => break,
                                        Err(e) => { yield Err(format!("frame error: {e}")); return; }
                                    }
                                }
                            }
                            Some(Err(_)) => { yield Err("request body read error".to_string()); return; }
                            None => { yield Err("request body truncated (no EOS)".to_string()); return; }
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
    }

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

    let body_stream: LocalBoxStream<'static, Result<Vec<u8>, worker::Error>> =
        match upstream_resp.into_parts().1 {
            worker::ResponseBody::Stream(s) => Box::pin(worker::ByteStream::from(s)),
            worker::ResponseBody::Body(bytes) => Box::pin(futures_util::stream::once(async move { Ok(bytes) })),
            worker::ResponseBody::Empty => Box::pin(futures_util::stream::empty()),
        };

    let compressor = compressor;
    let aead = aead;

    let stream = async_stream::stream! {
    let mut body_stream = body_stream;

    let mut content_length_zero = false;
    let mut is_sse = false; // 用于判断是否为 Server-Sent Events

    let mut head_buf = Vec::with_capacity(512);
    let _ = write!(&mut head_buf, "HTTP/1.1 {} {}\r\n", status, status_text(status));

    for (k, v) in resp_headers.iter() {
        if ["content-length", "transfer-encoding", "content-encoding"]
            .iter()
            .any(|&h| k.eq_ignore_ascii_case(h))
        {
            if k.eq_ignore_ascii_case("content-length") && v.trim() == "0" {
                content_length_zero = true;
            }
            continue;
        }
        if k.eq_ignore_ascii_case("content-type") && v.contains("text/event-stream") {
            is_sse = true;
        }
        let _ = write!(&mut head_buf, "{}: {}\r\n", k, v);
    }

    let is_informational = status >= 100 && status < 200;
    let body_allowed = !is_head
        && status != 204
        && status != 304
        && !is_informational
        && !content_length_zero;

    let use_chunked = body_allowed;

    if use_chunked {
        head_buf.extend_from_slice(b"Transfer-Encoding: chunked\r\n");
    }
    head_buf.extend_from_slice(b"\r\n");

    // 发送 Header 帧
    match pack_frame(&head_buf, compressor, aead, &key16, &key32) {
        Ok(frame) => yield Ok(Bytes::from(frame)),
        Err(e) => { yield Err(std::io::Error::other(format!("pack frame failed: {e}"))); return; }
    }

    if body_allowed {
        const BUFFER_THRESHOLD: usize = 16 * 1024;
        let threshold = if is_sse { 0 } else { BUFFER_THRESHOLD };

        let mut buffer = Vec::with_capacity(BUFFER_THRESHOLD);


        let flush_buffer = |buf: &mut Vec<u8>| -> Result<Bytes, String> {
            if buf.is_empty() {
                return Ok(Bytes::new());
            }
            let framed = if use_chunked {
                let mut chunk_buf = Vec::with_capacity(buf.len() + 20);
                let _ = write!(&mut chunk_buf, "{:x}\r\n", buf.len());
                chunk_buf.extend_from_slice(buf);
                chunk_buf.extend_from_slice(b"\r\n");
                chunk_buf
            } else {
                buf.clone()
            };

            buf.clear();

            pack_frame(&framed, compressor, aead, &key16, &key32)
                .map(Bytes::from)
        };

        loop {
            let bytes = match body_stream.next().await {
                Some(Ok(b)) => b,
                Some(Err(e)) => { yield Err(std::io::Error::other(format!("{e:?}"))); return; }
                None => break, // EOF
            };

            if bytes.is_empty() {
                continue;
            }

            if buffer.is_empty() && bytes.len() >= threshold {
                let framed = if use_chunked {
                    let mut chunk_buf = Vec::with_capacity(bytes.len() + 20);
                    let _ = write!(&mut chunk_buf, "{:x}\r\n", bytes.len());
                    chunk_buf.extend_from_slice(&bytes);
                    chunk_buf.extend_from_slice(b"\r\n");
                    chunk_buf
                } else {
                    bytes.to_vec()
                };

                match pack_frame(&framed, compressor, aead, &key16, &key32) {
                    Ok(frame) => yield Ok(Bytes::from(frame)),
                    Err(e) => { yield Err(std::io::Error::other(format!("pack error: {e}"))); return; }
                }
                continue;
            }

            buffer.extend_from_slice(&bytes);

            if buffer.len() >= threshold {
                match flush_buffer(&mut buffer) {
                    Ok(bytes) if bytes.is_empty() => {},
                    Ok(bytes) => yield Ok(bytes),
                    Err(e) => { yield Err(std::io::Error::other(e)); return; }
                }
            }
        }

        if !buffer.is_empty() {
             match flush_buffer(&mut buffer) {
                Ok(bytes) if !bytes.is_empty() => yield Ok(bytes),
                Err(e) => { yield Err(std::io::Error::other(e)); return; }
                _ => {}
            }
        }

        if use_chunked {
            let tail = b"0\r\n\r\n";
            match pack_frame(tail, compressor, aead, &key16, &key32) {
                Ok(frame) => yield Ok(Bytes::from(frame)),
                Err(e) => { yield Err(std::io::Error::other(format!("pack frame failed: {e}"))); return; }
            }
        }
    }

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