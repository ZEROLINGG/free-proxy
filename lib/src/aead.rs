//lib/src/aead.rs
use anyhow::Result;

pub trait Cipher: Send + Sync {
    fn encrypt<T: AsRef<[u8]>>(data: T, key: &[u8]) -> Result<Vec<u8>>;
    fn decrypt<T: AsRef<[u8]>>(data: T, key: &[u8]) -> Result<Vec<u8>>;
}

// ─── GCM 模式辅助宏（nonce 12 字节前置） ─────────────────────────────────────

/// GCM 加密输出格式：[ nonce (12 B) | ciphertext + tag (plaintext.len() + 16 B) ]
macro_rules! impl_gcm_cipher {
    ($struct:ty, $cipher_type:ty, $key_len:expr) => {
        impl Cipher for $struct {
            fn encrypt<T: AsRef<[u8]>>(data: T, key: &[u8]) -> Result<Vec<u8>> {
                use aes_gcm::aead::{Aead, Generate, KeyInit};

                anyhow::ensure!(
                    key.len() == $key_len,
                    "invalid key length: expected {}, got {}",
                    $key_len,
                    key.len()
                );

                let cipher = <$cipher_type>::new_from_slice(key)
                    .map_err(|e| anyhow::anyhow!("invalid key: {e}"))?;
                let nonce = aes_gcm::aead::Nonce::<$cipher_type>::generate();

                let ciphertext = cipher
                    .encrypt(&nonce, data.as_ref())
                    .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;

                let mut output = Vec::with_capacity(12 + ciphertext.len());
                output.extend_from_slice(nonce.as_slice());
                output.extend_from_slice(&ciphertext);
                Ok(output)
            }

            fn decrypt<T: AsRef<[u8]>>(data: T, key: &[u8]) -> Result<Vec<u8>> {
                use aes_gcm::aead::{Aead, KeyInit};

                let data = data.as_ref();
                anyhow::ensure!(
                    key.len() == $key_len,
                    "invalid key length: expected {}, got {}",
                    $key_len,
                    key.len()
                );
                anyhow::ensure!(
                    data.len() >= 12,
                    "ciphertext too short: {} bytes",
                    data.len()
                );

                let cipher = <$cipher_type>::new_from_slice(key)
                    .map_err(|e| anyhow::anyhow!("invalid key: {e}"))?;
                let (nonce_bytes, payload) = data.split_at(12);
                let nonce = aes_gcm::aead::Nonce::<$cipher_type>::try_from(nonce_bytes)
                    .map_err(|_| anyhow::anyhow!("nonce length mismatch"))?;

                cipher
                    .decrypt(&nonce, payload)
                    .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))
            }
        }
    };
}

// ─── GCM-SIV 模式辅助宏（nonce 12 字节前置） ─────────────────────────────────
macro_rules! impl_gcm_siv_cipher {
    ($struct:ty, $cipher_type:ty, $key_len:expr) => {
        impl Cipher for $struct {
            fn encrypt<T: AsRef<[u8]>>(data: T, key: &[u8]) -> Result<Vec<u8>> {
                use aes_gcm_siv::aead::{Aead, Generate, KeyInit};

                anyhow::ensure!(
                    key.len() == $key_len,
                    "invalid key length: expected {}, got {}",
                    $key_len,
                    key.len()
                );

                let cipher = <$cipher_type>::new_from_slice(key)
                    .map_err(|e| anyhow::anyhow!("invalid key: {e}"))?;
                let nonce = aes_gcm_siv::aead::Nonce::<$cipher_type>::generate();

                let ciphertext = cipher
                    .encrypt(&nonce, data.as_ref())
                    .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;

                let mut output = Vec::with_capacity(12 + ciphertext.len());
                output.extend_from_slice(nonce.as_slice());
                output.extend_from_slice(&ciphertext);
                Ok(output)
            }

            fn decrypt<T: AsRef<[u8]>>(data: T, key: &[u8]) -> Result<Vec<u8>> {
                use aes_gcm_siv::aead::{Aead, KeyInit};

                let data = data.as_ref();
                anyhow::ensure!(
                    key.len() == $key_len,
                    "invalid key length: expected {}, got {}",
                    $key_len,
                    key.len()
                );
                anyhow::ensure!(
                    data.len() >= 12,
                    "ciphertext too short: {} bytes",
                    data.len()
                );

                let cipher = <$cipher_type>::new_from_slice(key)
                    .map_err(|e| anyhow::anyhow!("invalid key: {e}"))?;
                let (nonce_bytes, payload) = data.split_at(12);
                let nonce = aes_gcm_siv::aead::Nonce::<$cipher_type>::try_from(nonce_bytes)
                    .map_err(|_| anyhow::anyhow!("nonce length mismatch"))?;

                cipher
                    .decrypt(&nonce, payload)
                    .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))
            }
        }
    };
}

// ─── ChaCha20 系列辅助宏 ──────────────────────────────────────────────────────

/// ChaCha20-Poly1305（nonce 12 B）与 XChaCha20-Poly1305（nonce 24 B）。
/// 输出格式：[ nonce ($nonce_len B) | ciphertext + tag (plaintext.len() + 16 B) ]
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

pub struct Aes128Gcm;
impl_gcm_cipher!(Aes128Gcm, aes_gcm::Aes128Gcm, 16);

pub struct Aes256Gcm;
impl_gcm_cipher!(Aes256Gcm, aes_gcm::Aes256Gcm, 32);

pub struct Aes128GcmSiv;
impl_gcm_siv_cipher!(Aes128GcmSiv, aes_gcm_siv::Aes128GcmSiv, 16);

pub struct Aes256GcmSiv;
impl_gcm_siv_cipher!(Aes256GcmSiv, aes_gcm_siv::Aes256GcmSiv, 32);

/// ChaCha20-Poly1305，nonce 12 字节，密钥 32 字节
pub struct ChaCha20Poly1305;
impl_chacha_cipher!(ChaCha20Poly1305, chacha20poly1305::ChaCha20Poly1305, 12);

/// XChaCha20-Poly1305，nonce 24 字节，密钥 32 字节
pub struct XChaCha20Poly1305;
impl_chacha_cipher!(XChaCha20Poly1305, chacha20poly1305::XChaCha20Poly1305, 24);

/// Ascon-AEAD128（NIST SP 800-232），密钥 16 字节，nonce 16 字节，tag 16 字节
pub struct Ascon128;
impl_ascon_cipher!(Ascon128, ascon_aead128::AsconAead128);

// ─── 单元测试 ─────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"The quick brown fox jumps over the lazy dog 0123456789";

    fn round_trip<C: Cipher>(label: &str, key: &[u8]) {
        let encrypted = C::encrypt(SAMPLE, key).expect("encrypt failed");
        let decrypted = C::decrypt(&encrypted, key).expect("decrypt failed");
        assert_eq!(decrypted, SAMPLE, "{label}: round-trip mismatch");
        println!(
            "{label}: {} -> {} bytes ({:.1}%)",
            SAMPLE.len(),
            encrypted.len(),
            encrypted.len() as f64 / SAMPLE.len() as f64 * 100.0
        );
    }

    fn wrong_key_returns_err<C: Cipher>(label: &str, key: &[u8], bad_key: &[u8]) {
        let encrypted = C::encrypt(SAMPLE, key).expect("encrypt failed");
        let result = C::decrypt(&encrypted, bad_key);
        assert!(result.is_err(), "{label}: expected Err with wrong key");
    }

    #[test]
    fn test_aes128gcm_round_trip() {
        round_trip::<Aes128Gcm>("aes-128-gcm", &[0x42u8; 16]);
    }

    #[test]
    fn test_aes256gcm_round_trip() {
        round_trip::<Aes256Gcm>("aes-256-gcm", &[0x7Eu8; 32]);
    }

    #[test]
    fn test_aes128gcmsiv_round_trip() {
        round_trip::<Aes128GcmSiv>("aes-128-gcm-siv", &[0x42u8; 16]);
    }

    #[test]
    fn test_aes128gcmsiv_wrong_key() {
        wrong_key_returns_err::<Aes128GcmSiv>("aes-128-gcm-siv", &[0x42u8; 16], &[0x00u8; 16]);
    }

    #[test]
    fn test_aes256gcmsiv_round_trip() {
        round_trip::<Aes256GcmSiv>("aes-256-gcm-siv", &[0x7Eu8; 32]);
    }

    #[test]
    fn test_aes256gcmsiv_wrong_key() {
        wrong_key_returns_err::<Aes256GcmSiv>("aes-256-gcm-siv", &[0x7Eu8; 32], &[0x00u8; 32]);
    }

    #[test]
    fn test_chacha20poly1305_round_trip() {
        round_trip::<ChaCha20Poly1305>("chacha20-poly1305", &[0xABu8; 32]);
    }

    #[test]
    fn test_chacha20poly1305_wrong_key() {
        wrong_key_returns_err::<ChaCha20Poly1305>(
            "chacha20-poly1305",
            &[0xABu8; 32],
            &[0x00u8; 32],
        );
    }

    #[test]
    fn test_xchacha20poly1305_round_trip() {
        round_trip::<XChaCha20Poly1305>("xchacha20-poly1305", &[0xCDu8; 32]);
    }

    #[test]
    fn test_xchacha20poly1305_wrong_key() {
        wrong_key_returns_err::<XChaCha20Poly1305>(
            "xchacha20-poly1305",
            &[0xCDu8; 32],
            &[0x00u8; 32],
        );
    }

    #[test]
    fn test_invalid_key_length_rejected() {
        assert!(Aes128Gcm::encrypt(b"data", &[0u8; 15]).is_err());
        assert!(Aes256Gcm::encrypt(b"data", &[0u8; 31]).is_err());
        assert!(ChaCha20Poly1305::encrypt(b"data", &[0u8; 16]).is_err());
    }

    #[test]
    fn test_ascon128_round_trip() {
        round_trip::<Ascon128>("ascon-128", &[0x5Au8; 16]);
    }

    #[test]
    fn test_ascon128_wrong_key() {
        wrong_key_returns_err::<Ascon128>("ascon-128", &[0x5Au8; 16], &[0x00u8; 16]);
    }

    #[test]
    fn test_ascon128_invalid_key_length_rejected() {
        assert!(Ascon128::encrypt(b"data", &[0u8; 15]).is_err());
        assert!(Ascon128::encrypt(b"data", &[0u8; 17]).is_err());
        assert!(Ascon128::encrypt(b"data", &[0u8; 32]).is_err());
    }

    #[test]
    fn test_ascon128_short_ciphertext_rejected() {
        assert!(Ascon128::decrypt([0u8; 15], &[0u8; 16]).is_err());
        // 仅 16 字节 nonce、无 tag 负载同样不可解密
        assert!(Ascon128::decrypt([0u8; 16], &[0u8; 16]).is_err());
    }

    #[test]
    fn test_short_ciphertext_rejected() {
        assert!(Aes128Gcm::decrypt([0u8; 11], &[0u8; 16]).is_err());
        assert!(ChaCha20Poly1305::decrypt([0u8; 11], &[0u8; 32]).is_err());
        assert!(XChaCha20Poly1305::decrypt([0u8; 23], &[0u8; 32]).is_err());
    }
}
