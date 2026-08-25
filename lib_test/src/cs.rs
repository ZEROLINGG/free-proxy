#![allow(unused)]
use std::env;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::Duration;
use anyhow::{bail, Context};
use tokio::fs;
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::time::sleep;
use lib::algo::{ProxyAead, ProxyCompressor};
use lib::proxy::{Proxy, ProxyConfig};

pub struct Server {
    child: Option<Child>,
    key: Option<String>,
}

impl Server {
    pub fn new() -> anyhow::Result<Self> {
        match TcpListener::bind("127.0.0.1:80") {
            Ok(listener) => {
                drop(listener);
                Ok(Self {
                    child: None,
                    key: None,
                })
            }
            Err(e) => bail!("Port 80 is occupied or unavailable: {e}"),
        }
    }

    /// 获取当前的随机 key
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// 生成并写入 .dev.vars 配置文件
    async fn set_dev_vars(key: &str) -> anyhow::Result<()> {
        let project_root = Self::project_root()?;
        let dev_vars_path = project_root.join("server-rs").join(".dev.vars");

        let content = format!("key = \"{key}\"\ndomain = \"127.0.0.1\"\n");

        fs::write(&dev_vars_path, content)
            .await
            .with_context(|| format!("Failed to write .dev.vars at {dev_vars_path:?}"))?;

        Ok(())
    }

    fn project_root() -> anyhow::Result<PathBuf> {
        let exe_path = env::current_exe().context("Failed to get current executable path")?;

        let mut current_dir = exe_path.parent().context("Failed to get parent directory of exe")?;

        // 兼容 cargo test 产生的 target/debug/deps/ 路径
        // 如果当前处在 deps 目录，则多向上回退一层
        if current_dir.ends_with("deps") {
            current_dir = current_dir.parent().context("Failed to navigate out of deps directory")?;
        }

        let project_root = current_dir
            .parent() // -> target
            .and_then(|p| p.parent()) // -> lib_test
            .and_then(|p| p.parent()) // -> free-proxy (project root)
            .context("Failed to resolve project root directory")?;

        Ok(project_root.to_path_buf())
    }

    /// 生成随机 32 位 Hex 字符 key
    fn generate_random_key() -> String {
        let random_u128: u128 = rand::random();
        format!("{random_u128:032x}")
    }

    pub async fn start(&mut self) -> anyhow::Result<()> {
        let random_key = Self::generate_random_key();

        Self::set_dev_vars(&random_key).await?;

        let child = Command::new("pnpm")
            .arg("server-dev")
            .current_dir(Self::project_root()?)
            .spawn()
            .context("Failed to spawn pnpm server-dev")?;

        self.child = Some(child);
        self.key = Some(random_key);

        for _ in 0..60 * 45 {
            if TcpStream::connect("127.0.0.1:80").await.is_ok() {
                sleep(Duration::from_secs(5)).await;
                return Ok(());
            }
            sleep(Duration::from_secs(1)).await;
        }

        bail!("Server launched but failed to bind to port 80 in time");
    }

    pub async fn stop(&mut self) -> anyhow::Result<()> {
        if let Some(mut child) = self.child.take() {
            child.kill().await?;
        }
        self.key = None;
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

pub struct Client {
    pub proxy: Proxy,
}
impl Client {
    pub fn new<S: Into<String>>(key: S) -> anyhow::Result<Self> {
        match TcpListener::bind("127.0.0.1:18081") {
            Ok(listener) => {
                drop(listener);

                let cfg = ProxyConfig {
                    port: 18081,
                    domain: "127.0.0.1".into(),
                    use_https: false,  // 本地测试无法启用
                    auth_key: key.into(),
                    ca_dir: env::temp_dir().join("free-proxy.test"),
                    ca_key_secret: *b"0o9i8u7y6t5r3w3rj8wuhq6n26^8je(&",
                    compressor: ProxyCompressor::Lz4,
                    aead: ProxyAead::Aes128Gcm,
                    pref_ip: None,  // 本地测试无法启用
                };

                Ok(Self { proxy: Proxy::new(cfg)? })
            }
            Err(e) => bail!("Port 18081 is occupied or unavailable: {e}"),
        }
    }

    pub async fn start(&mut self) -> anyhow::Result<u16> {
        self.proxy.start().await
    }
    pub async fn stop(&mut self) {
        self.proxy.stop().await
    }
    pub fn is_running(&self) -> bool {
        self.proxy.is_running()
    }
    pub fn set_aead(&self, aead: &str) -> anyhow::Result<()> {
        self.proxy.set_aead(aead)
    }
    pub fn set_compressor(&self, compressor: &str) -> anyhow::Result<()> {
        self.proxy.set_compressor(compressor)
    }
    pub fn ca_cert_path(&self) -> PathBuf {
        self.proxy.ca_cert_path()
    }
    pub async fn check_availability(&self) -> anyhow::Result<(String, u64)> {
        self.proxy.check_availability().await.map(|r| (r.ip,r.latency_ms))
    }

}

pub const fn proxy_url() -> &'static str { "http://127.0.0.1:18081" }



