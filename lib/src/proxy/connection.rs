// lib/src/proxy/connection.rs

use anyhow::{bail, Context, Result};
use std::sync::Arc;
use bytes::BytesMut;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, timeout_at, Duration, Instant};

use crate::http::{split_host_port, HeaderPaser, ReqHeader};
use crate::proxy::body::body_extent;
use crate::proxy::core::{http::handle_http_proxy, ws::handle_ws_proxy};
use super::Shared;

/// 等待浏览器发来第一个请求的超时（固定 deadline，不因为收到部分字节而被重置）
const FIRST_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// 与浏览器完成 TLS(MITM) 握手允许的最长时间
const TLS_ACCEPT_TIMEOUT: Duration = Duration::from_secs(10);
/// 等待同一连接上下一个请求到达的空闲超时
pub(crate) const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// 单次读取缓冲大小
const READ_BUF: usize = 16 * 1024;


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
            let deadline = Instant::now() + FIRST_REQUEST_TIMEOUT;
            let (h2, r2) = match read_next_header(&mut tls_stream, &mut tls_parser, deadline).await? {
                HeadOutcome::Head(h, r) => (h, r),
                HeadOutcome::Closed => return Ok(()),
                HeadOutcome::TimedOut => bail!("timed out waiting for tunneled request"),
            };

            // wss://：隧道内是 MITM 解密后的明文升级请求，wss 标志位 = 1
            if h2.is_websocket_upgrade() {
                handle_ws_proxy(&mut tls_stream, &h2, r2, true, &shared).await?;
                return Ok(());
            }

            serve(tls_stream, tls_parser, h2, r2, true, shared).await
        } else {
            // ws://（Chrome 明文隧道）：直接解析 WS 升级请求，wss 标志位 = 0
            let mut plain_parser = HeaderPaser::new();
            let deadline = Instant::now() + FIRST_REQUEST_TIMEOUT;
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
            handle_ws_proxy(&mut socket, &h2, r2, false, &shared).await?;
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




// ─── 头解析 ────────────────────────────────────────────────────────────────

/// 读取下一个请求头的结果
pub(super) enum HeadOutcome {
    /// 请求头已收全（body 字节在 remaining 中，由事务处理阶段按语义消费/归还）
    Head(ReqHeader, BytesMut),
    /// 对端在完整请求头到达前正常关闭了连接（EOF，含 TLS 无 close_notify 的场景）
    Closed,
    /// 直到 deadline 都没有等到完整请求头
    TimedOut,
}

/// 从 stream 中持续读取数据，直到 parser 弹出一个完整请求头、
/// 对端正常关闭连接，或者到达 deadline。
pub(super) async fn read_next_header<S>(
    stream: &mut S,
    parser: &mut HeaderPaser,
    deadline: Instant,
) -> Result<HeadOutcome>
where
    S: AsyncRead + Unpin,
{
    if let Some((head, remaining)) = parser.try_pop()? {
        return Ok(HeadOutcome::Head(head, remaining));
    }

    loop {
        let mut buf = [0u8; READ_BUF];
        let n = match timeout_at(deadline, stream.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(HeadOutcome::Closed);
            }
            Ok(Err(e)) => return Err(e).context("read failed"),
            Err(_) => return Ok(HeadOutcome::TimedOut),
        };
        if n == 0 {
            return Ok(HeadOutcome::Closed);
        }
        parser.push(&buf[..n])?;
        if let Some((head, remaining)) = parser.try_pop()? {
            return Ok(HeadOutcome::Head(head, remaining));
        }
    }
}

// ─── Keep-Alive 循环 ─────────────────────────────────────────────────────────

/// 统一的 keep-alive 循环：明文 HTTP 用 TcpStream 单态化一份，
/// 隧道内 HTTPS 用 TlsStream 单态化另一份。CONNECT 不可能在此出现。
pub(super) async fn serve<S>(
    mut stream: S,
    mut parser: HeaderPaser,
    mut header: ReqHeader,
    mut remaining: BytesMut,
    is_https: bool,
    shared: Arc<Shared>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        if header.is_connect() {
            bail!("unexpected CONNECT within an established stream");
        }

        let keep_alive = handle_one_request(&mut stream, &mut parser, &header, remaining, is_https, &shared).await?;

        if !keep_alive {
            break;
        }

        let deadline = Instant::now() + IDLE_TIMEOUT;
        match read_next_header(&mut stream, &mut parser, deadline).await? {
            HeadOutcome::Head(h, r) => {
                header = h;
                remaining = r;
            }
            HeadOutcome::Closed | HeadOutcome::TimedOut => break,
        }
    }

    let _ = stream.shutdown().await;
    Ok(())
}


// ─── 单请求转发 ──────────────────────────────────────────────────────────────

/// 转发单个请求：按 HTTP 语义判定请求体范围，读完浏览器请求体后立即以
/// EOS 完成 worker 请求体，再等待响应并回传。
/// Cloudflare edge 在请求体未完成前不会交付响应，因此不能"等响应再 EOS"。
///   - 上行：头帧（raw + https 标志）→ body 帧（头部超读字节 + 浏览器流）→ EOS；
///   - 请求体边界：Content-Length 按计数、chunked 按四态解码（只转发 chunk 数据
///     负载，剥离分帧字节），无 body 请求立即 EOS；超读字节中不属于 body 的部分
///     归还 parser（keep-alive 流水线）；浏览器中途 EOF 照发 EOS（截断，worker 侧报错）；
///   - 下行：响应帧流解包写回浏览器。
///
/// 返回是否可复用连接（keep-alive）：relay 成功且客户端未断开即可复用。
/// 处理单个请求：解析 Body 范围，记录日志，并根据请求类型路由到 WS 隧道或标准 HTTP 转发。
async fn handle_one_request<S>(
    stream: &mut S,
    parser: &mut HeaderPaser,
    header: &ReqHeader,
    remaining: BytesMut,
    is_https: bool,
    shared: &Arc<Shared>,
) -> Result<bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // ---------- 请求体范围判定 ----------
    let extent = body_extent(&header.headers)?;

    crate::debug!(
        "{:<7} {:<15.30} {:<15.30} {:?} {:.64?}",
        header.method,
        header
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("HOST"))
            .map(|(_, v)| v)
            .unwrap_or(&"unknown".to_string()),
        header.path,
        extent,
        format!("{:?}", header.headers)
    );

    // ---------- WebSocket 升级请求：转入 WS 隧道，连接不再复用 ----------
    if header.is_websocket_upgrade() {
        handle_ws_proxy(stream, header, remaining, is_https, shared).await?;
        return Ok(false);
    }

    // ---------- 标准 HTTP 转发 ----------
    handle_http_proxy(stream, parser, header, remaining, extent, is_https, shared).await
}



