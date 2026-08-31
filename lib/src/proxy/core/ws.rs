// lib/src/proxy/core/ws.rs

use anyhow::{anyhow, bail, Context, Result};
use bytes::{Buf, Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{CloseCode, Message, Upgrade};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{split, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, WriteHalf};
use tokio::sync::mpsc;
use tokio::task::JoinError;
use crate::algo::{ProxyAlgo, decode_chunk, encode_chunk};
use crate::http::ReqHeader;
use crate::proxy::connection::IDLE_TIMEOUT;
use crate::tool::gen_auth_token;
use crate::ws::{WsCache, WsData, WsFrame, WsTunnelMsg};

use super::Shared;

const TUNNEL_PING_INTERVAL: Duration = Duration::from_secs(60);
/// 每轮读取前保证至少剩余这么多可写容量
const READ_CHUNK: usize = 64 * 1024;
/// 控制帧（pong / close / 错误页）有界通道容量：体积小、频率低，32 足够。
const CTRL_CHANNEL_CAP: usize = 32;
/// 业务下行数据有界通道容量：writer 来不及写时提供缓冲，形成自然的 TCP 背压。
const DATA_CHANNEL_CAP: usize = 64;

pub(crate) async fn handle_ws_proxy<S>(
    stream: S,
    header: &ReqHeader,
    remaining: BytesMut,
    is_https: bool,
    shared: &Arc<Shared>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let algo = shared.algo();
    let key16 = shared.key16;
    let key32 = shared.key32;

    let mut stream = stream;

    // ---------- 头帧：浏览器原始请求 + 末尾 wss 标志（与 HTTP 路径同款） ----------
    let mut head_frame = BytesMut::with_capacity(header.raw.len() + 1);
    head_frame.extend_from_slice(&header.raw);
    head_frame.extend_from_slice(&[if is_https { 1 } else { 0 }]);

    // ---------- 建立隧道（必须 HTTP/1.1，ws_client 已 http1_only） ----------
    let url = crate::http::UrlBuilder::new()
        .base(shared.worker_url.as_str())
        .path(algo.ws_path().as_str())
        .build()
        .context("build ws tunnel url failed")?;

    let ws_client = shared.ws_client();

    let upgraded = match ws_client
        .get(&url)
        .bearer_auth(gen_auth_token(&shared.token_base))
        .upgrade()
        .send()
        .await
    {
        Ok(u) => u,
        Err(e) => {
            crate::error!("tunnel handshake failed: {e:#}");
            let _ = stream
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                .await;
            return Err(e).context("ws tunnel handshake failed");
        }
    };
    let mut ws = match upgraded.into_websocket().await {
        Ok(w) => w,
        Err(e) => {
            crate::error!("tunnel upgrade failed: {e:#}");
            let _ = stream
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                .await;
            return Err(e).context("ws tunnel upgrade failed");
        }
    };

    // ---------- 首消息：头帧（BytesMut::to_vec 免去 freeze 中间态） ----------
    ws.send(Message::Binary(tunnel_payload(
        &WsTunnelMsg::HeadFrame(head_frame.to_vec()),
        algo,
        &key16,
        &key32,
    )?))
        .await
        .context("send ws head frame failed")?;

    // ---------- 拆分 TCP 读写半 + WS 收发半 ----------
    let (mut rd, wr) = split(stream);
    let (mut ws_tx, mut ws_rx) = ws.split();

    // ---------- 写侧改为 单写协程 + 双优先级有界 channel ----------
    // upload / download 不再直接持锁写 TCP，而是把待写字节丢进 channel，
    // 由唯一 writer 串行落盘。控制帧（pong/close/错误页）走 ctrl（高优先），
    // 业务下行走 data（低优先），writer 内 biased select 保证控制帧优先出队，
    // 从根上消除此前 Arc<Mutex<WriteHalf>> 的锁竞争与优先级反转。
    let (ctrl_tx, ctrl_rx) = mpsc::channel::<Bytes>(CTRL_CHANNEL_CAP);
    let (data_tx, data_rx) = mpsc::channel::<Bytes>(DATA_CHANNEL_CAP);

    let mut writer_handle = tokio::spawn(run_writer(wr, ctrl_rx, data_rx));

    let upload_ctrl = ctrl_tx.clone();
    let download_ctrl = ctrl_tx.clone();
    drop(ctrl_tx); // 父协程不再持有 sender；writer 依赖两侧 sender 全部 drop 后自然收尾

    // [A] 上行：浏览器帧流 → 语义化 WsTunnelMsg
    let mut upload_handle = tokio::spawn(async move {
        let mut buf = BytesMut::from(remaining);
        let mut cache = WsCache::new();

        // 心跳用固定节拍 interval：不因数据活跃而被无限期推迟（修复 sleep 每次
        // 重建导致的心跳饥饿），Delay 模式保证错过拍的 tick 立即补发。
        let mut ping_interval = tokio::time::interval(TUNNEL_PING_INTERVAL);
        ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ping_interval.tick().await; // 消费首个立即触发的 tick

        loop {
            let mut needs_flush = false;

            // 先榨干缓冲中的完整帧（不 await，避免 select 分支内的重入）
            loop {
                let Some((frame, consumed)) = WsFrame::parse(&buf)? else {
                    break;
                };
                buf.advance(consumed);

                if frame.is_ping() {
                    let pong = WsFrame::new_pong(frame.payload.clone(), None);
                    upload_ctrl
                        .send(Bytes::from(pong.to_bytes()))
                        .await
                        .map_err(|_| anyhow!("writer channel closed"))?;

                    let payload = tunnel_payload(
                        &WsTunnelMsg::Ping(frame.payload.clone()),
                        algo,
                        &key16,
                        &key32,
                    )?; // 用于立即刷新WsTunnel，无回pong
                    ws_tx.feed(Message::Binary(payload)).await?;
                    needs_flush = true;

                } else if frame.is_pong() {
                    // 浏览器应答我们的保活 Ping，忽略
                } else if frame.is_close() {
                    let close_info = frame.close_info();
                    let resp = WsFrame::new_close(close_info.clone(), None);
                    upload_ctrl
                        .send(Bytes::from(resp.to_bytes()))
                        .await
                        .map_err(|_| anyhow!("writer channel closed"))?;

                    let payload = tunnel_payload(
                        &WsTunnelMsg::Close(close_info),
                        algo,
                        &key16,
                        &key32,
                    )?;
                    ws_tx.send(Message::Binary(payload)).await?;
                    ws_tx.send(Message::Close {
                        code: CloseCode::Iana(1000),
                        reason: String::new(),
                    })
                        .await?;
                    return Ok::<(), anyhow::Error>(());
                } else if frame.is_data() {
                    cache.push(frame)?;
                    match cache.try_pop()? {
                        Some(WsData::Text(text)) => {
                            let payload = tunnel_payload(&WsTunnelMsg::Text(text), algo, &key16, &key32)?;
                            ws_tx.feed(Message::Binary(payload)).await?;
                            needs_flush = true;
                        }
                        Some(WsData::Binary(bin)) => {
                            let payload = tunnel_payload(&WsTunnelMsg::Binary(bin), algo, &key16, &key32)?;
                            ws_tx.feed(Message::Binary(payload)).await?;
                            needs_flush = true;
                        }
                        None => {}
                    }
                }
            }

            if needs_flush {
                ws_tx.flush().await?;
            }

            // 保证 read_buf 有足够剩余可写容量：
            if buf.capacity() - buf.len() < READ_CHUNK / 4 {
                buf.reserve(READ_CHUNK);
            }

            tokio::select! {
                _ = ping_interval.tick() => {
                    ws_tx.send(Message::Ping(Bytes::new())).await?;
                }
                r = tokio::time::timeout(IDLE_TIMEOUT, rd.read_buf(&mut buf)) => {
                    // read_buf 已内部推进游标（len == 读取字节数），此处 zero-op，
                    // 绝不手动 advance_mut——双重推进会外泄未初始化字节。
                    match r {
                        Ok(Ok(n)) if n > 0 => {}
                        Ok(Ok(_)) => {
                            // EOF：浏览器侧断开，通知 worker 后结束
                            ws_tx
                                .send(Message::Close {
                                    code: CloseCode::Iana(1000),
                                    reason: String::new(),
                                })
                                .await?;
                            return Ok::<(), anyhow::Error>(());
                        }
                        Ok(Err(e)) => {
                            return Err(anyhow::Error::new(e).context("read failed"));
                        }
                        Err(_) => {
                            // 假死：浏览器侧超时，通知 worker 后结束
                            ws_tx
                                .send(Message::Close {
                                    code: CloseCode::Iana(1000),
                                    reason: String::new(),
                                })
                                .await?;
                            return Ok::<(), anyhow::Error>(());
                        }
                    }
                }
            }
        }
    });

    // [B] 下行：worker 的 Return 原始字节 / Error 错误反馈 → 经 channel 交给 writer
    let mut download_handle = tokio::spawn(async move {
        // 用于追踪是否已经给浏览器转发过 101 升级响应
        let mut has_upgraded = false;

        while let Some(msg) = ws_rx.next().await {
            match msg {
                Ok(Message::Binary(enc)) => {
                    let plain = decode_chunk(&enc, algo.compressor, algo.aead, &key16, &key32)
                        .context("decrypt tunnel message failed")?;
                    let tunnel = WsTunnelMsg::deserialize(&plain)
                        .context("deserialize tunnel message failed")?;

                    match tunnel {
                        // 101 响应头 / 数据帧 / close 帧：零解析直写（走 data 通道）
                        WsTunnelMsg::Return(raw) => {
                            has_upgraded = true;
                            data_tx
                                .send(Bytes::from(raw))
                                .await
                                .map_err(|_| anyhow!("writer channel closed"))?;
                        }
                        // 服务端明确抛出的内部错误
                        WsTunnelMsg::Error(err_msg) => {
                            crate::error!("received explicit error from worker: {err_msg}");

                            // 错误反馈走 ctrl 通道，确保优先于积压业务数据写出
                            if has_upgraded {
                                // 已建立 WS 连接：发送标准 WS Close 帧通知浏览器
                                // 1011: Internal Server Error
                                let close_frame =
                                    WsFrame::new_close(Some((1011, Some(err_msg.clone()))), None);
                                let _ = download_ctrl.send(Bytes::from(close_frame.to_bytes())).await;
                            } else {
                                // 尚未升级成功：请求在 Worker 内直接失败，返回 HTTP 错误
                                // （否则浏览器会卡死或者报 WS 协议违规）
                                let resp = format!(
                                    "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nProxy WS Error: {}\n",
                                    err_msg
                                );
                                let _ = download_ctrl.send(Bytes::from(resp.into_bytes())).await;
                            }
                            bail!("Worker upstream failed: {}", err_msg);
                        }
                        _ => {}
                    }
                }
                Ok(Message::Close { .. }) => {
                    return Ok::<(), anyhow::Error>(());
                }
                Ok(_) => {}
                Err(e) => {
                    crate::error!("tunnel read error: {e}");
                    return Err(e).context("ws tunnel read failed");
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    });

    // ---------- 收尾：谁先结束，谁触发另一方取消，writer 最后排空善后 ----------
    // upload / download 任一方向结束后，先 abort 另一方向（其持有的 sender 随任务
    // 结束被 drop）；随后不直接 abort writer，而是 await 它把队列中已入队但尚未
    // 落盘的字节（close 帧 / 错误页）写完再自然退出，避免"半截关闭"丢字节。
    let which;
    tokio::select! {
        res = &mut writer_handle => {
            which = "writer";
            log_task_result("writer", res);
            upload_handle.abort();
            download_handle.abort();
        }
        res = &mut upload_handle => {
            which = "upload";
            log_task_result("upload", res);
            download_handle.abort();
        }
        res = &mut download_handle => {
            which = "download";
            log_task_result("download", res);
            upload_handle.abort();
        }
    }

    if which != "upload" {
        log_task_result("upload", upload_handle.await);
    }
    if which != "download" {
        log_task_result("download", download_handle.await);
    }
    if which != "writer" {
        // upload / download 均已结束（正常或被取消），各自持有的 sender 全部 drop，
        // 两个 channel 关闭，writer 排空队列后自然返回；此处 await 把善后字节落盘。
        log_task_result("writer", writer_handle.await);
    }

    Ok(())
}

/// 唯一的浏览器侧写协程
async fn run_writer<W>(
    mut wr: WriteHalf<W>,
    mut ctrl_rx: mpsc::Receiver<Bytes>,
    mut data_rx: mpsc::Receiver<Bytes>,
) -> Result<()>
where
    W: AsyncWrite + Send + 'static,
{
    let mut ctrl_open = true;
    let mut data_open = true;

    loop {
        if !ctrl_open && !data_open {
            break;
        }

        tokio::select! {
            biased;
            maybe = ctrl_rx.recv(), if ctrl_open => {
                match maybe {
                    Some(bytes) => wr.write_all(&bytes).await.context("write ctrl frame to browser failed")?,
                    None => ctrl_open = false,
                }
            }
            maybe = data_rx.recv(), if data_open => {
                match maybe {
                    Some(bytes) => wr.write_all(&bytes).await.context("write data frame to browser failed")?,
                    None => data_open = false,
                }
            }
        }
    }
    let _ = wr.shutdown().await;
    Ok(())
}

fn log_task_result(name: &str, res: std::result::Result<Result<()>, JoinError>) {
    match res {
        Ok(Ok(())) => {}
        Ok(Err(e)) => crate::warn!("{name} task terminated with error: {e:#}"),
        Err(e) if e.is_cancelled() => {
        }
        Err(e) => crate::warn!("{name} task panicked: {e:#}"),
    }
}

/// 序列化 + 压缩加密为隧道消息负载
fn tunnel_payload(
    msg: &WsTunnelMsg,
    algo: ProxyAlgo,
    key16: &[u8],
    key32: &[u8],
) -> Result<Bytes> {
    let serialized = msg.serialize()?;
    Ok(Bytes::from(encode_chunk(
        &serialized,
        algo.compressor,
        algo.aead,
        key16,
        key32,
    )?))
}