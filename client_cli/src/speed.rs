// 两阶段 IP 优选测速（CLI 终端版）：核心逻辑复用 lib::client::speed
use anyhow::Result;
use lib::client::speed::{run_two_phase, with_deadline as lib_with_deadline};
use std::sync::atomic::AtomicBool;

pub use lib::client::speed::SpeedTestOpts;

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
    println!("开始优选测速: 采样 {total} 个 IP,阶段1 tcping 并发 {}...", opts.tcping_limit);
    println!("提示: 请先关闭 tun 或其他代理,否则测速将失败。");

    let cancel = AtomicBool::new(false);

    // 进度由 lib 内部节流，CLI 仅负责打印
    let (best, results) = run_two_phase(
        domain,
        auth_key,
        opts,
        Some(&cancel),
        |done, total_c, rtt| {
            match rtt {
                Some(ms) => progress!("[tcping] {done}/{total_c} 当前延迟 {ms:.0}ms"),
                None => progress!("[tcping] {done}/{total_c}"),
            }
            true
        },
        |done, total_c| {
            progress!("[health] {done}/{total_c}");
            true
        },
    )
    .await?;

    progress!("[tcping] 完成\n");
    progress!("[health] 完成: {}/{} 通过\n", results.len(), total);

    Ok((best, results))
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
    lib_with_deadline(f).await
}
