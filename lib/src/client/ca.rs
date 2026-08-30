// CA 证书管理:与 GUI 客户端共用同一 CA 目录(app_data_dir/ca)与
// 设备 UID 派生密钥,两端导出的证书完全一致,导入一次两端通用。
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::proxy::tls::TlsManager;

use super::config::app_data_dir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaInfo {
    pub dir: PathBuf,
    pub path: PathBuf,
    pub cert_pem: String,
    pub rebuilt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallReport {
    pub ca_path: PathBuf,
    /// Linux 下系统 CA 目标路径（其他平台为 None）
    pub dest_path: Option<String>,
    /// 安装过程中的非致命警告（如部分 NSS 注入失败）
    pub warnings: Vec<String>,
}

pub fn ca_dir() -> Result<PathBuf> {
    Ok(app_data_dir()?.join("ca"))
}

pub fn ca_dir_with(base: &Path) -> PathBuf {
    base.join("ca")
}

/// ca_dir/ca.salt:首次运行随机生成并持久化(非机密,仅保证派生密钥的
/// 安装独立性);已存在且长度正确则复用,损坏则重新生成。
/// 文件权限在 unix 下收紧为 0o600，原子写入避免并发竞态。
pub fn load_or_create_ca_salt(dir: &Path) -> Result<[u8; 32]> {
    let salt_path = dir.join("ca.salt");
    if salt_path.exists() {
        if let Ok(data) = std::fs::read(&salt_path) {
            if data.len() == 32 {
                #[cfg(unix)]
                {
                    // 修复旧文件权限（原先 0o644）
                    if let Ok(meta) = std::fs::metadata(&salt_path) {
                        use std::os::unix::fs::PermissionsExt;
                        let mode = meta.permissions().mode() & 0o777;
                        if mode != 0o600 {
                            let _ = std::fs::set_permissions(&salt_path, std::fs::Permissions::from_mode(0o600));
                        }
                    }
                }
                let mut salt = [0u8; 32];
                salt.copy_from_slice(&data);
                return Ok(salt);
            }
        }
    }
    let salt: [u8; 32] = rand::random();
    std::fs::create_dir_all(dir)
        .with_context(|| format!("创建 CA 目录失败: {}", dir.display()))?;
    // 原子写入：优先 create_new，冲突则回退读取
    #[cfg(unix)]
    {
        use std::io::ErrorKind;
        use std::os::unix::fs::OpenOptionsExt;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&salt_path)
        {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(&salt)
                    .with_context(|| format!("写入 ca.salt 失败: {}", salt_path.display()))?;
                f.sync_all().ok();
                return Ok(salt);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                // 并发下已被其他进程创建，回退读取
                if let Ok(data) = std::fs::read(&salt_path) {
                    if data.len() == 32 {
                        let mut s = [0u8; 32];
                        s.copy_from_slice(&data);
                        return Ok(s);
                    }
                }
                // 损坏则走下面的覆盖写
            }
            Err(e) => return Err(e).with_context(|| format!("写入 ca.salt 失败: {}", salt_path.display())),
        }
        // 非 AlreadyExists 或损坏，尝试普通覆盖写（带 0o600）
        write_salt_file_unix(&salt_path, &salt)?;
        return Ok(salt);
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&salt_path, salt)
            .with_context(|| format!("写入 ca.salt 失败: {}", salt_path.display()))?;
        Ok(salt)
    }
}

#[cfg(unix)]
fn write_salt_file_unix(path: &Path, data: &[u8; 32]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("salt.tmp");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("写入 ca.salt 失败: {}", tmp.display()))?;
    f.write_all(data)?;
    f.sync_all().ok();
    std::fs::rename(&tmp, path)
        .with_context(|| format!("写入 ca.salt 失败: {}", path.display()))?;
    Ok(())
}

/// CA 私钥保护密钥(32B):由设备 uid + ca.salt 派生。uid 无法获取时直接报错。
/// 调用方需提供 uid（CLI 用 machine_uid::get()，GUI 用 tauri_plugin_machine_uid）
pub fn derive_secret_for_uid(uid: &str, dir: &Path) -> Result<[u8; 32]> {
    let salt = load_or_create_ca_salt(dir)?;
    crate::tool::derive_ca_key_secret(uid, &salt).context("派生 CA 密钥失败")
}

pub fn key_secret_for(dir: &Path) -> Result<[u8; 32]> {
    let uid = get_machine_uid().map_err(|e| anyhow!("获取设备 UID 失败: {e}"))?;
    derive_secret_for_uid(&uid, dir)
}

#[cfg(not(target_arch = "wasm32"))]
fn get_machine_uid() -> Result<String> {
    machine_uid::get().map_err(|e| anyhow!("{e}"))
}

#[cfg(target_arch = "wasm32")]
fn get_machine_uid() -> Result<String> {
    Err(anyhow!("machine_uid not available on wasm"))
}

/// 供 CLI/GUI 统一调用的 ca_key_secret（使用 app_data_dir/ca）
pub fn ca_key_secret() -> Result<[u8; 32]> {
    key_secret_for(&ca_dir()?)
}

/// 返回结构化 CA 信息，不再直接 println（由调用方决定如何展示）
pub fn ca_info() -> Result<CaInfo> {
    let dir = ca_dir()?;
    ca_info_with_dir(&dir)
}

pub fn ca_info_with_dir(dir: &Path) -> Result<CaInfo> {
    let secret = key_secret_for(dir)?;
    let mgr = TlsManager::init(dir, &secret)?;
    let path = TlsManager::ca_cert_path(dir);
    Ok(CaInfo {
        dir: dir.to_path_buf(),
        path,
        cert_pem: mgr.ca_cert_pem().to_string(),
        rebuilt: mgr.rebuilt(),
    })
}

pub fn ca_info_with_uid(dir: &Path, uid: &str) -> Result<CaInfo> {
    let secret = derive_secret_for_uid(uid, dir)?;
    let mgr = TlsManager::init(dir, &secret)?;
    let path = TlsManager::ca_cert_path(dir);
    Ok(CaInfo {
        dir: dir.to_path_buf(),
        path,
        cert_pem: mgr.ca_cert_pem().to_string(),
        rebuilt: mgr.rebuilt(),
    })
}

// 兼容旧名：保留 show_info/show_dir 薄包装，但不再直接打印，仅返回结构化数据
// 调用方（CLI）负责打印；lib 层不触及 stdout
pub fn show_info() -> Result<CaInfo> {
    ca_info()
}

pub fn show_dir() -> Result<PathBuf> {
    let dir = ca_dir()?;
    let secret = ca_key_secret()?;
    let _ = TlsManager::init(&dir, &secret)?;
    Ok(dir)
}

// ── helpers ──────────────────────────────────────────────────────────────

fn shell_quote(s: &str) -> String {
    // 单引号包裹，内部单引号转义为 '\'' 
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[cfg(target_os = "linux")]
fn ensure_certutil() -> Result<()> {
    let has = Command::new("which")
        .arg("certutil")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !has {
        return Err(anyhow!(
            "缺少 certutil 工具,无法为浏览器安装根证书。\n请先手动安装:\n\
             - Debian/Ubuntu: sudo apt install libnss3-tools\n\
             - Fedora/RHEL:   sudo dnf install nss-tools\n\
             - Arch Linux:    sudo pacman -S nss\n\
             - openSUSE:      sudo zypper install mozilla-nss-tools"
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn detect_linux_dest() -> Result<(String, String)> {
    let cert_name = "com.zz.freeproxy";
    let cert_file = format!("{cert_name}.crt");
    if PathBuf::from("/usr/local/share/ca-certificates").exists() {
        Ok((
            format!("/usr/local/share/ca-certificates/{cert_file}"),
            "update-ca-certificates".into(),
        ))
    } else if PathBuf::from("/etc/pki/ca-trust/source/anchors").exists() {
        Ok((
            format!("/etc/pki/ca-trust/source/anchors/{cert_file}"),
            "update-ca-trust extract".into(),
        ))
    } else if PathBuf::from("/etc/ca-certificates/trust-source/anchors").exists() {
        Ok((
            format!("/etc/ca-certificates/trust-source/anchors/{cert_file}"),
            "trust extract-compat".into(),
        ))
    } else if PathBuf::from("/etc/pki/trust/anchors").exists() {
        Ok((
            format!("/etc/pki/trust/anchors/{cert_file}"),
            "update-ca-certificates".into(),
        ))
    } else {
        Err(anyhow!("不支持当前 Linux 发行版,请手动安装 CA"))
    }
}

/// 将 CA 安装为系统/浏览器信任的根证书(按平台调用系统工具)。
/// 统一使用 std::process::Command，返回结构化报告而非直接 println。
pub fn install() -> Result<InstallReport> {
    let dir = ca_dir()?;
    let secret = ca_key_secret()?;
    let _ = TlsManager::init(&dir, &secret)?;
    let ca_path = TlsManager::ca_cert_path(&dir);
    if !ca_path.exists() || !ca_path.is_file() {
        return Err(anyhow!("CA 证书不存在: {}", ca_path.display()));
    }
    install_from_path(&ca_path)
}

/// 供 Tauri 侧传入 uid 的安装入口
pub fn install_with_uid(uid: &str) -> Result<InstallReport> {
    let dir = ca_dir()?;
    let salt = load_or_create_ca_salt(&dir)?;
    let secret = crate::tool::derive_ca_key_secret(uid, &salt).context("派生 CA 密钥失败")?;
    let _ = TlsManager::init(&dir, &secret)?;
    let ca_path = TlsManager::ca_cert_path(&dir);
    if !ca_path.exists() || !ca_path.is_file() {
        return Err(anyhow!("CA 证书不存在: {}", ca_path.display()));
    }
    install_inner(&ca_path)
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn install_inner(ca_path: &Path) -> Result<InstallReport> {
    let ca_path_owned = ca_path.to_path_buf();
    #[cfg(target_os = "linux")]
    {
        ensure_certutil()?;
        let (dest_path, update_cmd) = detect_linux_dest()?;

        // 第一步: pkexec cp + chmod + update，参数化避免 shell 注入
        // 使用 pkexec + sh -c 时对路径做单引号转义
        let ca_q = shell_quote(&ca_path_owned.to_string_lossy());
        let dest_q = shell_quote(&dest_path);
        let root_script = format!("cp {ca_q} {dest_q} && chmod 644 {dest_q} && {update_cmd}");
        let root_output = Command::new("pkexec")
            .args(["sh", "-c", &root_script])
            .output()
            .context("执行 pkexec 失败")?;
        if !root_output.status.success() {
            return Err(anyhow!(
                "安装系统 CA 失败(用户取消或出错): {}",
                String::from_utf8_lossy(&root_output.stderr)
            ));
        }

        // 第二步: 注入 NSS，使用环境变量传递路径并单引号转义
        let cert_name = "com.zz.freeproxy";
        let ca_path_str = ca_path_owned.to_string_lossy().into_owned();
        let ca_q2 = shell_quote(&ca_path_str);
        let cert_q = shell_quote(cert_name);
        let user_script = format!(
            r#"
CERT_NAME={cert_q}
CA_PATH={ca_q2}
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
"#
        );
        let user_output = Command::new("sh")
            .args(["-c", &user_script])
            .output()
            .context("注入 NSS 数据库失败")?;
        let mut warnings = Vec::new();
        if !user_output.status.success() {
            warnings.push(format!(
                "部分 NSS 数据库注入失败: {}",
                String::from_utf8_lossy(&user_output.stderr)
            ));
        }
        return Ok(InstallReport {
            ca_path: ca_path_owned,
            dest_path: Some(dest_path),
            warnings,
        });
    }
    #[cfg(target_os = "windows")]
    {
        let ca_path_str = ca_path_owned.to_string_lossy().into_owned();
        let output = Command::new("certutil")
            .args(["-addstore", "Root", &ca_path_str])
            .output()
            .context("执行 certutil 失败")?;
        if !output.status.success() {
            return Err(anyhow!(
                "Windows CA 安装失败(用户拒绝或出错)。\nStdout: {}\nStderr: {}",
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        return Ok(InstallReport {
            ca_path: ca_path_owned,
            dest_path: None,
            warnings: Vec::new(),
        });
    }
    #[cfg(target_os = "macos")]
    {
        let ca_path_str = ca_path_owned.to_string_lossy().into_owned();
        let script = format!(
            "do shell script \"security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain {}\" with administrator privileges",
            shell_quote(&ca_path_str)
        );
        let output = Command::new("osascript")
            .args(["-e", &script])
            .output()
            .context("执行 osascript 失败")?;
        if !output.status.success() {
            return Err(anyhow!(
                "macOS CA 安装失败(用户拒绝或出错): {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        return Ok(InstallReport {
            ca_path: ca_path_owned,
            dest_path: None,
            warnings: Vec::new(),
        });
    }
    #[allow(unreachable_code)]
    Err(anyhow!("暂不支持当前操作系统"))
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn install_inner(_ca_path: &Path) -> Result<InstallReport> {
    Err(anyhow!("暂不支持当前操作系统"))
}

/// 供外部（Tauri 指定目录）直接安装给定路径的 CA（已保证文件存在）
pub fn install_from_path(ca_path: &Path) -> Result<InstallReport> {
    install_inner(ca_path)
}
