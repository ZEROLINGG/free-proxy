// server-rs：Axum 应用装配（状态、鉴权、路由）。
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use axum::{routing::get, routing::post, Router};
use std::sync::Arc;
use worker::{Context, Date, Env};

use lib::tool::{token_auth, DerivedKeys};

use crate::proxy_http::proxy;
use crate::proxy_ws::proxy_ws;
use crate::subscribe::subscribe;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) keys: DerivedKeys,
    pub(crate) ctx: Arc<Context>,
    pub(crate) env: Arc<Env>,
}

#[macro_export]
macro_rules! error {
    ($code:ident, $($arg:tt)*) => {{
        let err_msg = format!($($arg)*);
        lib::error!("{err_msg}");
        (axum::http::StatusCode::$code, err_msg)
    }};
}

/// 基于时间戳的 Bearer Token 鉴权中间件，防重放。
pub(crate) async fn auth_middleware(
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
                    if token_auth(token, &state.token_base, now) {
                        return Ok(next.run(req).await);
                    }
                }
            }
        }
    }
    Err(StatusCode::UNAUTHORIZED)
}


pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async { Date::now().as_millis().to_string() }),
        )
        .route("/api/{version}/{target}", post(proxy))
        .route("/ws/{version}/{target}", get(proxy_ws))
        .layer(axum::middleware::from_fn_with_state(
            state.keys.clone(),
            auth_middleware,
        ))
        .with_state(state)
        // 刻意注册在鉴权 layer 之后：订阅客户端（Clash/sing-box）无法携带 Bearer token
        .route("/subscribe/{port}", get(subscribe))
        .fallback(|| async { (StatusCode::NOT_FOUND, "not found") })
}

/// HTTP 状态码 -> Reason Phrase，proxy_http / proxy_ws 共用。
pub(crate) fn status_text(code: u16) -> &'static str {
    StatusCode::from_u16(code)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or("Unknown")
}

