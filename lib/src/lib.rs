pub mod aead;
pub mod algo;
pub mod base;
pub mod compress;
pub mod ecc;
pub mod hash;
pub mod http;
pub mod kdf;
pub mod tool;

#[cfg(feature = "client")]
pub mod proxy;
#[cfg(feature = "client")]
pub mod speed_test;

#[cfg(test)]
mod tests {
    use crate::tool::{derive_keys, gen_auth_token};

    #[test]
    fn it_works() {
        let domain = "free-proxy.bcsz8833221.workers.dev";
        let key = "f5ebb761334cb5551fb3b3722e50ab15";
        let keys = derive_keys(key, domain).unwrap();
        let token = gen_auth_token(&keys.token_base);
        println!("{}", token);
    }
}
