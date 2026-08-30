// 前台运行本地代理:
//   启动 -> 打印实际端口 -> 自动可用性检测 -> 进入 stdin 交互(热切换) ->
//   Ctrl+C 或 stop 命令优雅退出。
use anyhow::{Context, Result};
use lib::proxy::{Proxy, ProxyConfig};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::timeout;
use std::time::Duration;

use crate::ca::ca_key_secret;
use crate::config::{ProxySettings, app_data_dir};

fn ca_dir() -> Result<std::path::PathBuf> {
    Ok(app_data_dir()?.join("ca"))
}

fn build_proxy(s: &ProxySettings, ca: std::path::PathBuf, secret: [u8; 32]) -> Result<Proxy> {
    let compressor = s.compressor.parse().context("无效的压缩算法")?;
    let aead = s.aead.parse().context("无效的加密算法")?;
    let cfg = ProxyConfig {
        port: s.local_port,
        domain: s.domain.trim().to_string(),
        use_https: s.use_https,
        auth_key: s.auth_key.clone(),
        ca_dir: ca,
        ca_key_secret: secret,
        compressor,
        aead,
        pref_ip: s.pref_ip.clone(),
    };
    Proxy::new(cfg)
}

/// 处理一行 stdin 命令;返回 true 表示请求退出
async fn handle_stdin(line: &str, proxy: &Proxy) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return false;
    }
    let parts: Vec<&str> = line.split_whitespace().collect();
    match parts[0] {
        "algo" => {
            if parts.len() != 3 {
                println!("用法: algo <压缩算法> <加密算法>   例如: algo zstd chacha20poly1305");
            } else {
                let mut ok = true;
                if let Err(e) = proxy.set_compressor(parts[1]) {
                    println!("设置压缩算法失败: {e}");
                    ok = false;
                }
                if let Err(e) = proxy.set_aead(parts[2]) {
                    println!("设置加密算法失败: {e}");
                    ok = false;
                }
                if ok {
                    println!("算法已热切换: 压缩={} 加密={}", parts[1], parts[2]);
                }
            }
        }
        "ip" => {
            if parts.len() < 2 {
                println!("用法: ip <优选IP|off>   例如: ip 104.16.39.227 或 ip off");
            } else {
                let val = if parts[1] == "off" { None } else { Some(parts[1]) };
                match proxy.set_ip(val) {
                    Ok(()) => match val {
                        Some(v) => println!("优选 IP 已切换: {v}"),
                        None => println!("已清除优选 IP,回退 DNS 解析"),
                    },
                    Err(e) => println!("设置优选 IP 失败: {e}"),
                }
            }
        }
        "check" => match proxy.check_availability().await {
            Ok(check) => println!(
                "链路正常,出口 IP: {} ({} ms)",
                check.ip, check.latency_ms
            ),
            Err(e) => println!("链路检测失败: {e:#}"),
        },
        "stop" | "exit" | "quit" => {
            println!("正在停止代理...");
            return true;
        }
        "help" => print_help(),
        _ => println!("未知命令: {} (输入 help 查看可用命令)", parts[0]),
    }
    false
}

fn print_help() {
    println!("可用命令:");
    println!("  algo <压缩> <加密>   热切换算法,如: algo zstd ascon128");
    println!("  ip <IP|off>          热切换优选 IP,如: ip 104.16.39.227 / ip off");
    println!("  check                重新检测链路(出口 IP + 延迟)");
    println!("  stop / exit / quit   停止代理并退出");
    println!("  help                 显示本帮助");
}

/// 启动前台代理并进入交互循环
pub async fn run(s: &ProxySettings) -> Result<()> {
    s.validate()?;

    let mut proxy = build_proxy(s, ca_dir()?, ca_key_secret()?)?;
    let port = proxy.start().await.context("启动本地代理失败")?;
    println!("代理已启动: http://127.0.0.1:{port}");
    println!("(输入 help 查看命令;输入 stop 或按 Ctrl+C 退出)");

    // 链路可用性检测(与 GUI 启动后行为一致)
    match timeout(Duration::from_secs(15), proxy.check_availability()).await {
        Ok(Ok(check)) => println!(
            "链路正常,出口 IP: {} ({} ms)",
            check.ip, check.latency_ms
        ),
        Ok(Err(e)) => println!("链路检测失败: {e:#}"),
        Err(_) => println!("链路检测超时(>15s)"),
    }

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        let line = tokio::select! {
            l = lines.next_line() => match l {
                Ok(Some(l)) => l,
                Ok(None) => { println!("stdin 已关闭,代理停止。"); break; }
                Err(e) => { println!("读取 stdin 失败: {e}"); break; }
            },
            _ = tokio::signal::ctrl_c() => { println!("\n收到 Ctrl+C,正在停止代理..."); break; }
        };
        if handle_stdin(&line, &proxy).await {
            break;
        }
    }

    proxy.stop().await;
    println!("代理已停止。");
    Ok(())
}

