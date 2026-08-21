// lib/src/proxy/mod.rs
//
// 本地 HTTP 代理的门面与生命周期：
//   - ProxyConfig：对外配置入口
//   - Shared：跨连接共享状态（密钥、算法、上游 client、优选 IP、TLS）
//   - Proxy：启停、算法/优选 IP 热切换、可用性检测
//   - 协议细节按职责分布在子模块：connection（连接分发）、relay（转发引擎）、
//     body（请求体边界解析）、client（上游通信层）、tls（MITM）、ws（WS 隧道）
//
// 明文 HTTP / CONNECT 隧道内 HTTPS 统一为泛型 serve() 循环（编译期单态化两份）；
// keep-alive：收到 EOS 且客户端未断开即可复用连接。

mod body;
mod client;
mod connection;
mod relay;
mod tls;
mod ws;

pub use client::{check_proxy_availability, ProxyCheck};
pub use tls::TlsManager;
pub use crate::algo::{ProxyAead, ProxyAlgo, ProxyCompressor};
pub use crate::tool::{gen_auth_token, xoroshiro128};

use anyhow::{bail, ensure, Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::http::{split_host_port, UrlBuilder};
use crate::tool::derive_keys;
use client::{build_main_client, build_pref_client, build_ws_client};
use connection::{handle_connection, is_benign_disconnect};

#[derive(Clone, Debug)]
pub struct ProxyConfig {
    pub port: u16,                // 本地代理监听端口
    pub domain: String,
    /// 是否通过https连接worker 一般不推荐
    pub use_https: bool,
    pub auth_key: String,
    pub ca_dir: PathBuf,
    /// CA 私钥保护密钥（32B，由设备 uid + 随机盐派生，见 derive_ca_key_secret）
    pub ca_key_secret: [u8; 32],
    pub compressor: ProxyCompressor,
    pub aead: ProxyAead,
    /// 可选的优选 IP
    pub pref_ip: Option<String>,
}

/// 跨连接共享的状态：密钥、算法、上游 client、优选 IP、TLS 管理器。
pub(crate) struct Shared {
    pub(crate) worker_url: String,
    pub(crate) token_base: [u8; 16],
    pub(crate) key16: [u8; 16],
    pub(crate) key32: [u8; 32],
    algo: RwLock<ProxyAlgo>,
    client: reqwest::Client,
    /// WebSocket 隧道专用 client：必须 HTTP/1.1（升级请求不能走 h2）
    ws_client: RwLock<reqwest::Client>,
    pref_ip: RwLock<Option<String>>,
    pref_client: RwLock<Option<reqwest::Client>>,
    pub(crate) tls: Arc<TlsManager>,
}

impl Shared {
    pub(crate) fn new(cfg: &ProxyConfig) -> Result<Self> {
        // domain 不允许携带端口：worker 侧密钥派生与 token 校验均基于纯 host（env secret
        // "domain"），带端口会导致两端 token 不匹配（全链路 401）。
        let (_host, port) = split_host_port(&cfg.domain).context("invalid domain")?;
        ensure!(
            port.is_none(),
            "domain must not contain a port, got {:?}",
            cfg.domain
        );

        let worker_url = UrlBuilder::new()
            .https(cfg.use_https)
            .host(cfg.domain.as_str())
            .build()?;

        let derived = derive_keys(&cfg.auth_key, &cfg.domain)?;
        let (key16, key32, token_base) = (derived.key16, derived.key32, derived.token_base);

        let client = build_main_client()?;
        let tls = Arc::new(TlsManager::init(&cfg.ca_dir, &cfg.ca_key_secret)?);

        let initial_algo = ProxyAlgo::new(cfg.compressor, cfg.aead);

        // 初始化优选IP相关的Client
        let pref_ip_str = cfg
            .pref_ip
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let initial_pref_ip = pref_ip_str.map(str::to_owned);
        let pref_client = build_pref_client(&worker_url, pref_ip_str)?;
        let ws_client = build_ws_client(&worker_url, pref_ip_str)?;

        Ok(Self {
            worker_url,
            token_base,
            key16,
            key32,
            algo: RwLock::new(initial_algo),
            client,
            ws_client: RwLock::new(ws_client),
            pref_ip: RwLock::new(initial_pref_ip),
            pref_client: RwLock::new(pref_client),
            tls,
        })
    }

    /// 当前启用的算法组合
    pub(crate) fn algo(&self) -> ProxyAlgo {
        *self
            .algo
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 热切换压缩/加密算法：对后续请求立即生效，无需重启代理
    pub(crate) fn set_algo(&self, compressor: ProxyCompressor, aead: ProxyAead) {
        let mut guard = self
            .algo
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = ProxyAlgo::new(compressor, aead);
    }

    /// 热切换 AEAD 加密算法（接受字符串，如 "aes128gcm" / "chacha20poly1305"）
    pub(crate) fn set_aead(&self, aead: &str) -> Result<()> {
        let aead: ProxyAead = aead
            .parse()
            .with_context(|| format!("invalid aead: {aead:?}"))?;

        let mut guard = self
            .algo
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.aead = aead;
        Ok(())
    }

    /// 热切换压缩算法（接受字符串，如 "zstd" / "gzip" / "lz4"）
    pub(crate) fn set_compressor(&self, compressor: &str) -> Result<()> {
        let compressor: ProxyCompressor = compressor
            .parse()
            .with_context(|| format!("invalid compressor: {compressor:?}"))?;

        let mut guard = self
            .algo
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.compressor = compressor;
        Ok(())
    }

    /// 当前优选 IP（None 表示走 DNS 解析）
    pub(crate) fn ip(&self) -> Option<String> {
        let guard = self
            .pref_ip
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clone()
    }

    /// 热切换优选 IP：Some(ip) 时构建带 `.resolve(domain, ip:port)` 的专用
    /// reqwest client（SNI/Host 仍是域名）；None 清除并回退 DNS 解析。
    pub(crate) fn set_ip(&self, ip: Option<&str>) -> Result<()> {
        let ip = ip.map(str::trim).filter(|s| !s.is_empty());
        let client = build_pref_client(&self.worker_url, ip)?;
        let ws_client = build_ws_client(&self.worker_url, ip)?;

        let mut ip_guard = self
            .pref_ip
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut client_guard = self
            .pref_client
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut ws_client_guard = self
            .ws_client
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *ip_guard = ip.map(str::to_owned);
        *client_guard = client;
        *ws_client_guard = ws_client;
        Ok(())
    }

    /// 当前生效的上游 client（存在优选 IP 时用其专用 client，否则主 client）
    pub(crate) fn get_client(&self) -> reqwest::Client {
        self.pref_client
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .unwrap_or_else(|| self.client.clone())
    }

    /// WebSocket 隧道专用 client（HTTP/1.1）
    pub(crate) fn ws_client(&self) -> reqwest::Client {
        self.ws_client
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

pub struct Proxy {
    cfg: ProxyConfig,
    shared: Arc<Shared>,
    task: Option<JoinHandle<Result<()>>>,
}

impl Proxy {
    pub fn new(mut cfg: ProxyConfig) -> Result<Self> {
        ensure!(!cfg.auth_key.is_empty(), "auth_key must not be empty");
        cfg.domain = cfg.domain.trim().to_string();

        let shared = Arc::new(Shared::new(&cfg)?);

        Ok(Self {
            cfg,
            shared,
            task: None,
        })
    }

    /// 启动本地 HTTP 代理（仅监听 127.0.0.1），返回实际端口
    pub async fn start(&mut self) -> Result<u16> {
        if self.task.is_some() {
            return Err(anyhow::anyhow!("proxy already started"));
        }

        let listener = TcpListener::bind(("127.0.0.1", self.cfg.port))
            .await
            .with_context(|| format!("failed to bind 127.0.0.1:{}", self.cfg.port))?;
        let port = listener.local_addr()?.port();
        let shared = Arc::clone(&self.shared);

        let task = tokio::spawn(async move {
            loop {
                let (socket, addr) = match listener.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("proxy: accept error: {e}");
                        break;
                    }
                };
                let shared = Arc::clone(&shared);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(socket, shared).await {
                        if is_benign_disconnect(&e) {
                            #[cfg(debug_assertions)]
                            eprintln!("proxy: connection {addr}: {e:#} (benign disconnect)");
                        } else {
                            eprintln!("proxy: connection {addr}: {e:#?}");
                        }
                    }
                });
            }
            Ok(())
        });

        self.task = Some(task);
        Ok(port)
    }

    pub async fn check_availability(&self) -> Result<ProxyCheck> {
        if !self.is_running() {
            bail!("proxy is not running (call start() first)");
        }
        check_proxy_availability(self.cfg.port).await
    }

    /// 停止代理（中止监听循环，不等待）
    pub async fn stop(&mut self) {
        if let Some(handle) = self.task.take() {
            handle.abort();
            let _ = handle.await;
        }
    }

    pub fn port(&self) -> u16 {
        self.cfg.port
    }

    pub fn is_running(&self) -> bool {
        self.task.is_some()
    }

    pub fn ca_cert_path(&self) -> PathBuf {
        TlsManager::ca_cert_path(&self.cfg.ca_dir)
    }

    /// 当前启用的算法组合
    pub fn algo(&self) -> ProxyAlgo {
        self.shared.algo()
    }

    /// 热切换压缩/加密算法：对后续请求立即生效，无需重启代理
    pub fn set_algo(&self, compressor: ProxyCompressor, aead: ProxyAead) {
        self.shared.set_algo(compressor, aead);
    }

    /// 热切换 AEAD 加密算法（接受字符串，如 "aes128gcm" / "chacha20poly1305"）
    pub fn set_aead(&self, aead: &str) -> Result<()> {
        self.shared.set_aead(aead)
    }

    /// 热切换压缩算法（接受字符串，如 "zstd" / "gzip" / "lz4"）
    pub fn set_compressor(&self, compressor: &str) -> Result<()> {
        self.shared.set_compressor(compressor)
    }

    /// 当前优选 IP（None 表示走 DNS 解析）
    pub fn ip(&self) -> Option<String> {
        self.shared.ip()
    }

    /// 热切换优选 IP：Some(ip) 时构建带 `.resolve(domain, ip:port)` 的专用
    /// reqwest client（SNI/Host 仍是域名）；None 清除并回退 DNS 解析。
    pub fn set_ip(&self, ip: Option<&str>) -> Result<()> {
        self.shared.set_ip(ip)
    }
}
