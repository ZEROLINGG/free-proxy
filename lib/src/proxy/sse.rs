// lib/src/proxy/sse.rs
// 极简 SSE 流解析器：server-rs 以 SSE 形式回传打包后的数据块。
// 事件格式（axum Sse 输出）：
//   data: <base91>\r\n\r\n        → 数据块
//   event: error\r\ndata: <msg>\r\n\r\n
//   event: done\r\n\r\n
// 兼容 \r\n 与 \n 两种行尾。

#[derive(Debug, PartialEq, Clone)]
pub enum SseEvent {
    /// `data:` 行的内容（多行按 SSE 规范以 \n 连接）
    Data(String),
    /// `event: error`，携带错误消息
    Error(String),
    /// `event: done`，流正常结束
    Done,
}

pub struct SseParser {
    buffer: Vec<u8>,
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(4096),
        }
    }

    /// 喂入一块原始字节，解析出完整事件（可能 0..n 个）
    pub fn push(&mut self, chunk: &[u8], out: &mut Vec<SseEvent>) {
        self.buffer.extend_from_slice(chunk);

        loop {
            // 事件以空行分隔：\r\n\r\n 或 \n\n（块内行尾统一在 parse_block 处理）
            let Some((block_end, sep_len)) = find_event_boundary(&self.buffer) else {
                return;
            };

            let block: Vec<u8> = self.buffer.drain(..block_end).collect();
            self.buffer.drain(..sep_len);

            if let Some(ev) = parse_block(&block) {
                out.push(ev);
            }
        }
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

/// 找到事件边界：返回 (块结束位置, 分隔符长度)
fn find_event_boundary(buf: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < buf.len() {
        if buf[i] == b'\n' {
            if buf.get(i + 1) == Some(&b'\n') {
                return Some((i, 2));
            }
            // 也兼容 \r\n\r\n
            if buf.get(i + 1) == Some(&b'\r') && buf.get(i + 2) == Some(&b'\n') {
                return Some((i, 3));
            }
        }
        i += 1;
    }
    None
}

/// 解析一个事件块（不含尾部空行）
fn parse_block(block: &[u8]) -> Option<SseEvent> {
    let text = String::from_utf8_lossy(block);
    let mut event_field = "message";
    let mut data_lines: Vec<&str> = Vec::new();

    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(rest) = line.strip_prefix("event:") {
            event_field = rest.trim();
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
        // 其他字段（id/retry/注释）忽略
    }

    match event_field {
        "done" => Some(SseEvent::Done),
        "error" => Some(SseEvent::Error(data_lines.join("\n"))),
        _ => {
            if data_lines.is_empty() {
                None // 空块或纯注释块
            } else {
                Some(SseEvent::Data(data_lines.join("\n")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut parser = SseParser::new();
        let mut out = Vec::new();
        for c in chunks {
            parser.push(c, &mut out);
        }
        out
    }

    #[test]
    fn test_single_data_event() {
        let events = parse_all(&[b"data: abc123\r\n\r\n"]);
        assert_eq!(events, vec![SseEvent::Data("abc123".into())]);
    }

    #[test]
    fn test_done_event() {
        let events = parse_all(&[b"event: done\r\n\r\n"]);
        assert_eq!(events, vec![SseEvent::Done]);
    }

    #[test]
    fn test_error_event() {
        let events = parse_all(&[b"event: error\r\ndata: fetch failed\r\n\r\n"]);
        assert_eq!(events, vec![SseEvent::Error("fetch failed".into())]);
    }

    #[test]
    fn test_multiple_events_in_one_chunk() {
        let events = parse_all(&[b"data: a\r\n\r\nevent: done\r\n\r\n"]);
        assert_eq!(events, vec![SseEvent::Data("a".into()), SseEvent::Done]);
    }

    #[test]
    fn test_chunk_split_across_boundaries() {
        let events = parse_all(&[b"data: a", b"bc\r\n", b"\r\n"]);
        assert_eq!(events, vec![SseEvent::Data("abc".into())]);
    }

    #[test]
    fn test_lf_only_line_endings() {
        let events = parse_all(&[b"data: x\n\nevent: done\n\n"]);
        assert_eq!(events, vec![SseEvent::Data("x".into()), SseEvent::Done]);
    }

    #[test]
    fn test_mixed_endings() {
        let events = parse_all(&[b"data: x\n\r\n"]);
        assert_eq!(events, vec![SseEvent::Data("x".into())]);
    }

    #[test]
    fn test_multiline_data_joined() {
        let events = parse_all(&[b"data: line1\r\ndata: line2\r\n\r\n"]);
        assert_eq!(events, vec![SseEvent::Data("line1\nline2".into())]);
    }

    #[test]
    fn test_blank_blocks_skipped() {
        let events = parse_all(&[b"\r\n\r\ndata: x\r\n\r\n"]);
        assert_eq!(events, vec![SseEvent::Data("x".into())]);
    }

    #[test]
    fn test_pending_data_not_emitted() {
        let mut parser = SseParser::new();
        let mut out = Vec::new();
        parser.push(b"data: partial", &mut out);
        assert!(out.is_empty());
        parser.push(b"\r\n\r\n", &mut out);
        assert_eq!(out, vec![SseEvent::Data("partial".into())]);
    }
}
