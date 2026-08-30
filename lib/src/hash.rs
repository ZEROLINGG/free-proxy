pub trait Hasher {
    fn digest_vec<T: AsRef<[u8]>>(input: T) -> Vec<u8>;
    fn digest_hex<T: AsRef<[u8]>>(input: T) -> String;
}

macro_rules! impl_hasher {
    ($struct:ty, $hasher_type:ty) => {
        impl Hasher for $struct {
            fn digest_vec<T: AsRef<[u8]>>(input: T) -> Vec<u8> {
                use digest::Digest;
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

// ====== SHA-1 实现 ======
pub struct Sha1;
impl_hasher!(Sha1, sha1::Sha1);

// ====== SHA-2 系列实现 ======
pub struct Sha256;
impl_hasher!(Sha256, sha2::Sha256);

pub struct Sha512;
impl_hasher!(Sha512, sha2::Sha512);



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

    // 辅助函数：运行所有的通用测试
    fn run_all_generic_tests<H: Hasher>(label: &str) {
        test_deterministic::<H>(label);
        test_diff_input::<H>(label);
        test_hex_output::<H>(label);
    }

    #[test]
    fn test_sha1() {
        run_all_generic_tests::<Sha1>("sha1");
    }

    #[test]
    fn test_sha256() {
        run_all_generic_tests::<Sha256>("sha256");
    }

    #[test]
    fn test_sha512() {
        run_all_generic_tests::<Sha512>("sha512");
    }



    #[test]
    fn test_known_sha1_vector() {
        // SHA1("abc") = a9993e364706816aba3e25717850c26c9cd0d89d
        let hex = Sha1::digest_hex(b"abc");
        assert_eq!(hex, "a9993e364706816aba3e25717850c26c9cd0d89d");
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


}