use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use anyhow::anyhow;
use crate::hash::{Blake3, Hasher};
use crate::kdf::{HkdfSha256, Kdf, Pbkdf2HmacSha256};

pub fn xoroshiro128(s: u128) -> u128 {
    let mut s0 = (s >> 64) as u64;
    let mut s1 = s as u64;

    s1 ^= s0;
    s0 = s0.rotate_left(24) ^ s1 ^ (s1 << 16);
    s1 = s1.rotate_left(37);

    ((s0 as u128) << 64) | (s1 as u128)
}

pub fn dexoroshiro128(s: u128) -> u128 {
    let out0 = (s >> 64) as u64;
    let out1 = s as u64;

    let s1_step1 = out1.rotate_right(37);
    let s0_orig = (out0 ^ s1_step1 ^ (s1_step1 << 16)).rotate_right(24);

    let s1_orig = s1_step1 ^ s0_orig;

    ((s0_orig as u128) << 64) | (s1_orig as u128)
}

pub fn token_anth<D: AsRef<[u8]>>(token_tmp: &str, token_base: D, now: u64) -> bool {
    if let Ok(x) = hex::decode(token_tmp) {
        let y: &[u8] = token_base.as_ref();
        let mut z = [0u8; 16];
        for i in 0..16 {
            let y_byte = y.get(i).copied().unwrap_or(0);
            let x_byte = x.get(i).copied().unwrap_or(0);
            z[i] = x_byte ^ y_byte;
        }

        let mut data = u128::from_be_bytes(z);
        for _ in 0..6 {
            data = dexoroshiro128(data);
        }

        let num = data as u64;
        let nonce = (data >> 64) as u64;
        if nonce != 0 {
            if now.abs_diff(num) < 30_000 {
                return true;
            }
        }
    }
    false
}
pub fn token_gen<D: AsRef<[u8]>>(token_base: D, now: u64, nonce: u64) -> String {
    let final_data = ((nonce as u128) << 64) | (now as u128);

    let mut data = final_data;
    for _ in 0..6 {
        data = xoroshiro128(data);
    }

    let z = data.to_be_bytes();

    let y = token_base.as_ref();
    let mut x = [0u8; 16];

    for i in 0..16 {
        let y_byte = y.get(i).copied().unwrap_or(0);
        x[i] = z[i] ^ y_byte;
    }

    hex::encode(x)
}

#[cfg(feature = "client")]
pub fn gen_auth_token(token_base: &[u8; 16]) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    static TOKEN_NONCE: LazyLock<AtomicU64> = LazyLock::new(|| {
        let m = [0u8; 1];
        let n = vec![0u8; 1];
        let x = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
        let y = m.as_ptr() as u128;
        let z = n.as_ptr() as u128;
        let mut t = (x ^ y ^ z)
            .wrapping_mul(x)
            .wrapping_mul(y)
            .wrapping_mul(z);
        for _ in 0..3 {
            t = xoroshiro128(t);
        }
        AtomicU64::new(t as u64)
    });

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let nonce = TOKEN_NONCE
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |curr| {
            Some(xoroshiro128(curr as u128) as u64)
        })
        .unwrap_or_else(|v| v);

    token_gen(token_base, now, nonce)
}

pub fn xor_obfuscate<D, K16, K32>(data: D, key16: K16, key32: K32) -> Vec<u8>
where
    D: AsRef<[u8]>,
    K16: AsRef<[u8]>,
    K32: AsRef<[u8]>,
{
    let key16 = key16.as_ref();
    let key32 = key32.as_ref();
    data.as_ref()
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            let k16 = key16[i % key16.len()];
            let k32 = key32[i % key32.len()];
            b ^ (k16 ^ k32).wrapping_mul(
                (k16 % 127).wrapping_add((k32 % 131).wrapping_add(i.wrapping_mul(3) as u8 % 163)),
            )
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct DerivedKeys {
    pub key16: [u8; 16],
    pub key32: [u8; 32],
    pub token_base: [u8; 16],
}


pub fn derive_keys(auth_key: &str, domain: &str) -> anyhow::Result<DerivedKeys> {
    let k = Blake3::digest_vec(auth_key.as_bytes());
    let d = Blake3::digest_vec(domain.as_bytes());
    let key16: [u8; 16] = Pbkdf2HmacSha256::derive(&k, &d, 16)?
        .try_into()
        .map_err(|v: Vec<u8>| anyhow!("key16 derived {} bytes, expected 16", v.len()))?;
    let key32: [u8; 32] = Pbkdf2HmacSha256::derive(&k, &d, 32)?
        .try_into()
        .map_err(|v: Vec<u8>| anyhow!("key32 derived {} bytes, expected 32", v.len()))?;
    let token_base: [u8; 16] = HkdfSha256::derive(&key32, &key16, 16)?
        .try_into()
        .map_err(|v: Vec<u8>| anyhow!("token_base derived {} bytes, expected 16", v.len()))?;
    Ok(DerivedKeys {
        key16,
        key32,
        token_base,
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    /// 测试核心加密/解密逻辑是否完全可逆
    #[test]
    fn test_xoroshiro_reversibility() {
        let original_data: u128 = 0x1234567890ABCDEF_FEDCBA0987654321;

        // 模拟 token_gen 中的 6 轮混淆
        let mut encrypted = original_data;
        for _ in 0..6 {
            encrypted = xoroshiro128(encrypted);
        }

        assert_ne!(original_data, encrypted, "数据经过混淆后应当发生改变");

        // 模拟 token_anth 中的 6 轮还原
        let mut decrypted = encrypted;
        for _ in 0..6 {
            decrypted = dexoroshiro128(decrypted);
        }

        assert_eq!(
            original_data, decrypted,
            "解密后的数据必须与原始数据完全一致"
        );
    }

    /// 测试正常的 Token 生成和校验（Happy Path）
    #[test]
    fn test_token_valid() {
        let base = b"1234567890123456";
        let now = 1700000000;
        let nonce = 999; // 不能为0

        let token = token_gen(base, now, nonce);
        println!("Generated Token: {}", token);

        let is_valid = token_anth(&token, base, now);
        assert!(is_valid, "正常生成的 Token 应当校验通过");
    }

    #[test]
    fn test_token_wrong_base() {
        let base_correct = b"16_bytes_secret_";
        let base_wrong = b"16_bytes_WRONG__";
        let now = 100_000;
        let nonce = 123;

        let token = token_gen(base_correct, now, nonce);

        // 使用错误的密钥去验证
        let is_valid = token_anth(&token, base_wrong, now);
        assert!(!is_valid, "使用错误的密钥不应当校验通过");
    }
}
