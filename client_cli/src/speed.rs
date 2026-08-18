// 两阶段 IP 优选测速(移植自 client_tauri commands/speed.rs 的编排逻辑):
//   阶段1 tcping:并发探测 CF 官方网段随机 IP 的 TCP 连通性/延迟
//   阶段2 health:对最快的一批 IP 做 Worker /health 健康检查(带 Bearer token)
// 进度输出到 stderr(实时刷新),结果表格输出到 stdout;--apply 将最优 IP 写回配置。
use anyhow::{anyhow, Context, Result};
use lib::speed_test::health::{batch_health, default_matcher};
use lib::speed_test::ip::IpBuffer;
use lib::speed_test::tcping::batch_tcping;
use lib::tool::derive_keys;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// 与 GUI 一致的默认值(SpeedTestOpts)
#[derive(Clone, Copy, Debug)]
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

/// 会话整体时长上限(与 GUI SESSION_HARD_DEADLINE 一致)
const SESSION_HARD_DEADLINE: Duration = Duration::from_secs(120);
/// 进度刷新间隔(与 GUI PROGRESS_THROTTLE 一致)
const PROGRESS_THROTTLE: Duration = Duration::from_millis(500);
/// 结果表格最多行数(与 GUI DONE_MAX_ROWS 一致)
const DONE_MAX_ROWS: usize = 50;
/// 测速与健康检查统一端口(大陆环境 *.workers.dev 走 HTTP/80 绕过 SNI 阻断)
const TEST_PORT: u16 = 80;

/// Cloudflare 官方 IPv4 网段(与 GUI speed.rs CF_IP_V4 一致)
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

/// 带 \r 覆盖的 stderr 进度行(仅当 stderr 是终端时用 \r,否则换行)
macro_rules! progress {
    ($($arg:tt)*) => {{
        use std::io::IsTerminal;
        if std::io::stderr().is_terminal() {
            eprint!("\r\x1b[2K{}", format!($($arg)*));
        } else {
            eprintln!("{}", format!($($arg)*));
        }
    }};
}

/// 运行完整测速,返回 (best_ip, 结果列表)。写回配置由调用方决定(--apply)。
pub async fn speed_test(domain: &str, auth_key: &str, opts: &SpeedTestOpts) -> Result<(Option<String>, Vec<(String, f32)>)> {
    let total = opts.total.max(1);
    let tcping_timeout = Duration::from_millis(opts.tcping_timeout_ms.max(1));

    let cidrs: Vec<&str> = CF_IP_V4.split_whitespace().collect();
    let buf = IpBuffer::new(cidrs, total).context("构建 IP 采样缓冲失败")?;

    println!("开始优选测速: 采样 {total} 个 IP,阶段1 tcping 并发 {}...", opts.tcping_limit);
    println!("提示: 请先关闭 tun 或其他代理,否则测速将失败。");

    // ── 阶段1: tcping ──
    let mut last_emit = Instant::now();
    let (results, aborted) = batch_tcping(
        buf,
        opts.tcping_limit.max(1),
        TEST_PORT,
        tcping_timeout,
        |done, rtt| {
            if done >= total || last_emit.elapsed() >= PROGRESS_THROTTLE {
                last_emit = Instant::now();
                match rtt {
                    Some(ms) => progress!("[tcping] {done}/{total} 当前延迟 {ms:.0}ms"),
                    None => progress!("[tcping] {done}/{total}"),
                }
            }
            true
        },
    )
    .await?;
    if aborted {
        return Err(anyhow!("测速被中止"));
    }
    progress!("[tcping] 完成: {}/{} 可达\n", results.len(), total);

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

    // ── 阶段2: health(带 Bearer token)──
    let token = {
        let keys = derive_keys(auth_key, domain).context("派生密钥失败")?;
        lib::proxy::gen_auth_token(&keys.token_base)
    };
    println!("阶段2: 对 {candidate_total} 个最快 IP 做 Worker 健康检查...");

    let mut last_emit = Instant::now();
    let (healthy, aborted) = batch_health(
        candidates,
        opts.health_limit.max(1),
        domain,
        TEST_PORT,
        Some(token),
        false,
        Duration::from_millis(opts.health_timeout_ms.max(1)),
        default_matcher,
        |done, total_c| {
            if done >= total_c || last_emit.elapsed() >= PROGRESS_THROTTLE {
                last_emit = Instant::now();
                progress!("[health] {done}/{total_c}");
            }
            true
        },
    )
    .await?;
    if aborted {
        return Err(anyhow!("测速被中止"));
    }
    progress!("[health] 完成: {}/{} 通过\n", healthy.len(), candidate_total);

    let final_results: Vec<(String, f32)> = sorted
        .iter()
        .filter(|(ip, _)| healthy.iter().any(|h| h.ip() == ip.ip()))
        .take(DONE_MAX_ROWS)
        .map(|(ip, rtt)| (ip.ip().to_string(), *rtt))
        .collect();

    let best = final_results.first().cloned();
    Ok((best.as_ref().map(|(ip, _)| ip.clone()), final_results))
}

/// 打印结果表格
pub fn print_results(results: &[(String, f32)], best_ip: Option<&str>) {
    println!("\n┌──────────────────────────┬────────────┐");
    println!("│ IP                        │ 延迟(ms)   │");
    println!("├──────────────────────────┼────────────┤");
    for (ip, rtt) in results {
        let mark = if Some(ip.as_str()) == best_ip { " ◀ 最优" } else { "" };
        println!("│ {:<24} │ {:<10.0}{} │", ip, rtt, mark);
    }
    println!("└──────────────────────────┴────────────┘");
    if let Some(ip) = best_ip {
        println!("\n最优 IP: {ip}");
        println!("可在代理页的「优选 IP」栏使用,或执行:");
        println!("  client_cli config set prefIp {ip}");
    } else {
        println!("\n没有通过健康检查的 IP。");
    }
}

/// 硬超时包装:整体超过 SESSION_HARD_DEADLINE 自动中止(与 GUI 一致)
pub async fn with_deadline<F, T>(f: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    tokio::select! {
        r = f => r,
        _ = sleep(SESSION_HARD_DEADLINE) => Err(anyhow!(
            "测速超过 {} 秒上限,已自动中止",
            SESSION_HARD_DEADLINE.as_secs()
        )),
    }
}