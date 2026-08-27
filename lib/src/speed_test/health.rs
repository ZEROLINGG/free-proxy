use anyhow::Result;
use ipnetwork::IpNetwork;
use reqwest::{Client, StatusCode};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;

use crate::http::{UrlBuilder};

/// 检测该 IP 能否正常访问我们的 Worker：
/// 强制将 domain 解析到目标 IP（SNI/Host 仍是域名），带上 Bearer token。
pub async fn health<F>(
    ip: &IpNetwork,
    domain: &str,
    port: u16,
    token: Option<&str>,
    use_https: bool,
    timeout_dur: Duration,
    matcher: F,
) -> bool
where
    F: Fn(StatusCode, &[u8]) -> bool,
{
    let host = domain.trim().to_string();

    let url = match UrlBuilder::new()
        .https(use_https)
        .host(host.as_str())
        .port(port)
        .path("/health")
        .build()
    {
        Ok(u) => u,
        Err(_) => return false,
    };
    let socket_addr = SocketAddr::new(ip.ip(), port);

    // 构建专属的 Client，强制将 domain 解析到指定的目标 IP
    let client_result = Client::builder()
        .resolve(&host, socket_addr)
        .no_proxy()
        .build();

    let client = match client_result {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to build client: {e:#?}");
            return false;
        }
    };

    let mut request_builder = client.get(&url).timeout(timeout_dur);
    if let Some(token) = token {
        request_builder = request_builder.bearer_auth(token);
    }

    match request_builder.send().await {
        Ok(response) => {
            let status = response.status();
            match response.bytes().await {
                Ok(bytes) => matcher(status, &bytes),
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

/// 并发健康检查，返回通过检查的 IP 列表。
///
/// `progress` 在每完成一个 IP 检查后调用：参数 `(已完成数, 总数)`，
/// 返回 `false` 表示中止（在途任务会被丢弃，返回已收集的部分结果）。
/// 返回值 `(healthy_ips, aborted)`。
pub async fn batch_health<F>(
    ips: Vec<IpNetwork>,
    limit: usize,
    domain: &str,
    port: u16,
    token: Option<String>,
    use_https: bool,
    timeout_dur: Duration,
    matcher: F,
    mut progress: impl FnMut(u64, u64) -> bool,
) -> Result<(Vec<IpNetwork>, bool)>
where
    F: Fn(StatusCode, &[u8]) -> bool + Send + Sync + 'static,
{
    let total = ips.len() as u64;
    let mut set = JoinSet::new();
    let mut healthy_ips = Vec::new();
    let mut aborted = false;
    let mut done: u64 = 0;

    let domain = Arc::new(domain.to_string());
    let token = Arc::new(token);
    let matcher = Arc::new(matcher);

    for ip in ips {
        while set.len() >= limit {
            if let Some(res) = set.join_next().await {
                if let Ok(Some(valid_ip)) = res {
                    healthy_ips.push(valid_ip);
                }
                done += 1;
                if !progress(done, total) {
                    aborted = true;
                    break;
                }
            }
        }
        if aborted {
            break;
        }

        let d = Arc::clone(&domain);
        let t = Arc::clone(&token);
        let m = Arc::clone(&matcher);

        set.spawn(async move {
            let is_ok = health(
                &ip,
                &d,
                port,
                t.as_deref(),
                use_https,
                timeout_dur,
                |status, body| m(status, body),
            )
            .await;

            if is_ok { Some(ip) } else { None }
        });
    }

    if aborted {
        return Ok((healthy_ips, true));
    }

    while let Some(res) = set.join_next().await {
        if let Ok(Some(valid_ip)) = res {
            healthy_ips.push(valid_ip);
        }
        done += 1;
        if !progress(done, total) {
            aborted = true;
            break;
        }
    }

    if aborted {
        return Ok((healthy_ips, true));
    }
    anyhow::ensure!(!healthy_ips.is_empty(), "No healthy ips found");

    Ok((healthy_ips, false))
}

/// 默认健康检查匹配器：status 成功且 body 为 ≥10 位数字（worker /health 返回毫秒时间戳）
pub fn default_matcher(status: StatusCode, body: &[u8]) -> bool {
    if !status.is_success() {
        return false;
    }
    let body_str = std::str::from_utf8(body).unwrap_or("").trim();
    match body_str.parse::<u64>() {
        Ok(_) => body_str.len() >= 10,
        Err(_) => false,
    }
}
