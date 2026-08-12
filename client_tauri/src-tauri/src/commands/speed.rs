use std::net::SocketAddr;
// 会话式 IP 优选测速：
//   speed_test_start  → 后台任务 + 立即返回 gen(代际号)
//   speed_test_cancel → 置取消标志 + abort 后台任务
//   speed_test_state  → 拉模式查询是否在跑
// 进度/结果全部经事件推送，payload 携带 gen，前端忽略过期代际的事件。
use anyhow::Result as AnyhowResult;
use lib::speed_test::health::{batch_health, default_matcher};
use lib::speed_test::ip::IpBuffer;
use lib::speed_test::tcping::batch_tcping;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use reqwest::Client;
use tauri::{AppHandle, Emitter, Runtime};
use lib::tool::derive_keys;
use super::settings::ProxySettings;
use super::Result;

/// 单次测速会话的整体时长上限，超时自动中止并报错
const SESSION_HARD_DEADLINE: Duration = Duration::from_secs(120);
/// 进度事件节流间隔（距上次 emit 不足此间隔时合并到下一次）
const PROGRESS_THROTTLE: Duration = Duration::from_millis(500);
/// done 事件返回的结果条数上限（前端按此渲染，避免大列表卡顿）
const DONE_MAX_ROWS: usize = 50;
/// 测速与健康检查统一使用的端口（大陆环境 *.workers.dev 走 HTTP/80 绕过 SNI 阻断）
const TEST_PORT: u16 = 80;
/// worker_health 的默认探测 IP 池（prefIp 优先，之后按序尝试）
const DEFAULT_WORKER_IPS: [&str; 5] = [
    "104.17.23.238",
    "104.16.39.227",
    "104.16.124.96",
    "172.64.0.117",
    "162.158.0.6",
];

/// Cloudflare 官方 IPv4 网段（IP 优选采样范围）
static CF_IP_V4: &str = "
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

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct SpeedTestOpts {
    pub total: u64,
    pub tcping_limit: usize,
    pub tcping_timeout_ms: u64,
    pub health_limit: usize,
    pub health_timeout_ms: u64,
}

impl Default for SpeedTestOpts {
    // 默认值必须与前端 client_tauri/src/store/speedTest.ts 的 opts 初始值保持一致
    // （跨语言契约，改动两端需同步）
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

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Tcping,
    Health,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IpResult {
    ip: String,
    rtt_ms: f32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PhasePayload {
    gen: u64,
    phase: Phase,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpeedProgressPayload {
    gen: u64,
    phase: Phase,
    tested: u64,
    total: u64,
    rtt_ms: Option<f32>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpeedDonePayload {
    gen: u64,
    results: Vec<IpResult>,
    best_ip: Option<String>,
    tested: u64,
    healthy: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    gen: u64,
    message: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CancelledPayload {
    gen: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpeedTestState {
    pub running: bool,
}

enum SpeedOutcome {
    Done(SpeedDonePayload),
    Cancelled,
}

// ── 会话管理 ───────────────────────────────────────────────────────────────

struct Session {
    handle: tauri::async_runtime::JoinHandle<()>,
    cancel: Arc<AtomicBool>,
}

static SESSION: RwLock<Option<Arc<Session>>> = RwLock::new(None);
static NEXT_GEN: AtomicU64 = AtomicU64::new(0);

fn cancel_session() {
    let old = {
        let mut guard = SESSION.write().unwrap_or_else(|e| e.into_inner());
        guard.take()
    };
    if let Some(s) = old {
        s.cancel.store(true, Ordering::Relaxed);
        s.handle.abort();
    }
}

/// 应用退出时清理会话（由 RunEvent::ExitRequested 调用）
pub fn shutdown() {
    cancel_session();
}

// ── 命令 ───────────────────────────────────────────────────────────────────

/// 启动一次测速：替换旧会话，后台运行，立即返回本次会话的代际号（gen）。
#[tauri::command]
pub async fn speed_test_start<R: Runtime>(
    app: AppHandle<R>,
    s: ProxySettings,
    opts: SpeedTestOpts,
) -> Result<u64> {
    cancel_session();

    let gen = NEXT_GEN.fetch_add(1, Ordering::Relaxed);
    let cancel = Arc::new(AtomicBool::new(false));
    let task_cancel = Arc::clone(&cancel);
    let handle = tauri::async_runtime::spawn(async move {
        run_speed_test(app, s, opts, task_cancel, gen).await;
    });

    let mut guard = SESSION.write().map_err(|e| e.to_string())?;
    *guard = Some(Arc::new(Session { handle, cancel }));
    Ok(gen)
}

/// 中止当前测速会话（若有）
#[tauri::command]
pub fn speed_test_cancel() -> Result<()> {
    cancel_session();
    Ok(())
}

/// 查询是否有测速会话在运行（前端 watchdog 兜底用）
#[tauri::command]
pub fn speed_test_state() -> SpeedTestState {
    let running = SESSION.read().map(|g| g.is_some()).unwrap_or(false);
    SpeedTestState { running }
}

// ── 后台任务 ───────────────────────────────────────────────────────────────

async fn run_speed_test<R: Runtime>(
    app: AppHandle<R>,
    s: ProxySettings,
    opts: SpeedTestOpts,
    cancel: Arc<AtomicBool>,
    gen: u64,
) {
    let outcome = {
        let deadline = tokio::time::sleep(SESSION_HARD_DEADLINE);
        tokio::pin!(deadline);
        tokio::select! {
            r = run_speed_test_inner(&app, &s, &opts, &cancel, gen) => r,
            _ = &mut deadline => {
                cancel.store(true, Ordering::Relaxed);
                Err(anyhow::anyhow!(
                    "speed test timed out after {}s",
                    SESSION_HARD_DEADLINE.as_secs()
                ))
            }
        }
    };

    // 先清理会话，再发终结事件；会话已被替换/取消接管时静默退出
    let is_owner = {
        let mut guard = SESSION.write().unwrap_or_else(|e| e.into_inner());
        let owner = guard
            .as_ref()
            .map(|cur| Arc::ptr_eq(&cur.cancel, &cancel))
            .unwrap_or(false);
        if owner {
            *guard = None;
        }
        owner
    };
    if !is_owner {
        return;
    }

    match outcome {
        Ok(SpeedOutcome::Done(payload)) => {
            let _ = app.emit("speed-test:done", payload);
        }
        Ok(SpeedOutcome::Cancelled) => {
            let _ = app.emit("speed-test:cancelled", CancelledPayload { gen });
        }
        Err(e) => {
            let _ = app.emit(
                "speed-test:error",
                ErrorPayload { gen, message: format!("{e:#}") },
            );
        }
    }
}

async fn run_speed_test_inner<R: Runtime>(
    app: &AppHandle<R>,
    s: &ProxySettings,
    opts: &SpeedTestOpts,
    cancel: &AtomicBool,
    gen: u64,
) -> AnyhowResult<SpeedOutcome> {
    let total = opts.total.max(1);
    let tcping_timeout = Duration::from_millis(opts.tcping_timeout_ms.max(1));

    let domain = s.domain.trim().to_string();

    let cidrs: Vec<&str> = CF_IP_V4.split_whitespace().collect();
    let buf = IpBuffer::new(cidrs, total)?;

    let _ = app.emit("speed-test:phase", PhasePayload { gen, phase: Phase::Tcping });

    let mut last_emit = Instant::now();
    let (results, aborted) = batch_tcping(
        buf,
        opts.tcping_limit.max(1),
        TEST_PORT,
        tcping_timeout,
        |done, rtt| {
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
            if done >= total || last_emit.elapsed() >= PROGRESS_THROTTLE {
                last_emit = Instant::now();
                let _ = app.emit(
                    "speed-test:progress",
                    SpeedProgressPayload {
                        gen,
                        phase: Phase::Tcping,
                        tested: done,
                        total,
                        rtt_ms: rtt,
                    },
                );
            }
            true
        },
    )
    .await?;

    if aborted || cancel.load(Ordering::Relaxed) {
        return Ok(SpeedOutcome::Cancelled);
    }

    let tested = results.len() as u64;
    let mut sorted = results;
    sorted.sort_by(|a, b| a.1.total_cmp(&b.1));

    let candidates: Vec<_> = sorted
        .iter()
        .take(opts.health_limit)
        .map(|(ip, _)| *ip)
        .collect();
    let candidate_total = candidates.len() as u64;
    if candidate_total == 0 {
        anyhow::bail!("no reachable IPs to health-check");
    }

    // ── 阶段 2：health（与 tcping 同端口，HTTP 明文绕过 SNI 阻断）──
    let token = {
        let keys = derive_keys(&s.auth_key, &domain)?;
        lib::proxy::gen_auth_token(&keys.token_base)
    };
    let _ = app.emit("speed-test:phase", PhasePayload { gen, phase: Phase::Health });

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
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
            if done >= total_c || last_emit.elapsed() >= PROGRESS_THROTTLE {
                last_emit = Instant::now();
                let _ = app.emit(
                    "speed-test:progress",
                    SpeedProgressPayload {
                        gen,
                        phase: Phase::Health,
                        tested: done,
                        total: total_c,
                        rtt_ms: None,
                    },
                );
            }
            true
        },
    )
    .await?;

    if aborted || cancel.load(Ordering::Relaxed) {
        return Ok(SpeedOutcome::Cancelled);
    }

    let final_results: Vec<IpResult> = sorted
        .iter()
        .filter(|(ip, _)| healthy.iter().any(|h| h.ip() == ip.ip()))
        .take(DONE_MAX_ROWS)
        .map(|(ip, rtt)| IpResult {
            ip: ip.ip().to_string(),
            rtt_ms: *rtt,
        })
        .collect();

    let best_ip = final_results.first().map(|r| r.ip.clone());
    Ok(SpeedOutcome::Done(SpeedDonePayload {
        gen,
        results: final_results,
        best_ip,
        tested,
        healthy: healthy.len(),
    }))
}

/// 验证 worker 配置：HTTP/80 + `.resolve()` 绕过 DNS 污染与 SNI 阻断。
/// IP 顺序：prefIp（若设置）→ 内置默认池；任一返回 200 即视为通过。
#[tauri::command]
pub async fn worker_health(s: ProxySettings) -> Result<bool> {
    let host = s.domain.trim().to_string();
    let token = {
        let keys = derive_keys(&s.auth_key, &host).map_err(super::err_str)?;
        lib::proxy::gen_auth_token(&keys.token_base)
    };

    let mut ips: Vec<String> = Vec::new();
    if let Some(ip) = s.pref_ip.as_deref().map(str::trim).filter(|ip| !ip.is_empty()) {
        ips.push(ip.to_string());
    }
    ips.extend(DEFAULT_WORKER_IPS.iter().map(|s| s.to_string()));

    for ip in ips {
        let addr: SocketAddr = format!("{ip}:{TEST_PORT}")
            .parse()
            .map_err(super::err_str)?;
        let client = match Client::builder()
            .resolve(&host, addr)
            .timeout(Duration::from_secs(3))
            .no_proxy()
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("worker_health: failed to build client for {ip}: {e}");
                continue;
            }
        };

        let url = lib::http::UrlBuilder::new()
            .https(false)
            .host(host.as_str())
            .port(TEST_PORT)
            .path("/health")
            .build()
            .map_err(super::err_str)?;
        let resp = match client.get(url).bearer_auth(&token).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("worker_health: {ip} failed: {e}");
                continue;
            }
        };
        let status = resp.status();
        match resp.bytes().await {
            Ok(body) if default_matcher(status, &body) => return Ok(true),
            Ok(_) => eprintln!("worker_health: {ip} matcher failed (status {status})"),
            Err(e) => eprintln!("worker_health: {ip} read body failed: {e}"),
        }
    }
    Ok(false)
}
