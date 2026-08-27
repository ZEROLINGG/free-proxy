 // 会话式 IP 优选测速：
 //   speed_test_start  → 后台任务 + 立即返回 gen(代际号)
 //   speed_test_cancel → 置取消标志 + abort 后台任务
 //   speed_test_state  → 拉模式查询是否在跑
 // 进度/结果全部经事件推送，payload 携带 gen，前端忽略过期代际的事件。
use super::settings::ProxySettings;
use super::Result;
use anyhow::Result as AnyhowResult;
pub use lib::client::speed::SpeedTestOpts;
use lib::client::speed::{
    SESSION_HARD_DEADLINE, reject_domain_port, run_two_phase, worker_health as lib_worker_health,
};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter, Runtime};

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
                    "测速超过 {} 秒上限,已自动中止",
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
                ErrorPayload {
                    gen,
                    message: format!("{e:#}"),
                },
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
    // 提前校验 domain 端口（与 lib 保持一致）
    reject_domain_port(s.domain.trim())?;

    let total = opts.total.max(1);

    // 阶段1 开始
    let _ = app.emit(
        "speed-test:phase",
        PhasePayload {
            gen,
            phase: Phase::Tcping,
        },
    );

    // 用 lib 的两阶段核心，进度回调中 emit 事件并检查 cancel
    let mut tcping_done: u64 = 0;
    let mut tcping_total = total;

    // 阶段2 事件将在回调中切换
    let mut in_health = false;

    let res = run_two_phase(
        &s.domain,
        &s.auth_key,
        opts,
        Some(cancel),
        |done, total_c, rtt| {
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
            tcping_done = done;
            tcping_total = total_c;
            let _ = app.emit(
                "speed-test:progress",
                SpeedProgressPayload {
                    gen,
                    phase: Phase::Tcping,
                    tested: done,
                    total: total_c,
                    rtt_ms: rtt,
                },
            );
            true
        },
        |done, total_c| {
            if !in_health {
                in_health = true;
                let _ = app.emit(
                    "speed-test:phase",
                    PhasePayload {
                        gen,
                        phase: Phase::Health,
                    },
                );
            }
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
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
            true
        },
    )
    .await;

    match res {
        Ok((best_ip, results)) => {
            // 若 cancel 在完成后被置位，视为取消
            if cancel.load(Ordering::Relaxed) {
                return Ok(SpeedOutcome::Cancelled);
            }
            let healthy = results.len();
            let final_results: Vec<IpResult> = results
                .into_iter()
                .map(|(ip, rtt)| IpResult { ip, rtt_ms: rtt })
                .collect();
            let best = best_ip.clone();
            Ok(SpeedOutcome::Done(SpeedDonePayload {
                gen,
                results: final_results,
                best_ip: best,
                tested: tcping_done.max(tcping_total),
                healthy,
            }))
        }
        Err(e) => {
            let msg = format!("{e:#}");
            if msg.contains("中止") || cancel.load(Ordering::Relaxed) {
                Ok(SpeedOutcome::Cancelled)
            } else {
                Err(e)
            }
        }
    }
}

/// 验证 worker 配置：复用 lib 的并发池实现
#[tauri::command]
pub async fn worker_health(s: ProxySettings) -> Result<bool> {
    lib_worker_health(&s.domain, &s.auth_key, s.pref_ip.as_deref())
        .await
        .map_err(super::err_str)
}
