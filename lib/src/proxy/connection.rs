// lib/src/proxy/connection.rs
//
// 连接分发器：TCP accept 后的连接级协议路由。
// 读首包 → 判断 CONNECT / 明文 HTTP → 探测隧道内首字节（0x16 = TLS
// ClientHello）→ 决定挂载 MITM TLS 或按明文 WS 解析 → 移交 relay::serve
// 或 ws::handle_ws_tunnel。

use anyhow::{bail, Context, Result};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration, Instant};

use crate::http::{split_host_port, HeaderPaser};

use super::relay::{read_next_header, serve, HeadOutcome};
use super::ws;
use super::Shared;

/// 等待浏览器发来第一个请求的超时（固定 deadline，不因为收到部分字节而被重置）
const FIRST_REQUEST_TIMEOUT: Duration = Duration::from_secs(96);
/// CONNECT 握手后等待隧道内首个请求的超时
const TLS_FIRST_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// 与浏览器完成 TLS(MITM) 握手允许的最长时间
const TLS_ACCEPT_TIMEOUT: Duration = Duration::from_secs(10);

/// 连接入口：读首个请求头，按是否 CONNECT 决定本地 TLS(MITM) 握手，
/// 之后统一进入 keep-alive 转发循环（泛型单态化：TcpStream / TlsStream<TcpStream>）。
pub(super) async fn handle_connection(mut socket: TcpStream, shared: Arc<Shared>) -> Result<()> {
    let mut parser = HeaderPaser::new();
    let deadline = Instant::now() + FIRST_REQUEST_TIMEOUT;
    let (header, remaining) = match read_next_header(&mut socket, &mut parser, deadline).await? {
        HeadOutcome::Head(h, r) => (h, r),
        HeadOutcome::Closed => return Ok(()),
        HeadOutcome::TimedOut => bail!("timed out waiting for first request"),
    };

    if header.is_connect() {
        let authority = header.path.trim();
        anyhow::ensure!(!authority.is_empty(), "CONNECT without authority");
        let (host, _port) = split_host_port(authority).context("invalid CONNECT authority")?;
        anyhow::ensure!(!host.is_empty(), "CONNECT without authority");

        socket
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        socket.flush().await?;

        // 探测隧道内首字节：0x16 = TLS ClientHello（wss，走 MITM）；
        // 否则为明文（Chrome 对 ws:// 走 CONNECT 但不加密），按 WS 升级请求解析。
        // peek 不消费数据，TLS 路径与原有逻辑完全一致。
        let mut probe = [0u8; 1];
        let is_tls = match timeout(TLS_ACCEPT_TIMEOUT, socket.peek(&mut probe)).await {
            Ok(Ok(1)) => probe[0] == 0x16,
            Ok(Ok(_)) => false, // 立即 EOF
            Ok(Err(e)) => return Err(e).context("peek tunneled data failed"),
            Err(_) => bail!("timed out waiting for tunneled data after CONNECT"),
        };

        if is_tls {
            let mut tls_stream = timeout(TLS_ACCEPT_TIMEOUT, shared.tls.accept(socket, Some(host)))
                .await
                .context("timed out establishing TLS with client")?
                .context("TLS handshake failed")?;

            // 隧道内重新解析真实请求头（与 TCP 阶段的 over-read 无关）
            let mut tls_parser = HeaderPaser::new();
            let deadline = Instant::now() + TLS_FIRST_REQUEST_TIMEOUT;
            let (h2, r2) = match read_next_header(&mut tls_stream, &mut tls_parser, deadline).await? {
                HeadOutcome::Head(h, r) => (h, r),
                HeadOutcome::Closed => return Ok(()),
                HeadOutcome::TimedOut => bail!("timed out waiting for tunneled request"),
            };

            // wss://：隧道内是 MITM 解密后的明文升级请求，wss 标志位 = 1
            if h2.is_websocket_upgrade() {
                ws::handle_ws_tunnel(&mut tls_stream, &h2, r2, true, &shared).await?;
                return Ok(());
            }

            serve(tls_stream, tls_parser, h2, r2, true, shared).await
        } else {
            // ws://（Chrome 明文隧道）：直接解析 WS 升级请求，wss 标志位 = 0
            let mut plain_parser = HeaderPaser::new();
            let deadline = Instant::now() + TLS_FIRST_REQUEST_TIMEOUT;
            let (h2, r2) = match read_next_header(&mut socket, &mut plain_parser, deadline).await? {
                HeadOutcome::Head(h, r) => (h, r),
                HeadOutcome::Closed => return Ok(()),
                HeadOutcome::TimedOut => bail!("timed out waiting for tunneled request"),
            };
            anyhow::ensure!(
                h2.is_websocket_upgrade(),
                "plain CONNECT tunnel expected websocket upgrade, got {:?}",
                h2.path
            );
            ws::handle_ws_tunnel(&mut socket, &h2, r2, false, &shared).await?;
            Ok(())
        }
    } else {
        serve(socket, parser, header, remaining, false, shared).await
    }
}

// ─── 错误分类（日志降噪） ─────────────────────────────────────────────────────

pub(super) fn is_benign_disconnect(e: &anyhow::Error) -> bool {
    e.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .map(|io_err| {
                matches!(
                    io_err.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::UnexpectedEof
                )
            })
            .unwrap_or(false)
    })
}
