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
#[derive(Debug)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_body_extent_basic() {
        // 无 Body
        assert_eq!(body_extent(&[]).unwrap(), BodyExtent::NoBody);

        // Content-Length
        assert_eq!(
            body_extent(&[("Content-Length".into(), "1024".into())]).unwrap(),
            BodyExtent::ContentLength(1024)
        );

        // Chunked
        assert_eq!(
            body_extent(&[("Transfer-Encoding".into(), "chunked".into())]).unwrap(),
            BodyExtent::Chunked
        );
    }

    #[test]
    fn test_body_extent_overrides_and_smuggling_defenses() {
        // RFC 9112 规定：Transfer-Encoding: chunked 的优先级高于 Content-Length
        let headers = vec![
            ("Content-Length".into(), "999".into()),
            ("Transfer-Encoding".into(), "chunked".into()),
        ];
        assert_eq!(body_extent(&headers).unwrap(), BodyExtent::Chunked);

        // 防走私：重复的 Content-Length 必须严格拦截
        let headers = vec![
            ("Content-Length".into(), "10".into()),
            ("content-length".into(), "20".into()), // 大小写不敏感的重复
        ];
        assert!(body_extent(&headers).is_err());

        // 防走私：非法的 Content-Length 必须严格拦截
        let headers = vec![("Content-Length".into(), "-10".into())];
        assert!(body_extent(&headers).is_err());
        let headers = vec![("Content-Length".into(), "10 abc".into())];
        assert!(body_extent(&headers).is_err());
    }

    #[test]
    fn test_split_body_prefix() {
        let data = b"1234567890";

        // 无 Body: 全部属于下一个请求 (0属于当前, 10属于下一个)
        assert_eq!(split_body_prefix(data, &BodyExtent::NoBody), (0, 10));

        // Chunked: 在解码前无法预知边界，故当前分配器全部吞下 (10属于当前, 0属于下一个)
        assert_eq!(split_body_prefix(data, &BodyExtent::Chunked), (10, 0));

        // Content-Length: 截断
        assert_eq!(split_body_prefix(data, &BodyExtent::ContentLength(4)), (4, 6));
        assert_eq!(split_body_prefix(data, &BodyExtent::ContentLength(20)), (10, 0));
    }

    #[test]
    fn test_pump_tracker_content_length() {
        let mut tracker = PumpTracker::new(&BodyExtent::ContentLength(5)).unwrap();

        // 推入部分数据
        let res = tracker.push(b"123").unwrap();
        assert_eq!(&res.payload[..], b"123");
        assert_eq!(res.end_at, None); // 尚未结束

        // 推入超出边界的数据 (流水线粘包)
        let res = tracker.push(b"45_NEXT_REQ").unwrap();
        assert_eq!(&res.payload[..], b"45"); // 只取剩下的 2 字节
        assert_eq!(res.end_at, Some(2)); // 精确指出在索引 2 处结束 (后面的 _NEXT_REQ 归还 parser)
    }

    #[test]
    fn test_chunked_decoder_happy_path() {
        let mut decoder = ChunkedDecoder::new();

        // 标准的 Chunked 流：5 字节 + 2 字节 + 结束块
        let data = b"5\r\nhello\r\n2\r\n! \r\n0\r\n\r\n";

        let res = decoder.feed(data).unwrap();
        assert_eq!(&res.payload[..], b"hello! ");

        assert_eq!(res.end_at, Some(22));
    }

    #[test]
    fn test_chunked_decoder_pipelining() {
        let mut decoder = ChunkedDecoder::new();

        // 包含扩展头(;ext)、数据、尾部Trailer，以及紧随其后的下一个 HTTP 请求
        let data = b"4;ext=123\r\nRust\r\n0\r\nMy-Trailer: xyz\r\n\r\nGET / HTTP/1.1\r\n";

        let res = decoder.feed(data).unwrap();

        // 解码器必须剥离 size、ext、CRLF 和 Trailer，只保留纯数据
        assert_eq!(&res.payload[..], b"Rust");

        // 精确定位："...\r\n\r\n" 结束的索引位置。
        // "4;ext=123\r\n" (11) + "Rust\r\n" (6) + "0\r\n" (3) + "My-Trailer: xyz\r\n\r\n" (19) = 39
        assert_eq!(res.end_at, Some(39));
    }

    #[test]
    fn test_chunked_decoder_fragmentation() {
        let mut decoder = ChunkedDecoder::new();
        let payload = b"3\r\nfoo\r\n3\r\nbar\r\n0\r\n\r\n";

        // 极限测试：TCP 严重拆包，每次只收到 1 个字节
        let mut combined_payload = Vec::new();
        let mut end_at = None;

        for i in 0..payload.len() {
            let chunk = &payload[i..=i];
            let res = decoder.feed(chunk).unwrap();
            combined_payload.extend_from_slice(&res.payload);

            if res.end_at.is_some() {
                end_at = res.end_at;
                // 确保只有在最后一个字节时才触发结束
                assert_eq!(i, payload.len() - 1);
            }
        }

        assert_eq!(&combined_payload[..], b"foobar");
        assert_eq!(end_at, Some(1)); // 最后一次 feed 长度为 1，在索引 1 处结束
    }

    #[test]
    fn test_chunked_decoder_tricky_payload() {
        let mut decoder = ChunkedDecoder::new();
        // 负载内容恰好包含了像 Chunk 结束符的特征，验证基于长度计数的解码不会被内容干扰
        let data = b"8\r\n0\r\n\r\nxxx\r\n0\r\n\r\n";
        let res = decoder.feed(data).unwrap();

        assert_eq!(&res.payload[..], b"0\r\n\r\nxxx");

        assert_eq!(res.end_at, Some(18));
    }

    #[test]
    fn test_chunked_decoder_malformed_errors() {
        // 错误 1: Chunk 声明长度为 3，读取 3 字节 "foo" 之后，紧跟的必须是 \r\n，但这里给的是 "X\n"
        let mut dec = ChunkedDecoder::new();
        assert!(dec.feed(b"3\r\nfooX\n0\r\n\r\n").is_err());

        // 错误 2: 非法的十六进制长度
        let mut dec = ChunkedDecoder::new();
        assert!(dec.feed(b"ZZZ\r\nhello\r\n0\r\n\r\n").is_err());

        // 错误 3: 空的长度行
        let mut dec = ChunkedDecoder::new();
        assert!(dec.feed(b"\r\nhello\r\n").is_err());
    }

    #[test]
    fn test_chunked_decoder_dos_protection() {
        let mut decoder = ChunkedDecoder::new();

        // 制造一个长达 70KB 的 Chunk Size 行（没有任何 \r\n 换行），模拟攻击者耗尽内存
        let malicious_data = vec![b'A'; 70 * 1024];

        let result = decoder.feed(&malicious_data);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("chunk size line too long"));
    }
}