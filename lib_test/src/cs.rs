#![allow(unused)]
use anyhow::{Context, bail};
use lib::algo::{ProxyAead, ProxyCompressor};
use lib::proxy::{Proxy, ProxyConfig};
use lib::tool::{DerivedKeys, derive_keys, gen_auth_token};
use shell_engine::Shell;
use std::env;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::fs;
use tokio::time::sleep;

pub struct Server {
    child: Option<Shell>,
    key: Option<String>,
    crash_flag: Option<Arc<AtomicBool>>,
}

impl Server {
    pub fn new() -> anyhow::Result<Self> {
        match TcpListener::bind("127.0.0.1:80") {
            Ok(listener) => {
                drop(listener);
                Ok(Self {
                    child: None,
                    key: None,
                    crash_flag: None,
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

        let content = format!(
            r#"key = "{key}"
domain = "127.0.0.1"
log = "debug"
"#
        );

        fs::write(&dev_vars_path, content)
            .await
            .with_context(|| format!("Failed to write .dev.vars at {dev_vars_path:?}"))?;

        Ok(())
    }

    fn project_root() -> anyhow::Result<PathBuf> {
        let exe_path = env::current_exe().context("Failed to get current executable path")?;

        let mut current_dir = exe_path
            .parent()
            .context("Failed to get parent directory of exe")?;

        // 兼容 cargo test 产生的 target/debug/deps/ 路径
        if current_dir.ends_with("deps") {
            current_dir = current_dir
                .parent()
                .context("Failed to navigate out of deps directory")?;
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

        // 创建崩溃标志
        let crash_flag = Arc::new(AtomicBool::new(false));
        let crash_flag_clone = crash_flag.clone();

        let mut child = Shell::new(if cfg!(windows) { "powershell" } else { "sh" })
            .enable_pty()
            .work_dir(Self::project_root()?)
            .disable_snapshot()
            .line_callback()
            .on_output(move |line| {
                let flag = crash_flag_clone.clone();
                async move {
                    if !line.trim().is_empty()
                        && !line.contains("[custom build]")
                        && !line.contains(
                        r#"pnpm server-dev;echo "$((222*2)) [ERROR]:server-dev $((222*2))""#,
                    )
                        && !line.contains("Using secrets defined in .dev.vars")
                        && !line.contains("╭───────────────────────────╮")
                        && !(line.contains("│  [b] ") && line.contains("open a browser"))
                        && !(line.contains("│  [d] ") && line.contains("open devtools"))
                        && !(line.contains("│  [e] ") && line.contains("open local explorer"))
                        && !(line.contains("│  [t] ") && line.contains("start tunnel"))
                        && !(line.contains("│  [c] ") && line.contains("clear console"))
                        && !(line.contains("│  [x] ") && line.contains("to exit"))
                        && !line.contains("╰───────────────────────────╯")
                        && !line.contains("Starting local server...")
                        && !line.contains("Local package.json exists, but node_modules missing, did you mean to install?")
                        && !(line.contains("bash") && line.contains("$"))
                        && !(line.contains("> @ server-dev ") && line.contains("free-proxy"))
                        && !line.contains("> cd server-rs && wrangler dev")
                        && !line.contains("If you think this is a bug then please create an issue at")
                        && !line.contains("Command failed with exit code 1.")
                        && !(line.contains("✘") && line.contains("ERROR") && line.contains("[") && line.contains("]") && line.contains(" "))
                        && !line.contains("Note that there is a newer version of Wrangler available")
                        && !line.contains("Windows PowerShell")
                        && !line.contains("Copyright (C) Microsoft Corporation.")
                        && !line.contains("All rights reserved.")
                        && !line.contains("Install the latest PowerShell for new features and improvements!")
                        && !line.contains("https://aka.ms/PSWindows")
                        && !(line.starts_with("PS ") && line.contains(">"))
                        && !line.contains("Loading personal and system profiles took")

                    {
                        if line.contains("444 [ERROR]:server-dev 444") {
                            eprintln!("\x1b[1;31m[ERROR]wrangler crashed abnormally!!!\x1b[0m");
                            // 设置崩溃标志
                            flag.store(true, Ordering::SeqCst);
                        } else {
                            println!("{line}");
                        }
                    }
                }

            })
            .spawn()
            .await?;

        child
            .send_line(r#"pnpm server-dev;echo "$((222*2)) [ERROR]:server-dev $((222*2))""#)
            .await?;

        self.child = Some(child);
        self.key = Some(random_key);
        self.crash_flag = Some(crash_flag); // 保存标志
        self.wait_until_healthy().await
    }

    pub async fn ensure_health(&mut self) -> anyhow::Result<()> {
        let key = self.key.as_deref().context("random key missing")?;
        let keys = derive_keys(key, "127.0.0.1")?;

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .context("failed to build health probe client")?;
        if !self.health(&client, &keys).await && !self.health(&client, &keys).await {
            if let Some(child) = self.child.as_mut() {
                child.reset().await?;
                child
                    .send_line(r#"pnpm server-dev;echo "$((222*2)) [ERROR]:server-dev $((222*2))""#)
                    .await?;
                // 重启后重置崩溃标志
                if let Some(flag) = &self.crash_flag {
                    flag.store(false, Ordering::SeqCst);
                }
                self.wait_until_healthy().await?;
            } else {
                return self.start().await;
            }
        }
        Ok(())
    }

    pub async fn health(&self, client: &reqwest::Client, keys: &DerivedKeys) -> bool {
        let token = gen_auth_token(&keys.token_base);
        let ok = match client
            .get("http://127.0.0.1/health")
            .bearer_auth(token)
            .send()
            .await
        {
            Ok(resp) => {
                resp.status().is_success()
                    && resp
                        .text()
                        .await
                        .unwrap_or_default()
                        .trim()
                        .parse::<u64>()
                        .is_ok()
            }
            Err(_) => false,
        };
        ok
    }

    async fn wait_until_healthy(&self) -> anyhow::Result<()> {
        let key = self.key.as_deref().context("random key missing")?;
        let keys = derive_keys(key, "127.0.0.1")?;

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .context("failed to build health probe client")?;

        let mut consecutive = 0u32;
        for _ in 0..60 * 45 {
            // 首次需要构建
            // 检查崩溃标志，一旦置位立即退出
            if let Some(flag) = &self.crash_flag {
                if flag.load(Ordering::SeqCst) {
                    bail!("Server process exited unexpectedly during startup");
                }
            }

            let ok = self.health(&client, &keys).await;
            consecutive = if ok { consecutive + 1 } else { 0 };
            if consecutive >= 3 {
                return Ok(());
            }
            sleep(Duration::from_secs(1)).await;
        }

        bail!("Server launched but /health never became ready in time");
    }

    pub async fn stop(&mut self) -> anyhow::Result<()> {
        if let Some(mut child) = self.child.take() {
            child.exit().await?;
        }
        self.key = None;
        self.crash_flag = None; // 清理崩溃标志
        Ok(())
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
                    use_https: false, // 本地测试无法启用
                    auth_key: key.into(),
                    ca_dir: env::temp_dir().join("free-proxy.test"),
                    ca_key_secret: *b"0o9i8u7y6t5r3w3rj8wuhq6n26^8je(&",
                    compressor: ProxyCompressor::Lz4,
                    aead: ProxyAead::Aes128Gcm,
                    pref_ip: None, // 本地测试无法启用
                };

                Ok(Self {
                    proxy: Proxy::new(cfg)?,
                })
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
        self.proxy
            .check_availability()
            .await
            .map(|r| (r.ip, r.latency_ms))
    }
}

pub const fn proxy_url() -> &'static str {
    "http://127.0.0.1:18081"
}
