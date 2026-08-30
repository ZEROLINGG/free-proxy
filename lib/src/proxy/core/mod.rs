pub(crate) mod ws;
pub(crate) mod http;

use anyhow::{Context, Result};
use bytes::Bytes;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::timeout;

use super::Shared;

/// 单次读取缓冲大小
const READ_BUF: usize = 16 * 1024;

/// 单次原始读取的结果
pub(super) enum RawRead {
    Data(Bytes),
    Eof,
    TimedOut,
}

pub(super) async fn read_raw<S: AsyncRead + Unpin>(stream: &mut S, idle: Duration) -> Result<RawRead> {
    let mut buf = [0u8; READ_BUF];
    match timeout(idle, stream.read(&mut buf)).await {
        Ok(Ok(0)) => Ok(RawRead::Eof),
        Ok(Ok(n)) => Ok(RawRead::Data(Bytes::copy_from_slice(&buf[..n]))),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(RawRead::Eof),
        Ok(Err(e)) => Err(e).context("read failed"),
        Err(_) => Ok(RawRead::TimedOut),
    }
}
