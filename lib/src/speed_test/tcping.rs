use anyhow::{Result, anyhow};
use ipnetwork::IpNetwork;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::speed_test::ip::IpBuffer;

/// 单 IP 的 TCP 连接测速，返回毫秒 RTT
pub async fn tcping(addr: &IpNetwork, port: u16, timeout_dur: Duration) -> Result<f32> {
    let socket = SocketAddr::new(addr.ip(), port);

    let start = Instant::now();
    let stream = timeout(timeout_dur, TcpStream::connect(socket))
        .await
        .map_err(|_| anyhow!("tcp connect timeout"))??;
    drop(stream);

    let elapsed = start.elapsed();
    Ok(elapsed.as_secs_f32() * 1000.0)
}

/// 从 IpBuffer 并发测速，返回 (ip, rtt_ms) 列表（按弹出顺序，未排序）。
///
/// `progress` 在每完成一个任务后调用：参数 `(已完成数, 本次 RTT 毫秒)`，
/// 失败任务的 RTT 为 `None`。返回 `false` 表示中止（在途任务会被丢弃，
/// 返回已收集的部分结果）。返回值 `(results, aborted)`。
pub async fn batch_tcping(
    ip_buffer: IpBuffer,
    limit: usize,
    port: u16,
    timeout_dur: Duration,
    mut progress: impl FnMut(u64, Option<f32>) -> bool,
) -> Result<(Vec<(IpNetwork, f32)>, bool)> {
    let mut results = Vec::new();
    let mut errors: Vec<(IpNetwork, String)> = Vec::new();
    let mut join_set = JoinSet::new();
    let limit = limit.max(1);
    for _ in 0..limit {
        if let Some(ip) = ip_buffer.pop() {
            join_set.spawn(async move {
                let res = tcping(&ip, port, timeout_dur).await;
                (ip, res)
            });
        } else {
            break;
        }
    }

    let mut aborted = false;
    let mut completed: u64 = 0;
    while let Some(join_res) = join_set.join_next().await {
        completed += 1;
        let mut rtt: Option<f32> = None;
        match join_res {
            Ok((ip, tcping_res)) => match tcping_res {
                Ok(latency) => {
                    results.push((ip, latency));
                    rtt = Some(latency);
                }
                Err(e) => errors.push((ip, e.to_string())),
            },
            Err(e) => {
                errors.push((
                    IpNetwork::V4("0.0.0.0/0".parse().unwrap()),
                    format!("task panicked: {e}"),
                ));
            }
        }

        if !progress(completed, rtt) {
            aborted = true;
            break;
        }
        if let Some(ip) = ip_buffer.pop() {
            join_set.spawn(async move {
                let res = tcping(&ip, port, timeout_dur).await;
                (ip, res)
            });
        }
    }

    if aborted {
        return Ok((results, true));
    }
    if results.is_empty() {
        let sample: Vec<String> = errors
            .iter()
            .take(10)
            .map(|(ip, e)| format!("  {}: {}", ip.ip(), e))
            .collect();
        anyhow::bail!(
            "[batch_tcping] no results ({} attempted, {} failed)\nSample errors:\n{}",
            errors.len(),
            errors.len(),
            sample.join("\n")
        );
    }
    Ok((results, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speed_test::ip::IpBuffer;
    static RAW: &str = "
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
    #[tokio::test]
    #[ignore = "requires network access to Cloudflare IPs"]
    async fn test_tcping() -> Result<()> {
        let ips: Vec<&str> = RAW.split_whitespace().collect();
        let total = 1024;
        let buf = IpBuffer::new(ips, total)?;
        let (results, aborted) =
            batch_tcping(buf, 256, 443, Duration::from_secs_f32(0.5), |_, _| true).await?;
        assert!(!aborted, "progress callback never aborts");
        for (ip, ping) in results {
            println!("[*]{:<40} | {:.2} ms", ip.ip(), ping);
        }

        Ok(())
    }
}
