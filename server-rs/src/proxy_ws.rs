// server-rs：WebSocket 隧道加密代理（/ws/{version}/{target}）。
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use futures_util::future::{select, Either};
use futures_util::StreamExt;
use std::pin::pin;
use worker::{console_error, send::IntoSendFuture, Fetch};

use lib::algo::{decode_chunk, encode_chunk, ProxyAead, ProxyCompressor};
use lib::http::{parse_head, UrlBuilder};
use lib::ws::{calc_sec_ws_accept, WsFrame, WsTunnelMsg};

use crate::app::{status_text, AppState};
use crate::error;

/// 将任意 `WsTunnelMsg` 序列化 + 压缩加密后发给客户端（错误静默丢弃，由调用方兜底）。
fn ws_send_msg(
    server: &worker::WebSocket,
    msg: &WsTunnelMsg,
    compressor: ProxyCompressor,
    aead: ProxyAead,
    key16: &[u8; 16],
    key32: &[u8; 32],
) {
    if let Ok(serialized) = msg.serialize() {
        if let Ok(encrypted) = encode_chunk(&serialized, compressor, aead, key16, key32) {
            let _ = server.send_with_bytes(encrypted);
        }
    }
}

/// 将可直接写入本地 TCP 的原始字节（RFC 6455 帧 / HTTP 响应头）封装为
/// `WsTunnelMsg::Return`，加密后发给客户端；客户端解密后不解析直接写 socket。
fn ws_send_return(
    server: &worker::WebSocket,
    payload: &[u8],
    compressor: ProxyCompressor,
    aead: ProxyAead,
    key16: &[u8; 16],
    key32: &[u8; 32],
) {
    ws_send_msg(
        server,
        &WsTunnelMsg::Return(payload.to_vec()),
        compressor,
        aead,
        key16,
        key32,
    );
}

/// 将错误信息封装为 `WsTunnelMsg::Error`，加密后发给客户端。
fn ws_send_error(
    server: &worker::WebSocket,
    err_msg: &str,
    compressor: ProxyCompressor,
    aead: ProxyAead,
    key16: &[u8; 16],
    key32: &[u8; 32],
) {
    ws_send_msg(
        server,
        &WsTunnelMsg::Error(err_msg.to_string()),
        compressor,
        aead,
        key16,
        key32,
    );
}

#[worker::send]
pub(crate) async fn proxy_ws(
    State(state): State<AppState>,
    Path((version, target)): Path<(String, String)>,
    req: Request,
) -> Result<Response, (StatusCode, String)> {
    use worker::{WebSocketPair, WebsocketEvent};

    let is_upgrade = req
        .headers()
        .get("connection")
        .and_then(|v| v.to_str().ok())
        .map_or(false, |v| v.eq_ignore_ascii_case("upgrade"));
    let is_websocket = req
        .headers()
        .get("upgrade")
        .and_then(|v| v.to_str().ok())
        .map_or(false, |v| v.eq_ignore_ascii_case("websocket"));

    if !(is_upgrade && is_websocket) {
        return Err(error!(BAD_REQUEST, "Request not supported"));
    }

    let key16 = state.keys.key16;
    let key32 = state.keys.key32;

    let compressor = ProxyCompressor::from_version(&version)
        .map_err(|_| error!(BAD_REQUEST, "unsupported version: {}", version))?;
    let aead = ProxyAead::from_target(&target)
        .map_err(|_| error!(BAD_REQUEST, "unsupported api: {}", target))?;

    let pair = WebSocketPair::new().map_err(|e| error!(INTERNAL_SERVER_ERROR, "{}", e))?;
    let server = pair.server;
    let client = pair.client;
    server.accept().map_err(|e| error!(INTERNAL_SERVER_ERROR, "{}", e))?;

    state.ctx.wait_until(async move {
        let mut event_stream = match server.events() {
            Ok(e) => e,
            Err(e) => {
                console_error!("[proxy_ws] register event stream failed: {e}");
                return;
            }
        };
        let server_tx = server.clone();

        // ---------- 阶段 1：头帧 = 浏览器的原始 WS 升级请求 + 末尾 1 字节 wss 标志 ----------
        let head_frame = loop {
            match event_stream.next().await {
                Some(Ok(WebsocketEvent::Message(msg))) => {
                    if let Some(bytes) = msg.bytes() {
                        if bytes.is_empty() {
                            ws_send_error(&server_tx, "Client sent empty head frame", compressor, aead, &key16, &key32);
                            continue;
                        }
                        break bytes;
                    }
                    ws_send_error(&server_tx, "Client sent non-binary message (text)", compressor, aead, &key16, &key32);
                }
                Some(Ok(WebsocketEvent::Close(_))) => {
                    console_error!("[proxy_ws] closed before head frame");
                    return;
                }
                Some(Err(e)) => {
                    let msg = format!("Event stream error before head frame: {e}");
                    ws_send_error(&server_tx, &msg, compressor, aead, &key16, &key32);
                    return;
                }
                None => {
                    console_error!("[proxy_ws] event stream ended before head frame");
                    return;
                }
            }
        };

        let data = match decode_chunk(&head_frame, compressor, aead, &key16, &key32) {
            Ok(d) => d,
            Err(e) => {
                let msg = format!("Decode head frame failed: {e}");
                ws_send_error(&server_tx, &msg, compressor, aead, &key16, &key32);
                let _ = server_tx.close(Some(1002), Some("Decode Error"));
                return;
            }
        };
        let raw_head = match WsTunnelMsg::deserialize(&data) {
            Ok(WsTunnelMsg::HeadFrame(h)) => h,
            _ => {
                ws_send_error(&server_tx, "Protocol violation: Expected HeadFrame", compressor, aead, &key16, &key32);
                let _ = server_tx.close(Some(1002), Some("Protocol Violation"));
                return;
            }
        };
        if raw_head.is_empty() {
            ws_send_error(&server_tx, "Protocol violation: Empty HeadFrame data", compressor, aead, &key16, &key32);
            let _ = server_tx.close(Some(1002), Some("Protocol Violation"));
            return;
        }

        let protocol = if raw_head[raw_head.len() - 1] % 2 == 0 { "http" } else { "https" };
        let head = match parse_head(&raw_head[..raw_head.len() - 1]) {
            Ok(h) => h,
            Err(e) => {
                let msg = format!("Parse HTTP head failed: {e}");
                ws_send_error(&server_tx, &msg, compressor, aead, &key16, &key32);
                let _ = server_tx.close(Some(1002), Some("Parse Head Error"));
                return;
            }
        };

        let is_absolute_form = ["http://", "https://", "ws://", "wss://"]
            .iter()
            .any(|p| head.target.starts_with(p));
        if head.host.is_none() && !is_absolute_form {
            ws_send_error(&server_tx, "Missing Host header in request", compressor, aead, &key16, &key32);
            let _ = server_tx.close(Some(1002), Some("Missing Host Header"));
            return;
        }

        // ---------- 阶段 2：直构上游请求 ----------
        let mut init = worker::RequestInit::new();
        init.with_method(worker::Method::Get);
        let fetch_headers = worker::Headers::new();

        let mut client_ws_key: Option<String> = None;
        for (k, v) in head.headers.iter() {
            let kl = k.to_ascii_lowercase();
            match kl.as_str() {
                "host" | "content-length" | "transfer-encoding" | "expect" | "connection"
                | "upgrade" | "sec-websocket-key" | "sec-websocket-version"
                | "sec-websocket-extensions" => {}
                _ => { let _ = fetch_headers.append(k, *v); }
            }
            if kl == "sec-websocket-key" {
                client_ws_key = Some(v.to_string());
            }
        }
        let _ = fetch_headers.set("Upgrade", "websocket");
        let _ = fetch_headers.set("Connection", "Upgrade");
        init.with_headers(fetch_headers);

        let full_url = if is_absolute_form {
            head.target.replace("ws://", "http://").replace("wss://", "https://")
        } else {
            match UrlBuilder::new().scheme(protocol).host(head.host).path(head.target).build() {
                Ok(u) => u,
                Err(e) => {
                    ws_send_error(&server_tx, &format!("Build upstream URL failed: {e}"), compressor, aead, &key16, &key32);
                    let _ = server_tx.close(Some(1002), Some("Invalid Target URL"));
                    return;
                }
            }
        };

        let outbound = match worker::Request::new_with_init(&full_url, &init) {
            Ok(r) => r,
            Err(e) => {
                ws_send_error(&server_tx, &format!("Build upstream request failed: {e}"), compressor, aead, &key16, &key32);
                let _ = server_tx.close(Some(1011), Some("Internal Error"));
                return;
            }
        };
        let upstream_resp = match Fetch::Request(outbound).send().into_send().await {
            Ok(r) => r,
            Err(e) => {
                ws_send_error(&server_tx, &format!("Fetch upstream failed: {e}"), compressor, aead, &key16, &key32);
                let _ = server_tx.close(Some(1011), Some("Upstream Fetch Error"));
                return;
            }
        };
        let status = upstream_resp.status_code();

        // ---------- 阶段 3：组装原始 HTTP 响应头并下发 ----------
        let resp_headers: Vec<(String, String)> = upstream_resp.headers().entries().collect();
        let mut resp_head = Vec::new();
        resp_head.extend_from_slice(format!("HTTP/1.1 {} {}\r\n", status, status_text(status)).as_bytes());
        for (k, v) in resp_headers.iter() {
            if ["transfer-encoding", "content-length"].iter().any(|&h| k.eq_ignore_ascii_case(h)) {
                continue;
            }
            if k.eq_ignore_ascii_case("sec-websocket-accept") {
                if let Some(kk) = &client_ws_key {
                    resp_head.extend_from_slice(
                        format!("sec-websocket-accept: {}\r\n", calc_sec_ws_accept(kk)).as_bytes(),
                    );
                    continue;
                }
            }
            resp_head.extend_from_slice(format!("{}: {}\r\n", k, v).as_bytes());
        }
        resp_head.extend_from_slice(b"\r\n");
        // 注意：此处若 status != 101，客户端收到这个 return 就可以构造本地 HTTP 40x 响应，
        // 这本身就是一个明确的失败信号，所以直接 ws_send_return 发走 HTTP 响应报文即可。
        ws_send_return(&server_tx, &resp_head, compressor, aead, &key16, &key32);

        if status != 101 {
            let _ = server_tx.close(Some(1011), Some("Upstream refused"));
            return;
        }

        let upstream_ws = match upstream_resp.websocket() {
            Some(ws) => ws,
            None => {
                ws_send_error(&server_tx, "Upstream response missing WebSocket object", compressor, aead, &key16, &key32);
                let _ = server_tx.close(Some(1011), Some("Upstream Missing WS"));
                return;
            }
        };
        let mut upstream_events = match upstream_ws.events() {
            Ok(e) => e,
            Err(e) => {
                ws_send_error(&server_tx, &format!("Upstream event stream error: {e}"), compressor, aead, &key16, &key32);
                let _ = server_tx.close(Some(1011), Some("Upstream Event Error"));
                return;
            }
        };
        if upstream_ws.accept().is_err() {
            ws_send_error(&server_tx, "Accept upstream WebSocket failed", compressor, aead, &key16, &key32);
            let _ = server_tx.close(Some(1011), Some("Upstream Accept Error"));
            return;
        }

        // ---------- 阶段 4：全双工转发 (Wasm-safe Select) ----------
        let upstream_tx = upstream_ws.clone();
        let server_tx_client = server_tx.clone();

        let client_to_upstream = async move {
            while let Some(event) = event_stream.next().await {
                let event = match event {
                    Ok(e) => e,
                    Err(e) => {
                        console_error!("[proxy_ws] Client event stream error: {}", e);
                        break;
                    }
                };
                match event {
                    WebsocketEvent::Message(msg) => {
                        let Some(bytes) = msg.bytes() else { continue };
                        let Ok(plain) = decode_chunk(&bytes, compressor, aead, &key16, &key32) else {
                            ws_send_error(&server_tx_client, "Decode chunk from client failed", compressor, aead, &key16, &key32);
                            break;
                        };
                        let Ok(tunnel) = WsTunnelMsg::deserialize(&plain) else {
                            ws_send_error(&server_tx_client, "Deserialize WsTunnelMsg failed", compressor, aead, &key16, &key32);
                            continue;
                        };

                        match tunnel {
                            WsTunnelMsg::Text(t) => { let _ = upstream_tx.send_with_str(&t); }
                            WsTunnelMsg::Binary(b) => { let _ = upstream_tx.send_with_bytes(b); }
                            WsTunnelMsg::Ping(_) | WsTunnelMsg::Pong(_) => {}
                            WsTunnelMsg::Close(c) => {
                                match c {
                                    Some((code, reason)) => { let _ = upstream_tx.close(Some(code), reason); }
                                    None => { let _ = upstream_tx.close(None, None::<&str>); }
                                }
                                break;
                            }
                            _ => {}
                        }
                    }
                    WebsocketEvent::Close(c) => {
                        let _ = upstream_tx.close(Some(c.code()), Some(c.reason().as_str()));
                        break;
                    }
                }
            }
        };

        let server_tx_up = server_tx.clone();
        let upstream_to_client = async move {
            while let Some(event) = upstream_events.next().await {
                let event = match event {
                    Ok(e) => e,
                    Err(e) => {
                        console_error!("[proxy_ws] Upstream event stream error: {}", e);
                        break;
                    }
                };
                match event {
                    WebsocketEvent::Message(msg) => {
                        let frame = if let Some(text) = msg.text() {
                            WsFrame::new_text(text, None)
                        } else if let Some(bytes) = msg.bytes() {
                            WsFrame::new_binary(bytes, None)
                        } else {
                            continue;
                        };
                        ws_send_return(&server_tx_up, &frame.to_bytes(), compressor, aead, &key16, &key32);
                    }
                    WebsocketEvent::Close(c) => {
                        let code = c.code();
                        let reason = c.reason();
                        let frame = WsFrame::new_close(Some((code, Some(reason.clone()))), None);
                        ws_send_return(&server_tx_up, &frame.to_bytes(), compressor, aead, &key16, &key32);
                        break;
                    }
                }
            }
        };

        // 在栈上 Pin 住两个 Future，以满足 select 的 Unpin 约束 (零内存分配开销)
        let client_fut = pin!(client_to_upstream);
        let upstream_fut = pin!(upstream_to_client);

        // 使用纯 future 的 select 等待其中任一结束。
        // 当发生退出时，未完成的一方会被立刻 Drop 掉，有效防止 Worker 内存泄漏。
        match select(client_fut, upstream_fut).await {
            Either::Left((_, _pending_upstream)) => {
                // 客户端发出了退出信号（或者报错退出），主动切断还在连接的上游 WebSocket
                let _ = upstream_ws.close(Some(1000), Some("Client disconnected"));
            }
            Either::Right((_, _pending_client)) => {
                // 上游发出了退出信号，主动切断客户端隧道
                let _ = server_tx.close(Some(1000), Some("Upstream disconnected"));
            }
        }
    });

    let worker_resp = worker::Response::from_websocket(client).map_err(|e| error!(INTERNAL_SERVER_ERROR, "{}", e))?;

    Ok(worker_resp.into())
}