// server-rs：Cloudflare Worker 服务端入口。
use anyhow::Result;
use axum::body::Body;
use std::sync::{Arc, OnceLock};
use tower_service::Service;
use worker::{Context, Env, HttpRequest};
use worker_macros::event;

use lib::tool::{derive_keys, DerivedKeys};

mod app;
mod proxy_http;
mod proxy_ws;
mod subscribe;

/// 密钥派生只需在冷启动时做一次，Isolate 存活期间复用。
static STATE: OnceLock<DerivedKeys> = OnceLock::new();

fn build_state(env: &Env) -> Result<DerivedKeys> {
    let key = env.secret("key").map(|s| s.to_string())?;
    let domain = env.secret("domain").map(|s| s.to_string())?;
    derive_keys(&key, &domain)
}

#[event(fetch)]
async fn fetch(req: HttpRequest, env: Env, ctx: Context) -> Result<axum::http::Response<Body>> {
    let keys = match STATE.get() {
        Some(s) => s.clone(),
        None => {
            let s = build_state(&env)?;
            let _ = STATE.set(s.clone());
            s
        }
    };

    Ok(app::router(app::AppState {
        keys,
        ctx: Arc::new(ctx),
    })
    .call(req)
    .await?)
}
