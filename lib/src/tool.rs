use crate::aead::{Ascon128, Cipher};
use crate::base::{Base64, Encoder};
use crate::hash::{Sha256, Hasher};
use crate::kdf::{HkdfSha256, Kdf};
use anyhow::anyhow;

pub fn token_auth(token_tmp: &str, token_base: &[u8; 16], now: u64) -> bool {
    let encrypted_data = match Base64::decode(token_tmp) {
        Ok(data) => data,
        Err(_) => return false,
    };
    let decrypted_payload = match Ascon128::decrypt(encrypted_data, token_base.as_ref()) {
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

    match Ascon128::encrypt(&payload, token_base.as_ref()) {
        Ok(encrypted) => Base64::encode(encrypted).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

#[cfg(feature = "client")]
pub fn gen_auth_token(token_base: &[u8; 16]) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let nonce = rand::random::<u64>();

    token_gen(token_base, now, nonce)
}

#[derive(Clone, Debug)]
pub struct DerivedKeys {
    pub key16: [u8; 16],
    pub key32: [u8; 32],
    pub token_base: [u8; 16],
}


pub fn derive_keys(auth_key: &str, domain: &str) -> anyhow::Result<DerivedKeys> {
    let k = Sha256::digest_vec(auth_key.as_bytes());
    let d = Sha256::digest_vec(domain.as_bytes());
    let key16: [u8; 16] = HkdfSha256::derive(&k, &d, 16)?
        .try_into()
        .map_err(|v: Vec<u8>| anyhow!("key16 derived {} bytes, expected 16", v.len()))?;
    let key32: [u8; 32] = HkdfSha256::derive(&k, &d, 32)?
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
    let ikm = Sha256::digest_vec(device_uid.as_bytes());
    let out = HkdfSha512::derive(ikm, salt, 32)?;
    Ok(out
        .try_into()
        .expect("hkdf output length is pinned to 32 bytes"))
}


#[cfg(test)]
mod tests {
    use super::*;

    const TEST_TOKEN_BASE: &[u8; 16] = b"1234567890123456";
    const BASE_TIME: u64 = 1_600_000_000_000;


    #[test]
    fn test_token_happy_path() {
        let nonce = 123456789;
        let token = token_gen(TEST_TOKEN_BASE, BASE_TIME, nonce);

        assert!(!token.is_empty(), "Token should not be empty");

        assert!(token_auth(&token, TEST_TOKEN_BASE, BASE_TIME));
    }

    #[test]
    fn test_token_time_window_validation() {
        let nonce = 987654321;
        let token = token_gen(TEST_TOKEN_BASE, BASE_TIME, nonce);

        // 测试时间窗口: < 30_000 毫秒 (正负 30 秒)

        // 1. 刚好在窗口内 (未来)
        assert!(token_auth(&token, TEST_TOKEN_BASE, BASE_TIME + 29_999));
        // 2. 刚好在窗口外 (未来，拒绝)
        assert!(!token_auth(&token, TEST_TOKEN_BASE, BASE_TIME + 30_000));

        // 3. 刚好在窗口内 (过去)
        assert!(token_auth(&token, TEST_TOKEN_BASE, BASE_TIME - 29_999));
        // 4. 刚好在窗口外 (过去，拒绝)
        assert!(!token_auth(&token, TEST_TOKEN_BASE, BASE_TIME - 30_000));
    }

    #[test]
    fn test_token_wrong_key_rejected() {
        let nonce = 11111;
        let token = token_gen(TEST_TOKEN_BASE, BASE_TIME, nonce);

        let wrong_key: &[u8; 16] = b"0000000000000000";
        assert!(!token_auth(&token, wrong_key, BASE_TIME));
    }

    #[test]
    fn test_token_tampering_rejected() {
        let nonce = 22222;
        let mut token = token_gen(TEST_TOKEN_BASE, BASE_TIME, nonce);

        let last_char = token.pop().unwrap();
        let tampered_char = if last_char == 'A' { 'B' } else { 'A' };
        token.push(tampered_char);

        assert!(!token_auth(&token, TEST_TOKEN_BASE, BASE_TIME));
    }

    #[test]
    fn test_token_invalid_base64() {
        assert!(!token_auth("not_a_valid_base64_!@#", TEST_TOKEN_BASE, BASE_TIME));
    }



    #[cfg(feature = "client")]
    #[test]
    fn test_gen_auth_token_integration() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let token = gen_auth_token(TEST_TOKEN_BASE);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        assert!(token_auth(&token, TEST_TOKEN_BASE, now));
    }


    #[test]
    fn test_derive_keys_determinism() {
        let auth_key = "super_secret_password";
        let domain = "api.example.com";

        let keys1 = derive_keys(auth_key, domain).unwrap();
        let keys2 = derive_keys(auth_key, domain).unwrap();

        assert_eq!(keys1.key16, keys2.key16);
        assert_eq!(keys1.key32, keys2.key32);
        assert_eq!(keys1.token_base, keys2.token_base);
    }

    #[test]
    fn test_derive_keys_domain_separation() {
        let auth_key = "super_secret_password";

        let keys_domain_a = derive_keys(auth_key, "siteA.com").unwrap();
        let keys_domain_b = derive_keys(auth_key, "siteB.com").unwrap();

        assert_ne!(keys_domain_a.key16, keys_domain_b.key16);
        assert_ne!(keys_domain_a.key32, keys_domain_b.key32);
        assert_ne!(keys_domain_a.token_base, keys_domain_b.token_base);
    }

}