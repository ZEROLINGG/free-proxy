// lib/src/frames.rs
// 二进制帧协议（客户端 / server-rs 共享，无 tokio 依赖，可编译进 wasm32）：
//   帧 = [ u32 大端长度(4B) | 负载(长度字节) ]，帧间直接相连、无分隔符。
//   零长帧（长度 0，无负载）为 EOS 结束标记：客户端收到 EOS 才算流正常完成；
//   流 EOF 而未收到 EOS 视为截断（协议错误）。早段错误以非 2xx 状态码表达。
//   FrameCache 按需零拷贝切分（BytesMut 视图），无需逐帧复制。

use anyhow::{Result, bail};
use bytes::BytesMut;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(parser: &mut FrameCache) -> Result<Vec<BytesMut>> {
        let mut out = Vec::new();
        loop {
            match parser.try_pop()? {
                Frame::Frame(f) => out.push(f),
                Frame::Eos => return Ok(out),
                Frame::None => return Ok(out),
            }
        }
    }

    /// make_frame → push/try_pop 往返字节一致（结尾带 EOS）
    #[test]
    fn test_round_trip() {
        let payloads: Vec<Vec<u8>> = vec![
            b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n".to_vec(),
            vec![0u8, 1, 2, 3, 255],
            vec![7u8; 5000],
        ];
        let mut wire = Vec::new();
        for p in &payloads {
            wire.extend_from_slice(&make_frame(p));
        }
        wire.extend_from_slice(&make_frame(b"")); // EOS

        let mut parser = FrameCache::new();
        parser.push(&wire);
        let out = drain(&mut parser).unwrap();
        assert_eq!(out.len(), payloads.len());
        for (f, p) in out.iter().zip(payloads.iter()) {
            assert_eq!(&f[..], p.as_slice());
        }
    }

    /// 头部（4 字节）被拆成多个 chunk
    #[test]
    fn test_header_split_across_chunks() {
        let frame = make_frame(b"hello world");
        let (a, b) = frame.split_at(2);

        let mut parser = FrameCache::new();
        parser.push(a);
        assert!(matches!(parser.try_pop().unwrap(), Frame::None));
        parser.push(b);
        match parser.try_pop().unwrap() {
            Frame::Frame(f) => assert_eq!(&f[..], b"hello world"),
            other => panic!("expected Frame, got {other:?}"),
        }
    }

    /// 负载被拆成多个 chunk
    #[test]
    fn test_payload_split_across_chunks() {
        let frame = make_frame(b"hello world");
        let (a, b) = frame.split_at(6);

        let mut parser = FrameCache::new();
        parser.push(a);
        assert!(matches!(parser.try_pop().unwrap(), Frame::None));
        parser.push(b);
        match parser.try_pop().unwrap() {
            Frame::Frame(f) => assert_eq!(&f[..], b"hello world"),
            other => panic!("expected Frame, got {other:?}"),
        }
    }

    /// 单 chunk 含多帧（服务端连续产出的场景）
    #[test]
    fn test_multiple_frames_in_one_chunk() {
        let frames: [&[u8]; 3] = [b"first", b"second", b"third"];
        let mut wire = Vec::new();
        for p in frames {
            wire.extend_from_slice(&make_frame(p));
        }

        let mut parser = FrameCache::new();
        parser.push(&wire);
        let out = drain(&mut parser).unwrap();
        let got: Vec<&[u8]> = out.iter().map(|f| &f[..]).collect();
        assert_eq!(got, frames.to_vec());
    }

    /// 空 chunk 不产出且不报错
    #[test]
    fn test_empty_chunk_noop() {
        let mut parser = FrameCache::new();
        parser.push(b"");
        assert!(matches!(parser.try_pop().unwrap(), Frame::None));
    }

    /// 零长帧 → Eos（正常结束标记，不是错误）
    #[test]
    fn test_zero_length_frame_is_eos() {
        let mut parser = FrameCache::new();
        parser.push(&[0u8; FRAME_HEADER_LEN]);
        assert!(matches!(parser.try_pop().unwrap(), Frame::Eos));
    }

    /// EOS 后：push 被忽略，try_pop 恒返回 Eos
    #[test]
    fn test_eos_is_sticky() {
        let mut wire = Vec::new();
        wire.extend_from_slice(&make_frame(b"data"));
        wire.extend_from_slice(&make_frame(b"")); // EOS
        wire.extend_from_slice(&make_frame(b"more")); // 应被忽略

        let mut parser = FrameCache::new();
        parser.push(&wire);
        assert!(matches!(parser.try_pop().unwrap(), Frame::Frame(_)));
        assert!(matches!(parser.try_pop().unwrap(), Frame::Eos));
        assert!(matches!(parser.try_pop().unwrap(), Frame::Eos));
    }

    /// 超长帧（伪造长度）被拒绝
    #[test]
    fn test_oversize_frame_rejected() {
        let mut wire = Vec::new();
        wire.extend_from_slice(&(MAX_FRAME_PAYLOAD as u32 + 1).to_be_bytes());
        wire.extend_from_slice(b"x");

        let mut parser = FrameCache::new();
        parser.push(&wire);
        assert!(parser.try_pop().is_err());
    }

    /// 头部伪造超大长度 → 头部齐备后立即拒绝，不等待完整负载
    #[test]
    fn test_oversize_header_rejected_before_payload() {
        let mut parser = FrameCache::new();
        let mut wire = Vec::new();
        wire.extend_from_slice(&(MAX_FRAME_PAYLOAD as u32 + 1).to_be_bytes());
        wire.extend_from_slice(&[0u8; 100]);

        parser.push(&wire);
        assert!(parser.try_pop().is_err());
        // bail 即终止解析，buffer 不会向"声称的超大长度"方向积累
        assert!(parser.buffer.len() <= wire.len());
    }

    /// 帧顺序保持
    #[test]
    fn test_frame_order_preserved() {
        let payloads: Vec<Vec<u8>> = (0..20).map(|i| vec![i as u8; 16]).collect();
        let mut wire = Vec::new();
        for p in &payloads {
            wire.extend_from_slice(&make_frame(p));
        }

        let mut parser = FrameCache::new();
        for part in wire.chunks(7) {
            parser.push(part);
        }
        let out = drain(&mut parser).unwrap();
        assert_eq!(out.len(), payloads.len());
        for (f, p) in out.iter().zip(payloads.iter()) {
            assert_eq!(&f[..], p.as_slice());
        }
    }
}
