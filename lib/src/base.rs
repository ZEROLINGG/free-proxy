use anyhow::{Context, Result};

pub trait Encoder {
    fn encode<T: AsRef<[u8]>>(input: T) -> Result<String>;
    fn decode(input: &str) -> Result<Vec<u8>>;
}

use base64::Engine;
pub struct Base64;

impl Encoder for Base64 {
    fn encode<T: AsRef<[u8]>>(input: T) -> Result<String> {
        Ok(base64::engine::general_purpose::STANDARD.encode(input))
    }

    fn decode(input: &str) -> Result<Vec<u8>> {
        base64::engine::general_purpose::STANDARD
            .decode(input)
            .context("invalid base64")
    }
}

pub struct Base85;

impl Encoder for Base85 {
    fn encode<T: AsRef<[u8]>>(input: T) -> Result<String> {
        Ok(z85::encode(input.as_ref()))
    }

    fn decode(input: &str) -> Result<Vec<u8>> {
        z85::decode(input).map_err(|e| anyhow::anyhow!("ascii85 decode error: {:?}", e))
    }
}

pub struct Base91;

impl Encoder for Base91 {
    fn encode<T: AsRef<[u8]>>(input: T) -> Result<String> {
        let encoded_bytes = base91::slice_encode(input.as_ref());
        Ok(String::from_utf8(encoded_bytes).context("invalid utf8 in base91 encoding")?)
    }

    fn decode(input: &str) -> Result<Vec<u8>> {
        let decoded_bytes = base91::slice_decode(input.as_bytes());
        Ok(decoded_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<E: Encoder>(label: &str) {
        let mut data = Vec::new();
        data.resize(1024 * 1024 * 1, 10);

        let now = std::time::Instant::now();
        let encoded = E::encode(&data).expect("encode failed");
        let decoded = E::decode(&encoded).expect("decode failed");
        let elapsed = now.elapsed();
        assert_eq!(decoded, data, "{label}: round-trip mismatch");
        println!(
            "{label}: {} -> {} chars. time: {:.3?}",
            data.len(),
            encoded.len(),
            elapsed
        );
    }

    #[test]
    fn test_all_round_trips() {
        round_trip::<Base64>("base64");
        round_trip::<Base85>("base85");
        round_trip::<Base91>("base91");
    }
}
