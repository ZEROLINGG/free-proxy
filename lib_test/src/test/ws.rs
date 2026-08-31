// lib_test/src/test/ws.rs
//
// WebSocket 隧道 E2E 测试用例。
// 链路：test client → proxy(18081) → Worker(80) → web.rs(18082)
// 覆盖：文本/二进制帧双向透传、大 payload、控制帧、关闭传播、并发连接。

use anyhow::{bail, ensure, Result};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilder as _, Upgrade as _};
use crate::util;

fn ws_base() -> &'static str {
    "http://localhost:18082"
}

fn ws_client() -> reqwest::Client {
    reqwest::Client::builder()
        .proxy(reqwest::Proxy::all("http://127.0.0.1:18081").unwrap())
        .danger_accept_invalid_certs(true)
        .http1_only()
        .build()
        .unwrap()
}

async fn ws_connect(path: &str) -> Result<reqwest_websocket::WebSocket> {
    let url = format!("{}{path}", ws_base());
    let ws = ws_client()
        .get(&url)
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    Ok(ws)
}

// ─── 基础 echo ──────────────────────────────────────────────────────────────

pub async fn ws_echo_text() -> Result<()> {
    let mut ws = ws_connect("/ws/echo").await?;
    ws.send(Message::Text("hello".into())).await?;
    match ws.next().await {
        Some(Ok(Message::Text(t))) => ensure!(t == "hello", "echo mismatch: {t:?}"),
        other => bail!("expected Text echo, got {other:?}"),
    }
    Ok(())
}

pub async fn ws_echo_binary() -> Result<()> {
    let mut ws = ws_connect("/ws/echo").await?;
    let payload = util::pattern_bytes(1024);
    ws.send(Message::Binary(payload.clone().into())).await?;
    match ws.next().await {
        Some(Ok(Message::Binary(b))) => ensure!(
            b.as_ref() == payload.as_slice(),
            "binary echo mismatch: len={}",
            b.len()
        ),
        other => bail!("expected Binary echo, got {other:?}"),
    }
    Ok(())
}

pub async fn ws_echo_roundtrip_10() -> Result<()> {
    let mut ws = ws_connect("/ws/echo").await?;
    for i in 0..10 {
        let msg = format!("msg-{i}");
        ws.send(Message::Text(msg.clone())).await?;
        match ws.next().await {
            Some(Ok(Message::Text(t))) => ensure!(t == msg, "roundtrip {i} mismatch: {t:?}"),
            other => bail!("roundtrip {i}: expected Text, got {other:?}"),
        }
    }
    Ok(())
}

// ─── 大 payload ─────────────────────────────────────────────────────────────

pub async fn ws_large_binary_64k() -> Result<()> {
    let mut ws = ws_connect("/ws/echo").await?;
    let payload = util::pattern_bytes(64 * 1024);
    ws.send(Message::Binary(payload.clone().into())).await?;
    match ws.next().await {
        Some(Ok(Message::Binary(b))) => ensure!(
            b.as_ref() == payload.as_slice(),
            "64k echo mismatch: got {} bytes",
            b.len()
        ),
        other => bail!("expected Binary echo, got {other:?}"),
    }
    Ok(())
}

pub async fn ws_large_binary_1mb() -> Result<()> {
    let mut ws = ws_connect("/ws/echo").await?;
    let payload = util::pattern_bytes(1024 * 1024);
    ws.send(Message::Binary(payload.clone().into())).await?;
    match ws.next().await {
        Some(Ok(Message::Binary(b))) => ensure!(
            b.as_ref() == payload.as_slice(),
            "1mb echo mismatch: got {} bytes",
            b.len()
        ),
        other => bail!("expected Binary echo, got {other:?}"),
    }
    Ok(())
}

// ─── 控制帧 ─────────────────────────────────────────────────────────────────

pub async fn ws_ping_pong() -> Result<()> {
    let mut ws = ws_connect("/ws/echo").await?;
    ws.send(Message::Ping(vec![0x01, 0x02, 0x03].into())).await?;
    match ws.next().await {
        Some(Ok(Message::Pong(p))) => ensure!(
            p.as_ref() == &[0x01, 0x02, 0x03],
            "pong payload mismatch"
        ),
        other => bail!("expected Pong, got {other:?}"),
    }
    Ok(())
}

// ─── 关闭传播 ───────────────────────────────────────────────────────────────

pub async fn ws_close_from_client() -> Result<()> {
    let mut ws = ws_connect("/ws/echo").await?;
    // 发一条数据确认连接正常
    ws.send(Message::Text("before-close".into())).await?;
    let _ = ws.next().await;

    // 客户端发起关闭
    ws.send(Message::Close {
        code: reqwest_websocket::CloseCode::Normal,
        reason: "test bye".into(),
    }).await?;

    // 确认收到关闭帧或连接结束
    let mut got_close = false;
    for _ in 0..5 {
        match ws.next().await {
            Some(Ok(Message::Close { .. })) => { got_close = true; break; }
            None => { got_close = true; break; }
            _ => continue,
        }
    }
    ensure!(got_close, "did not receive close confirmation");
    Ok(())
}

pub async fn ws_close_from_server() -> Result<()> {
    let mut ws = ws_connect("/ws/close").await?;
    // 服务器会立即发 Close(1000)
    let mut got_close = false;
    for _ in 0..5 {
        match ws.next().await {
            Some(Ok(Message::Close { .. })) => { got_close = true; break; }
            None => { got_close = true; break; }
            _ => continue,
        }
    }
    ensure!(got_close, "did not receive server-initiated close");
    Ok(())
}

// ─── 下行批量推送 ───────────────────────────────────────────────────────────

pub async fn ws_binary_stream_down() -> Result<()> {
    let mut ws = ws_connect("/ws/binary-stream").await?;
    let mut total_bytes = 0usize;
    let frame_count = 20usize;
    let frame_size = 1024usize;

    for i in 0..frame_count {
        match ws.next().await {
            Some(Ok(Message::Binary(b))) => {
                let mut expect = vec![0u8; frame_size];
                util::fill_pattern(&mut expect, i as u64 * frame_size as u64);
                ensure!(
                    b.as_ref() == expect.as_slice(),
                    "frame {i} content mismatch: got {} bytes",
                    b.len()
                );
                total_bytes += b.len();
            }
            other => bail!("frame {i}: expected Binary, got {other:?}"),
        }
    }
    ensure!(
        total_bytes == frame_count * frame_size,
        "total bytes mismatch: {total_bytes}"
    );
    Ok(())
}

// ─── 并发连接 ───────────────────────────────────────────────────────────────

pub async fn ws_concurrent_5() -> Result<()> {
    let mut handles = Vec::new();
    for id in 0..5u32 {
        handles.push(tokio::spawn(async move {
            let mut ws = ws_connect("/ws/echo").await?;
            let msg = format!("concurrent-{id}");
            ws.send(Message::Text(msg.clone())).await?;
            match ws.next().await {
                Some(Ok(Message::Text(t))) => {
                    ensure!(t == msg, "concurrent {id} mismatch: {t:?}");
                    Ok::<_, anyhow::Error>(())
                }
                other => bail!("concurrent {id}: expected Text, got {other:?}"),
            }
        }));
    }
    let mut failed = 0;
    for h in handles {
        if h.await?.is_err() {
            failed += 1;
        }
    }
    ensure!(failed == 0, "{failed}/5 concurrent WS tests failed");
    Ok(())
}

// ─── 边界值 ─────────────────────────────────────────────────────────────────

pub async fn ws_binary_symmetric_4k() -> Result<()> {
    let mut ws = ws_connect("/ws/echo").await?;
    let payload = util::pattern_bytes(4096);
    ws.send(Message::Binary(payload.clone().into())).await?;
    match ws.next().await {
        Some(Ok(Message::Binary(b))) => ensure!(
            b.as_ref() == payload.as_slice(),
            "4k echo mismatch: got {} bytes",
            b.len()
        ),
        other => bail!("expected Binary echo, got {other:?}"),
    }
    Ok(())
}

pub async fn ws_binary_symmetric_65k() -> Result<()> {
    let mut ws = ws_connect("/ws/echo").await?;
    let payload = util::pattern_bytes(65536);
    ws.send(Message::Binary(payload.clone().into())).await?;
    match ws.next().await {
        Some(Ok(Message::Binary(b))) => ensure!(
            b.as_ref() == payload.as_slice(),
            "65k echo mismatch: got {} bytes",
            b.len()
        ),
        other => bail!("expected Binary echo, got {other:?}"),
    }
    Ok(())
}

// ─── 在线 WebSocket echo 服务器 ─────────────────────────────────────────────

pub async fn ws_online_echo() -> Result<()> {
    let ws = ws_client()
        .get("wss://echo.websocket.org")
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;

    let (mut tx, mut rx) = ws.split();

    // echo.websocket.org 连接后先发一条服务器标识消息，跳过它
    match rx.next().await {
        Some(Ok(Message::Text(_))) => {}
        other => bail!("expected server welcome message, got {other:?}"),
    }

    let test_payloads = ["2", "333", "www", "1234"];
    for payload in test_payloads {
        tx.send(Message::Text(payload.into())).await?;
        match rx.next().await {
            Some(Ok(Message::Text(t))) => ensure!(
                t == payload,
                "online echo mismatch: sent {payload:?}, got {t:?}"
            ),
            other => bail!("online echo: expected Text for {payload:?}, got {other:?}"),
        }
    }

    tx.close().await?;
    Ok(())
}
