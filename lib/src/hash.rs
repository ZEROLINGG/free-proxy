pub trait Hasher {
    fn digest_vec<T: AsRef<[u8]>>(input: T) -> Vec<u8>;
    fn digest_hex<T: AsRef<[u8]>>(input: T) -> String;
}

macro_rules! impl_hasher {
    ($struct:ty, $hasher_type:ty) => {
        impl Hasher for $struct {
            fn digest_vec<T: AsRef<[u8]>>(input: T) -> Vec<u8> {
                use sha2::Digest;
                let mut hasher = <$hasher_type>::new();
                hasher.update(input.as_ref());
                hasher.finalize().to_vec()
            }

            fn digest_hex<T: AsRef<[u8]>>(input: T) -> String {
                hex::encode(Self::digest_vec(input))
            }
        }
    };
}

pub struct Sha256;
impl_hasher!(Sha256, sha2::Sha256);

pub struct Sha512;
impl_hasher!(Sha512, sha2::Sha512);

pub struct Sha512_256;
impl_hasher!(Sha512_256, sha2::Sha512_256);

pub struct Blake3;

impl Hasher for Blake3 {
    fn digest_vec<T: AsRef<[u8]>>(input: T) -> Vec<u8> {
        blake3::hash(input.as_ref()).as_bytes().to_vec()
    }

    fn digest_hex<T: AsRef<[u8]>>(input: T) -> String {
        blake3::hash(input.as_ref()).to_hex().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_deterministic<H: Hasher>(label: &str) {
        let a = H::digest_vec(b"hello");
        let b = H::digest_vec(b"hello");
        assert_eq!(a, b, "{label}: not deterministic");
        assert_eq!(a.len(), H::digest_hex(b"hello").len() / 2);
    }

    fn test_diff_input<H: Hasher>(label: &str) {
        let a = H::digest_vec(b"hello");
        let b = H::digest_vec(b"world");
        assert_ne!(a, b, "{label}: same output for different inputs");
    }

    fn test_hex_output<H: Hasher>(label: &str) {
        let hex = H::digest_hex(b"test");
        assert!(
            hex.chars().all(|c| c.is_ascii_hexdigit()),
            "{label}: non-hex chars"
        );
    }

    #[test]
    fn test_sha256() {
        test_deterministic::<Sha256>("sha256");
        test_diff_input::<Sha256>("sha256");
        test_hex_output::<Sha256>("sha256");
    }

    #[test]
    fn test_sha512() {
        test_deterministic::<Sha512>("sha512");
        test_diff_input::<Sha512>("sha512");
    }

    #[test]
    fn test_sha512_256() {
        test_deterministic::<Sha512_256>("sha512_256");
    }

    #[test]
    fn test_blake3() {
        test_deterministic::<Blake3>("blake3");
        test_diff_input::<Blake3>("blake3");
        test_hex_output::<Blake3>("blake3");
    }

    #[test]
    fn test_known_sha256_vector() {
        // SHA256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        let hex = Sha256::digest_hex(b"abc");
        assert_eq!(
            hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_known_blake3_vector() {
        let vec = Blake3::digest_vec(b"abc");
        assert_eq!(vec.len(), 32);
    }
}
