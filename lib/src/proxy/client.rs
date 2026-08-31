// lib/src/proxy/client.rs

use anyhow::{bail, Context, Result};
use std::net::{IpAddr, SocketAddr};
use std::time::SystemTime;
use tokio::task::JoinSet;
use tokio::time::{Duration, Instant};
use crate::{debug, info, warn};
use crate::http::{url_parse, UrlBuilder};

/// 与 worker 建立连接的超时 (保持短超时，实现快速失败)
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);

/// 代理可用性检测结果
#[derive(Debug, Clone)]
pub struct ProxyCheck {
    pub ip: String,
    pub latency_ms: u64,
}

const HEALTH_CHECK_ENDPOINTS: &[&str] = &[
    "http://api.ipify.org",
    "http://ip.me",
    "http://icanhazip.com",
    "http://ifconfig.me/ip",
];

/// 通过本地代理请求外部接口，返回出口 IP 与整链路耗时（最长 10s）。
pub async fn check_proxy_availability(port: u16) -> Result<ProxyCheck> {
    let proxy_url = UrlBuilder::new()
        .https(false)
        .host("127.0.0.1")
        .port(port)
        .build()
        .context("failed to build local proxy url")?;

    let proxy = reqwest::Proxy::all(&proxy_url).context("failed to configure proxy setting")?;

    let check_client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_secs(10))
        .danger_accept_invalid_certs(true) // 允许中间人/自签证书
        .build()
        .context("failed to build check client")?;

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_millis();

    let start = Instant::now();
    let mut set = JoinSet::new();

    for i in 0..8 {
        let client = check_client.clone();
        let endpoint = HEALTH_CHECK_ENDPOINTS[i % HEALTH_CHECK_ENDPOINTS.len()];
        let query = format!("{now}_{i}");
        let url = UrlBuilder::new()
            .base(endpoint)
            .append_query("_t", &query)
            .build()
            .with_context(|| format!("failed to build url for {endpoint}"))?;

        set.spawn(async move {
            tokio::time::sleep(Duration::from_millis(i as u64 * 15)).await;

            let resp = client
                .get(&url)
                .send()
                .await
                .with_context(|| format!("failed to request {url} via local proxy"))?;

            if !resp.status().is_success() {
                bail!("{} returned error status: {}", url, resp.status());
            }

            let ip = resp.text().await.context("failed to read response body")?;
            let clean_ip = ip.trim().to_string();

            if clean_ip.is_empty() {
                bail!("received empty response from {}", url);
            }

            Ok::<ProxyCheck, anyhow::Error>(ProxyCheck {
                ip: clean_ip,
                latency_ms: start.elapsed().as_millis() as u64,
            })
        });
    }

    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok(proxy_check)) => {
                debug!("Proxy check succeeded: {proxy_check:?}");
                set.abort_all();
                return Ok(proxy_check);
            }
            Ok(Err(e)) => {
                debug!("One health check task failed: {:#}", e);
                continue;
            }
            Err(e) => {
                warn!("Health check task panicked or cancelled: {:#}", e);
                continue;
            }
        }
    }

    bail!("All concurrent proxy checks failed, network might be down or proxy is misconfigured")
}

/// [优化] 提取基础 Client 构建器，遵循 DRY 原则。
/// 所有的 Client 都应该基于此基础参数构建，保证配置的单一事实来源 (Single Source of Truth)。
fn base_client_builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .no_proxy()
        .pool_max_idle_per_host(384)
        .pool_idle_timeout(Duration::from_secs(360))
        .tcp_keepalive(Duration::from_secs(60))
        .tcp_nodelay(true);

    #[cfg(not(feature = "http3"))]
    {
        builder
            .http2_initial_stream_window_size(2 * 1024 * 1024)
            .http2_initial_connection_window_size(16 * 1024 * 1024)
    }

    #[cfg(feature = "http3")]
    {
        builder.http3_prior_knowledge()
    }
}

/// 主 HTTP/2 (或 H3) 上游 client（worker_url 直连，无优选 IP 时使用）
pub(super) fn build_main_client() -> Result<reqwest::Client> {
    info!("Building main client");
    base_client_builder()
        .build()
        .context("failed to build main http client")
}


pub(super) fn build_pref_client(worker_url: &str, ip: Option<&str>) -> Result<Option<reqwest::Client>> {
    let Some(ip) = ip.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };

    let url = url_parse(worker_url)?;
    let host = url.host.ok_or_else(|| anyhow::anyhow!("invalid worker url: {worker_url:?}"))?;
    let port = url.port.ok_or_else(|| anyhow::anyhow!("invalid worker url: {worker_url:?}"))?;

    let addr: IpAddr = ip
        .parse()
        .with_context(|| format!("invalid preferred ip: {ip:?}"))?;

    info!("Building preferred IP client resolving {} to {}", host, addr);

    let client = base_client_builder()
        .resolve(&*host, SocketAddr::new(addr, port))
        .build()
        .context("failed to build preferred ip client")?;

    Ok(Some(client))
}

/// WebSocket 隧道专用 client：HTTP/1.1 才能完成 101 升级（h2 不支持），
/// 其余配置与主 client 保持一致；支持优选 IP 的 `.resolve()` 覆盖。
pub(super) fn build_ws_client(worker_url: &str, ip: Option<&str>) -> Result<reqwest::Client> {
    // 强制 HTTP/1.1 用于 WS 握手
    let mut builder = base_client_builder().http1_only();

    // 如果配置了优选 IP，则注入 DNS 覆盖
    if let Some(ip) = ip.map(str::trim).filter(|s| !s.is_empty()) {
        let url = url_parse(worker_url)?;
        let host = url.host.ok_or_else(|| anyhow::anyhow!("invalid worker url: {worker_url:?}"))?;
        let port = url.port.ok_or_else(|| anyhow::anyhow!("invalid worker url: {worker_url:?}"))?;

        let addr: IpAddr = ip
            .parse()
            .with_context(|| format!("invalid preferred ip: {ip:?}"))?;

        info!("Applying preferred IP {} for WebSocket client", addr);
        builder = builder.resolve(&*host, SocketAddr::new(addr, port));
    } else {
        info!("Building default WebSocket client");
    }

    builder.build().context("failed to build websocket client")
}