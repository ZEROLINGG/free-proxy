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
mod test;

/// 密钥派生只需在冷启动时做一次，Isolate 存活期间复用。
static STATE: OnceLock<DerivedKeys> = OnceLock::new();

fn build_state(env: &Env) -> Result<DerivedKeys> {
    let key = env.secret("key").map(|s| s.to_string())?;
    let domain = env.secret("domain").map(|s| s.to_string())?;
    derive_keys(&key, &domain)
}

#[allow(dead_code)]
static LOG_INIT: OnceLock<()> = OnceLock::new();
#[event(fetch)]
async fn fetch(req: HttpRequest, env: Env, ctx: Context) -> Result<axum::http::Response<Body>> {
    #[cfg(target_arch = "wasm32")]
    LOG_INIT.get_or_init(|| {
        lib::log::init_wasm(
            lib::log::WasmLogConfig {
                tag: "[worker]".into(),
                default_level: env.var("log").map_or("info".into(), |v| v.to_string()),
                with_ansi: env.var("ansi_log").map(|v|v.to_string())
                    .map_or(false, |v| {
                    let s = v.trim();
                    s.eq_ignore_ascii_case("true")
                        || s.eq_ignore_ascii_case("1")
                        || s.eq_ignore_ascii_case("use")
                        || s.eq_ignore_ascii_case("enable")
                }),
            }
        )
    });

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
        env: Arc::new(env),
    })
    .call(req)
    .await?)
}
