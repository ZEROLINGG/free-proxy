pub(crate) mod ws;
pub(crate) mod http;

use anyhow::{Context, Result};
use bytes::BytesMut;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::timeout;

use super::Shared;

/// 单次读取缓冲大小
const READ_BUF: usize = 16 * 1024;

/// 单次原始读取的结果
pub(super) enum RawRead {
    Data(BytesMut),
    Eof,
    TimedOut,
}

pub(super) async fn read_raw<S: AsyncRead + Unpin>(stream: &mut S, idle: Duration) -> Result<RawRead> {
    let mut buf = BytesMut::with_capacity(READ_BUF);
    match timeout(idle, stream.read_buf(&mut buf)).await {
        // read_buf 已内部推进游标（len == 本次读取字节数），此处直接返回，绝不再手动 advance_mut
        Ok(Ok(0)) => Ok(RawRead::Eof),
        Ok(Ok(_)) => Ok(RawRead::Data(buf)),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(RawRead::Eof),
        Ok(Err(e)) => Err(e).context("read failed"),
        Err(_) => Ok(RawRead::TimedOut),
    }
}
