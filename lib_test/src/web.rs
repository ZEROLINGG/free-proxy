#![allow(unused)]
// lib_test/src/web.rs
//
// 本地目标站（127.0.0.1:18082）：为 HTTP 代理 E2E 提供可控的"上游"。
// 所有数据生成逻辑与校验端共用 src/util.rs，保证两端零漂移。

use anyhow::Result;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Request};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use lib::compress::{Compressor, Zstd};
use lib::hash::{Hasher, Sha256};
use crate::util;

/// 流式下载的单块大小（决定 worker 响应侧的帧数量）
const DOWNLOAD_CHUNK: usize = 16 * 1024;
/// 慢速下载单块更小、块间带延时，验证流式中继
const SLOW_CHUNK: usize = 2 * 1024;
const SLOW_DELAY: Duration = Duration::from_millis(250);

async fn root() -> &'static str {
    "Hello, World!"
}

/// 回显 method / uri / 全部请求头：方法透传与请求头剥离行为的观测点
async fn echo(req: Request) -> String {
    let (parts, _) = req.into_parts();
    let mut s = format!("method={}\nuri={}\n", parts.method, parts.uri);
    for (k, v) in parts.headers.iter() {
        s.push_str(&format!("{}: {}\n", k, String::from_utf8_lossy(v.as_bytes())));
    }
    s
}

/// 接收整个 body，返回 `len=<n>;blake3=<hex>`（校验端比对用）
async fn upload(body: Bytes) -> String {
    format!("len={};blake3={}", body.len(), Sha256::digest_hex(&body))
}

/// 确定性模式数据的流式下载：多块传输以覆盖响应侧重分帧路径
async fn download(Path(size): Path<u64>) -> Response {
    stream_pattern(size, DOWNLOAD_CHUNK, false)
}

/// 慢速流式下载：块间 sleep，覆盖长时流式回传与 idle 超时余量
async fn download_slow(Path(size): Path<u64>) -> Response {
    stream_pattern(size, SLOW_CHUNK, true)
}

fn stream_pattern(total: u64, chunk_max: usize, slow: bool) -> Response {
    let chunk_max = chunk_max as u64;
    let stream = futures_util::stream::unfold(0u64, move |sent| async move {
        if sent >= total {
            return None;
        }
        let n = (total - sent).min(chunk_max) as usize;
        let mut buf = vec![0u8; n];
        util::fill_pattern(&mut buf, sent);
        if slow {
            tokio::time::sleep(SLOW_DELAY).await;
        }
        Some((Ok::<_, std::convert::Infallible>(buf), sent + n as u64))
    });
    Body::from_stream(stream).into_response()
}

/// 返回任意状态码；204/304/1xx 按语义不允许携带 body，hyper 会拒绝有 body 的响应
async fn status(Path(code): Path<u16>) -> Result<Response, StatusCode> {
    let sc = StatusCode::from_u16(code).map_err(|_| StatusCode::BAD_REQUEST)?;
    let bodyless = code < 200 || code == 204 || code == 304;
    let mut resp = if bodyless {
        StatusCode::default().into_response()
    } else {
        (sc, format!("status-body-{code}")).into_response()
    };
    *resp.status_mut() = sc;
    Ok(resp)
}

/// 重定向链：/redirect/{n} → … → /redirect/0 → "redirect-done"
/// 验证 worker manual-redirect 下由客户端自行跟随的多跳转发
async fn redirect(Path(n): Path<usize>) -> Redirect {
    if n == 0 {
        Redirect::to("/redirect-done")
    } else {
        Redirect::to(&format!("/redirect/{}", n - 1))
    }
}

async fn redirect_done() -> &'static str {
    "redirect-done"
}

/// 多个 Set-Cookie 头：验证 worker 响应头透传保留重复头
async fn cookies() -> Response {
    let mut resp = "cookies-set".into_response();
    let h = resp.headers_mut();
    h.append(header::SET_COOKIE, HeaderValue::from_static("a=1; Path=/"));
    h.append(header::SET_COOKIE, HeaderValue::from_static("b=2; Path=/"));
    h.append(header::SET_COOKIE, HeaderValue::from_static("c=3; Path=/"));
    resp
}


async fn zstd() -> Response {
    let mut data = *b"w7y37y7d37dguwnjicjoe0iw9uj8hg";
    let mut resp = Body::from(Zstd::compress(data).unwrap()).into_response();
    resp.headers_mut()
        .insert(header::CONTENT_ENCODING, HeaderValue::from_static("zstd"));
    resp
}

async fn print_request_middleware(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();
    println!("--> [localhost web] {} {} headers: {:?}", method, uri, headers);
    let start = Instant::now();
    let response = next.run(req).await;
    let latency = start.elapsed();
    println!(
        "<-- [localhost web] {} {} - 状态码: {} (耗时: {:?})",
        method,
        uri,
        response.status(),
        latency
    );
    response
}

pub struct WebServer {
    shutdown_tx: std::sync::Mutex<Option<oneshot::Sender<()>>>,
    server_task: std::sync::Mutex<Option<JoinHandle<Result<()>>>>,
}

impl WebServer {
    pub fn new() -> Self {
        Self {
            shutdown_tx: std::sync::Mutex::new(None),
            server_task: std::sync::Mutex::new(None),
        }
    }

    pub async fn start(&self) -> Result<()> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:18082").await?;

        let app = Router::new()
            .route("/", get(root))
            .route("/echo", get(echo).post(echo).put(echo).delete(echo).patch(echo).options(echo))
            .route("/upload", post(upload).put(upload))
            .route("/download/{size}", get(download))
            .route("/download-slow/{size}", get(download_slow))
            .route("/status/{code}", get(status))
            .route("/redirect/{n}", get(redirect))
            .route("/redirect-done", get(redirect_done))
            .route("/cookies", get(cookies))
            .route("/zstd", get(zstd))
            .layer(axum::extract::DefaultBodyLimit::disable())
            .layer(middleware::from_fn(print_request_middleware));

        let (tx, rx) = oneshot::channel::<()>();

        *self.shutdown_tx.lock().expect("") = Some(tx);

        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await?;
            Ok(())
        });

        *self.server_task.lock().expect("") = Some(handle);
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.lock().expect("").take() {
            let _ = tx.send(());
        }

        // 先取出句柄再 await，避免 MutexGuard 跨 await 持有
        let handle = self.server_task.lock().expect("").take();
        if let Some(handle) = handle {
            handle.await??;
        }

        Ok(())
    }
}

pub const fn baseurl() -> &'static str {
    "http://localhost:18082"
}
