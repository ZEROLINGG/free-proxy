use ipnetwork::IpNetwork;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

// ── IpRange ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IpRange {
    network: IpNetwork,
}

impl IpRange {
    pub fn new(s: &str) -> anyhow::Result<Self> {
        let s = s.trim();
        let network = if s.contains('/') {
            IpNetwork::from_str(s).map_err(|e| anyhow::anyhow!("invalid CIDR '{}': {}", s, e))?
        } else {
            let ip = IpAddr::from_str(s)
                .map_err(|e| anyhow::anyhow!("invalid IP address '{}': {}", s, e))?;
            let prefix = match ip {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            IpNetwork::new(ip, prefix)?
        };
        Ok(IpRange { network })
    }

    fn range_u128(&self) -> (u128, u128) {
        match self.network {
            IpNetwork::V4(net) => {
                let base = u32::from(net.network()) as u128;
                let host_bits = 32u32 - net.prefix() as u32;
                let size = 1u128 << host_bits;
                (base, base + size - 1)
            }
            IpNetwork::V6(net) => {
                let base = u128::from(net.network());
                let host_bits = 128u32 - net.prefix() as u32;
                if host_bits == 128 {
                    (0, u128::MAX)
                } else {
                    let size = 1u128 << host_bits;
                    (base, base + size - 1)
                }
            }
        }
    }

    fn capacity(&self) -> u128 {
        let (s, e) = self.range_u128();
        (e - s).saturating_add(1)
    }

    fn is_v4(&self) -> bool {
        matches!(self.network, IpNetwork::V4(_))
    }
}

// ── 按区间大小加权，将 total 均匀分配给各 range ──────────────────────────
fn auto_distribute(ranges: &[IpRange], total: u64) -> Vec<usize> {
    let n = ranges.len();
    if n == 0 {
        return vec![];
    }

    let caps: Vec<u128> = ranges.iter().map(|r| r.capacity()).collect();
    let total_cap: u128 = caps.iter().sum();
    let total = total as u128;

    // ── floor 分配 + 记录小数部分 ──────────────────────────────────────
    // 每个 slot 至少 1，不超过其容量
    let mut counts: Vec<u128> = caps
        .iter()
        .map(|&cap| {
            let quota = (total * cap / total_cap).max(1);
            quota.min(cap)
        })
        .collect();

    // ── largest-remainder 补齐到 total ────────────────────────────────
    let allocated: u128 = counts.iter().sum();

    if allocated < total {
        // 计算各 slot 的小数余数，余数最大的优先补 1
        let mut remainders: Vec<(usize, u128)> = caps
            .iter()
            .enumerate()
            .map(|(i, &cap)| {
                // remainder = total*cap mod total_cap（分子的余数部分）
                let remainder = (total * cap) % total_cap;
                (i, remainder)
            })
            .collect();
        remainders.sort_by(|a, b| b.1.cmp(&a.1));

        let mut need = (total - allocated) as usize;
        for (i, _) in remainders {
            if need == 0 {
                break;
            }
            // 只补到容量上限
            if counts[i] < caps[i] {
                counts[i] += 1;
                need -= 1;
            }
        }
    } else if allocated > total {
        // max(1) 导致超出时，从最大 slot 开始削减
        let mut excess = (allocated - total) as usize;
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| counts[b].cmp(&counts[a]));
        for i in order {
            if excess == 0 {
                break;
            }
            let reduce = counts[i].saturating_sub(1).min(excess as u128);
            counts[i] -= reduce;
            excess -= reduce as usize;
        }
    }

    counts.into_iter().map(|c| c as usize).collect()
}

// ── 区间采样状态 ──────────────────────────────────────────────────────────

struct SampledRange {
    range: IpRange,
    /// 该 slot 的采样总数
    count: usize,
    /// 区间起点（绝对 u128 地址）
    start: u128,
    /// 普通区间宽度
    interval_size: u128,
    /// 最后一个区间的实际宽度
    last_size: u128,
    /// 已分发的区间下标
    index_counter: usize,
}

impl SampledRange {
    fn new(range: IpRange, count: usize) -> Self {
        let (start, end) = range.range_u128();
        let range_size = (end - start).saturating_add(1);
        let interval_size = (range_size / count.max(1) as u128).max(1);

        let last_size = if count > 0 {
            let last_start = start + (count as u128 - 1) * interval_size;
            (end - last_start).saturating_add(1)
        } else {
            interval_size
        };

        Self {
            range,
            count,
            start,
            interval_size,
            last_size,
            index_counter: 0,
        }
    }

    fn remaining(&self) -> usize {
        self.count.saturating_sub(self.index_counter)
    }

    /// 取下一个 IP，区间内随机偏移
    fn next_ip(&mut self) -> Option<IpNetwork> {
        if self.index_counter >= self.count {
            return None;
        }

        let idx = self.index_counter;
        self.index_counter += 1;

        let interval_start = self.start + idx as u128 * self.interval_size;

        let actual_size = if idx == self.count - 1 {
            self.last_size
        } else {
            self.interval_size
        };

        let offset = if actual_size <= 1 {
            0
        } else {
            cheap_random(self as *const _ as usize) % actual_size
        };

        let ip_u128 = interval_start + offset;
        let ip_addr = if self.range.is_v4() {
            IpAddr::V4(Ipv4Addr::from(ip_u128 as u32))
        } else {
            IpAddr::V6(Ipv6Addr::from(ip_u128))
        };

        IpNetwork::new(ip_addr, if self.range.is_v4() { 32 } else { 128 }).ok()
    }
}

// ── IpBuffer ──────────────────────────────────────────────────────────────

struct IpBufferInner {
    slots: Vec<SampledRange>,
}

impl IpBufferInner {
    /// 按剩余比例最大的 slot 取下一个（Weighted Round-Robin）
    fn next_weighted(&mut self) -> Option<IpNetwork> {
        let best = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.remaining() > 0)
            .max_by(|(_, a), (_, b)| {
                // remaining/count 交叉相乘，避免浮点
                let lhs = a.remaining() as u128 * b.count as u128;
                let rhs = b.remaining() as u128 * a.count as u128;
                lhs.cmp(&rhs)
            })
            .map(|(i, _)| i)?;

        self.slots[best].next_ip()
    }
}

pub struct IpBuffer {
    inner: Mutex<IpBufferInner>,
    popped: AtomicU64,
    total: u64,
}

impl IpBuffer {
    /// - `ips`  : CIDR 或单 IP 字符串列表
    /// - `total`: 最多弹出数量；各 slot 的配额由此自动按区间大小加权分配
    pub fn new(ips: Vec<&str>, total: u64) -> anyhow::Result<Self> {
        anyhow::ensure!(!ips.is_empty(), "ip list is empty");

        let ranges = ips
            .iter()
            .map(|s| IpRange::new(s))
            .collect::<anyhow::Result<Vec<_>>>()?;

        let counts = auto_distribute(&ranges, total);

        let slots = ranges
            .into_iter()
            .zip(counts)
            .map(|(range, count)| SampledRange::new(range, count))
            .collect();

        Ok(Self {
            inner: Mutex::new(IpBufferInner { slots }),
            popped: AtomicU64::new(0),
            total,
        })
    }

    pub fn pop(&self) -> Option<IpNetwork> {
        let ticket = self.popped.fetch_add(1, Ordering::Relaxed);
        if ticket >= self.total {
            self.popped.fetch_sub(1, Ordering::Relaxed);
            return None;
        }

        let mut guard = self.inner.lock().unwrap();
        match guard.next_weighted() {
            Some(ip) => Some(ip),
            None => {
                self.popped.fetch_sub(1, Ordering::Relaxed);
                None
            }
        }
    }

    pub fn popped_count(&self) -> u64 {
        self.popped.load(Ordering::Relaxed)
    }
}

// ── 工具函数 ──────────────────────────────────────────────────────────────

fn cheap_random(seed: usize) -> u128 {
    static CTR: AtomicUsize = AtomicUsize::new(0);
    let s = CTR.fetch_add(1, Ordering::Relaxed);
    let t = &s as *const _ as usize;
    let mut x = s ^ seed ^ t;
    x = x.wrapping_mul(cheap_random as *const () as usize | 1);
    x = x.rotate_left(usize::BITS / 2);
    x = x.swap_bytes();
    x as u128
}

// ── 测试 ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    static RAW_V4: &str = "
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

    static RAW_V6: &str = "
2400:cb00::/32
2606:4700::/32
2803:f800::/32
2405:b500::/32
2405:8100::/32
2a06:98c0::/29
2c0f:f248::/32";

    #[tokio::test]
    async fn cloudflare_v4() {
        let ips: Vec<&str> = RAW_V4.split_whitespace().collect();
        let total = 8000;
        let buf = IpBuffer::new(ips, total).unwrap();
        let mut i = 0u64;
        while let Some(ip) = buf.pop() {
            i += 1;
            println!("{}", ip.ip());
        }
        assert_eq!(i, total);
    }

    #[tokio::test]
    async fn cloudflare_v6() {
        let ips: Vec<&str> = RAW_V6.split_whitespace().collect();
        let total = 8000;
        let buf = IpBuffer::new(ips, total).unwrap();
        let mut i = 0u64;
        while let Some(ip) = buf.pop() {
            i += 1;
            println!("{}", ip.ip());
        }
        assert_eq!(i, total);
    }

    #[tokio::test]
    async fn total_exceeds_capacity() {
        // 两个 /32，容量共 2，要求 total=10 → 只能弹出 2 个
        let ips = vec!["1.2.3.4/32", "5.6.7.8/32"];
        let buf = IpBuffer::new(ips, 10).unwrap();
        let mut i = 0u64;
        while let Some(_) = buf.pop() {
            i += 1;
        }
        assert_eq!(i, 2, "capped by actual capacity");
    }
}
