// lib/src/algo.rs
// 客户端（lib::proxy）与服务端（free-proxy worker）共享的算法分发层：
//   - 压缩 / AEAD 枚举及「压缩 + 加密」编解码管线，两端同一份实现；
//   - URL 路径契约 /api/{version}/{target} 的生成（客户端）与解析（服务端）
//     由同一映射承担，任何一侧改动都会被往返单测拦截。
//
// 仅依赖无条件编译模块（aead/compress/tool/base），可同时编译进
// native 客户端与 wasm32 服务端。

use anyhow::{Result, anyhow};

use crate::aead::{
    Aes128Gcm, Aes128GcmSiv, Aes256Gcm, Aes256GcmSiv, ChaCha20Poly1305, Cipher, XChaCha20Poly1305,
};
use crate::compress::{Compressor, Gzip, Lz4, Zstd};
use crate::tool::xor_obfuscate;

/// 压缩算法（与 server-rs 的 URL version 参数映射，见下方 `version()`）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyCompressor {
    Zstd,
    Gzip,
    Lz4,
    /// 不压缩
    None,
}

impl std::str::FromStr for ProxyCompressor {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "zstd" => Ok(Self::Zstd),
            "gzip" => Ok(Self::Gzip),
            "lz4" => Ok(Self::Lz4),
            "none" => Ok(Self::None),
            _ => Err(anyhow!("invalid compressor: {s}")),
        }
    }
}

impl ProxyCompressor {
    pub const ALL: [ProxyCompressor; 4] = [Self::Zstd, Self::Gzip, Self::Lz4, Self::None];

    /// 规范名称（与 `FromStr` 输入格式一致，供 UI/序列化使用）。
    /// `name()` → `from_str()` 的往返由 `test_name_roundtrip` 锁定。
    pub fn name(self) -> &'static str {
        match self {
            Self::Zstd => "zstd",
            Self::Gzip => "gzip",
            Self::Lz4 => "lz4",
            Self::None => "none",
        }
    }

    /// URL 路径中的 version 段（客户端生成 /api/v1..v4）
    pub fn version(self) -> &'static str {
        match self {
            Self::Zstd => "v1",
            Self::Gzip => "v2",
            Self::Lz4 => "v3",
            Self::None => "v4",
        }
    }

    /// 由 URL 路径 version 段反向解析（服务端使用）
    pub fn from_version(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "v1" => Ok(Self::Zstd),
            "v2" => Ok(Self::Gzip),
            "v3" => Ok(Self::Lz4),
            "v4" => Ok(Self::None),
            _ => Err(anyhow!("invalid version: {s}")),
        }
    }

    fn compress(self, data: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Zstd => Zstd::compress(data),
            Self::Gzip => Gzip::compress(data),
            Self::Lz4 => Lz4::compress(data),
            Self::None => Ok(data.to_vec()),
        }
    }

    pub(crate) fn decompress(self, data: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Zstd => Zstd::decompress(data),
            Self::Gzip => Gzip::decompress(data),
            Self::Lz4 => Lz4::decompress(data),
            Self::None => Ok(data.to_vec()),
        }
    }
}

impl Default for ProxyCompressor {
    fn default() -> Self {
        Self::Lz4
    }
}

/// AEAD 加密算法（与 server-rs 的 URL target 参数映射，见下方 `target()`）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyAead {
    Aes128Gcm,
    Aes256Gcm,
    Aes128GcmSiv,
    Aes256GcmSiv,
    ChaCha20Poly1305,
    XChaCha20Poly1305,
    /// 不加密，仅做 XOR 混淆（XOR 自反，加解密同一表达式）
    None,
}

impl std::str::FromStr for ProxyAead {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().replace('-', "_").as_str() {
            "aes128gcm" | "aes_128_gcm" => Ok(Self::Aes128Gcm),
            "aes256gcm" | "aes_256_gcm" => Ok(Self::Aes256Gcm),
            "aes128gcmsiv" | "aes_128_gcm_siv" => Ok(Self::Aes128GcmSiv),
            "aes256gcmsiv" | "aes_256_gcm_siv" => Ok(Self::Aes256GcmSiv),
            "chacha20poly1305" | "chacha20_poly1305" => Ok(Self::ChaCha20Poly1305),
            "xchacha20poly1305" | "xchacha20_poly1305" => Ok(Self::XChaCha20Poly1305),
            "none" => Ok(Self::None),
            _ => Err(anyhow!("invalid aead: {s}")),
        }
    }
}

impl ProxyAead {
    pub const ALL: [ProxyAead; 7] = [
        Self::Aes128Gcm,
        Self::Aes256Gcm,
        Self::Aes128GcmSiv,
        Self::Aes256GcmSiv,
        Self::ChaCha20Poly1305,
        Self::XChaCha20Poly1305,
        Self::None,
    ];

    /// 规范名称（与 `FromStr` 输入格式一致，供 UI/序列化使用）。
    /// `name()` → `from_str()` 的往返由 `test_name_roundtrip` 锁定。
    pub fn name(self) -> &'static str {
        match self {
            Self::Aes128Gcm => "aes128gcm",
            Self::Aes256Gcm => "aes256gcm",
            Self::Aes128GcmSiv => "aes128gcmsiv",
            Self::Aes256GcmSiv => "aes256gcmsiv",
            Self::ChaCha20Poly1305 => "chacha20poly1305",
            Self::XChaCha20Poly1305 => "xchacha20poly1305",
            Self::None => "none",
        }
    }

    /// URL 路径中的 target 段（客户端生成 /api/.../{target}）
    pub fn target(self) -> &'static str {
        match self {
            Self::Aes128Gcm => "auth",
            Self::Aes256Gcm => "login",
            Self::Aes128GcmSiv => "info",
            Self::Aes256GcmSiv => "logout",
            Self::ChaCha20Poly1305 => "time",
            Self::XChaCha20Poly1305 => "log",
            Self::None => "get",
        }
    }

    /// 由 URL 路径 target 段反向解析（服务端使用）
    pub fn from_target(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "auth" => Ok(Self::Aes128Gcm),
            "login" => Ok(Self::Aes256Gcm),
            "info" => Ok(Self::Aes128GcmSiv),
            "logout" => Ok(Self::Aes256GcmSiv),
            "time" => Ok(Self::ChaCha20Poly1305),
            "log" => Ok(Self::XChaCha20Poly1305),
            "get" => Ok(Self::None),
            _ => Err(anyhow!("invalid target: {s}")),
        }
    }

    fn encrypt(self, data: &[u8], key16: &[u8], key32: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Aes128Gcm => Aes128Gcm::encrypt(data, key16),
            Self::Aes256Gcm => Aes256Gcm::encrypt(data, key32),
            Self::Aes128GcmSiv => Aes128GcmSiv::encrypt(data, key16),
            Self::Aes256GcmSiv => Aes256GcmSiv::encrypt(data, key32),
            Self::ChaCha20Poly1305 => ChaCha20Poly1305::encrypt(data, key32),
            Self::XChaCha20Poly1305 => XChaCha20Poly1305::encrypt(data, key32),
            Self::None => Ok(xor_obfuscate(data, key16, key32)),
        }
    }

    fn decrypt(self, data: &[u8], key16: &[u8], key32: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Aes128Gcm => Aes128Gcm::decrypt(data, key16),
            Self::Aes256Gcm => Aes256Gcm::decrypt(data, key32),
            Self::Aes128GcmSiv => Aes128GcmSiv::decrypt(data, key16),
            Self::Aes256GcmSiv => Aes256GcmSiv::decrypt(data, key32),
            Self::ChaCha20Poly1305 => ChaCha20Poly1305::decrypt(data, key32),
            Self::XChaCha20Poly1305 => XChaCha20Poly1305::decrypt(data, key32),
            Self::None => Ok(xor_obfuscate(data, key16, key32)),
        }
    }
}

impl Default for ProxyAead {
    fn default() -> Self {
        Self::Aes128Gcm
    }
}

/// 当前启用的算法组合（每次请求时读取，支持运行中热切换）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProxyAlgo {
    pub compressor: ProxyCompressor,
    pub aead: ProxyAead,
}

impl ProxyAlgo {
    pub fn new(compressor: ProxyCompressor, aead: ProxyAead) -> Self {
        Self { compressor, aead }
    }

    /// 生成 worker 请求路径 /api/{version}/{target}
    pub fn api_path(self) -> String {
        format!("/api/{}/{}", self.compressor.version(), self.aead.target())
    }

    /// 生成 worker WebSocket 路径 /ws/{version}/{target}
    pub fn ws_path(self) -> String {
        format!("/ws/{}/{}", self.compressor.version(), self.aead.target())
    }
}

/// 编码管线：先压缩后加密（客户端上行 / 服务端响应块共用）。
/// 与服务端逻辑逐字节对称，`None` 组合退化为 XOR 混淆。
pub fn encode_chunk(
    raw: &[u8],
    compressor: ProxyCompressor,
    aead: ProxyAead,
    key16: &[u8],
    key32: &[u8],
) -> Result<Vec<u8>> {
    let compressed = compressor.compress(raw)?;
    aead.encrypt(&compressed, key16, key32)
}

/// 解码管线：先解密后解压（encode_chunk 的严格逆操作）。
pub fn decode_chunk(
    data: &[u8],
    compressor: ProxyCompressor,
    aead: ProxyAead,
    key16: &[u8],
    key32: &[u8],
) -> Result<Vec<u8>> {
    let decrypted = aead.decrypt(data, key16, key32)?;
    compressor.decompress(&decrypted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_path_mapping() {
        assert_eq!(
            ProxyAlgo::new(ProxyCompressor::Zstd, ProxyAead::Aes128Gcm).api_path(),
            "/api/v1/auth"
        );
        assert_eq!(
            ProxyAlgo::new(ProxyCompressor::Gzip, ProxyAead::Aes256Gcm).api_path(),
            "/api/v2/login"
        );
        assert_eq!(
            ProxyAlgo::new(ProxyCompressor::Lz4, ProxyAead::XChaCha20Poly1305).api_path(),
            "/api/v3/log"
        );
        assert_eq!(
            ProxyAlgo::new(ProxyCompressor::None, ProxyAead::None).api_path(),
            "/api/v4/get"
        );
    }

    #[test]
    fn test_ws_path_mapping() {
        assert_eq!(
            ProxyAlgo::new(ProxyCompressor::Zstd, ProxyAead::Aes128Gcm).ws_path(),
            "/ws/v1/auth"
        );
        assert_eq!(
            ProxyAlgo::new(ProxyCompressor::Gzip, ProxyAead::Aes256Gcm).ws_path(),
            "/ws/v2/login"
        );
        assert_eq!(
            ProxyAlgo::new(ProxyCompressor::Lz4, ProxyAead::XChaCha20Poly1305).ws_path(),
            "/ws/v3/log"
        );
        assert_eq!(
            ProxyAlgo::new(ProxyCompressor::None, ProxyAead::None).ws_path(),
            "/ws/v4/get"
        );
    }

    /// URL 路径契约往返：客户端生成的 version/target 必须能被服务端解析回原值。
    #[test]
    fn test_version_roundtrip() {
        for c in ProxyCompressor::ALL {
            assert_eq!(
                ProxyCompressor::from_version(c.version()).unwrap(),
                c,
                "{c:?}"
            );
        }
    }

    #[test]
    fn test_target_roundtrip() {
        for a in ProxyAead::ALL {
            assert_eq!(ProxyAead::from_target(a.target()).unwrap(), a, "{a:?}");
        }
    }

    /// 规范名称往返：`from_str(name())` 必须还原原值，
    /// 锁定 UI/序列化使用的名称与解析器保持一致。
    #[test]
    fn test_name_roundtrip() {
        use std::str::FromStr;
        for c in ProxyCompressor::ALL {
            assert_eq!(ProxyCompressor::from_str(c.name()).unwrap(), c, "{c:?}");
        }
        for a in ProxyAead::ALL {
            assert_eq!(ProxyAead::from_str(a.name()).unwrap(), a, "{a:?}");
        }
    }

    #[test]
    fn test_from_version_rejects_unknown() {
        assert!(ProxyCompressor::from_version("v9").is_err());
        assert!(ProxyCompressor::from_version("").is_err());
        assert!(ProxyCompressor::from_version("zstd").is_err());
    }

    #[test]
    fn test_from_target_rejects_unknown() {
        assert!(ProxyAead::from_target("admin").is_err());
        assert!(ProxyAead::from_target("").is_err());
        assert!(ProxyAead::from_target("aes128gcm").is_err());
    }

    /// 全组合（4 压缩 × 7 AEAD）encode/decode 往返，锁定两端管线对称。
    #[test]
    fn test_encode_decode_all_combos() {
        let key16 = [0x42u8; 16];
        let key32 = [0x7Eu8; 32];
        let payload = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n".to_vec();

        for c in ProxyCompressor::ALL {
            for a in ProxyAead::ALL {
                let enc = encode_chunk(&payload, c, a, &key16, &key32).unwrap();
                let dec = decode_chunk(&enc, c, a, &key16, &key32).unwrap();
                assert_eq!(dec, payload, "{c:?} + {a:?}");
            }
        }
    }

    #[test]
    fn test_xor_obfuscate_matches_server_vector() {
        // 硬编码向量：与服务端 encode_chunk 的 "None" aead 实现逐字节对齐。
        // 使用非平凡密钥与 >256 字节的 payload，覆盖 i*3 溢出 u8 的区间。
        let key16 = [0x42u8; 16];
        let key32 = [0x7Eu8; 32];
        let data: Vec<u8> = (0..300u32)
            .map(|i| (i.wrapping_mul(7) & 0xff) as u8)
            .collect();

        let out = xor_obfuscate(&data, &key16, &key32);

        // 自反：解密（同一表达式）必须还原原文
        assert_eq!(xor_obfuscate(&out, &key16, &key32), data);

        // 首尾抽样固定向量（防止实现被无意改动而两端静默失配）
        let mut expected = vec![0u8; 300];
        for (i, e) in expected.iter_mut().enumerate() {
            let k16 = key16[i % 16];
            let k32 = key32[i % 32];
            let c = i.wrapping_mul(3) as u8 % 163;
            *e = data[i]
                ^ (k16 ^ k32).wrapping_mul((k16 % 127).wrapping_add((k32 % 131).wrapping_add(c)));
        }
        assert_eq!(out, expected);
        assert_eq!(
            hex::encode(&out[..64]),
            "00b36609cca712dd986b4ef1247fbae53083d6d99c57e28d483bfe4174afca1560d30669ac07b27d38cbae11c49f1a45902376b9fc3782f9a46f02d5581bd6a1"
        );
    }

    #[test]
    fn test_none_roundtrip_via_algo() {
        let compressor = ProxyCompressor::None;
        let aead = ProxyAead::None;
        let payload = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n\x00".to_vec();
        let key16 = [0x42u8; 16];
        let key32 = [0x7Eu8; 32];

        let enc = encode_chunk(&payload, compressor, aead, &key16, &key32).unwrap();
        let raw = decode_chunk(&enc, compressor, aead, &key16, &key32).unwrap();
        assert_eq!(raw, payload);
    }

    /// 端到端管线：服务端 pack_frame(encode_chunk) → 帧流 → 客户端 FrameCache → decode_chunk
    /// 锁定"服务端打包 → 客户端解析"对称性（二进制帧流重构后的核心契约）。
    #[test]
    fn test_frame_wire_pipeline_all_combos() {
        use crate::frames::{Frame, FrameCache, make_frame};

        let key16 = [0x42u8; 16];
        let key32 = [0x7Eu8; 32];
        let chunks = [
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),
            b"1a\r\nhello world hello world hello world\r\n".to_vec(),
            b"0\r\n\r\n".to_vec(),
        ];

        for c in ProxyCompressor::ALL {
            for a in ProxyAead::ALL {
                // 服务端：逐块打包成一帧，帧与帧直接相连，末尾零长帧 = EOS
                let mut wire = Vec::new();
                for chunk in &chunks {
                    let enc = encode_chunk(chunk, c, a, &key16, &key32).unwrap();
                    wire.extend_from_slice(&make_frame(&enc));
                }
                wire.extend_from_slice(&make_frame(b"")); // EOS

                // 客户端：分块喂入（模拟网络分片）→ try_pop 收集帧 → 解密解压
                let mut parser = FrameCache::new();
                for part in wire.chunks(7) {
                    parser.push(part);
                }
                parser.push(b""); // 空块 no-op

                let mut out = Vec::new();
                let mut eos = false;
                loop {
                    match parser.try_pop().unwrap() {
                        Frame::Frame(f) => out.push(f),
                        Frame::None => break,
                        Frame::Eos => {
                            eos = true;
                            break;
                        }
                    }
                }
                assert!(eos, "{c:?} + {a:?}: missing EOS");

                assert_eq!(out.len(), chunks.len(), "{c:?} + {a:?}");
                for (enc, expect) in out.iter().zip(chunks.iter()) {
                    let raw = decode_chunk(enc, c, a, &key16, &key32).unwrap();
                    assert_eq!(&raw, expect, "{c:?} + {a:?}");
                }
            }
        }
    }

    /// 请求方向（流式上传）管线：客户端"头帧(head+https标志) → body 帧 → EOS"，
    /// 服务端按帧解析并还原出 head 与完整 body。锁定新协议契约。
    #[test]
    fn test_upload_pipeline_head_frame_then_body_frames() {
        use crate::frames::{Frame, FrameCache, make_frame};

        let key16 = [0x42u8; 16];
        let key32 = [0x7Eu8; 32];

        // 浏览器原始请求：头 + body（Content-Length: 11）
        let head = b"POST /upload HTTP/1.1\r\nHost: example.com\r\nContent-Length: 11\r\n\r\n";
        let body = b"hello world";

        for c in ProxyCompressor::ALL {
            for a in ProxyAead::ALL {
                // 客户端打包：头帧 = head + https 标志位（末尾字节）
                let mut head_frame = head.to_vec();
                head_frame.push(1u8); // https
                let mut wire = Vec::new();
                wire.extend_from_slice(&make_frame(
                    &encode_chunk(&head_frame, c, a, &key16, &key32).unwrap(),
                ));
                for chunk in body.chunks(4) {
                    wire.extend_from_slice(&make_frame(
                        &encode_chunk(chunk, c, a, &key16, &key32).unwrap(),
                    ));
                }
                wire.extend_from_slice(&make_frame(b"")); // EOS = 请求体结束

                // 服务端解析（模拟网络分片）
                let mut parser = FrameCache::new();
                for part in wire.chunks(7) {
                    parser.push(part);
                }

                // 第一帧 = 头帧
                let first = match parser.try_pop().unwrap() {
                    Frame::Frame(f) => f,
                    other => panic!("{c:?} + {a:?}: expected head frame, got {other:?}"),
                };
                let decoded_head = decode_chunk(&first, c, a, &key16, &key32).unwrap();
                assert_eq!(&decoded_head[..decoded_head.len() - 1], head, "{c:?} + {a:?}");
                assert_eq!(decoded_head[decoded_head.len() - 1], 1, "{c:?} + {a:?}");

                // 其余帧 = body 帧，EOS 收尾
                let mut assembled = Vec::new();
                let mut eos = false;
                loop {
                    match parser.try_pop().unwrap() {
                        Frame::Frame(f) => {
                            assembled.extend_from_slice(&decode_chunk(&f, c, a, &key16, &key32).unwrap());
                        }
                        Frame::None => break,
                        Frame::Eos => {
                            eos = true;
                            break;
                        }
                    }
                }
                assert!(eos, "{c:?} + {a:?}: missing EOS");
                assert_eq!(assembled, body, "{c:?} + {a:?}");
            }
        }
    }
}
