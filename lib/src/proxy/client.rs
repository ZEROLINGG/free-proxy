// lib/src/proxy/client.rs
//
// 上游 Worker 通信层：reqwest 客户端构建（含优选 IP 的 DNS 覆盖）与
// 整链路可用性检测。所有"主动向外发起连接 / 构造客户端"的逻辑收敛于此。

use anyhow::{bail, Context, Result};
use std::net::{IpAddr, SocketAddr};
use std::time::SystemTime;
use tokio::task::JoinSet;
use tokio::time::{Duration, Instant};

use crate::http::{url_parse, UrlBuilder};

/// 与 worker 建立连接的超时
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);

/// 代理可用性检测结果
#[derive(Debug, Clone)]
pub struct ProxyCheck {
    pub ip: String,
    pub latency_ms: u64,
}

/// 通过本地代理请求 http://ip.me，返回出口 IP 与整链路耗时（最长 10s）。
pub async fn check_proxy_availability(port: u16) -> Result<ProxyCheck> {
    let proxy_url = UrlBuilder::new()
        .https(false)
        .host("127.0.0.1")
        .port(port)
        .build()
        .context("failed to configure proxy setting")?;
    let proxy = reqwest::Proxy::all(&proxy_url).context("failed to configure proxy setting")?;

    let check_client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .build()
        .context("failed to build check client")?;

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_millis();

    let start = Instant::now();

    let mut set = JoinSet::new();
    let endpoints = ["http://api.ipify.org", "http://ip.me"];

    // 发起6个并发请求（每个接口各3个）
    for i in 0..6 {
        let client = check_client.clone();
        let endpoint = endpoints[i % 2];
        let query = format!("{now}_{i}");
        let url = UrlBuilder::new()
            .base(endpoint)
            .append_query("_t", &query)
            .build()
            .with_context(|| format!("failed to build url for {endpoint}"))?;

        set.spawn(async move {
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
                set.abort_all();
                return Ok(proxy_check);
            }
            Ok(Err(_)) => continue,
            Err(_) => continue,
        }
    }

    bail!("All 6 concurrent proxy checks failed")
}

/// 主 HTTP/2 上游 client（worker_url 直连，无优选 IP 时使用）
pub(super) fn build_main_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .no_proxy()
        .pool_max_idle_per_host(512)
        .pool_idle_timeout(Duration::from_secs(360))
        .tcp_keepalive(Duration::from_secs(60))
        .http2_initial_stream_window_size(2 * 1024 * 1024)
        .http2_initial_connection_window_size(16 * 1024 * 1024)
        .tcp_nodelay(true)
        .build()
        .context("failed to build http client")
}

/// 优选 IP 专用 client：配置 `.resolve(domain, ip:port)`（SNI/Host 仍是域名）。
/// ip 为 None / 空串时返回 None（调用方回退到主 client 的 DNS 解析）。
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
    let client = reqwest::Client::builder()
        .resolve(&*host, SocketAddr::new(addr, port))
        .connect_timeout(CONNECT_TIMEOUT)
        .no_proxy()
        .pool_max_idle_per_host(512)
        .pool_idle_timeout(Duration::from_secs(360))
        .tcp_keepalive(Duration::from_secs(60))
        .http2_initial_stream_window_size(2 * 1024 * 1024)
        .http2_initial_connection_window_size(16 * 1024 * 1024)
        .tcp_nodelay(true)
        .build()?;
    Ok(Some(client))
}

/// WebSocket 隧道专用 client：HTTP/1.1 才能完成 101 升级（h2 不支持），
/// 其余配置与主 client 保持一致；支持优选 IP 的 `.resolve()` 覆盖。
pub(super) fn build_ws_client(worker_url: &str, ip: Option<&str>) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .http1_only()
        .connect_timeout(CONNECT_TIMEOUT)
        .no_proxy()
        .pool_max_idle_per_host(512)
        .pool_idle_timeout(Duration::from_secs(360))
        .tcp_keepalive(Duration::from_secs(60))
        .tcp_nodelay(true);

    if let Some(ip) = ip.map(str::trim).filter(|s| !s.is_empty()) {
        let url = url_parse(worker_url)?;
        let host = url.host.ok_or_else(|| anyhow::anyhow!("invalid worker url: {worker_url:?}"))?;
        let port = url.port.ok_or_else(|| anyhow::anyhow!("invalid worker url: {worker_url:?}"))?;
        let addr: IpAddr = ip
            .parse()
            .with_context(|| format!("invalid preferred ip: {ip:?}"))?;
        builder = builder.resolve(&*host, SocketAddr::new(addr, port));
    }

    Ok(builder.build()?)
}
