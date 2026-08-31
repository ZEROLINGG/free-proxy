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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::{ProxyAead, ProxyAlgo, ProxyCompressor};


    #[test]
    fn test_make_frame_normal() {
        let payload = b"hello world";
        let frame = make_frame(payload);

        // 长度 = 4 (header) + 11 (payload) = 15
        assert_eq!(frame.len(), 15);
        // 大端编码的 11 (0x00, 0x00, 0x00, 0x0B)
        assert_eq!(&frame[0..4], &[0, 0, 0, 11]);
        assert_eq!(&frame[4..], payload);
    }

    #[test]
    fn test_make_frame_eos() {
        let frame = make_frame(&[]);
        assert_eq!(frame.len(), 4);
        assert_eq!(&frame[..], &[0, 0, 0, 0]);
    }


    #[test]
    fn test_cache_normal_pop() {
        let mut cache = FrameCache::new();
        let frame = make_frame(b"ping");

        cache.push(&frame);

        match cache.try_pop().unwrap() {
            Frame::Frame(data) => assert_eq!(&data[..], b"ping"),
            _ => panic!("Expected Frame"),
        }

        // 弹出后缓存应该为空，再 pop 返回 None
        assert_eq!(cache.try_pop().unwrap(), Frame::None);
    }

    #[test]
    fn test_cache_fragmentation_and_reassembly() {
        // 测试拆包场景：网络数据一段一段到达
        let mut cache = FrameCache::new();
        let payload = b"fragmented payload";
        let frame = make_frame(payload);

        cache.push(&frame[0..2]);
        assert_eq!(cache.try_pop().unwrap(), Frame::None);

        cache.push(&frame[2..7]);
        assert_eq!(cache.try_pop().unwrap(), Frame::None);

        cache.push(&frame[7..]);

        match cache.try_pop().unwrap() {
            Frame::Frame(data) => assert_eq!(&data[..], payload),
            _ => panic!("Expected Frame"),
        }
    }

    #[test]
    fn test_cache_multiple_frames_in_one_push() {
        let mut cache = FrameCache::new();

        let mut multi_frame_data = Vec::new();
        multi_frame_data.extend_from_slice(&make_frame(b"frame1"));
        multi_frame_data.extend_from_slice(&make_frame(b"frame2"));
        multi_frame_data.extend_from_slice(&make_frame(b"frame3"));

        cache.push(&multi_frame_data);

        let expected = [b"frame1", b"frame2", b"frame3"];
        for expected_payload in expected.iter() {
            match cache.try_pop().unwrap() {
                Frame::Frame(data) => assert_eq!(&data[..], *expected_payload),
                _ => panic!("Expected Frame"),
            }
        }

        assert_eq!(cache.try_pop().unwrap(), Frame::None);
    }

    #[test]
    fn test_cache_eos_behavior() {
        let mut cache = FrameCache::new();

        cache.push(&make_frame(b"last data"));
        cache.push(&make_frame(&[])); // EOS 帧

        match cache.try_pop().unwrap() {
            Frame::Frame(data) => assert_eq!(&data[..], b"last data"),
            _ => panic!("Expected Frame"),
        }

        assert_eq!(cache.try_pop().unwrap(), Frame::Eos);

        assert_eq!(cache.try_pop().unwrap(), Frame::Eos);

        cache.push(&make_frame(b"zombie data"));
        assert_eq!(cache.try_pop().unwrap(), Frame::Eos);
    }

    #[test]
    fn test_cache_oversized_frame_rejection() {
        let mut cache = FrameCache::new();

        let malicious_len: u32 = (MAX_FRAME_PAYLOAD + 1024) as u32;
        let mut malicious_frame = malicious_len.to_be_bytes().to_vec();
        malicious_frame.extend_from_slice(b"fake data");

        cache.push(&malicious_frame);

        let result = cache.try_pop();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds max"));
    }


    #[test]
    fn test_enc_frame_integration() {
        let key16 = b"1234567890123456";
        let key32 = b"12345678901234567890123456789012";
        let algo = ProxyAlgo::new(ProxyCompressor::None, ProxyAead::Ascon128);

        let payload = b"integration test for enc_frame";

        // enc_frame 执行加密并组装为帧
        let frame_bytes = enc_frame(payload, algo, key16, key32).unwrap();

        // 将产生的帧字节压入解析器
        let mut cache = FrameCache::new();
        cache.push(&frame_bytes);

        // 弹出帧
        let popped_frame = cache.try_pop().unwrap();
        match popped_frame {
            Frame::Frame(enc_data) => {
                // 确保数据已加密/长度发生变化（由于带有 tag/nonce，密文长度应大于原数据）
                assert!(enc_data.len() > payload.len());

                // 将帧的载荷拿去走逆向解密管线（测试和 algo.rs 闭环）
                let decoded = crate::algo::decode_chunk(&enc_data, algo.compressor, algo.aead, key16, key32).unwrap();
                assert_eq!(&decoded[..], payload);
            }
            _ => panic!("Expected enc_frame to generate a valid Frame"),
        }
    }
}