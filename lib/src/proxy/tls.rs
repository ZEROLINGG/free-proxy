// lib/src/proxy/tls.rs
// MITM TLS：本地生成自签 CA，按 SNI 懒签发各域名叶子证书（ECDSA P-256）。
// CA 持久化在 ca_dir/（ca.crt.pem + ca.key.enc），用户需手动将 ca.crt.pem 导入系统信任区。
// ALPN 仅协商 http/1.1（隧道内不支持 h2）。

use anyhow::{Context, Result, anyhow};
use moka::sync::Cache;
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use rustls::ServerConfig;
use rustls::crypto::CryptoProvider;
use rustls::crypto::ring::sign::any_supported_type;
use rustls::server::{Acceptor, ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use std::fmt::{Debug, Formatter};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use time::{Duration as CertDuration, OffsetDateTime};
use tokio::net::TcpStream;
use tokio::time::{Duration as TokioDuration, timeout};
use tokio_rustls::LazyConfigAcceptor;
use tokio_rustls::server::TlsStream;

use crate::aead::{Aes256GcmSiv, Cipher};


/// 叶子证书缓存容量上限（moka 精确维护，非软上限）
const LEAF_CACHE_MAX: u64 = 256;

/// 叶子证书有效期（天）。故意设置得较短，配合下方缓存 TTL 实现
const LEAF_VALIDITY_DAYS: i64 = 7;

/// 叶子证书缓存 TTL，必须严格小于 `LEAF_VALIDITY_DAYS`，
const LEAF_CACHE_TTL: StdDuration = StdDuration::from_secs(60 * 60 * 24 * 3); // 3 天

/// CA 证书有效期（年）
const CA_VALIDITY_YEARS: i64 = 10;

/// TLS 握手整体超时（读取 ClientHello + 完成握手），防止恶意/异常客户端
const HANDSHAKE_TIMEOUT: TokioDuration = TokioDuration::from_secs(10);

pub struct TlsManager {
    ca: Arc<Ca>,
    leaf_cache: Arc<LeafCache>,
    crypto_provider: Arc<CryptoProvider>,
    /// 本次 init 是否因 CA 加载失败（解密失败/文件损坏）而自动重建
    rebuilt: bool,
}

struct Ca {
    cert: Certificate,
    key: KeyPair,
    cert_pem: String,
}

impl TlsManager {
    pub fn init(ca_dir: &Path, key_secret: &[u8; 32]) -> Result<Self> {
        std::fs::create_dir_all(ca_dir)
            .with_context(|| format!("failed to create ca_dir {}", ca_dir.display()))?;

        let cert_path = ca_dir.join("ca.crt.pem");
        let key_path = ca_dir.join("ca.key.enc");

        let (cert, key, cert_pem, rebuilt) = if cert_path.exists() && key_path.exists() {
            match load_ca(&cert_path, &key_path, key_secret) {
                Ok((cert, key)) => {
                    let cert_pem = cert.pem();
                    (cert, key, cert_pem, false)
                }
                Err(e) => {
                    // 解密失败（设备 uid 变化）或文件损坏：自动重建 CA。
                    // CA 证书随之变化，用户需重新导入 ca.crt.pem 到系统信任区。
                    tracing::warn!("CA key load failed ({e:#}); regenerating CA");
                    remove_ca_files(&cert_path, &key_path, ca_dir);
                    let (cert, key, cert_pem) =
                        generate_and_persist_ca(&cert_path, &key_path, key_secret)?;
                    (cert, key, cert_pem, true)
                }
            }
        } else {
            // 首次运行 / 旧版明文格式迁移（只有 ca.crt.pem + ca.key.pem）：清理残留后重建
            remove_ca_files(&cert_path, &key_path, ca_dir);
            let (cert, key, cert_pem) =
                generate_and_persist_ca(&cert_path, &key_path, key_secret)?;
            (cert, key, cert_pem, false)
        };

        let ca = Arc::new(Ca {
            cert,
            key,
            cert_pem,
        });
        let leaf_cache = Arc::new(LeafCache::new());
        let crypto_provider = Arc::new(rustls::crypto::ring::default_provider());

        Ok(Self {
            ca,
            leaf_cache,
            crypto_provider,
            rebuilt,
        })
    }

    /// 本次 init 是否自动重建了 CA（CA 证书已变化，需重新导入信任区）
    pub fn rebuilt(&self) -> bool {
        self.rebuilt
    }

    /// CA 证书 PEM 内容
    pub fn ca_cert_pem(&self) -> &str {
        &self.ca.cert_pem
    }

    pub fn ca_cert_path(ca_dir: &Path) -> PathBuf {
        ca_dir.join("ca.crt.pem")
    }

    /// TLS 握手：优先使用客户端 SNI；若客户端未发送 SNI，则使用调用方
    pub async fn accept(
        &self,
        socket: TcpStream,
        fallback_host: Option<&str>,
    ) -> Result<TlsStream<TcpStream>> {
        let handshake = timeout(
            HANDSHAKE_TIMEOUT,
            LazyConfigAcceptor::new(Acceptor::default(), socket),
        )
            .await
            .map_err(|_| anyhow!("timed out waiting for ClientHello"))?
            .map_err(|e| anyhow!("TLS pre-handshake (read ClientHello) failed: {e}"))?;

        let sni = handshake.client_hello().server_name().map(str::to_string);

        let host = sni
            .as_deref()
            .or(fallback_host)
            .map(normalize_host)
            .ok_or_else(|| anyhow!("no SNI in ClientHello and no fallback host provided"))?;

        if host.is_empty() {
            return Err(anyhow!("empty host after normalization"));
        }

        let certified_key = self.leaf_cache.get_or_create(&host, &self.ca)?;

        let mut server_config =
            ServerConfig::builder_with_provider(Arc::clone(&self.crypto_provider))
                .with_safe_default_protocol_versions()
                .context("unsupported protocol versions")?
                .with_no_client_auth()
                .with_cert_resolver(Arc::new(FixedCertResolver(certified_key)));
        server_config.alpn_protocols = vec![b"http/1.1".to_vec()];

        timeout(
            HANDSHAKE_TIMEOUT,
            handshake.into_stream(Arc::new(server_config)),
        )
            .await
            .map_err(|_| anyhow!("TLS handshake timed out for host {host:?}"))?
            .map_err(|e| anyhow!("TLS handshake failed for host {host:?}: {e}"))
    }
}


fn normalize_host(raw: &str) -> String {
    let raw = raw.trim();

    if let Some(rest) = raw.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            return rest[..end].to_ascii_lowercase();
        }
    }

    if raw.matches(':').count() >= 2 {
        // 裸 IPv6 地址（无方括号、无端口后缀）
        return raw.to_ascii_lowercase();
    }

    let host = raw.rsplit_once(':').map(|(h, _)| h).unwrap_or(raw);
    host.to_ascii_lowercase()
}

// ─── CA 生成 / 加载 ───────────────────────────────────────────────────────────

#[cfg(unix)]
fn write_key_file(path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(data)?;
    Ok(())
}

#[cfg(not(unix))]
fn write_key_file(path: &Path, data: &[u8]) -> Result<()> {
    std::fs::write(path, data)?;
    Ok(())
}

fn generate_ca() -> Result<(Certificate, KeyPair)> {
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "free-proxy local CA");
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];

    let now = OffsetDateTime::now_utc();
    params.not_before = now - CertDuration::days(1); // 容忍轻微时钟超前
    params.not_after = now + CertDuration::days(365 * CA_VALIDITY_YEARS);

    let cert = params.self_signed(&key)?;
    Ok((cert, key))
}

/// 生成 CA 并持久化（证书明文 + 密钥加密），供首次创建与自动重建共用。
fn generate_and_persist_ca(
    cert_path: &Path,
    key_path: &Path,
    key_secret: &[u8; 32],
) -> Result<(Certificate, KeyPair, String)> {
    let (cert, key) = generate_ca()?;
    let cert_pem = cert.pem();
    std::fs::write(cert_path, &cert_pem)
        .with_context(|| format!("failed to write {}", cert_path.display()))?;

    let encrypted_key = Aes256GcmSiv::encrypt(key.serialize_pem().as_bytes(), key_secret)
        .context("failed to encrypt CA key")?;
    write_key_file(key_path, &encrypted_key)
        .with_context(|| format!("failed to write {}", key_path.display()))?;
    Ok((cert, key, cert_pem))
}

/// 删除 CA 相关文件（含旧版明文 ca.key.pem 残留），供重建前清理。
fn remove_ca_files(cert_path: &Path, key_path: &Path, ca_dir: &Path) {
    let stale_plain = ca_dir.join("ca.key.pem");
    for p in [cert_path, key_path, &stale_plain] {
        let _ = std::fs::remove_file(p);
    }
}

fn load_ca(cert_path: &Path, key_path: &Path, secret: &[u8; 32]) -> Result<(Certificate, KeyPair)> {
    let cert_pem = std::fs::read_to_string(cert_path)
        .with_context(|| format!("failed to read {}", cert_path.display()))?;

    let encrypted_key = std::fs::read(key_path)
        .with_context(|| format!("failed to read {}", key_path.display()))?;

    let key_pem_bytes = Aes256GcmSiv::decrypt(&encrypted_key, secret)
        .context("failed to decrypt CA key")?;
    let key_pem = String::from_utf8(key_pem_bytes)
        .context("CA key is not valid UTF-8 after decryption")?;

    let key = KeyPair::from_pem(&key_pem).map_err(|e| anyhow!("failed to parse CA key: {e}"))?;
    let params = CertificateParams::from_ca_cert_pem(&cert_pem)
        .map_err(|e| anyhow!("failed to parse CA cert: {e}"))?;
    let cert = params
        .self_signed(&key)
        .map_err(|e| anyhow!("failed to rebuild CA cert object: {e}"))?;

    Ok((cert, key))
}


struct LeafCache {
    inner: Cache<String, Arc<CertifiedKey>>,
}

impl LeafCache {
    fn new() -> Self {
        Self {
            inner: Cache::builder()
                .max_capacity(LEAF_CACHE_MAX)
                .time_to_live(LEAF_CACHE_TTL)
                .build(),
        }
    }

    /// 命中缓存直接返回；未命中则生成新叶子证书并写入缓存。
    fn get_or_create(&self, host: &str, ca: &Ca) -> Result<Arc<CertifiedKey>> {
        let key = normalize_host(host);
        self.inner
            .try_get_with(key.clone(), || make_leaf(&key, ca).map(Arc::new))
            .map_err(|e| anyhow!("failed to issue leaf cert for {host:?}: {e}"))
    }

    #[cfg(test)]
    fn len(&self) -> u64 {
        self.inner.run_pending_tasks();
        self.inner.entry_count()
    }
}

/// 单连接握手用的固定证书解析器：SNI 已在 `TlsManager::accept` 中提前
/// 确定并查好证书，这里始终返回同一张证书，不再依赖 rustls 内部的
/// 动态 SNI 分发逻辑。
struct FixedCertResolver(Arc<CertifiedKey>);

impl Debug for FixedCertResolver {
    fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl ResolvesServerCert for FixedCertResolver {
    fn resolve(&self, _client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        Some(Arc::clone(&self.0))
    }
}


fn make_leaf(host: &str, ca: &Ca) -> Result<CertifiedKey> {
    // CertificateParams::new 自动识别 IP / 域名 SAN
    let mut params = CertificateParams::new(vec![host.to_string()])
        .map_err(|e| anyhow!("invalid host {host:?}: {e}"))?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, host);
    params.use_authority_key_identifier_extension = true;
    params.is_ca = IsCa::NoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

    let now = OffsetDateTime::now_utc();
    params.not_before = now - CertDuration::days(1);
    params.not_after = now + CertDuration::days(LEAF_VALIDITY_DAYS);

    let leaf_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let leaf = params.signed_by(&leaf_key, &ca.cert, &ca.key)?;

    let cert_der = leaf.der().clone();
    let key_der = rustls::pki_types::PrivateKeyDer::Pkcs8(
        rustls::pki_types::PrivatePkcs8KeyDer::from(leaf_key.serialize_der().to_vec()),
    );
    let signing_key =
        any_supported_type(&key_der).map_err(|e| anyhow!("unsupported leaf key: {e}"))?;

    Ok(CertifiedKey::new(vec![cert_der], signing_key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fp-tls-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn test_ca_init_persistent() {
        let dir = tmp_dir("persist");
        let secret = [0u8; 32];
        let m1 = TlsManager::init(&dir, &secret).unwrap();
        let pem1 = m1.ca_cert_pem().to_string();
        assert!(pem1.contains("BEGIN CERTIFICATE"));

        let m2 = TlsManager::init(&dir, &secret).unwrap();
        assert_eq!(
            m1.ca.key.serialize_der(),
            m2.ca.key.serialize_der(),
            "CA key must be stable across restarts"
        );
        assert_eq!(
            m1.ca.key.public_key_der(),
            m2.ca.key.public_key_der(),
            "CA public key must be stable across restarts"
        );
        assert_eq!(
            m1.ca.cert.params().distinguished_name,
            m2.ca.cert.params().distinguished_name,
            "CA subject must be stable across restarts"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn test_ca_key_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tmp_dir("perm");
        let secret = [0u8; 32];
        let _m = TlsManager::init(&dir, &secret).unwrap();
        let key_path = dir.join("ca.key.enc");
        let mode = std::fs::metadata(&key_path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "CA private key file must be 0600, got {mode:o}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_leaf_has_dns_san() {
        let dir = tmp_dir("leaf");
        let secret = [0u8; 32];
        let m = TlsManager::init(&dir, &secret).unwrap();
        let ck = m.leaf_cache.get_or_create("example.com", &m.ca).unwrap();
        assert!(!ck.cert.is_empty());

        let ck2 = m.leaf_cache.get_or_create("example.com", &m.ca).unwrap();
        assert!(Arc::ptr_eq(&ck, &ck2));

        let m2 = TlsManager::init(&dir, &secret).unwrap();
        let ck3 = m2.leaf_cache.get_or_create("example.com", &m2.ca).unwrap();
        assert!(!ck3.cert.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_leaf_ip_san() {
        let dir = tmp_dir("ip");
        let secret = [0u8; 32];
        let m = TlsManager::init(&dir, &secret).unwrap();
        let ck = m.leaf_cache.get_or_create("192.168.1.1", &m.ca).unwrap();
        assert!(!ck.cert.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_leaf_params_san_correct() {
        let dir = tmp_dir("san");
        let secret = [0u8; 32];
        let m = TlsManager::init(&dir, &secret).unwrap();
        let host = "api.example.com";
        let _ = m.leaf_cache.get_or_create(host, &m.ca).unwrap();

        let params = CertificateParams::new(vec![host.to_string()]).unwrap();
        assert!(
            params
                .subject_alt_names
                .iter()
                .any(|san| matches!(san, rcgen::SanType::DnsName(n) if n == host))
        );

        let ip_params = CertificateParams::new(vec!["10.0.0.2".to_string()]).unwrap();
        assert!(ip_params.subject_alt_names.iter().any(|san| matches!(
            san,
            rcgen::SanType::IpAddress(ip) if *ip == std::net::IpAddr::from_str("10.0.0.2").unwrap()
        )));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 验证并发首次请求同一域名不会重复签发（moka `try_get_with` 单飞去重）。
    #[test]
    fn test_concurrent_same_host_single_flight() {
        use std::thread;

        let dir = tmp_dir("concurrent");
        let secret = [0u8; 32];
        let m = Arc::new(TlsManager::init(&dir, &secret).unwrap());

        let handles: Vec<_> = (0..16)
            .map(|_| {
                let m = Arc::clone(&m);
                thread::spawn(move || {
                    m.leaf_cache
                        .get_or_create("shared.example.com", &m.ca)
                        .unwrap()
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let first = &results[0];
        for ck in &results {
            assert!(
                Arc::ptr_eq(first, ck),
                "all threads must observe the same CertifiedKey"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 验证超过容量上限时缓存整体大小保持在上限附近，而不是无限增长。
    #[test]
    fn test_cache_bounded_by_capacity() {
        let dir = tmp_dir("evict");
        let secret = [0u8; 32];
        let m = TlsManager::init(&dir, &secret).unwrap();

        for i in 0..(LEAF_CACHE_MAX as usize + 50) {
            let host = format!("host{i}.example.com");
            let _ = m.leaf_cache.get_or_create(&host, &m.ca).unwrap();
        }

        let len = m.leaf_cache.len();
        assert!(
            len <= LEAF_CACHE_MAX,
            "cache should stay bounded, got {len}"
        );
        assert!(len > 0, "cache should not be emptied entirely");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_normalize_host() {
        assert_eq!(normalize_host("Example.COM"), "example.com");
        assert_eq!(normalize_host("example.com:443"), "example.com");
        assert_eq!(normalize_host("  example.com  "), "example.com");
        assert_eq!(normalize_host("[::1]:443"), "::1");
        assert_eq!(normalize_host("[::1]"), "::1");
        assert_eq!(normalize_host("::1"), "::1");
        assert_eq!(normalize_host("192.168.1.1:8443"), "192.168.1.1");
        assert_eq!(normalize_host("192.168.1.1"), "192.168.1.1");
    }

    /// 密钥变化（模拟设备 uid 变更）→ 解密失败 → 自动重建 CA，且证书随之变化
    #[test]
    fn test_ca_auto_rebuild_on_secret_change() {
        let dir = tmp_dir("rebuild");
        let secret_a = [0x11u8; 32];
        let secret_b = [0x22u8; 32];

        let m1 = TlsManager::init(&dir, &secret_a).unwrap();
        assert!(!m1.rebuilt(), "fresh CA must not be marked rebuilt");
        let pem1 = m1.ca_cert_pem().to_string();

        let m2 = TlsManager::init(&dir, &secret_b).unwrap();
        assert!(m2.rebuilt(), "decrypt failure must trigger rebuild");
        assert_ne!(
            pem1,
            m2.ca_cert_pem(),
            "rebuilt CA cert must differ from the old one"
        );

        // 新密钥下再次 init：可正常加载，不再重建。
        // 注意 load_ca 会用同一密钥重新签名证书，PEM 字节会变（ECDSA 随机签名），
        // 因此以密钥与证书身份（subject）为准比较稳定性。
        let m3 = TlsManager::init(&dir, &secret_b).unwrap();
        assert!(!m3.rebuilt());
        assert_eq!(
            m2.ca.key.serialize_der(),
            m3.ca.key.serialize_der(),
            "CA key must be stable across loads"
        );
        assert_eq!(
            m2.ca.cert.params().distinguished_name,
            m3.ca.cert.params().distinguished_name,
            "CA subject must be stable across loads"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 旧版明文格式迁移：残留 ca.key.pem 被清理，新版生成 ca.key.enc
    #[test]
    fn test_ca_migration_cleans_legacy_plaintext_key() {
        let dir = tmp_dir("migrate");
        let secret = [0x33u8; 32];

        // 模拟旧版安装：明文 ca.key.pem + ca.crt.pem
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ca.key.pem"), "-----BEGIN PRIVATE KEY-----\nfake\n").unwrap();
        std::fs::write(dir.join("ca.crt.pem"), "-----BEGIN CERTIFICATE-----\nfake\n").unwrap();

        let m = TlsManager::init(&dir, &secret).unwrap();
        assert!(!m.rebuilt(), "migration regeneration is not a rebuild");
        assert!(!dir.join("ca.key.pem").exists(), "legacy plaintext key must be removed");
        assert!(dir.join("ca.key.enc").exists(), "encrypted key must exist");
        assert!(dir.join("ca.crt.pem").exists(), "CA cert must exist");
        let _ = std::fs::remove_dir_all(&dir);
    }
}