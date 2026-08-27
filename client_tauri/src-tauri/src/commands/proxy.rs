use lib::proxy::{Proxy, ProxyAead, ProxyCompressor, ProxyConfig, TlsManager};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{LazyLock, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_machine_uid::MachineUidExt;
use tauri_plugin_opener::OpenerExt;

use super::settings::ProxySettings;
use super::Result;

pub struct ProxyState {
    pub proxy: Proxy,
    pub actual_port: u16,
}

pub static PROXY: LazyLock<RwLock<Option<ProxyState>>> = LazyLock::new(|| RwLock::new(None));

/// CA 目录固定为 app_data_dir/ca（复用 lib 的 ca 逻辑，目录取 Tauri 的 app_data_dir 以保证路径一致）
fn ca_dir<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    lib::client::ca::ca_dir_with(&app.path().app_data_dir().unwrap_or_default())
}

/// CA 私钥保护密钥（32B）：由设备 uid（machine-uid 插件）+ ca_dir/ca.salt 派生。
fn ca_key_secret<R: Runtime>(app: &AppHandle<R>) -> Result<[u8; 32]> {
    let dir = ca_dir(app);
    let uid = app
        .try_machine_uid()
        .ok_or_else(|| "machine-uid 插件未初始化".to_string())?
        .get_machine_uid()
        .map_err(super::err_str)?
        .id
        .ok_or_else(|| "获取设备 UID 失败".to_string())?;
    let salt = lib::client::ca::load_or_create_ca_salt(&dir).map_err(super::err_str)?;
    lib::tool::derive_ca_key_secret(&uid, &salt).map_err(super::err_str)
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub running: bool,
    pub port: u16,
    pub ip: Option<String>,
    pub compressor: String,
    pub aead: String,
}

fn status_from_state() -> ProxyStatus {
    let guard = PROXY.read().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(st) => {
            let algo = st.proxy.algo();
            ProxyStatus {
                running: st.proxy.is_running(),
                port: st.actual_port,
                ip: st.proxy.ip(),
                compressor: algo.compressor.name().to_string(),
                aead: algo.aead.name().to_string(),
            }
        }
        None => ProxyStatus::default(),
    }
}

fn emit_status<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.emit("proxy:status", status_from_state());
}

/// 启动本地代理（幂等：先停旧实例）。返回实际监听端口。
/// 若新实例启动失败，尽力恢复旧实例，避免代理静默下线。
#[tauri::command]
pub async fn proxy_start<R: Runtime>(app: AppHandle<R>, s: ProxySettings) -> Result<u16> {
    let compressor: ProxyCompressor = s.compressor.parse().map_err(super::err_str)?;
    let aead: ProxyAead = s.aead.parse().map_err(super::err_str)?;
    let key_secret = ca_key_secret(&app)?;

    let cfg = ProxyConfig {
        port: s.local_port,
        domain: s.domain,
        use_https: s.use_https,
        auth_key: s.auth_key,
        ca_dir: ca_dir(&app),
        ca_key_secret: key_secret,
        compressor,
        aead,
        pref_ip: s.pref_ip,
    };

    // 先构造（校验配置），失败时不打扰正在运行的旧实例
    let mut proxy = Proxy::new(cfg).map_err(super::err_str)?;

    // 停掉旧实例（新实例同端口重启需先释放端口）
    let mut old = {
        let mut guard = PROXY.write().map_err(|e| e.to_string())?;
        guard.take()
    };
    if let Some(prev) = old.as_mut() {
        prev.proxy.stop().await;
    }

    match proxy.start().await {
        Ok(port) => {
            {
                let mut guard = PROXY.write().map_err(|e| e.to_string())?;
                *guard = Some(ProxyState {
                    proxy,
                    actual_port: port,
                });
            }
            emit_status(&app);
            let result = tokio::time::timeout(
                Duration::from_secs(10),
                lib::proxy::check_proxy_availability(port),
            )
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("可用性检测超时(>10s)")));
            let _ = app.emit("proxy:availability", availability_payload(result));
            Ok(port)
        }
        Err(e) => {
            let err_msg = format!("{e}");
            if let Some(mut prev) = old {
                match prev.proxy.start().await {
                    Ok(p) => {
                        {
                            let mut guard = PROXY.write().map_err(|e| e.to_string())?;
                            *guard = Some(ProxyState {
                                proxy: prev.proxy,
                                actual_port: p,
                            });
                        }
                        emit_status(&app);
                        Err(format!("启动失败（{err_msg}）；已恢复原代理"))
                    }
                    Err(e2) => Err(format!(
                        "启动失败（{err_msg}）；恢复原代理也失败（{e2}），需手动重启代理"
                    )),
                }
            } else {
                Err(err_msg)
            }
        }
    }
}

#[tauri::command]
pub async fn proxy_stop<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    let old = {
        let mut guard = PROXY.write().map_err(|e| e.to_string())?;
        guard.take()
    };
    if let Some(mut prev) = old {
        prev.proxy.stop().await;
    }
    emit_status(&app);
    Ok(())
}

#[tauri::command]
pub fn proxy_status() -> ProxyStatus {
    status_from_state()
}

#[tauri::command]
pub fn proxy_set_aead<R: Runtime>(app: AppHandle<R>, aead: String) -> Result<()> {
    let guard = PROXY.read().map_err(|e| e.to_string())?;
    let st = guard
        .as_ref()
        .ok_or_else(|| "代理未启动".to_string())?;
    st.proxy.set_aead(&aead).map_err(super::err_str)?;
    drop(guard);
    emit_status(&app);
    Ok(())
}

#[tauri::command]
pub fn proxy_set_compressor<R: Runtime>(app: AppHandle<R>, compressor: String) -> Result<()> {
    let guard = PROXY.read().map_err(|e| e.to_string())?;
    let st = guard
        .as_ref()
        .ok_or_else(|| "代理未启动".to_string())?;
    st.proxy
        .set_compressor(&compressor)
        .map_err(super::err_str)?;
    drop(guard);
    emit_status(&app);
    Ok(())
}

#[tauri::command]
pub fn proxy_set_ip<R: Runtime>(app: AppHandle<R>, ip: Option<String>) -> Result<()> {
    let guard = PROXY.read().map_err(|e| e.to_string())?;
    let st = guard
        .as_ref()
        .ok_or_else(|| "代理未启动".to_string())?;
    st.proxy.set_ip(ip.as_deref()).map_err(super::err_str)?;
    drop(guard);
    emit_status(&app);
    Ok(())
}

#[tauri::command]
pub fn open_ca_dir<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    let dir = ca_dir(&app);
    let secret = ca_key_secret(&app)?;
    // TlsManager::init 为阻塞 IO，同步执行即可（open_ca_dir 为同步命令，非 async）
    TlsManager::init(&dir, &secret).map_err(super::err_str)?;

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        app.opener()
            .open_path(dir.display().to_string(), None::<&str>)
            .map_err(super::err_str)?;
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        todo!()
    }

    Ok(())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CaInfo {
    pub path: String,
    pub cert_pem: String,
    /// 本次 init 是否自动重建了 CA（设备 uid 变化/密钥文件损坏），前端据此提示重新导入
    pub rebuilt: bool,
}

#[tauri::command]
pub fn ca_info<R: Runtime>(app: AppHandle<R>) -> Result<CaInfo> {
    let dir = ca_dir(&app);
    let uid = app
        .try_machine_uid()
        .ok_or_else(|| "machine-uid plugin not initialized".to_string())?
        .get_machine_uid()
        .map_err(super::err_str)?
        .id
        .ok_or_else(|| "failed to obtain device uid".to_string())?;
    let info = lib::client::ca::ca_info_with_uid(&dir, &uid).map_err(super::err_str)?;
    Ok(CaInfo {
        path: info.path.display().to_string(),
        cert_pem: info.cert_pem,
        rebuilt: info.rebuilt,
    })
}

/// 代理可用性检测结果（同步检测后通过事件推送）
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProxyAvailability {
    pub ok: bool,
    pub ip: Option<String>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

fn availability_payload<E: std::fmt::Display>(
    result: std::result::Result<lib::proxy::ProxyCheck, E>,
) -> ProxyAvailability {
    match result {
        Ok(check) => ProxyAvailability {
            ok: true,
            ip: Some(check.ip),
            latency_ms: Some(check.latency_ms),
            error: None,
        },
        Err(e) => ProxyAvailability {
            ok: false,
            ip: None,
            latency_ms: None,
            error: Some(format!("{e}")),
        },
    }
}

/// 手动重测代理可用性（结果通过 proxy:availability 事件推送）
#[tauri::command]
pub async fn proxy_check_availability<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    let port = {
        let guard = PROXY.read().map_err(|e| e.to_string())?;
        let st = guard
            .as_ref()
            .ok_or_else(|| "代理未启动".to_string())?;
        st.actual_port
    };
    let result = lib::proxy::check_proxy_availability(port).await;
    let _ = app.emit("proxy:availability", availability_payload(result));
    Ok(())
}

/// 应用退出时清理：停止本地代理（若运行中）。连接任务随进程退出终止。
pub fn shutdown() {
    let old = {
        let mut guard = PROXY.write().unwrap_or_else(|e| e.into_inner());
        guard.take()
    };
    if let Some(mut prev) = old {
        tauri::async_runtime::spawn(async move {
            prev.proxy.stop().await;
        });
    }
}

#[tauri::command]
pub async fn install_ca<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    // 统一使用 lib 的 std::process::Command 实现，阻塞操作移至 spawn_blocking
    let dir = ca_dir(&app);
    let secret = ca_key_secret(&app)?;
    let dir_clone = dir.clone();
    tokio::task::spawn_blocking(move || TlsManager::init(&dir_clone, &secret))
        .await
        .map_err(|e| e.to_string())?
        .map_err(super::err_str)?;
    let ca_path = dir.join("ca.crt.pem");
    if !ca_path.exists() || !ca_path.is_file() {
        return Err("CA 证书不存在".into());
    }
    let ca_path_clone = ca_path.clone();
    let _report = tokio::task::spawn_blocking(move || lib::client::ca::install_from_path(&ca_path_clone))
        .await
        .map_err(|e| e.to_string())?
        .map_err(super::err_str)?;
    Ok(())
}
