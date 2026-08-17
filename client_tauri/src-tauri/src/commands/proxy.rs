use lib::proxy::{Proxy, ProxyAead, ProxyCompressor, ProxyConfig, TlsManager};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};
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

/// CA 目录固定为 app_data_dir/ca
fn ca_dir<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    app.path().app_data_dir().unwrap_or_default().join("ca")
}

/// CA 私钥保护密钥（32B）：由设备 uid（machine-uid 插件）+ ca_dir/ca.salt 派生。
/// uid 无法获取时直接报错（容器/系统无 machine-id 等场景）。
fn ca_key_secret<R: Runtime>(app: &AppHandle<R>) -> Result<[u8; 32]> {
    let dir = ca_dir(app);
    let uid = app
        .try_machine_uid()
        .ok_or_else(|| "machine-uid plugin not initialized".to_string())?
        .get_machine_uid()
        .map_err(super::err_str)?
        .id
        .ok_or_else(|| "failed to obtain device uid".to_string())?;
    let salt = load_or_create_ca_salt(&dir)?;
    lib::tool::derive_ca_key_secret(&uid, &salt).map_err(super::err_str)
}

/// ca_dir/ca.salt：首次运行随机生成并持久化（非机密，仅保证派生密钥的
/// 安装独立性）；已存在且长度正确则复用，损坏则重新生成。
fn load_or_create_ca_salt(dir: &Path) -> Result<[u8; 32]> {
    let salt_path = dir.join("ca.salt");
    if salt_path.exists() {
        if let Ok(data) = std::fs::read(&salt_path) {
            if data.len() == 32 {
                let mut salt = [0u8; 32];
                salt.copy_from_slice(&data);
                return Ok(salt);
            }
        }
    }
    let salt: [u8; 32] = rand::random();
    std::fs::create_dir_all(dir).map_err(super::err_str)?;
    std::fs::write(&salt_path, salt).map_err(super::err_str)?;
    Ok(salt)
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
            let result = lib::proxy::check_proxy_availability(port).await;
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
        .ok_or_else(|| "proxy not started".to_string())?;
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
        .ok_or_else(|| "proxy not started".to_string())?;
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
        .ok_or_else(|| "proxy not started".to_string())?;
    st.proxy.set_ip(ip.as_deref()).map_err(super::err_str)?;
    drop(guard);
    emit_status(&app);
    Ok(())
}

#[tauri::command]
pub fn open_ca_dir<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    let dir = ca_dir(&app);
    let secret = ca_key_secret(&app)?;
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
    let secret = ca_key_secret(&app)?;
    let mgr = TlsManager::init(&dir, &secret).map_err(super::err_str)?;
    Ok(CaInfo {
        path: TlsManager::ca_cert_path(&dir).display().to_string(),
        cert_pem: mgr.ca_cert_pem().to_string(),
        rebuilt: mgr.rebuilt(),
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
            .ok_or_else(|| "proxy not started".to_string())?;
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
    #[cfg(desktop)]
    use tauri_plugin_shell::ShellExt;
    let dir = ca_dir(&app);
    let ca_path = dir.join("ca.crt.pem");

    if !ca_path.exists() || !ca_path.is_file() {
        return Err("CA certificate not found".into());
    }

    #[cfg(target_os = "linux")]
    {

        let shell = app.shell();
        let cert_name = "com.zz.freeproxy"; // NSS 数据库中显示的证书名称
        let cert_file_name = format!("{}.crt", cert_name);
        let ca_path_str = ca_path.to_string_lossy().into_owned();

        // 1. 前置依赖检查：certutil 对于 Linux 浏览器导入根证书是强依赖
        let check_certutil = shell.command("which").args(["certutil"]).output().await;
        let has_certutil = check_certutil.map(|o| o.status.success()).unwrap_or(false);
        if !has_certutil {
            return Err("Missing 'certutil' tool for browser certificate installation.\nPlease install it manually first:\n- Debian/Ubuntu: sudo apt install libnss3-tools\n- Fedora/RHEL: sudo dnf install nss-tools\n- Arch Linux: sudo pacman -S nss\n- openSUSE: sudo zypper install mozilla-nss-tools".into());
        }

        let (dest_dir, update_cmd) = if PathBuf::from("/usr/local/share/ca-certificates").exists() {
            // Debian / Ubuntu
            ("/usr/local/share/ca-certificates", "update-ca-certificates")
        } else if PathBuf::from("/etc/pki/ca-trust/source/anchors").exists() {
            // RHEL / CentOS / Fedora
            (
                "/etc/pki/ca-trust/source/anchors",
                "update-ca-trust extract",
            )
        } else if PathBuf::from("/etc/ca-certificates/trust-source/anchors").exists() {
            // Arch Linux
            (
                "/etc/ca-certificates/trust-source/anchors",
                "trust extract-compat",
            )
        } else if PathBuf::from("/etc/pki/trust/anchors").exists() {
            // SUSE / openSUSE
            ("/etc/pki/trust/anchors", "update-ca-certificates")
        } else {
            return Err("Unsupported Linux distribution. Please install CA manually.".into());
        };

        let dest_path = format!("{}/{}", dest_dir, cert_file_name);


        // ==========================================
        // 第一步：Root 权限 - 更新系统级 CA
        // ==========================================
         {
            let root_script = format!(
                r#"
                cp "{ca_path}" "{dest_path}" && chmod 644 "{dest_path}" && {update_cmd}
                "#,
                ca_path = ca_path_str,
                dest_path = dest_path,
                update_cmd = update_cmd
            );

            let root_output = shell
                .command("pkexec")
                .args(["sh", "-c", &root_script])
                .output()
                .await
                .map_err(|e| e.to_string())?;

            if !root_output.status.success() {
                return Err(format!(
                    "Failed to install system CA via pkexec (User cancelled or error): {}",
                    String::from_utf8_lossy(&root_output.stderr)
                ));
            }
        }

        // ==========================================
        // 第二步：普通用户权限 - 使用 certutil 注入浏览器 NSS 数据库
        // ==========================================
        let user_script = format!(
            r#"
            CERT_NAME="{cert_name}"
            CA_PATH="{ca_path}"
            DIRS=""

            if [ -d "$HOME/.pki/nssdb" ]; then
                DIRS="$DIRS $HOME/.pki/nssdb"
            else
                mkdir -p "$HOME/.pki/nssdb"
                certutil -N -d sql:"$HOME/.pki/nssdb" --empty-password
                DIRS="$DIRS $HOME/.pki/nssdb"
            fi


            if [ -d "$HOME/.mozilla/firefox" ]; then
                for prof in "$HOME/.mozilla/firefox/"*.*; do
                    if [ -d "$prof" ]; then DIRS="$DIRS $prof"; fi
                done
            fi

            for db in $DIRS; do
                certutil -d sql:"$db" -A -t "C,," -n "$CERT_NAME" -i "$CA_PATH" 2>/dev/null || \
                certutil -d "$db" -A -t "C,," -n "$CERT_NAME" -i "$CA_PATH" 2>/dev/null || true
            done
            "#,
            cert_name = cert_name,
            ca_path = ca_path_str
        );

        let user_output = shell
            .command("sh")
            .args(["-c", &user_script])
            .output()
            .await
            .map_err(|e| e.to_string())?;

        if !user_output.status.success() {
            println!(
                "Warning: Failed to inject to some user NSS databases: {}",
                String::from_utf8_lossy(&user_output.stderr)
            );
        }

        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let shell = app.shell();
        let ca_path_str = ca_path.to_string_lossy().into_owned();

        let output = shell
            .command("certutil")
            .args(["-addstore", "Root", &ca_path_str])
            .output()
            .await
            .map_err(|e| e.to_string())?;

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            return Err(format!(
                "Failed to install CA on Windows (User rejected or error occurred).\nStdout: {}\nStderr: {}",
                stdout.trim(),
                stderr.trim()
            ));
        }

        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let shell = app.shell();
        let ca_path_str = ca_path.to_string_lossy().into_owned();

        let script = format!(
            "do shell script \"security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain '{}'\" with administrator privileges",
            ca_path_str
        );

        let output = shell
            .command("osascript")
            .args(["-e", &script])
            .output()
            .await
            .map_err(|e| e.to_string())?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "Failed to install CA on macOS (User rejected or error occurred).\nStderr: {}",
                stderr.trim()
            )
            .into());
        }

        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("OS not supported".into())
}
