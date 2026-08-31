// lib/src/proxy/body.rs
//
// 纯请求体边界解析：不依赖任何网络 IO / tokio 运行时，可独立单测。
// 职责：根据请求头判定 body 范围（Content-Length / chunked / 无），
// 并在原始字节流中精确解析 chunked 分帧边界（解码出纯负载，分帧字节丢弃）。

use anyhow::{anyhow, bail, Context, Result};
use bytes::{Buf, BytesMut};

/// chunk 分块行 / trailer 行长度上限（防御畸形输入）
const CHUNK_LINE_MAX: usize = 64 * 1024;

/// 请求体范围判定：驱动"何时完成请求体（EOS）"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BodyExtent {
    /// 无请求体（含 Content-Length: 0）
    NoBody,
    /// Content-Length 计数的定长 body
    ContentLength(u64),
    /// Transfer-Encoding: chunked（解码：只转发 chunk 数据负载）
    Chunked,
}

/// 从已解析请求头判定请求体范围（遵循 RFC 9112）：
/// Transfer-Encoding 覆盖 Content-Length；仅 chunked 定义分帧。
pub(super) fn body_extent(headers: &[(String, String)]) -> Result<BodyExtent> {
    let mut cl: Option<u64> = None;
    let mut chunked = false;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-length") {
            let v = v.trim();
            let n: u64 = v
                .parse()
                .with_context(|| format!("invalid content-length: {v:?}"))?;
            if cl.is_some() {
                bail!("duplicate content-length headers");
            }
            cl = Some(n);
        } else if k.eq_ignore_ascii_case("transfer-encoding")
            && v.split(',').any(|t| t.trim().eq_ignore_ascii_case("chunked"))
        {
            chunked = true;
        }
    }
    if chunked {
        return Ok(BodyExtent::Chunked);
    }
    Ok(match cl {
        Some(0) | None => BodyExtent::NoBody,
        Some(n) => BodyExtent::ContentLength(n),
    })
}

/// 拆分头部超读字节：返回 (属于本请求 body 的前缀长度, 超出部分长度)。
/// 无 body 请求的超读字节全部是下一请求（keep-alive 流水线），须归还 parser。
pub(super) fn split_body_prefix(remaining: &[u8], extent: &BodyExtent) -> (usize, usize) {
    match extent {
        BodyExtent::NoBody => (0, remaining.len()),
        BodyExtent::ContentLength(n) => {
            let take = (remaining.len() as u64).min(*n) as usize;
            (take, remaining.len() - take)
        }
        BodyExtent::Chunked => (remaining.len(), 0),
    }
}

/// 一次性 push 的处理结果
pub(super) struct PumpPushed {
    /// 应转发给 worker 的负载字节（chunked 模式下已解码：仅 chunk 数据，无分帧）
    pub payload: BytesMut,
    /// Some(take)：请求体已结束，本段前 take 字节属于 body（其后为下一请求）；
    /// None：尚未结束。
    pub end_at: Option<usize>,
}

/// 泵送阶段 body 进度跟踪：Content-Length 计数 或 chunked 分帧解码。
pub(super) enum PumpTracker {
    ContentLength(u64),
    Chunked(ChunkedDecoder),
}

impl PumpTracker {
    pub(super) fn new(extent: &BodyExtent) -> Option<Self> {
        match extent {
            BodyExtent::NoBody => None,
            BodyExtent::ContentLength(n) => Some(Self::ContentLength(*n)),
            BodyExtent::Chunked => Some(Self::Chunked(ChunkedDecoder::new())),
        }
    }

    /// 处理一段原始字节：返回应转发的负载与请求体是否结束。
    pub(super) fn push(&mut self, data: &[u8]) -> Result<PumpPushed> {
        match self {
            Self::ContentLength(left) => {
                let take = (*left as usize).min(data.len());
                *left -= take as u64;
                let end_at = if *left == 0 { Some(take) } else { None };
                let payload = BytesMut::from(&data[..take]);
                Ok(PumpPushed { payload, end_at })
            }
            Self::Chunked(decoder) => decoder.feed(data),
        }
    }
}

/// chunked 分帧解码状态
enum ChunkState {
    /// 等待一行 chunk-size（可带扩展）
    ExpectSizeLine,
    /// 读取 chunk 数据（按声明长度计数，数据进 out）
    SkipData,
    /// 等待 chunk 数据后的 CRLF
    ExpectChunkCrlf,
    /// 0 长度块之后：逐行扫 trailer 区，首个空行即结束
    TailAfterZero,
}

/// chunked 原始字节流的分帧解码状态机。
/// 按帧边界精确识别：size 行 / 数据 / CRLF / 0 结尾 / trailer，
/// 只把 chunk **数据负载**拷入 out（分帧字节全部丢弃。负载内出现
/// "0\r\n\r\n" 之类的序列不会干扰帧边界判定）。
/// 结束点（含末尾 "0\r\n\r\n"，兼容 trailer 区）之后的字节属下一请求。
pub(super) struct ChunkedDecoder {
    state: ChunkState,
    buf: BytesMut,
    /// 已解码的 chunk 数据负载（累积后整段取走转发给 worker）
    out: BytesMut,
    /// 已消费的原始输入字节数（含分帧，用于定位请求体结束时的相对偏移）
    consumed: u64,
    skip_remaining: u64,
    done: bool,
}

impl ChunkedDecoder {
    pub(super) fn new() -> Self {
        Self {
            state: ChunkState::ExpectSizeLine,
            buf: BytesMut::with_capacity(256),
            out: BytesMut::with_capacity(1024),
            consumed: 0,
            skip_remaining: 0,
            done: false,
        }
    }

    /// 喂入一段原始字节（含完整分帧）。返回应转发的解码负载，以及请求体
    /// 是否已结束：end_at 为 Some(take) 表示请求体在本段内结束于 take 处
    /// （本段 take.. 之后为下一请求字节，调用方归还 parser）。
    pub(super) fn feed(&mut self, chunk: &[u8]) -> Result<PumpPushed> {
        if self.done {
            return Ok(PumpPushed {
                payload: BytesMut::new(),
                end_at: Some(0),
            });
        }
        let base = self.consumed;
        self.buf.extend_from_slice(chunk);
        let mut end_at: Option<usize> = None;

        loop {
            match self.state {
                ChunkState::ExpectSizeLine => match find_bytes(&self.buf, b"\r\n") {
                    Some(p) => {
                        let size = parse_chunk_size(&self.buf[..p])?;
                        let line_len = p + 2;
                        self.buf.advance(line_len);
                        self.consumed += line_len as u64;
                        self.state = if size == 0 {
                            ChunkState::TailAfterZero
                        } else {
                            self.skip_remaining = size;
                            ChunkState::SkipData
                        };
                    }
                    None => {
                        if self.buf.len() > CHUNK_LINE_MAX {
                            bail!("malformed chunked body: chunk size line too long");
                        }
                        break;
                    }
                },
                ChunkState::SkipData => {
                    let skip = self.skip_remaining.min(self.buf.len() as u64) as usize;
                    if skip > 0 {
                        // 解码：只取 chunk 数据进 out，分帧字节随后丢弃
                        self.out.extend_from_slice(&self.buf[..skip]);
                        self.buf.advance(skip);
                        self.consumed += skip as u64;
                        self.skip_remaining -= skip as u64;
                    }
                    if self.skip_remaining == 0 {
                        self.state = ChunkState::ExpectChunkCrlf;
                    } else {
                        break; // 数据未收齐，等更多输入
                    }
                }
                ChunkState::ExpectChunkCrlf => {
                    if self.buf.len() >= 2 {
                        if &self.buf[..2] == b"\r\n" {
                            self.buf.advance(2);
                            self.consumed += 2;
                            self.state = ChunkState::ExpectSizeLine;
                        } else {
                            bail!("malformed chunked body: missing CRLF after chunk data");
                        }
                    } else {
                        break;
                    }
                }
                ChunkState::TailAfterZero => {
                    // 0 长度块之后：trailer 区逐行扫描，首个空行（直接 CRLF）即结束
                    if self.buf.len() >= 2 && &self.buf[..2] == b"\r\n" {
                        let end = self.consumed + 2;
                        self.done = true;
                        let rel = (end.saturating_sub(base) as usize).min(chunk.len());
                        end_at = Some(rel);
                        break;
                    }
                    match find_bytes(&self.buf, b"\r\n") {
                        Some(p) => {
                            let line_len = p + 2;
                            if line_len > CHUNK_LINE_MAX {
                                bail!("malformed chunked body: trailer too long");
                            }
                            self.buf.advance(line_len);
                            self.consumed += line_len as u64;
                        }
                        None => {
                            if self.buf.len() > CHUNK_LINE_MAX {
                                bail!("malformed chunked body: trailer too long");
                            }
                            break;
                        }
                    }
                }
            }
        }

        Ok(PumpPushed {
            payload: std::mem::take(&mut self.out),
            end_at,
        })
    }
}

/// 子串查找（无内存分配）
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 解析 chunk-size 行（允许 chunk 扩展 "1a;ext=x\r\n"）
fn parse_chunk_size(line: &[u8]) -> Result<u64> {
    let hex_len = line
        .iter()
        .position(|&b| !b.is_ascii_hexdigit())
        .unwrap_or(line.len());
    if hex_len == 0 {
        bail!("malformed chunked body: empty chunk size line");
    }
    let hex_str = std::str::from_utf8(&line[..hex_len])
        .map_err(|_| anyhow!("malformed chunked body: invalid chunk size"))?;
    u64::from_str_radix(hex_str, 16).map_err(|_| anyhow!("malformed chunked body: invalid chunk size"))
}

