// lib/src/algo.rs
// 客户端（lib::proxy）与服务端（free-proxy worker）共享的算法分发层：
//   - 压缩 / AEAD 枚举及「压缩 + 加密」编解码管线，两端同一份实现；
//   - URL 路径契约 /api/{version}/{target} 的生成（客户端）与解析（服务端）
//     由同一映射承担，任何一侧改动都会被往返单测拦截。
//
// 仅依赖无条件编译模块（aead/compress/tool/base），可同时编译进
// native 客户端与 wasm32 服务端。

use anyhow::{Result, anyhow};

use crate::aead::{Ascon128, ChaCha20Poly1305, Cipher};
use crate::compress::{Compressor, Gzip, Lz4, Zstd};

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
    ChaCha20Poly1305,
    /// NIST SP 800-232 Ascon-AEAD128（密钥/随机 nonce/tag 均为 16 字节）
    Ascon128,
}

impl std::str::FromStr for ProxyAead {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().replace('-', "_").as_str() {
            "chacha20poly1305" | "chacha20_poly1305" => Ok(Self::ChaCha20Poly1305),
            "ascon128" => Ok(Self::Ascon128),
            _ => Err(anyhow!("invalid aead: {s}")),
        }
    }
}

impl ProxyAead {
    pub const ALL: [ProxyAead; 2] = [Self::ChaCha20Poly1305, Self::Ascon128];

    /// 规范名称（与 `FromStr` 输入格式一致，供 UI/序列化使用）。
    /// `name()` → `from_str()` 的往返由 `test_name_roundtrip` 锁定。
    pub fn name(self) -> &'static str {
        match self {
            Self::ChaCha20Poly1305 => "chacha20poly1305",
            Self::Ascon128 => "ascon128",
        }
    }

    /// URL 路径中的 target 段（客户端生成 /api/.../{target}）
    pub fn target(self) -> &'static str {
        match self {
            Self::ChaCha20Poly1305 => "time",
            Self::Ascon128 => "get",
        }
    }

    /// 由 URL 路径 target 段反向解析（服务端使用）
    pub fn from_target(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "time" => Ok(Self::ChaCha20Poly1305),
            "get" => Ok(Self::Ascon128),
            _ => Err(anyhow!("invalid target: {s}")),
        }
    }

    pub fn encrypt(self, data: &[u8], key16: &[u8], key32: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::ChaCha20Poly1305 => ChaCha20Poly1305::encrypt(data, key32),
            Self::Ascon128 => Ascon128::encrypt(data, key16),
        }
    }

    pub fn decrypt(self, data: &[u8], key16: &[u8], key32: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::ChaCha20Poly1305 => ChaCha20Poly1305::decrypt(data, key32),
            Self::Ascon128 => Ascon128::decrypt(data, key16),
        }
    }
}

impl Default for ProxyAead {
    fn default() -> Self {
        Self::Ascon128
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

