// client_cli:free-proxy 命令行客户端
// 与 GUI 客户端共用配置(app_data_dir/settings.json)与 CA 目录(app_data_dir/ca)。
use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand};

mod ca;
mod config;
mod health;
mod run;
mod speed;
mod subscribe;

#[derive(Parser)]
#[command(
    name = "client_cli",
    version,
    about = "free-proxy 命令行客户端(与 GUI 共用配置与 CA)",
    after_help = "配置文件与 CA 目录位于应用数据目录(与 GUI 客户端共用):\n  ~/.local/share/com.zz.freeproxy (Linux) 或对应平台位置"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 前台运行本地代理(启动后可用 stdin 命令热切换)
    Run(RunArgs),
    /// 两阶段优选 IP 测速(tcping + Worker 健康检查)
    SpeedTest(SpeedArgs),
    /// 验证 Worker 配置是否可用
    Health(HealthArgs),
    /// CA 证书管理
    Ca(CaArgs),
    /// 读写共用配置
    Config(ConfigArgs),
    /// 打印订阅链接(Clash / sing-box / v2rayN 等导入用)
    Subscribe(SubscribeArgs),
    /// 展示当前配置
    Status,
}

#[derive(Args)]
struct RunArgs {
    /// Worker 域名(如 free-proxy.xxx.workers.dev,不带端口)
    #[arg(long)]
    domain: Option<String>,
    /// 认证密钥
    #[arg(long)]
    key: Option<String>,
    /// 本地监听端口(默认取配置)
    #[arg(long)]
    port: Option<u16>,
    /// 通过 https 连接 Worker(默认 http)
    #[arg(long)]
    https: bool,
    /// 压缩算法(zstd / gzip / lz4 / none)
    #[arg(long)]
    compressor: Option<String>,
    /// 加密算法(aes128gcm / aes256gcm / chacha20poly1305 / ...)
    #[arg(long)]
    aead: Option<String>,
    /// 优选 IP(如 104.16.39.227)
    #[arg(long)]
    ip: Option<String>,
}

#[derive(Args)]
struct SpeedArgs {
    /// 采样 IP 总数(默认 8000)
    #[arg(long)]
    total: Option<u64>,
    /// tcping 并发数(默认 96)
    #[arg(long)]
    tcping_limit: Option<usize>,
    /// tcping 单次超时毫秒(默认 500)
    #[arg(long)]
    tcping_timeout_ms: Option<u64>,
    /// 进入 health 阶段的最快 IP 数(默认 32)
    #[arg(long)]
    health_limit: Option<usize>,
    /// health 单次超时毫秒(默认 2000)
    #[arg(long)]
    health_timeout_ms: Option<u64>,
    /// 测速完成后将最优 IP 写入配置
    #[arg(long)]
    apply: bool,
    /// Worker 域名(默认取配置)
    #[arg(long)]
    domain: Option<String>,
    /// 认证密钥(默认取配置)
    #[arg(long)]
    key: Option<String>,
}

#[derive(Args)]
struct HealthArgs {
    /// Worker 域名(默认取配置)
    #[arg(long)]
    domain: Option<String>,
    /// 认证密钥(默认取配置)
    #[arg(long)]
    key: Option<String>,
    /// 优先探测的 IP(默认取配置中的优选 IP)
    #[arg(long)]
    ip: Option<String>,
}

#[derive(Subcommand)]
enum CaCmd {
    /// 显示 CA 证书信息(路径 + PEM)
    Info,
    /// 打印 CA 目录路径
    Dir,
    /// 安装 CA 到系统信任区(需要系统权限授权)
    Install,
}

#[derive(Args)]
struct CaArgs {
    #[command(subcommand)]
    cmd: CaCmd,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// 读取某个配置项
    Get {
        /// 配置项: domain / useHttps / authKey / localPort / compressor / aead / prefIp
        key: String,
    },
    /// 写入某个配置项(仅当前用户可见)
    Set { key: String, value: String },
}

#[derive(Args)]
struct ConfigArgs {
    #[command(subcommand)]
    cmd: ConfigCmd,
}

#[derive(Args)]
struct SubscribeArgs {
    /// 本地代理端口(默认取配置中的 localPort;若正在运行以实际端口为准)
    #[arg(long)]
    port: Option<u16>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let rt = tokio::runtime::Runtime::new().map_err(|e| anyhow!("初始化运行时失败: {e}"))?;
    match cli.cmd {
        Cmd::Run(a) => {
            let mut s = config::load()?;
            if let Some(v) = a.domain {
                s.domain = v;
            }
            if let Some(v) = a.key {
                s.auth_key = v;
            }
            if let Some(v) = a.port {
                s.local_port = v;
            }
            if a.https {
                s.use_https = true;
            }
            if let Some(v) = a.compressor {
                s.compressor = v;
            }
            if let Some(v) = a.aead {
                s.aead = v;
            }
            if let Some(v) = a.ip {
                s.pref_ip = Some(v);
            }
            rt.block_on(run::run(&s))
        }
        Cmd::SpeedTest(a) => {
            let s = config::load()?;
            let domain = a.domain.unwrap_or(s.domain.trim().to_string());
            let auth_key = a.key.unwrap_or(s.auth_key.clone());
            anyhow::ensure!(!domain.is_empty(), "domain(Worker 域名)不能为空(可用 --domain 指定)");
            anyhow::ensure!(!auth_key.is_empty(), "认证密钥不能为空(可用 --key 指定)");
            let mut opts = speed::SpeedTestOpts::default();
            if let Some(v) = a.total {
                opts.total = v;
            }
            if let Some(v) = a.tcping_limit {
                opts.tcping_limit = v;
            }
            if let Some(v) = a.tcping_timeout_ms {
                opts.tcping_timeout_ms = v;
            }
            if let Some(v) = a.health_limit {
                opts.health_limit = v;
            }
            if let Some(v) = a.health_timeout_ms {
                opts.health_timeout_ms = v;
            }
            rt.block_on(async move {
                let (best, results) = speed::with_deadline(speed::speed_test(&domain, &auth_key, &opts)).await?;
                speed::print_results(&results, best.as_deref());
                if a.apply {
                    if let Some(ip) = best {
                        let mut s = config::load()?;
                        s.pref_ip = Some(ip.clone());
                        config::save(&s)?;
                        println!("已将最优 IP {ip} 写入配置。");
                    } else {
                        println!("未生成最优 IP,配置未修改。");
                    }
                }
                Ok(())
            })
        }
        Cmd::Health(a) => {
            let s = config::load()?;
            let domain = a.domain.unwrap_or(s.domain.trim().to_string());
            let auth_key = a.key.unwrap_or(s.auth_key.clone());
            let pref_ip = a.ip.or(s.pref_ip);
            anyhow::ensure!(!domain.is_empty(), "domain(Worker 域名)不能为空(可用 --domain 指定)");
            anyhow::ensure!(!auth_key.is_empty(), "认证密钥不能为空(可用 --key 指定)");
            rt.block_on(async move {
                match health::worker_health(&domain, &auth_key, pref_ip.as_deref()).await {
                    Ok(true) => {
                        println!("验证通过:Worker 配置可用。");
                        Ok(())
                    }
                    Ok(false) => Err(anyhow!(
                        "验证失败:所有候选 IP 均无法访问 Worker。\n\
                         请检查 domain / 认证密钥是否正确,或网络环境(可尝试优选 IP)。"
                    )),
                    Err(e) => Err(e),
                }
            })
        }
        Cmd::Ca(ca_cmd) => match ca_cmd.cmd {
            CaCmd::Info => ca::show_info(),
            CaCmd::Dir => ca::show_dir(),
            CaCmd::Install => ca::install(),
        },
        Cmd::Config(cfg_cmd) => match cfg_cmd.cmd {
            ConfigCmd::Get { key } => {
                let s = config::load()?;
                let json = serde_json::to_value(&s)?;
                let value = json
                    .get(&key)
                    .ok_or_else(|| anyhow!("未知配置项: {key}"))?;
                println!("{key} = {}", serde_json::to_string_pretty(value)?);
                Ok(())
            }
            ConfigCmd::Set { key, value } => {
                let s = config::load()?;
                let mut json = serde_json::to_value(&s)?;
                let v: serde_json::Value = serde_json::from_str(&value)
                    .unwrap_or_else(|_| serde_json::Value::String(value));
                match json.get_mut(&key) {
                    Some(field) => {
                        *field = v.clone();
                        let updated: config::ProxySettings = serde_json::from_value(json)?;
                        if key == "authKey" {
                            println!("提示:认证密钥已明文写入配置文件,请确保文件权限安全。");
                        }
                        config::save(&updated)?;
                        println!("已更新 {key} = {v}");
                        Ok(())
                    }
                    None => Err(anyhow!("未知配置项: {key}")),
                }
            }
        },
        Cmd::Subscribe(a) => {
            let s = config::load()?;
            let port = a.port.unwrap_or(s.local_port);
            let url = subscribe::subscribe_url(&s, port)?;
            println!("{url}");
            Ok(())
        }
        Cmd::Status => {
            let s = config::load()?;
            println!("当前配置:{}", serde_json::to_string_pretty(&s)?);
            Ok(())
        }
    }
}