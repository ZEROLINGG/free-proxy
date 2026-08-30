// lib/src/frames.rs
// 二进制帧协议（客户端 / server-rs 共享，无 tokio 依赖，可编译进 wasm32）：
//   帧 = [ u32 大端长度(4B) | 负载(长度字节) ]，帧间直接相连、无分隔符。
//   零长帧（长度 0，无负载）为 EOS 结束标记：客户端收到 EOS 才算流正常完成；
//   流 EOF 而未收到 EOS 视为截断（协议错误）。早段错误以非 2xx 状态码表达。
//   FrameCache 按需零拷贝切分（BytesMut 视图），无需逐帧复制。

use anyhow::{Result, bail};
use bytes::{Bytes, BytesMut};
use crate::algo::{encode_chunk, ProxyAlgo};

/// 帧头部长度：4 字节大端 u32
pub const FRAME_HEADER_LEN: usize = 4;
/// 单帧负载上限（防御：拒绝恶意超长帧头）
pub const MAX_FRAME_PAYLOAD: usize = 128 * 1024 * 1024;

/// 服务端：组装一帧（header + payload）。零长度负载即 EOS 结束标记帧。
pub fn make_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// 弹出结果：一帧数据 / EOS 结束标记 / 数据不足暂无帧
#[derive(Debug, PartialEq)]
pub enum Frame {
    /// 完整帧负载（原始加密字节，未解密；BytesMut 视图，零拷贝）
    Frame(BytesMut),
    /// 零长帧：帧流正常结束
    Eos,
    /// 数据不足，需继续 push 更多字节
    None,
}

/// 客户端：流式帧解析器（兼容帧头/负载跨网络分块，支持单块多帧）。
/// push 追加字节（EOS 后忽略新数据）；try_pop 弹出下一帧。
/// 协议错误（超长帧）在 try_pop 时点报出；收到 EOS 后 try_pop 恒返回 Eos。
pub struct FrameCache {
    buffer: BytesMut,
    is_eos: bool,
}

impl FrameCache {
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::with_capacity(8192),
            is_eos: false,
        }
    }

    /// 追加字节；收到 EOS 后新数据被忽略
    pub fn push(&mut self, data: &[u8]) {
        if !self.is_eos {
            self.buffer.extend_from_slice(data);
        }
    }

    /// 尝试弹出一个帧；超长帧返回 Err（解析器作废，调用方须丢弃）
    pub fn try_pop(&mut self) -> Result<Frame> {
        if self.is_eos {
            return Ok(Frame::Eos);
        }

        if self.buffer.len() < FRAME_HEADER_LEN {
            return Ok(Frame::None);
        }

        let mut len_bytes = [0u8; FRAME_HEADER_LEN];
        len_bytes.copy_from_slice(&self.buffer[..FRAME_HEADER_LEN]);
        let frame_len = u32::from_be_bytes(len_bytes) as usize;

        if frame_len == 0 {
            self.is_eos = true;
            self.buffer.clear();
            return Ok(Frame::Eos);
        }

        if frame_len > MAX_FRAME_PAYLOAD {
            bail!(
                "frame payload {frame_len} exceeds max {MAX_FRAME_PAYLOAD}"
            );
        }

        let total_len = FRAME_HEADER_LEN + frame_len;
        if self.buffer.len() < total_len {
            return Ok(Frame::None); // 负载未收全，等待更多数据
        }

        // 丢弃帧头，零拷贝切出负载视图
        let _ = self.buffer.split_to(FRAME_HEADER_LEN);
        let frame_data = self.buffer.split_to(frame_len);

        Ok(Frame::Frame(frame_data))
    }
}

impl Default for FrameCache {
    fn default() -> Self {
        Self::new()
    }
}

pub fn enc_frame(data: &[u8], algo: ProxyAlgo, key16: &[u8], key32: &[u8]) -> std::io::Result<Bytes> {
    let enc = encode_chunk(data, algo.compressor, algo.aead, key16, key32)
        .map_err(|e| std::io::Error::other(format!("encode failed: {e}")))?;
    Ok(Bytes::from(make_frame(&enc)))
}
