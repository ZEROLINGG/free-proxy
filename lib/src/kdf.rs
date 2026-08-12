use anyhow::Result;

/// 密钥派生函数（Key Derivation Function）统一接口
pub trait Kdf {
    /// 从密码/密钥材料派生固定长度密钥
    fn derive<P: AsRef<[u8]>, S: AsRef<[u8]>>(
        password: P,
        salt: S,
        output_len: usize,
    ) -> Result<Vec<u8>>;
}

// ─── PBKDF2 辅助宏 ────────────────────────────────────────────────────────────
macro_rules! impl_pbkdf2_kdf {
    ($struct:ty, $prf:ty, $iterations:expr) => {
        impl Kdf for $struct {
            fn derive<P: AsRef<[u8]>, S: AsRef<[u8]>>(
                password: P,
                salt: S,
                output_len: usize,
            ) -> Result<Vec<u8>> {
                use pbkdf2::pbkdf2_hmac;

                let password = password.as_ref();
                let salt = salt.as_ref();

                anyhow::ensure!(!password.is_empty(), "password must not be empty");
                anyhow::ensure!(!salt.is_empty(), "salt must not be empty");
                anyhow::ensure!(output_len > 0, "output_len must be greater than 0");

                let mut output = vec![0u8; output_len];
                pbkdf2_hmac::<$prf>(password, salt, $iterations, &mut output);
                Ok(output)
            }
        }
    };
}

// ─── scrypt 辅助宏 ────────────────────────────────────────────────────────────
macro_rules! impl_scrypt_kdf {
    ($struct:ty, $log_n:expr, $r:expr, $p:expr) => {
        impl Kdf for $struct {
            fn derive<P: AsRef<[u8]>, S: AsRef<[u8]>>(
                password: P,
                salt: S,
                output_len: usize,
            ) -> Result<Vec<u8>> {
                use scrypt::{Params, scrypt};

                let password = password.as_ref();
                let salt = salt.as_ref();

                anyhow::ensure!(!password.is_empty(), "password must not be empty");
                anyhow::ensure!(!salt.is_empty(), "salt must not be empty");
                anyhow::ensure!(output_len > 0, "output_len must be greater than 0");

                let params = Params::new($log_n, $r, $p)
                    .map_err(|e| anyhow::anyhow!("invalid scrypt params: {e}"))?;
                let mut output = vec![0u8; output_len];
                scrypt(password, salt, &params, &mut output)
                    .map_err(|e| anyhow::anyhow!("scrypt derivation failed: {e}"))?;
                Ok(output)
            }
        }
    };
}

// ─── HKDF 辅助宏 ──────────────────────────────────────────────────────────────
macro_rules! impl_hkdf_kdf {
    ($struct:ty, $hash:ty) => {
        impl Kdf for $struct {
            /// HKDF 派生
            ///
            /// - `password` → 输入密钥材料（IKM）
            /// - `salt` → 盐值（可选，空则使用全零盐）
            fn derive<P: AsRef<[u8]>, S: AsRef<[u8]>>(
                password: P,
                salt: S,
                output_len: usize,
            ) -> Result<Vec<u8>> {
                use hkdf::Hkdf;

                let ikm = password.as_ref();
                let salt = salt.as_ref();

                anyhow::ensure!(!ikm.is_empty(), "IKM must not be empty");
                anyhow::ensure!(output_len > 0, "output_len must be greater than 0");

                // salt 为空时传 None，HKDF 内部使用全零盐（RFC 5869 §2.2）
                let salt_opt = if salt.is_empty() { None } else { Some(salt) };

                let hk = Hkdf::<$hash>::new(salt_opt, ikm);
                let mut okm = vec![0u8; output_len];
                hk.expand(&[], &mut okm)
                    .map_err(|e| anyhow::anyhow!("HKDF expand failed: {e}"))?;
                Ok(okm)
            }
        }
    };
}

// ─── 具体实现 ─────────────────────────────────────────────────────────────────

/// PBKDF2-HMAC-SHA256，100,000 次迭代（OWASP 2023 推荐最低值）
pub struct Pbkdf2HmacSha256;
impl_pbkdf2_kdf!(Pbkdf2HmacSha256, sha2::Sha256, 100_000);

/// PBKDF2-HMAC-SHA256，600,000 次迭代（Apple 平台推荐值）
pub struct Pbkdf2HmacSha256High;
impl_pbkdf2_kdf!(Pbkdf2HmacSha256High, sha2::Sha256, 600_000);

/// PBKDF2-HMAC-SHA512，210,000 次迭代（OWASP 2023 推荐）
pub struct Pbkdf2HmacSha512;
impl_pbkdf2_kdf!(Pbkdf2HmacSha512, sha2::Sha512, 210_000);

/// scrypt，N = 2^15，r = 8，p = 1
pub struct ScryptDefault;
impl_scrypt_kdf!(ScryptDefault, 15, 8, 1);

/// scrypt，N = 2^17，r = 8，p = 1
pub struct ScryptHigh;
impl_scrypt_kdf!(ScryptHigh, 17, 8, 1);

/// HKDF-SHA256（快速密钥扩展）
pub struct HkdfSha256;
impl_hkdf_kdf!(HkdfSha256, sha2::Sha256);

/// HKDF-SHA512
pub struct HkdfSha512;
impl_hkdf_kdf!(HkdfSha512, sha2::Sha512);

// ─── 单元测试 ─────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &[u8] = b"correct horse battery staple";
    const SALT: &[u8] = b"seasalt1234567890";
    const OUTPUT_LEN: usize = 32;

    fn test_deterministic<K: Kdf>(label: &str) {
        let key1 = K::derive(PASSWORD, SALT, OUTPUT_LEN).expect("derive failed");
        let key2 = K::derive(PASSWORD, SALT, OUTPUT_LEN).expect("derive failed");
        assert_eq!(key1, key2, "{label}: not deterministic");
        assert_eq!(key1.len(), OUTPUT_LEN, "{label}: wrong output length");
        println!("{label}: ok, output = {} bytes", key1.len());
    }

    fn test_different_passwords<K: Kdf>(label: &str) {
        let key1 = K::derive(PASSWORD, SALT, OUTPUT_LEN).expect("derive failed");
        let key2 = K::derive(b"wrong password", SALT, OUTPUT_LEN).expect("derive failed");
        assert_ne!(key1, key2, "{label}: same output for different passwords");
    }

    fn test_different_salts<K: Kdf>(label: &str) {
        let key1 = K::derive(PASSWORD, SALT, OUTPUT_LEN).expect("derive failed");
        let key2 = K::derive(PASSWORD, b"differentsalt123", OUTPUT_LEN).expect("derive failed");
        assert_ne!(key1, key2, "{label}: same output for different salts");
    }

    fn test_invalid_inputs<K: Kdf>(label: &str) {
        assert!(
            K::derive(b"", SALT, OUTPUT_LEN).is_err(),
            "{label}: should reject empty password"
        );
        assert!(
            K::derive(PASSWORD, b"", OUTPUT_LEN).is_err(),
            "{label}: should reject empty salt"
        );
        assert!(
            K::derive(PASSWORD, SALT, 0).is_err(),
            "{label}: should reject zero output length"
        );
    }

    #[test]
    fn test_pbkdf2_sha256() {
        test_deterministic::<Pbkdf2HmacSha256>("pbkdf2-hmac-sha256");
        test_different_passwords::<Pbkdf2HmacSha256>("pbkdf2-hmac-sha256");
        test_different_salts::<Pbkdf2HmacSha256>("pbkdf2-hmac-sha256");
        test_invalid_inputs::<Pbkdf2HmacSha256>("pbkdf2-hmac-sha256");
    }

    #[test]
    fn test_pbkdf2_sha256_high() {
        test_deterministic::<Pbkdf2HmacSha256High>("pbkdf2-hmac-sha256-high");
    }

    #[test]
    fn test_pbkdf2_sha512() {
        test_deterministic::<Pbkdf2HmacSha512>("pbkdf2-hmac-sha512");
        test_different_passwords::<Pbkdf2HmacSha512>("pbkdf2-hmac-sha512");
    }

    #[test]
    fn test_scrypt_default() {
        test_deterministic::<ScryptDefault>("scrypt-default");
        test_different_passwords::<ScryptDefault>("scrypt-default");
        test_different_salts::<ScryptDefault>("scrypt-default");
        test_invalid_inputs::<ScryptDefault>("scrypt-default");
    }

    #[test]
    fn test_scrypt_high() {
        test_deterministic::<ScryptHigh>("scrypt-high");
    }

    #[test]
    fn test_hkdf_sha256() {
        test_deterministic::<HkdfSha256>("hkdf-sha256");
        test_different_passwords::<HkdfSha256>("hkdf-sha256");
        test_different_salts::<HkdfSha256>("hkdf-sha256");
        // HKDF 允许空 salt（退化为全零盐，符合 RFC 5869）
        let result = HkdfSha256::derive(PASSWORD, b"", OUTPUT_LEN);
        assert!(result.is_ok(), "hkdf should allow empty salt");
        assert!(
            HkdfSha256::derive(b"", SALT, OUTPUT_LEN).is_err(),
            "hkdf: empty ikm rejected"
        );
    }

    #[test]
    fn test_hkdf_sha512() {
        test_deterministic::<HkdfSha512>("hkdf-sha512");
    }

    #[test]
    fn test_variable_output_lengths() {
        for len in [16, 32, 64, 128] {
            let key = Pbkdf2HmacSha256::derive(PASSWORD, SALT, len).expect("derive failed");
            assert_eq!(key.len(), len, "output length mismatch for {len}");
        }
    }

    #[test]
    fn test_hkdf_rfc5869_extract_vector() {
        // RFC 5869 Test Case 1（SHA-256）IKM/salt，info 为空（我们的接口不带 info 参数）
        let ikm: &[u8] = &[
            0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b,
            0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b,
        ];
        let salt: &[u8] = &[
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
        ];
        let expected = hex::decode(
            "b2a3d45126d31fb6828ef00d76c6d54e9c2bd4785e49c6ad86e327d89d0de9408eeda1cbef2b03f30e05",
        )
        .unwrap();
        let okm = HkdfSha256::derive(ikm, salt, 42).expect("derive failed");
        assert_eq!(okm, expected, "hkdf-sha256 (empty info) mismatch");
    }
}
