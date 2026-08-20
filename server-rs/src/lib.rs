// server-rs：Cloudflare Worker 服务端。
// 请求方向：头帧（raw head + https 标志）→ parse_head 轻量借用解析（method/target/host/headers）
// → 直构上游请求 → body 帧解密后原样透传（无 body 语义解析）。
use anyhow::Result;
use axum::body::{Body, Bytes};
use axum::http::{header, HeaderMap, HeaderName, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{extract::Path, extract::Query, extract::Request, extract::State, middleware::Next};
use axum::{routing::get, routing::post, Router};
use futures_util::stream::Stream;
use futures_util::StreamExt;
use serde::Deserialize;
use std::sync::{Arc, OnceLock};
use tower_service::Service;
use worker::send::IntoSendFuture;
use worker::{Context, Date, Env, Fetch, HttpRequest};
use worker_macros::event;

use lib::algo::{decode_chunk, encode_chunk, ProxyAead, ProxyCompressor};
use lib::base::{Base64, Encoder};
use lib::frames::{make_frame, Frame, FrameCache};
use lib::http::{parse_head, UrlBuilder};
use lib::tool::token_anth;
use lib::tool::{derive_keys, DerivedKeys};

static STATE: OnceLock<DerivedKeys> = OnceLock::new();

async fn middleware(
    State(state): State<DerivedKeys>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let method = req.method();
    if method.eq(&Method::GET) || method.eq(&Method::POST) {
        if let Some(var) = req.headers().get("Authorization") {
            if let Ok(var) = var.to_str() {
                if let Some(token) = var.strip_prefix("Bearer ") {
                    let now = Date::now().as_millis();
                    if token_anth(token, state.token_base, now) {
                        return Ok(next.run(req).await);
                    }
                }
            }
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

#[derive(Clone)]
struct AppState {
    keys: DerivedKeys,
    ctx: Arc<Context>,
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route(
            "/health",
            get(|| async { Date::now().as_millis().to_string() }),
        )
        .route("/api/{version}/{target}", post(proxy))
        .route("/ws/{version}/{target}", get(proxy_ws))
        .layer(axum::middleware::from_fn_with_state(state.keys.clone(), middleware))
        .with_state(state)
        .route("/subscribe/{port}", get(subscribe))
        .fallback(|| async { "not found" })
}

// wasm场景下安全
pub struct SendStream<T>(T);
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

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

fn pack_frame(
    raw: &[u8],
    compressor: ProxyCompressor,
    aead: ProxyAead,
    key16: &[u8],
    key32: &[u8],
) -> std::result::Result<Vec<u8>, String> {
    let enc = encode_chunk(raw, compressor, aead, key16, key32).map_err(|e| e.to_string())?;
    Ok(make_frame(&enc))
}

#[derive(Deserialize, Debug)]
pub struct SubscribeQuery {
    pub target: Option<String>,
    pub flag: Option<String>,
}

enum SubType {
    Clash,
    Base64, // v2rayN, Shadowrocket, Quantumult X, PassWall 等
    SingBox,
}

pub async fn subscribe(
    Path(port): Path<String>,
    Query(query): Query<SubscribeQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let node_name = "[Cloudflare Worker]free-proxy";
    let host = "127.0.0.1";

    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let target_param = query
        .target
        .as_deref()
        .or(query.flag.as_deref())
        .unwrap_or("")
        .to_lowercase();

    let sub_type = if target_param.contains("clash")
        || ua.contains("clash")
        || ua.contains("mihomo")
        || ua.contains("verge")
        || ua.contains("stash")
    {
        SubType::Clash
    } else if target_param.contains("singbox")
        || target_param.contains("sing-box")
        || ua.contains("sing-box")
    {
        SubType::SingBox
    } else {
        SubType::Base64
    };

    let extra_headers = [
        (
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"subscription\"",
        ),
        (
            HeaderName::from_static("profile-update-interval"),
            "2400000",
        ),
        (
            HeaderName::from_static("subscription-userinfo"),
            "upload=0; download=0; total=10737418240000000000; expire=0",
        ),
    ];

    match sub_type {
        SubType::Clash => {
            let yaml_content = format!(
                r#"proxies:
  - name: "{node_name}"
    type: http
    server: {host}
    port: {port}

proxy-groups:
  - name: "Proxy"
    type: select
    proxies:
      - "{node_name}"
      - DIRECT

rules:
  - MATCH,Proxy
"#
            );
            (
                [(header::CONTENT_TYPE, "text/yaml; charset=utf-8")],
                extra_headers,
                yaml_content,
            )
                .into_response()
        }

        SubType::SingBox => {
            let json_content = format!(
                r#"{{
  "outbounds": [
    {{
      "type": "selector",
      "tag": "Proxy",
      "outbounds": ["{node_name}", "direct"]
    }},
    {{
      "type": "http",
      "tag": "{node_name}",
      "server": "{host}",
      "server_port": {port}
    }},
    {{
      "type": "direct",
      "tag": "direct"
    }}
  ]
}}"#,
                node_name = node_name,
                host = host,
                port = port
            );
            (
                [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
                extra_headers,
                json_content,
            )
                .into_response()
        }

        SubType::Base64 => {
            let raw_links = format!("http://{}:{}#{}\n", host, port, node_name);
            let encoded_content = Base64::encode(raw_links.as_bytes()).unwrap();
            (
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                extra_headers,
                encoded_content,
            )
                .into_response()
        }
    }
}


#[worker::send]
pub async fn proxy_ws(
    State(state): State<AppState>,
    Path((version, target)): Path<(String, String)>,
    req: Request,
) -> std::result::Result<Response, (StatusCode, String)> {
    //  需要使用state.ctx.wait_until()处理ws
    todo!()
}

#[worker::send]
pub async fn proxy(
    State(state): State<AppState>,
    Path((version, target)): Path<(String, String)>,
    req: Request,
) -> std::result::Result<Response, (StatusCode, String)> {
    let key16 = state.keys.key16;
    let key32 = state.keys.key32;

    // 算法映射与客户端共享（lib::algo），URL 契约有往返单测兜底
    let compressor = ProxyCompressor::from_version(&version).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            format!("unsupported version: {}", version),
        )
    })?;
    let aead = ProxyAead::from_target(&target).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            format!("unsupported api: {}", target),
        )
    })?;

    // ---------- 阶段 1：读取头帧（head + https 标志位，末尾字节） ----------
    // 请求体以帧流形式到达：第一帧是头帧，之后是 body 帧，零长帧 = EOS。
    let mut incoming = req.into_body().into_data_stream();
    let mut parser = FrameCache::new();
    let head_frame = loop {
        match parser.try_pop() {
            Ok(Frame::Frame(f)) => break f.to_vec(),
            Ok(Frame::Eos) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "empty request (EOS before head frame)".into(),
                ))
            }
            Ok(Frame::None) => {}
            Err(e) => return Err((StatusCode::BAD_REQUEST, format!("frame error: {e}"))),
        }
        match incoming.next().await {
            Some(Ok(b)) => parser.push(&b),
            Some(Err(_)) => {
                return Err((StatusCode::BAD_REQUEST, "request body read error".into()))
            }
            None => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "request body truncated (no head frame)".into(),
                ))
            }
        }
    };

    let data = decode_chunk(&head_frame, compressor, aead, &key16, &key32)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Unprocessable Request".to_string()))?;

    if data.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Unprocessable Request".to_string()));
    }
    let protocol = if data[data.len() - 1] % 2 == 0 {
        "http"
    } else {
        "https"
    };
    // 轻量借用解析：只提取 method/target/host/headers，其余语义一概不碰
    let head = parse_head(&data[..data.len() - 1])
        .map_err(|_| (StatusCode::BAD_REQUEST, "Unprocessable Request".to_string()))?;

    // HTTP/1.1 请求必须携带 Host；absolute-form 的 request-target 自带权威信息
    let is_absolute_form =
        head.target.starts_with("http://") || head.target.starts_with("https://");
    if head.host.is_none() && !is_absolute_form {
        return Err((StatusCode::BAD_REQUEST, "Missing Host header".to_string()));
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
            Err(e) => return Err((StatusCode::BAD_REQUEST, format!("frame error: {e}"))),
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
            .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid Header".to_string()))?;
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
        let body = worker::Body::from_stream(upstream_body).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("build upstream body stream failed: {e:?}"),
            )
        })?;
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
            .map_err(|_| (StatusCode::BAD_REQUEST, "Unprocessable Request".to_string()))?
    };

    let outbound = worker::Request::new_with_init(&full_url, &init).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("build upstream request failed: {e:?}"),
        )
    })?;

    // Fetch不像cloudflare:sockets 的 connect受到限制无法连接使用了cf服务的站点，能够解锁更多内容
    let mut upstream_resp = Fetch::Request(outbound)
        .send()
        .into_send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("fetch upstream failed: {e:?}"),
            )
        })?;

    let status: u16 = upstream_resp.status_code();
    let resp_headers: Vec<(String, String)> = upstream_resp.headers().entries().collect();
    let is_head = method == worker::Method::Head;

    let body_stream = upstream_resp.stream().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("get upstream stream failed: {e:?}"),
        )
    })?;

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
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("build response failed: {e}"),
            )
        })?;
    Ok(resp)
}

fn build_state(env: &Env) -> Result<DerivedKeys> {
    let key = env.secret("key").map(|s| s.to_string())?;
    let domain = env.secret("domain").map(|s| s.to_string())?;
    derive_keys(&key, &domain)
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    ctx: Context,
) -> Result<axum::http::Response<Body>> {
    let keys = match STATE.get() {
        Some(s) => s.clone(),
        None => {
            let s = build_state(&env)?;
            let _ = STATE.set(s.clone());
            s
        }
    };

    Ok(router(AppState { keys, ctx: Arc::new(ctx) }).call(req).await?)
}
