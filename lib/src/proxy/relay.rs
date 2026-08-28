// lib/src/proxy/relay.rs
//
// 核心转发引擎：以泛型 Stream（TcpStream / TlsStream）为输入输出，
// 完成"Stream 进 → Stream 出"的黑盒处理。
//   - read_next_header / HeadOutcome：请求头读取（httparse 零拷贝复用）
//   - serve：keep-alive 连接循环
//   - handle_one_request：单个请求的转发（头帧 → body 泵送 → EOS → 响应回传）
//   - relay_response：worker 响应帧流解包写回浏览器
//   - RawRead / read_raw：底层原始读取（ws.rs 亦在使用）

use anyhow::{anyhow, bail, Context, Result};
use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::{timeout, timeout_at, Duration, Instant};

use crate::algo::{decode_chunk, encode_chunk};
use crate::frames::{make_frame, Frame, FrameCache};
use crate::http::{HeaderPaser, ReqHeader, UrlBuilder};
use crate::tool::gen_auth_token;

use super::body::{body_extent, split_body_prefix, PumpTracker};
use super::ws;
use super::Shared;

/// 单个帧之间允许的最大间隔（超过视为假死连接）
const FRAME_IDLE_TIMEOUT: Duration = Duration::from_secs(96);
/// 请求 body 泵送阶段：两次成功读取之间的最大间隔（上传卡死兜底）
const BODY_IDLE_TIMEOUT: Duration = Duration::from_secs(96);
/// Keep-Alive 场景下，等待同一连接上下一个请求到达的空闲超时
const KEEP_ALIVE_IDLE_TIMEOUT: Duration = Duration::from_secs(96);

/// 单次读取缓冲大小
const READ_BUF: usize = 16 * 1024;

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

        let deadline = Instant::now() + KEEP_ALIVE_IDLE_TIMEOUT;
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

// ─── 原始读取 ────────────────────────────────────────────────────────────────

/// 单次原始读取的结果
pub(super) enum RawRead {
    Data(Bytes),
    Eof,
    TimedOut,
}

pub(super) async fn read_raw<S: AsyncRead + Unpin>(stream: &mut S, idle: Duration) -> Result<RawRead> {
    let mut buf = [0u8; READ_BUF];
    match timeout(idle, stream.read(&mut buf)).await {
        Ok(Ok(0)) => Ok(RawRead::Eof),
        Ok(Ok(n)) => Ok(RawRead::Data(Bytes::copy_from_slice(&buf[..n]))),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(RawRead::Eof),
        Ok(Err(e)) => Err(e).context("read failed"),
        Err(_) => Ok(RawRead::TimedOut),
    }
}

// ─── Body 泵送 ───────────────────────────────────────────────────────────────

/// 转发一段字节给 worker（带 96s 上限，防上传卡死）
async fn send_to_worker(tx: &tokio::sync::mpsc::Sender<Bytes>, data: &[u8]) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    if timeout(BODY_IDLE_TIMEOUT, tx.send(Bytes::copy_from_slice(data)))
        .await
        .map_err(|_| anyhow!("worker upload stalled"))?
        .is_err()
    {
        bail!("worker body channel closed unexpectedly");
    }
    Ok(())
}

/// 处理一段属于请求体的原始字节：把解码后的负载转发给 worker 并推进进度。
/// 返回 Some(take)：请求体已结束，本段前 take 字节属于 body（其后归 parser）；
/// None：尚未结束。
/// chunked 请求下 payload 已由 tracker 解码（分帧字节剥离），只转发纯负载，
/// 避免 worker 侧对同一分帧再编码造成"双重 chunked"。
async fn pump_chunk(
    tx: &tokio::sync::mpsc::Sender<Bytes>,
    tracker: &mut PumpTracker,
    data: &[u8],
) -> Result<Option<usize>> {
    let pushed = tracker.push(data)?;
    send_to_worker(tx, &pushed.payload).await?;
    Ok(pushed.end_at)
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
async fn handle_one_request<S>(
    stream: &mut S,
    parser: &mut HeaderPaser,
    header: &ReqHeader,
    mut remaining: BytesMut,
    is_https: bool,
    shared: &Arc<Shared>,
) -> Result<bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let algo = shared.algo();

    // ---------- 请求体范围判定 ----------
    let extent = body_extent(&header.headers)?;

    crate::debug!(
        "{:<7} {:<15.30} {:<15.30} {:?} {:.512?}",
        header.method,
        header
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("HOST"))
            .map(|(_, v)| v)
            .unwrap_or(&"unknown".to_string()),
        header.path,
        extent,
        format!("{:?}",header.headers)
    );

    // ---------- WebSocket 升级请求：转入 WS 隧道，连接不再复用 ----------
    if header.is_websocket_upgrade() {
        ws::handle_ws_tunnel(stream, header, remaining, is_https, shared).await?;
        return Ok(false);
    }

    // ---------- 首帧：raw 头部 + https 标志（零重建） ----------
    let mut head_frame = BytesMut::with_capacity(header.raw.len() + 1);
    head_frame.extend_from_slice(&header.raw);
    head_frame.extend_from_slice(&[if is_https { 1 } else { 0 }]);
    let head_frame = head_frame.freeze();

    // ---------- 超读字节拆分：body 前缀经通道转发，其余归还 parser ----------
    let (prefix_len, _push_len) = split_body_prefix(&remaining, &extent);
    let body_prefix = remaining.split_to(prefix_len).freeze();
    if !remaining.is_empty() {
        parser.push(&remaining)?;
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Bytes>(16);
    let key16 = shared.key16;
    let key32 = shared.key32;
    let body_stream = async_stream::stream! {
        match enc_frame(&head_frame, algo, &key16, &key32) {
            Ok(frame) => yield Ok(frame),
            Err(e) => { yield Err(e); return; }
        }
        while let Some(bytes) = rx.recv().await {
            if bytes.is_empty() {
                continue;
            }
            match enc_frame(&bytes, algo, &key16, &key32) {
                Ok(frame) => yield Ok(frame),
                Err(e) => { yield Err(e); return; }
            }
        }
        // 零长帧 = EOS，请求体结束
        yield Ok(Bytes::from(make_frame(b"")));
    };

    let url = UrlBuilder::new()
        .base(shared.worker_url.as_str())
        .path(algo.api_path().as_str())
        .build()
        .context("build worker request url failed")?;

    let resp_fut = shared
        .get_client()
        .post(url)
        .bearer_auth(gen_auth_token(&shared.token_base))
        .header("Content-Type", "application/octet-stream")
        .body(reqwest::Body::wrap_stream(body_stream))
        .send();

    // 把请求转移到后台任务运行，防止 tx.send().await 在当前 select 中导致协程级死锁
    let mut resp_task = tokio::spawn(resp_fut);

    // ---------- 泵送阶段：读完浏览器请求体即完成（EOS），再等响应 ----------
    let mut tracker = PumpTracker::new(&extent);
    let mut early_resp: Option<reqwest::Response> = None;
    let mut client_eof = false;
    let mut body_done = false;

    if !body_prefix.is_empty() {
        if let Some(take) = pump_chunk(&tx, tracker.as_mut().unwrap(), &body_prefix).await? {
            if take < body_prefix.len() {
                parser.push(&body_prefix[take..])?;
            }
            body_done = true;
        }
    }

    if tracker.is_some() && !body_done {
        loop {
            tokio::select! {
                biased;
                r = read_raw(stream, BODY_IDLE_TIMEOUT) => {
                    match r? {
                        RawRead::Data(data) => {
                            match pump_chunk(&tx, tracker.as_mut().unwrap(), &data).await? {
                                Some(take) => {
                                    if take < data.len() {
                                        parser.push(&data[take..])?;
                                    }
                                    break;
                                }
                                None => {}
                            }
                        }
                        RawRead::Eof => {
                            client_eof = true;
                            break;
                        }
                        RawRead::TimedOut => bail!("browser body idle timeout"),
                    }
                }
                res = &mut resp_task, if early_resp.is_none() => {
                    match res {
                        Ok(Ok(r)) => early_resp = Some(r),
                        Ok(Err(e)) => return Err(anyhow::Error::new(e).context("worker request failed")),
                        Err(e) => return Err(anyhow::Error::new(e).context("worker task panicked")),
                    }
                }
            }
        }
    }
    drop(tx); // 通知 worker 侧请求体结束（EOS 帧）

    // ---------- 响应阶段：等待 worker 响应并回传 ----------
    let resp = match early_resp {
        Some(r) => r,
        // 【修改点 3】：这里的 timeout 目标也由原先的 resp_fut 改为 resp_task
        None => match timeout(FRAME_IDLE_TIMEOUT, &mut resp_task).await {
            Ok(Ok(Ok(r))) => r,
            Ok(Ok(Err(e))) => return Err(anyhow::Error::new(e).context("worker request failed")),
            Ok(Err(e)) => return Err(anyhow::Error::new(e).context("worker task panicked")),
            Err(_) => {
                crate::warn!("worker response idle timeout");
                write_502(stream).await?;
                return Ok(false);
            }
        },
    };

    if !resp.status().is_success() {
        crate::error!(
            "worker request error: {}, body: {:.1024}...",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
        write_502(stream).await?;
        return Ok(false);
    }

    if !relay_response(stream, resp, algo, shared, parser).await? {
        return Ok(false);
    }

    Ok(!client_eof)
}

/// 压缩+加密并打包成一帧（body_stream 专用小工具）。
fn enc_frame(data: &[u8], algo: crate::algo::ProxyAlgo, key16: &[u8], key32: &[u8]) -> std::io::Result<Bytes> {
    let enc = encode_chunk(data, algo.compressor, algo.aead, key16, key32)
        .map_err(|e| std::io::Error::other(format!("encode failed: {e}")))?;
    Ok(Bytes::from(make_frame(&enc)))
}

// ─── 响应回传 ───────────────────────────────────────────────────────────────

/// 把 worker 的帧流解包写回浏览器。帧协议（FrameCache/decode_chunk）是
/// 私有最小化二进制协议，无需语义解析；同时探测客户端提前断开，
/// 多读到的字节交还 parser 供 keep-alive 下一轮使用。
async fn relay_response<S>(
    stream: &mut S,
    resp: reqwest::Response,
    algo: crate::algo::ProxyAlgo,
    shared: &Arc<Shared>,
    parser: &mut HeaderPaser,
) -> Result<bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut frame_cache = FrameCache::new();
    let mut resp_stream = Box::pin(resp.bytes_stream());
    let mut wrote = false;

    loop {
        tokio::select! {
            chunk = timeout(FRAME_IDLE_TIMEOUT, resp_stream.next()) => {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(_) => {
                        crate::debug!("frame idle timeout (no data for {:?})", FRAME_IDLE_TIMEOUT);
                        if !wrote {
                            write_502(stream).await?;
                        }
                        return Ok(false);
                    }
                };

                let bytes = match chunk {
                    Some(Ok(b)) => b,
                    Some(Err(e)) => {
                        crate::debug!("frame read error: {e}");
                        return Ok(false);
                    }
                    None => {
                        crate::debug!("stream ended without EOS (truncated)");
                        if !wrote {
                            write_502(stream).await?;
                        }
                        return Ok(false);
                    }
                };

                frame_cache.push(&bytes);
                loop {
                    match frame_cache.try_pop() {
                        Ok(Frame::Frame(raw_enc)) => {
                            let raw = decode_chunk(
                                &raw_enc,
                                algo.compressor,
                                algo.aead,
                                &shared.key16,
                                &shared.key32,
                            )
                            .context("decrypt/decompress failed")?;

                            stream.write_all(&raw).await?;
                            wrote = true;
                        }
                        Ok(Frame::None) => break,
                        Ok(Frame::Eos) => {
                            // 零长帧 = 流正常完成
                            stream.flush().await?;
                            return Ok(true);
                        }
                        Err(e) => {
                            crate::debug!("frame protocol error: {e}");
                            if !wrote {
                                write_502(stream).await?;
                            }
                            return Ok(false);
                        }
                    }
                }
            }
            closed = client_closed(stream, parser) => {
                if closed? {
                    return Ok(false);
                }
            }
        }
    }
}

/// 探测客户端是否已断开（读取可用字节）；未断开时字节存入 parser。
async fn client_closed<S: AsyncRead + Unpin>(
    stream: &mut S,
    parser: &mut HeaderPaser,
) -> Result<bool> {
    let mut buf = [0u8; READ_BUF];
    match stream.read(&mut buf).await {
        Ok(0) | Err(_) => Ok(true),
        Ok(n) => {
            parser.push(&buf[..n])?;
            Ok(false)
        }
    }
}

pub(super) async fn write_502<S: AsyncWrite + Unpin>(stream: &mut S) -> Result<()> {
    stream
        .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await?;
    stream.flush().await?;
    Ok(())
}