// Worker 健康检查:验证 domain + auth_key 配置是否可用。
// 移植自 client_tauri commands/speed.rs worker_health:
// HTTP/80 + .resolve() 绕过 DNS 污染与 SNI 阻断;
// IP 顺序:prefIp(若设置) → 内置默认池;任一返回 200(通过 matcher)即视为可用。
use anyhow::{anyhow, Context, Result};
use lib::http::{split_host_port, UrlBuilder};
use lib::speed_test::health::default_matcher;
use lib::tool::derive_keys;
use reqwest::Client;
use std::net::SocketAddr;
use std::time::Duration;

const TEST_PORT: u16 = 80;
const DEFAULT_WORKER_IPS: [&str; 5] = [
    "104.17.23.238",
    "104.16.39.227",
    "104.16.124.96",
    "172.64.0.117",
    "162.158.0.6",
];

/// 返回 true 表示 Worker 配置可用
pub async fn worker_health(domain: &str, auth_key: &str, pref_ip: Option<&str>) -> Result<bool> {
    let host = domain.trim().to_string();
    let (_host, port) = split_host_port(&host)?;
    if port.is_some() {
        return Err(anyhow!("domain 不能携带端口,当前值: {host:?}"));
    }

    let token = {
        let keys = derive_keys(auth_key, &host).context("派生密钥失败")?;
        lib::proxy::gen_auth_token(&keys.token_base)
    };

    let mut ips: Vec<String> = Vec::new();
    if let Some(ip) = pref_ip.map(str::trim).filter(|ip| !ip.is_empty()) {
        ips.push(ip.to_string());
        println!("使用优选 IP: {ip}");
    }
    ips.extend(DEFAULT_WORKER_IPS.iter().map(|s| s.to_string()));

    for ip in ips {
        let addr: SocketAddr = match format!("{ip}:{TEST_PORT}").parse() {
            Ok(a) => a,
            Err(_) => {
                eprintln!("无效 IP 地址: {ip},跳过");
                continue;
            }
        };
        let client = match Client::builder()
            .resolve(&host, addr)
            .timeout(Duration::from_secs(3))
            .no_proxy()
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("构建 HTTP 客户端失败 ({ip}): {e}");
                continue;
            }
        };

        let url = UrlBuilder::new()
            .https(false)
            .host(host.as_str())
            .port(TEST_PORT)
            .path("/health")
            .build()?;

        match client.get(url).bearer_auth(&token).send().await {
            Ok(resp) => {
                let status = resp.status();
                match resp.bytes().await {
                    Ok(body) if default_matcher(status, &body) => {
                        println!("Worker 配置有效(通过 {ip} 连接)");
                        return Ok(true);
                    }
                    Ok(_) => println!("{ip}: 响应异常(status {status}),尝试下一个..."),
                    Err(e) => println!("{ip}: 读取响应失败: {e},尝试下一个..."),
                }
            }
            Err(e) => println!("{ip}: 连接失败: {e},尝试下一个..."),
        }
    }

    Ok(false)
}