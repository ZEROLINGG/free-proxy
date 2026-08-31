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


/// HKDF-SHA256（快速密钥扩展）
pub struct HkdfSha256;
impl_hkdf_kdf!(HkdfSha256, sha2::Sha256);

/// HKDF-SHA512
pub struct HkdfSha512;
impl_hkdf_kdf!(HkdfSha512, sha2::Sha512);


#[cfg(test)]
mod tests {
    use super::*;

    const IKM: &[u8] = b"my_super_secret_password_material";
    const SALT: &[u8] = b"random_salt_value";

    // ==========================================
    // 基础功能与安全性测试
    // ==========================================

    #[test]
    fn test_hkdf_sha256_basic() {
        let key = HkdfSha256::derive(IKM, SALT, 32).unwrap();
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn test_hkdf_sha512_basic() {
        let key = HkdfSha512::derive(IKM, SALT, 64).unwrap();
        assert_eq!(key.len(), 64);
    }

    #[test]
    fn test_hkdf_determinism() {
        // 密码学函数的铁律：相同的输入必须产生完全相同的输出
        let key1 = HkdfSha256::derive(IKM, SALT, 32).unwrap();
        let key2 = HkdfSha256::derive(IKM, SALT, 32).unwrap();
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_hkdf_salt_matters() {
        // 盐值必须影响最终输出
        let key_with_salt1 = HkdfSha256::derive(IKM, b"salt_A", 32).unwrap();
        let key_with_salt2 = HkdfSha256::derive(IKM, b"salt_B", 32).unwrap();
        let key_without_salt = HkdfSha256::derive(IKM, b"", 32).unwrap();

        assert_ne!(key_with_salt1, key_with_salt2);
        assert_ne!(key_with_salt1, key_without_salt);
    }

    // ==========================================
    // 边界与异常控制测试
    // ==========================================

    #[test]
    fn test_error_empty_ikm() {
        // RFC 5869 要求 IKM 至少需要提供。虽然底层可能允许空，但你的封装禁止了空 IKM。
        let result = HkdfSha256::derive(b"", SALT, 32);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("IKM must not be empty"));
    }

    #[test]
    fn test_error_zero_output_len() {
        let result = HkdfSha256::derive(IKM, SALT, 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("greater than 0"));
    }

    #[test]
    fn test_error_output_too_large() {
        // HKDF 的输出长度上限为 255 * Hash_Length
        // 对于 SHA256，上限是 255 * 32 = 8160 字节
        let valid_max = HkdfSha256::derive(IKM, SALT, 8160);
        assert!(valid_max.is_ok());

        // 超过上限必须报错 (8161 字节)
        let invalid_large = HkdfSha256::derive(IKM, SALT, 8161);
        assert!(invalid_large.is_err());
        assert!(invalid_large.unwrap_err().to_string().contains("HKDF expand failed"));
    }

    #[test]
    fn test_empty_salt_handled_gracefully() {
        // 验证代码中 if salt.is_empty() { None } else { Some(salt) } 逻辑不崩溃
        // 且按预期输出 16 字节密钥
        let key = HkdfSha256::derive(IKM, b"", 16).unwrap();
        assert_eq!(key.len(), 16);
    }

    // ==========================================
    // RFC 5869 标准测试向量 (Test Vector 3)
    // 验证数学实现的绝对正确性
    // ==========================================

    #[test]
    fn test_rfc5869_test_vector_3() {
        // RFC 5869 Test Vector 3:
        // Hash = SHA-256
        // IKM  = 0x0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b (22 bytes)
        // salt = empty
        // info = empty (完全符合当前宏里的 expand(&[]) 逻辑)
        // L    = 42

        let ikm = [0x0b; 22];
        let expected_okm: [u8; 42] = [
            0x8d, 0xa4, 0xe7, 0x75, 0xa5, 0x63, 0xc1, 0x8f, 0x71, 0x5f, 0x80, 0x2a, 0x06, 0x3c,
            0x5a, 0x31, 0xb8, 0xa1, 0x1f, 0x5c, 0x5e, 0xe1, 0x87, 0x9e, 0xc3, 0x45, 0x4e, 0x5f,
            0x3c, 0x73, 0x8d, 0x2d, 0x9d, 0x20, 0x13, 0x95, 0xfa, 0xa4, 0xb6, 0x1a, 0x96, 0xc8,
        ];

        let result_okm = HkdfSha256::derive(&ikm, b"", 42).expect("Derivation failed");

        assert_eq!(result_okm, expected_okm);
    }
}
