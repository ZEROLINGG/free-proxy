#![allow(unused)]
use axum::{routing::get, Router};
use anyhow::{bail, Result};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use axum::{
    extract::Request,
    middleware::{self, Next},
    response::Response,


};
use std::time::Instant;
async fn root() -> &'static str {
    "Hello, World!"
}
async fn print_request_middleware(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    println!("--> [请求到达] {} {}", method, uri);
    let start = Instant::now();
    let response = next.run(req).await;
    let latency = start.elapsed();
    println!("<-- [请求完成] {} {} - 状态码: {} (耗时: {:?})",
             method, uri, response.status(), latency
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

        if let Some(handle) = self.server_task.lock().expect("").take() {
            handle.await??;
        }

        Ok(())
    }
}

pub const fn baseurl() -> &'static str { "http://localhost:18082" }