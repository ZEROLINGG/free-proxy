
use anyhow::{Context, Result, bail};
use bytes::{Buf, Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{CloseCode, Message, Upgrade};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{split, AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use crate::algo::{ProxyAlgo, decode_chunk, encode_chunk};
use crate::http::ReqHeader;
use crate::proxy::connection::IDLE_TIMEOUT;
use crate::proxy::core::{read_raw, RawRead};
use crate::tool::gen_auth_token;
use crate::ws::{WsCache, WsData, WsFrame, WsTunnelMsg};

use super::Shared;

const TUNNEL_PING_INTERVAL: Duration = Duration::from_secs(60);



/// 浏览器 WS 升级请求 → worker WS 隧道全生命周期。
///
/// `header` 为浏览器原始升级请求，`remaining` 为其后同包到达的超读字节
/// （可能已含首批 WS 帧），`is_https` 决定头帧末尾的 wss 标志位。
pub(crate) async fn handle_ws_proxy<S>(
    stream: &mut S,
    header: &ReqHeader,
    remaining: BytesMut,
    is_https: bool,
    shared: &Arc<Shared>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let algo = shared.algo();
    let key16 = shared.key16;
    let key32 = shared.key32;

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

    // ---------- 首消息：头帧 ----------
    ws.send(Message::Binary(tunnel_payload(
        &WsTunnelMsg::HeadFrame(head_frame.freeze().to_vec()),
        algo,
        &key16,
        &key32,
    )?))
        .await
        .context("send ws head frame failed")?;

    // ---------- 拆分 TCP 读写半 + WS 收发半，双任务并发 ----------
    let (mut rd, wr) = split(stream);
    let wr = Arc::new(Mutex::new(wr));
    let wr_upload = Arc::clone(&wr);
    let wr_download = Arc::clone(&wr);
    let (mut ws_tx, mut ws_rx) = ws.split();

    // [A] 上行：浏览器帧流 → 语义化 WsTunnelMsg
    let upload = async move {
        let mut buf = BytesMut::from(remaining);
        let mut cache = WsCache::new();

        loop {
            // 先榨干缓冲中的完整帧（不 await，避免 select 分支内的重入）
            loop {
                let Some((frame, consumed)) = WsFrame::parse(&buf)? else {
                    break;
                };
                buf.advance(consumed);

                if frame.is_ping() {
                    let pong = WsFrame::new_pong(frame.payload.clone(), None);

                    // 将 Mutex 锁的使用限定在最小块级作用域中。
                    // 锁被丢弃后，才能执行后续的 ws_tx.send().await 操作，彻底切断反压循环死锁。
                    {
                        let mut w = wr_upload.lock().await;
                        w.write_all(&pong.to_bytes()).await?;
                    }

                    let payload = tunnel_payload(
                        &WsTunnelMsg::Ping(frame.payload.clone()),
                        algo,
                        &key16,
                        &key32,
                    )?;
                    ws_tx.send(Message::Binary(payload)).await?;

                } else if frame.is_pong() {
                    // 浏览器应答我们的保活 Ping，忽略
                } else if frame.is_close() {
                    let close_info = frame.close_info();
                    let resp = WsFrame::new_close(close_info.clone(), None);

                    {
                        let mut w = wr_upload.lock().await;
                        w.write_all(&resp.to_bytes()).await?;
                    }

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
                            ws_tx.send(Message::Binary(payload)).await?;
                        }
                        Some(WsData::Binary(bin)) => {
                            let payload = tunnel_payload(&WsTunnelMsg::Binary(bin), algo, &key16, &key32)?;
                            ws_tx.send(Message::Binary(payload)).await?;
                        }
                        None => {}
                    }
                }
            }

            tokio::select! {
                _ = tokio::time::sleep(TUNNEL_PING_INTERVAL) => {
                    ws_tx.send(Message::Ping(Bytes::new())).await?;
                }
                r = read_raw(&mut rd, IDLE_TIMEOUT) => {
                    match r? {
                        RawRead::Data(data) => buf.extend_from_slice(&data),
                        RawRead::Eof | RawRead::TimedOut => {
                            // 浏览器侧断开/假死：通知 worker 后结束
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
    };

    // [B] 下行：worker 的 Return 原始字节 / Error 错误反馈 → 直写 TCP
    let download = async move {
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
                        // 101 响应头 / 数据帧 / close 帧：零解析直写
                        WsTunnelMsg::Return(raw) => {
                            has_upgraded = true;
                            // 注意：这里的 write_all.await 即使持有锁阻塞也没关系，
                            // 因为只有当浏览器读取缓冲满时它才会阻塞，这属于正常的反压(Backpressure)，不构成死锁环。
                            let mut w = wr_download.lock().await;
                            w.write_all(&raw).await?;
                        }
                        // 服务端明确抛出的内部错误
                        WsTunnelMsg::Error(err_msg) => {
                            crate::error!("received explicit error from worker: {err_msg}");
                            let mut w = wr_download.lock().await;

                            if has_upgraded {
                                // 如果已经建立了 WS 连接，发送标准的 WS Close 帧通知浏览器
                                // 1011: Internal Server Error
                                let close_frame = WsFrame::new_close(Some((1011, Some(err_msg.clone()))), None);
                                let _ = w.write_all(&close_frame.to_bytes()).await;
                            } else {
                                // 如果还没升级成功，说明请求直接在 Worker 内失败了。
                                // 需要返回 HTTP 错误，否则浏览器会卡死或者报 WS 协议违规
                                let resp = format!(
                                    "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nProxy WS Error: {}\n",
                                    err_msg
                                );
                                let _ = w.write_all(resp.as_bytes()).await;
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
    };

    // 当任意一端（如断网、被服务端踢出、读取超时）正常退出或报错时，
    // select! 机制会自动 Drop 另一个未完成的任务，从而关闭底层的 TCP 连接，避免内存泄漏与僵尸进程。
    tokio::select! {
        res = upload => {
            if let Err(e) = res {
                crate::warn!("upload terminated with error: {e:#}");
            }
        }
        res = download => {
            if let Err(e) = res {
                crate::warn!("download terminated with error: {e:#}");
            }
        }
    }

    Ok(())
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
