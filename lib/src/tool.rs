use crate::aead::{Aes128Gcm, Cipher};
use crate::base::{Base91, Encoder};
use crate::hash::{Blake3, Hasher};
use crate::kdf::{HkdfSha256, Kdf, Pbkdf2HmacSha256};
use anyhow::anyhow;

pub fn xoroshiro128(s: u128) -> u128 {
    let mut s0 = (s >> 64) as u64;
    let mut s1 = s as u64;

    s1 ^= s0;
    s0 = s0.rotate_left(24) ^ s1 ^ (s1 << 16);
    s1 = s1.rotate_left(37);

    ((s0 as u128) << 64) | (s1 as u128)
}

pub fn token_auth(token_tmp: &str, token_base: &[u8; 16], now: u64) -> bool {
    let encrypted_data = match Base91::decode(token_tmp) {
        Ok(data) => data,
        Err(_) => return false,
    };
    let decrypted_payload = match Aes128Gcm::decrypt(encrypted_data, token_base.as_ref()) {
        Ok(data) => data,
        Err(_) => return false,
    };
    if decrypted_payload.len() != 16 {
        return false;
    }
    let nonce_bytes: [u8; 8] = decrypted_payload[0..8].try_into().unwrap();
    let time_bytes: [u8; 8] = decrypted_payload[8..16].try_into().unwrap();
    let nonce = u64::from_be_bytes(nonce_bytes);
    let token_time = u64::from_be_bytes(time_bytes);
    if nonce != 0 {
        if now.abs_diff(token_time) < 30_000 {
            return true;
        }
    }
    false
}
pub fn token_gen(token_base: &[u8; 16], now: u64, nonce: u64) -> String {
    let mut payload = [0u8; 16];
    payload[0..8].copy_from_slice(&nonce.to_be_bytes());
    payload[8..16].copy_from_slice(&now.to_be_bytes());

    match Aes128Gcm::encrypt(&payload, token_base.as_ref()) {
        Ok(encrypted) => Base91::encode(encrypted).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

#[cfg(feature = "client")]
pub fn gen_auth_token(token_base: &[u8; 16]) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::LazyLock;
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


/// 由设备 uid 派生 CA 私钥保护密钥（32B）：
#[cfg(feature = "client")]
pub fn derive_ca_key_secret(device_uid: &str, salt: &[u8]) -> anyhow::Result<[u8; 32]> {
    use crate::kdf::{HkdfSha512, Kdf};
    let ikm = Blake3::digest_vec(device_uid.as_bytes());
    let out = HkdfSha512::derive(ikm, salt, 32)?;
    Ok(out
        .try_into()
        .expect("hkdf output length is pinned to 32 bytes"))
}


