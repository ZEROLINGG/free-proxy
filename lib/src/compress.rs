//lib/src/compress.rs
use anyhow::{Result, anyhow};

const MAX_COMPRESSION_RATIO: u64 = 1024;
const MAX_DECOMPRESSED_SIZE: u64 = 256 * 1024 * 1024; // 256 MiB
const MAX_PREALLOC: usize = 8 * 1024 * 1024; // 8 MiB
const ZSTD_WINDOW_LOG_MAX: u32 = 27; // 128 MiB window

pub trait Compressor {
    fn compress<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>>;
    fn decompress(input: &[u8]) -> Result<Vec<u8>>;
}

fn bomb_limit_for(input_len: usize) -> u64 {
    (input_len as u64)
        .saturating_mul(MAX_COMPRESSION_RATIO)
        .min(MAX_DECOMPRESSED_SIZE)
}

fn safe_prealloc_cap(declared: Option<u64>, fallback_input_len: usize) -> usize {
    match declared {
        Some(size) => (size as usize).min(MAX_PREALLOC),
        None => fallback_input_len.saturating_mul(3).min(MAX_PREALLOC),
    }
}

// ====================== Lz4 ======================

pub struct Lz4;

impl Compressor for Lz4 {
    fn compress<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>> {
        Ok(lz4_flex::compress_prepend_size(input.as_ref()))
    }

    fn decompress(input: &[u8]) -> Result<Vec<u8>> {
        if input.len() < 4 {
            return Err(anyhow!("Input too short to contain LZ4 size prefix"));
        }

        let declared_size = u32::from_le_bytes(input[..4].try_into()?) as u64;
        let bomb_limit = bomb_limit_for(input.len());

        if declared_size > bomb_limit {
            return Err(anyhow!(
                "LZ4 decompression bomb detected: declared size {} exceeds limit {}",
                declared_size,
                bomb_limit
            ));
        }

        let out = lz4_flex::decompress_size_prepended(input)?;

        if out.len() as u64 > bomb_limit {
            return Err(anyhow!("LZ4 decompression bomb detected during decode"));
        }

        Ok(out)
    }
}


// ====================== Zstd ======================

pub struct Zstd;

impl Compressor for Zstd {
    fn compress<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>> {
        Ok(zstd::encode_all(input.as_ref(), 5)?)
    }

    fn decompress(input: &[u8]) -> Result<Vec<u8>> {
        use std::io::Read;

        let bomb_limit = bomb_limit_for(input.len());

        let declared_size = zstd::zstd_safe::get_frame_content_size(input)
            .ok()
            .flatten();

        if let Some(size) = declared_size
            && size > bomb_limit
        {
            return Err(anyhow!(
                "Zstd decompression bomb detected via frame header: {} exceeds limit {}",
                size,
                bomb_limit
            ));
        }

        let estimated_cap = safe_prealloc_cap(declared_size, input.len());

        let mut buf = Vec::with_capacity(estimated_cap);
        let mut decoder = zstd::Decoder::new(input)?;

        decoder
            .window_log_max(ZSTD_WINDOW_LOG_MAX)
            .map_err(|e| anyhow!("failed to set zstd window log max: {e}"))?;

        let mut limited_reader = decoder.take(bomb_limit + 1);

        limited_reader.read_to_end(&mut buf)?;

        if buf.len() as u64 > bomb_limit {
            return Err(anyhow!("Zstd decompression bomb detected during read"));
        }

        Ok(buf)
    }
}

// ====================== Tests ======================

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog. 0123456789";

    fn round_trip<C: Compressor>(label: &str) {
        let compressed = C::compress(SAMPLE).expect("compress failed");
        let decompressed = C::decompress(&compressed).expect("decompress failed");
        assert_eq!(decompressed, SAMPLE, "{label}: round-trip mismatch");

        println!(
            "{label}: {} -> {} bytes ({:.2}%)",
            SAMPLE.len(),
            compressed.len(),
            compressed.len() as f64 / SAMPLE.len() as f64 * 100.0
        );
    }

    #[test]
    fn test_lz4() {
        round_trip::<Lz4>("lz4");
    }
    #[test]
    fn test_zstd() {
        round_trip::<Zstd>("zstd");
    }


    #[test]
    fn test_lz4_bomb_rejected() {
        let mut compressed = Lz4::compress(SAMPLE).expect("compress failed");
        // Tamper with the LZ4 size prefix, claiming ~4GB decompressed size.
        compressed[..4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        assert!(
            Lz4::decompress(&compressed).is_err(),
            "lz4 bomb should be rejected"
        );
    }

    #[test]
    fn test_zstd_small_round_trip() {
        assert_eq!(
            Zstd::decompress(&Zstd::compress(b"tiny").unwrap()).unwrap(),
            b"tiny"
        );
    }

    #[test]
    fn test_invalid_data_returns_err() {
        let garbage = b"this is definitely not valid compressed data!!!";

        assert!(Lz4::decompress(garbage).is_err());
        assert!(Zstd::decompress(garbage).is_err());
    }
}
