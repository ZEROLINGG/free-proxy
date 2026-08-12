use anyhow::Result;
use axum::body::Bytes;
use axum::http::{Method, StatusCode};
use axum::response::{
    sse::{Event, Sse},
    Response,
};
use axum::{extract::Path, extract::Request, extract::State, middleware::Next};
use axum::{routing::get, routing::post, Router};
use futures_util::stream::Stream;
use futures_util::StreamExt;
use std::sync::OnceLock;
use tower_service::Service;
use worker::send::IntoSendFuture;
use worker::{js_sys, Context, Date, Env, Fetch, HttpRequest};
use worker_macros::event;

use lib::algo::{decode_chunk, encode_chunk, ProxyAead, ProxyCompressor};
use lib::base::{Base64, Encoder};
use lib::http::http_parse_req;
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

fn router(state: DerivedKeys) -> Router {
    Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route(
            "/health",
            get(|| async { Date::now().as_millis().to_string() }),
        )
        .route("/api/{version}/{target}", post(proxy))
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(state, middleware))
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

fn pack_event(
    raw: &[u8],
    compressor: ProxyCompressor,
    aead: ProxyAead,
    key16: &[u8],
    key32: &[u8],
) -> std::result::Result<String, String> {
    let enc = encode_chunk(raw, compressor, aead, key16, key32).map_err(|e| e.to_string())?;
    Base64::encode(enc).map_err(|e| e.to_string())
}

pub async fn proxy(
    State(state): State<DerivedKeys>,
    Path((version, target)): Path<(String, String)>,
    body: Bytes,
) -> std::result::Result<Sse<impl Stream<Item = Result<Event>>>, String> {
    let key16 = state.key16;
    let key32 = state.key32;

    // 算法映射与客户端共享（lib::algo），URL 契约有往返单测兜底
    let compressor = ProxyCompressor::from_version(&version)
        .map_err(|_| format!("unsupported version: {}", version))?;
    let aead = ProxyAead::from_target(&target)
        .map_err(|_| format!("unsupported api: {}", target))?;

    let data: Vec<u8> = body.to_vec();
    let data = decode_chunk(&data, compressor, aead, &key16, &key32)
        .map_err(|_| "Unprocessable Request".to_string())?;

    if data.is_empty() {
        return Err("Unprocessable Request".to_string());
    }
    let protocol = if data[data.len() - 1] % 2 == 0 {
        "http"
    } else {
        "https"
    };
    let req = http_parse_req(&data[..data.len() - 1])
        .map_err(|_| "Unprocessable Request".to_string())?;

    // ---------- 构造并发出上游请求 ----------
    let mut init = worker::RequestInit::new();

    let method = match req.method {
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
    for (k, v) in req.headers.iter() {
        if !k.eq_ignore_ascii_case("host") {
            fetch_headers
                .append(k, v.as_ref())
                .map_err(|_| "Invalid Header".to_string())?;
        }
    }
    init.with_headers(fetch_headers);

    if method != worker::Method::Get && method != worker::Method::Head && !req.body.is_empty() {
        let uint8_arr = js_sys::Uint8Array::from(req.body);
        init.with_body(Some(uint8_arr.into()));
    }

    let full_url = req
        .full_url(protocol)
        .map_err(|_| "Unprocessable Request".to_string())?;

    let outbound = worker::Request::new_with_init(&full_url, &init)
        .map_err(|e| format!("build upstream request failed: {e:?}"))?;

    // Fetch不像cloudflare:sockets 的 connect受到限制无法连接使用了cf服务的站点，能够解锁更多内容
    let mut upstream_resp = Fetch::Request(outbound)
        .send()
        .into_send()
        .await
        .map_err(|e| format!("fetch upstream failed: {e:?}"))?;

    let status: u16 = upstream_resp.status_code();
    let resp_headers: Vec<(String, String)> = upstream_resp.headers().entries().collect();
    let is_head = method == worker::Method::Head;

    let body_stream = upstream_resp
        .stream()
        .map_err(|e| format!("get upstream stream failed: {e:?}"))?;

    // 枚举为 Copy，可直接移入流闭包
    let compressor = compressor;
    let aead = aead;

    // ============================================================
    // 核心：流式把 "status line + headers + (chunked) body" 逐段打包
    // 每个 SSE data 都是独立可解密/解压的最小单元，
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

        match pack_event(&head, compressor, aead, &key16, &key32) {
            Ok(b64) => yield Ok(Event::default().data(b64)),
            Err(e) => {
                yield Ok(Event::default().event("error").data(e));
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
                        yield Ok(Event::default().event("error").data(format!("{e:?}")));
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

                match pack_event(&framed, compressor, aead, &key16, &key32) {
                    Ok(b64) => yield Ok(Event::default().data(b64)),
                    Err(e) => {
                        yield Ok(Event::default().event("error").data(e));
                        return;
                    }
                }
            }

            if use_chunked {
                let tail = b"0\r\n\r\n".to_vec();
                match pack_event(&tail, compressor, aead, &key16, &key32) {
                    Ok(b64) => yield Ok(Event::default().data(b64)),
                    Err(e) => {
                        yield Ok(Event::default().event("error").data(e));
                        return;
                    }
                }
            }
        }

        yield Ok(Event::default().event("done"));
    };

    Ok(Sse::new(SendStream(Box::pin(stream))))
}

fn build_state(env: &Env) -> Result<DerivedKeys> {
    let key = env
        .secret("key")
        .map(|s| s.to_string())?;
    let domain = env
        .secret("domain")
        .map(|s| s.to_string())?;
    derive_keys(&key, &domain)
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: Context,
) -> Result<axum::http::Response<axum::body::Body>> {
    let state = match STATE.get() {
        Some(s) => s.clone(),
        None => {
            let s = build_state(&env)?;
            let _ = STATE.set(s.clone());
            s
        }
    };

    Ok(router(state).call(req).await?)
}

