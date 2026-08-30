

use anyhow::{Context, Result, bail, anyhow};
use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;
use crate::algo::{decode_chunk, ProxyAlgo};
use crate::frames::{enc_frame, make_frame, Frame, FrameCache};
use crate::http::{HeaderPaser, ReqHeader, UrlBuilder};
use crate::proxy::body::{split_body_prefix, BodyExtent, PumpTracker};
use crate::proxy::connection::IDLE_TIMEOUT;
use crate::tool::gen_auth_token;

use super::{read_raw, RawRead, READ_BUF, Shared};




/// 执行标准 HTTP 请求的代理转发：
/// 头帧 → body 泵送 → 等待 worker 响应 → 响应回传浏览器。
pub async fn handle_http_proxy<S>(
    stream: &mut S,
    parser: &mut HeaderPaser,
    header: &ReqHeader,
    mut remaining: BytesMut,
    extent: BodyExtent,
    is_https: bool,
    shared: &Arc<Shared>,
) -> Result<bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let algo = shared.algo();

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
                r = read_raw(stream, IDLE_TIMEOUT) => {
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
        None => match timeout(IDLE_TIMEOUT, &mut resp_task).await {
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

pub(super) async fn write_502<S: AsyncWrite + Unpin>(stream: &mut S) -> Result<()> {
    stream
        .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await?;
    stream.flush().await?;
    Ok(())
}



/// 转发一段字节给 worker（带 96s 上限，防上传卡死）
async fn send_to_worker(tx: &tokio::sync::mpsc::Sender<Bytes>, data: &[u8]) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    if timeout(IDLE_TIMEOUT, tx.send(Bytes::copy_from_slice(data)))
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


/// 把 worker 的帧流解包写回浏览器。帧协议（FrameCache/decode_chunk）是
/// 私有最小化二进制协议，无需语义解析；同时探测客户端提前断开，
/// 多读到的字节交还 parser 供 keep-alive 下一轮使用。
async fn relay_response<S>(
    stream: &mut S,
    resp: reqwest::Response,
    algo: ProxyAlgo,
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
            chunk = timeout(IDLE_TIMEOUT, resp_stream.next()) => {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(_) => {
                        crate::debug!("frame idle timeout (no data for {:?})", IDLE_TIMEOUT);
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
