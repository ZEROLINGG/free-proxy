//lib/src/aead.rs
use anyhow::Result;

pub trait Cipher: Send + Sync {
    fn encrypt<T: AsRef<[u8]>>(data: T, key: &[u8]) -> Result<Vec<u8>>;
    fn decrypt<T: AsRef<[u8]>>(data: T, key: &[u8]) -> Result<Vec<u8>>;
}

// ─── ChaCha20 系列辅助宏 ──────────────────────────────────────────────────────

/// ChaCha20-Poly1305（nonce 12 B）。
/// 输出格式：[ nonce (12 B) | ciphertext + tag (plaintext.len() + 16 B) ]
macro_rules! impl_chacha_cipher {
    ($struct:ty, $cipher_type:ty, $nonce_len:expr) => {
        impl Cipher for $struct {
            fn encrypt<T: AsRef<[u8]>>(data: T, key: &[u8]) -> Result<Vec<u8>> {
                use chacha20poly1305::aead::{Aead, Generate, KeyInit};

                anyhow::ensure!(
                    key.len() == 32,
                    "invalid key length: expected 32, got {}",
                    key.len()
                );

                let cipher = <$cipher_type>::new_from_slice(key)
                    .map_err(|e| anyhow::anyhow!("invalid key: {e}"))?;
                let nonce = chacha20poly1305::aead::Nonce::<$cipher_type>::generate();

                let ciphertext = cipher
                    .encrypt(&nonce, data.as_ref())
                    .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;

                let mut output = Vec::with_capacity($nonce_len + ciphertext.len());
                output.extend_from_slice(nonce.as_slice());
                output.extend_from_slice(&ciphertext);
                Ok(output)
            }

            fn decrypt<T: AsRef<[u8]>>(data: T, key: &[u8]) -> Result<Vec<u8>> {
                use chacha20poly1305::aead::{Aead, KeyInit};

                let data = data.as_ref();
                anyhow::ensure!(
                    key.len() == 32,
                    "invalid key length: expected 32, got {}",
                    key.len()
                );
                anyhow::ensure!(
                    data.len() >= $nonce_len,
                    "ciphertext too short: {} bytes",
                    data.len()
                );

                let cipher = <$cipher_type>::new_from_slice(key)
                    .map_err(|e| anyhow::anyhow!("invalid key: {e}"))?;
                let (nonce_bytes, payload) = data.split_at($nonce_len);
                let nonce = chacha20poly1305::aead::Nonce::<$cipher_type>::try_from(nonce_bytes)
                    .map_err(|_| anyhow::anyhow!("nonce length mismatch"))?;

                cipher
                    .decrypt(&nonce, payload)
                    .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))
            }
        }
    };
}

// ─── Ascon-AEAD128 模式（nonce 16 字节前置） ─────────────────────────────────
macro_rules! impl_ascon_cipher {
    ($struct:ty, $cipher_type:ty) => {
        impl Cipher for $struct {
            fn encrypt<T: AsRef<[u8]>>(data: T, key: &[u8]) -> Result<Vec<u8>> {
                use ascon_aead128::aead::{Aead, Generate, KeyInit};

                anyhow::ensure!(
                    key.len() == 16,
                    "invalid key length: expected 16, got {}",
                    key.len()
                );

                let cipher = <$cipher_type>::new_from_slice(key)
                    .map_err(|e| anyhow::anyhow!("invalid key: {e}"))?;
                let nonce = ascon_aead128::aead::Nonce::<$cipher_type>::generate();

                let ciphertext = cipher
                    .encrypt(&nonce, data.as_ref())
                    .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;

                let mut output = Vec::with_capacity(16 + ciphertext.len());
                output.extend_from_slice(nonce.as_slice());
                output.extend_from_slice(&ciphertext);
                Ok(output)
            }

            fn decrypt<T: AsRef<[u8]>>(data: T, key: &[u8]) -> Result<Vec<u8>> {
                use ascon_aead128::aead::{Aead, KeyInit};

                let data = data.as_ref();
                anyhow::ensure!(
                    key.len() == 16,
                    "invalid key length: expected 16, got {}",
                    key.len()
                );
                anyhow::ensure!(
                    data.len() >= 16,
                    "ciphertext too short: {} bytes",
                    data.len()
                );

                let cipher = <$cipher_type>::new_from_slice(key)
                    .map_err(|e| anyhow::anyhow!("invalid key: {e}"))?;
                let (nonce_bytes, payload) = data.split_at(16);
                let nonce = ascon_aead128::aead::Nonce::<$cipher_type>::try_from(nonce_bytes)
                    .map_err(|_| anyhow::anyhow!("nonce length mismatch"))?;

                cipher
                    .decrypt(&nonce, payload)
                    .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))
            }
        }
    };
}

// ─── 具体实现 ─────────────────────────────────────────────────────────────────

/// ChaCha20-Poly1305，nonce 12 字节，密钥 32 字节
pub struct ChaCha20Poly1305;
impl_chacha_cipher!(ChaCha20Poly1305, chacha20poly1305::ChaCha20Poly1305, 12);

/// Ascon-AEAD128（NIST SP 800-232），密钥 16 字节，nonce 16 字节，tag 16 字节
pub struct Ascon128;
impl_ascon_cipher!(Ascon128, ascon_aead128::AsconAead128);


#[cfg(test)]
mod tests {
    use super::*;

    // ─── 辅助常量 ─────────────────────────────────────────────────────────────
    const CHACHA_KEY: &[u8; 32] = b"12345678901234567890123456789012"; // 32 字节
    const ASCON_KEY: &[u8; 16] = b"1234567890123456"; // 16 字节
    const PLAINTEXT: &[u8] = b"Hello, RustCrypto! This is a highly secret message.";

    // ─── ChaCha20Poly1305 测试 ───────────────────────────────────────────────

    #[test]
    fn test_chacha20_basic_encrypt_decrypt() {
        let encrypted = ChaCha20Poly1305::encrypt(PLAINTEXT, CHACHA_KEY).unwrap();
        let decrypted = ChaCha20Poly1305::decrypt(&encrypted, CHACHA_KEY).unwrap();
        assert_eq!(PLAINTEXT, &decrypted[..]);
    }

    #[test]
    fn test_chacha20_invalid_key_length() {
        let short_key = b"too short";
        let long_key = b"this_key_is_way_too_long_for_chacha20_to_accept_it";

        assert!(ChaCha20Poly1305::encrypt(PLAINTEXT, short_key).is_err());
        assert!(ChaCha20Poly1305::encrypt(PLAINTEXT, long_key).is_err());
        assert!(ChaCha20Poly1305::decrypt(PLAINTEXT, short_key).is_err());
    }

    #[test]
    fn test_chacha20_ciphertext_too_short() {
        // ChaCha20 nonce is 12 bytes. Trying to decrypt 11 bytes should fail immediately.
        let short_ciphertext = vec![0u8; 11];
        let result = ChaCha20Poly1305::decrypt(&short_ciphertext, CHACHA_KEY);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "ciphertext too short: 11 bytes"
        );
    }

    #[test]
    fn test_chacha20_tamper_resistance() {
        let mut encrypted = ChaCha20Poly1305::encrypt(PLAINTEXT, CHACHA_KEY).unwrap();

        // 篡改密文的最后一个字节 (通常是 Poly1305 Tag 的一部分)
        let last_idx = encrypted.len() - 1;
        encrypted[last_idx] ^= 0x01; // 翻转最低位

        // 验证解密会失败，确保了 AEAD 的完整性保护
        let result = ChaCha20Poly1305::decrypt(&encrypted, CHACHA_KEY);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("decryption failed"));
    }

    #[test]
    fn test_chacha20_nonce_randomness() {
        // 使用相同的密钥和相同的明文，多次加密得到的结果应该不同 (因为 Nonce 是随机生成的)
        let enc1 = ChaCha20Poly1305::encrypt(PLAINTEXT, CHACHA_KEY).unwrap();
        let enc2 = ChaCha20Poly1305::encrypt(PLAINTEXT, CHACHA_KEY).unwrap();

        assert_ne!(enc1, enc2);
    }

    #[test]
    fn test_chacha20_empty_plaintext() {
        let empty_data: &[u8] = b"";
        let encrypted = ChaCha20Poly1305::encrypt(empty_data, CHACHA_KEY).unwrap();

        // 密文长度 = 12 (Nonce) + 0 (明文) + 16 (Tag) = 28 字节
        assert_eq!(encrypted.len(), 28);

        let decrypted = ChaCha20Poly1305::decrypt(&encrypted, CHACHA_KEY).unwrap();
        assert_eq!(empty_data, &decrypted[..]);
    }

    // ─── Ascon128 测试 ────────────────────────────────────────────────────────

    #[test]
    fn test_ascon128_basic_encrypt_decrypt() {
        let encrypted = Ascon128::encrypt(PLAINTEXT, ASCON_KEY).unwrap();
        let decrypted = Ascon128::decrypt(&encrypted, ASCON_KEY).unwrap();
        assert_eq!(PLAINTEXT, &decrypted[..]);
    }

    #[test]
    fn test_ascon128_invalid_key_length() {
        let short_key = b"short";
        assert!(Ascon128::encrypt(PLAINTEXT, short_key).is_err());
        assert!(Ascon128::decrypt(PLAINTEXT, short_key).is_err());
    }

    #[test]
    fn test_ascon128_ciphertext_too_short() {
        // Ascon128 nonce is 16 bytes.
        let short_ciphertext = vec![0u8; 15];
        let result = Ascon128::decrypt(&short_ciphertext, ASCON_KEY);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "ciphertext too short: 15 bytes"
        );
    }

    #[test]
    fn test_ascon128_tamper_resistance() {
        let mut encrypted = Ascon128::encrypt(PLAINTEXT, ASCON_KEY).unwrap();

        // 篡改 Nonce 部分 (前 16 字节内的某个字节)
        encrypted[5] ^= 0xFF;

        let result = Ascon128::decrypt(&encrypted, ASCON_KEY);
        assert!(result.is_err());
    }

    #[test]
    fn test_ascon128_nonce_randomness() {
        let enc1 = Ascon128::encrypt(PLAINTEXT, ASCON_KEY).unwrap();
        let enc2 = Ascon128::encrypt(PLAINTEXT, ASCON_KEY).unwrap();
        assert_ne!(enc1, enc2);
    }

    #[test]
    fn test_ascon128_empty_plaintext() {
        let empty_data: &[u8] = b"";
        let encrypted = Ascon128::encrypt(empty_data, ASCON_KEY).unwrap();

        // 密文长度 = 16 (Nonce) + 0 (明文) + 16 (Tag) = 32 字节
        assert_eq!(encrypted.len(), 32);

        let decrypted = Ascon128::decrypt(&encrypted, ASCON_KEY).unwrap();
        assert!(decrypted.is_empty());
    }
}
