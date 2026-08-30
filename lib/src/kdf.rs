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

