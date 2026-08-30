use anyhow::{anyhow, Context, Result};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::http::{UrlBuilder};
use crate::speed_test::health::{batch_health, default_matcher};
use crate::speed_test::ip::IpBuffer;
use crate::speed_test::tcping::batch_tcping;
use crate::tool::{derive_keys, gen_auth_token};

/// 会话整体时长上限
pub const SESSION_HARD_DEADLINE: Duration = Duration::from_secs(120);
/// 进度刷新间隔
pub const PROGRESS_THROTTLE: Duration = Duration::from_millis(500);
/// 结果表格最多行数
pub const DONE_MAX_ROWS: usize = 50;
/// 测速与健康检查统一端口
pub const TEST_PORT: u16 = 80;

/// Cloudflare 官方 IPv4 网段（与两端一致）
pub const CF_IP_V4: &str = "
173.245.48.0/20
103.21.244.0/22
103.22.200.0/22
103.31.4.0/22
141.101.64.0/18
108.162.192.0/18
190.93.240.0/20
188.114.96.0/20
197.234.240.0/22
198.41.128.0/17
162.158.0.0/15
104.16.0.0/13
104.24.0.0/14
172.64.0.0/13
131.0.72.0/22";

/// worker_health 的默认探测 IP 池（优选，供 health 阶段与单独 health 检查共用）
pub const DEFAULT_WORKER_IPS: [&str; 24] = [
    "104.17.23.238",
    "104.16.154.227",
    "104.16.124.96",
    "172.64.0.117",
    "162.158.0.6",
    "104.16.244.4",
    "104.17.23.0",
    "104.17.164.47",
    "104.19.35.37",
    "104.17.211.205",
    "104.17.60.203",
    "104.21.91.95",
    "104.17.217.114",
    "104.18.42.50",
    "104.25.253.190",
    "104.17.210.207",
    "104.17.171.139",
    "104.25.246.118",
    "104.21.83.106",
    "104.17.30.120",
    "104.18.45.139",
    "172.64.229.223",
    "104.16.251.224",
    "104.16.148.159",
];

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SpeedTestOpts {
    pub total: u64,
    pub tcping_limit: usize,
    pub tcping_timeout_ms: u64,
    pub health_limit: usize,
    pub health_timeout_ms: u64,
}

impl Default for SpeedTestOpts {
    fn default() -> Self {
        Self {
            total: 8000,
            tcping_limit: 96,
            tcping_timeout_ms: 500,
            health_limit: 32,
            health_timeout_ms: 2000,
        }
    }
}



/// 两阶段优选测速核心（无 UI 绑定）
/// progress_tcping(done,total,rtt) / progress_health(done,total) 返回 false 表示中止
pub async fn run_two_phase(
    domain: &str,
    auth_key: &str,
    opts: &SpeedTestOpts,
    cancel: Option<&AtomicBool>,
    mut progress_tcping: impl FnMut(u64, u64, Option<f32>) -> bool,
    mut progress_health: impl FnMut(u64, u64) -> bool,
) -> Result<(Option<String>, Vec<(String, f32)>)> {
    let domain = domain.trim().to_string();

    let total = opts.total.max(1);
    let tcping_timeout = Duration::from_millis(opts.tcping_timeout_ms.max(1));
    let cidrs: Vec<&str> = CF_IP_V4.split_whitespace().collect();
    let buf = IpBuffer::new(cidrs, total).context("构建 IP 采样缓冲失败")?;

    let mut last_emit = Instant::now();
    let (results, aborted) = batch_tcping(
        buf,
        opts.tcping_limit.max(1),
        TEST_PORT,
        tcping_timeout,
        |done, rtt| {
            if let Some(c) = cancel {
                if c.load(Ordering::Relaxed) {
                    return false;
                }
            }
            if done >= total || last_emit.elapsed() >= PROGRESS_THROTTLE {
                last_emit = Instant::now();
                return progress_tcping(done, total, rtt);
            }
            // 即使未到节流，也需传递取消信号
            true
        },
    )
    .await?;
    if aborted || cancel.map(|c| c.load(Ordering::Relaxed)).unwrap_or(false) {
        return Err(anyhow!("测速被中止"));
    }

    let mut sorted = results;
    sorted.sort_by(|a, b| a.1.total_cmp(&b.1));

    let candidates: Vec<_> = sorted
        .iter()
        .take(opts.health_limit)
        .map(|(ip, _)| *ip)
        .collect();
    let candidate_total = candidates.len() as u64;
    if candidate_total == 0 {
        return Err(anyhow!(
            "没有可达 IP。请关闭 tun/代理后重试,或检查网络环境。"
        ));
    }

    let token = {
        let keys = derive_keys(auth_key, &domain).context("派生密钥失败")?;
        gen_auth_token(&keys.token_base)
    };

    let mut last_emit = Instant::now();
    let (healthy, aborted) = batch_health(
        candidates,
        opts.health_limit.max(1),
        &domain,
        TEST_PORT,
        Some(token),
        false,
        Duration::from_millis(opts.health_timeout_ms.max(1)),
        default_matcher,
        |done, total_c| {
            if let Some(c) = cancel {
                if c.load(Ordering::Relaxed) {
                    return false;
                }
            }
            if done >= total_c || last_emit.elapsed() >= PROGRESS_THROTTLE {
                last_emit = Instant::now();
                return progress_health(done, total_c);
            }
            true
        },
    )
    .await?;
    if aborted || cancel.map(|c| c.load(Ordering::Relaxed)).unwrap_or(false) {
        return Err(anyhow!("测速被中止"));
    }

    let final_results: Vec<(String, f32)> = sorted
        .iter()
        .filter(|(ip, _)| healthy.iter().any(|h| h.ip() == ip.ip()))
        .take(DONE_MAX_ROWS)
        .map(|(ip, rtt)| (ip.ip().to_string(), *rtt))
        .collect();

    let best = final_results.first().cloned();
    Ok((best.as_ref().map(|(ip, _)| ip.clone()), final_results))
}

/// 硬超时包装:整体超过 SESSION_HARD_DEADLINE 自动中止
pub async fn with_deadline<F, T>(f: F) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    tokio::select! {
        r = f => r,
        _ = tokio::time::sleep(SESSION_HARD_DEADLINE) => Err(anyhow!(
            "测速超过 {} 秒上限,已自动中止",
            SESSION_HARD_DEADLINE.as_secs()
        )),
    }
}

/// Worker 健康检查：并发探测（最多6并发，24 IP池），任一成功即返回 true
pub async fn worker_health(domain: &str, auth_key: &str, pref_ip: Option<&str>) -> Result<bool> {
    let host = domain.trim().to_string();
    let token = {
        let keys = derive_keys(auth_key, &host).context("派生密钥失败")?;
        gen_auth_token(&keys.token_base)
    };

    let mut ips: Vec<String> = Vec::new();
    if let Some(ip) = pref_ip.map(str::trim).filter(|ip| !ip.is_empty()) {
        ips.push(ip.to_string());
    }
    ips.extend(DEFAULT_WORKER_IPS.iter().map(|s| s.to_string()));

    // 并发池：最多6并发，早停
    let mut set = tokio::task::JoinSet::new();
    let mut ips_iter = ips.into_iter();
    let max_concurrent = 6usize;
    for _ in 0..max_concurrent {
        if let Some(ip) = ips_iter.next() {
            let host_c = host.clone();
            let token_c = token.clone();
            set.spawn(async move { check_worker_ip(ip, host_c, token_c).await });
        }
    }
    while let Some(res) = set.join_next().await {
        match res {
            Ok(true) => {
                set.abort_all();
                return Ok(true);
            }
            _ => {
                if let Some(ip) = ips_iter.next() {
                    let host_c = host.clone();
                    let token_c = token.clone();
                    set.spawn(async move { check_worker_ip(ip, host_c, token_c).await });
                }
            }
        }
    }
    Ok(false)
}

async fn check_worker_ip(ip: String, host: String, token: String) -> bool {
    let addr: SocketAddr = match format!("{ip}:{TEST_PORT}").parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let client = match reqwest::Client::builder()
        .resolve(&host, addr)
        .timeout(Duration::from_secs(3))
        .no_proxy()
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let url = match UrlBuilder::new()
        .https(false)
        .host(host.as_str())
        .port(TEST_PORT)
        .path("/health")
        .build()
    {
        Ok(u) => u,
        Err(_) => return false,
    };
    let resp = match client.get(url).bearer_auth(&token).send().await {
        Ok(r) => r,
        Err(_) => return false,
    };
    let status = resp.status();
    match resp.bytes().await {
        Ok(body) if default_matcher(status, &body) => true,
        _ => false,
    }
}
