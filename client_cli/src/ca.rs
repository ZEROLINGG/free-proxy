// CA 证书管理:与 GUI 客户端共用同一 CA 目录(app_data_dir/ca)与
// 设备 UID 派生密钥,两端导出的证书完全一致,导入一次两端通用。
use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;

use lib::proxy::TlsManager;

use crate::config::app_data_dir;

pub fn ca_dir() -> Result<PathBuf> {
    Ok(app_data_dir()?.join("ca"))
}

/// ca_dir/ca.salt:首次运行随机生成并持久化(非机密,仅保证派生密钥的
/// 安装独立性);已存在且长度正确则复用,损坏则重新生成。
/// 与 client_tauri commands/proxy.rs load_or_create_ca_salt 保持一致。
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
    std::fs::create_dir_all(dir)
        .with_context(|| format!("创建 CA 目录失败: {}", dir.display()))?;
    std::fs::write(&salt_path, salt)
        .with_context(|| format!("写入 ca.salt 失败: {}", salt_path.display()))?;
    Ok(salt)
}

/// CA 私钥保护密钥(32B):由设备 uid(machine-uid,与 GUI 插件同一底层 crate)
/// + ca.salt 派生。uid 无法获取时直接报错。
pub fn ca_key_secret() -> Result<[u8; 32]> {
    key_secret_for(&ca_dir()?)
}

/// 指定目录版本(测试可注入临时目录)
pub(crate) fn key_secret_for(dir: &Path) -> Result<[u8; 32]> {
    let uid = machine_uid::get().map_err(|e| anyhow!("获取设备 UID 失败: {e}"))?;
    let salt = load_or_create_ca_salt(dir)?;
    lib::tool::derive_ca_key_secret(&uid, &salt).context("派生 CA 密钥失败")
}

pub fn show_info() -> Result<()> {
    let dir = ca_dir()?;
    let secret = ca_key_secret()?;
    let mgr = TlsManager::init(&dir, &secret)?;
    let path = TlsManager::ca_cert_path(&dir);
    println!("CA 目录:   {}", dir.display());
    println!("证书路径: {}", path.display());
    if mgr.rebuilt() {
        println!("⚠ 本次初始化检测到设备 UID 变化或密钥文件损坏,已自动重建 CA,");
        println!("  需要重新将证书导入系统信任区。");
    }
    println!("\n证书内容(PEM):");
    println!("{}", mgr.ca_cert_pem());
    Ok(())
}

pub fn show_dir() -> Result<()> {
    let dir = ca_dir()?;
    let secret = ca_key_secret()?;
    let _ = TlsManager::init(&dir, &secret)?;
    println!("{}", dir.display());
    Ok(())
}

/// 将 CA 安装为系统/浏览器信任的根证书(按平台调用系统工具)。
/// 移植自 client_tauri commands/proxy.rs install_ca。
pub fn install() -> Result<()> {
    let dir = ca_dir()?;
    let secret = ca_key_secret()?;
    let _ = TlsManager::init(&dir, &secret)?;
    let ca_path = TlsManager::ca_cert_path(&dir);
    if !ca_path.exists() || !ca_path.is_file() {
        return Err(anyhow!("CA 证书不存在: {}", ca_path.display()));
    }

    #[cfg(target_os = "linux")]
    {
        let cert_name = "com.zz.freeproxy";
        let cert_file_name = format!("{cert_name}.crt");
        let ca_path_str = ca_path.to_string_lossy().into_owned();

        let has_certutil = Command::new("which")
            .arg("certutil")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !has_certutil {
            return Err(anyhow!(
                "缺少 certutil 工具,无法为浏览器安装根证书。\n请先手动安装:\n\
                 - Debian/Ubuntu: sudo apt install libnss3-tools\n\
                 - Fedora/RHEL:   sudo dnf install nss-tools\n\
                 - Arch Linux:    sudo pacman -S nss\n\
                 - openSUSE:      sudo zypper install mozilla-nss-tools"
            ));
        }

        let (dest_dir, update_cmd) = if PathBuf::from("/usr/local/share/ca-certificates").exists() {
            ("/usr/local/share/ca-certificates", "update-ca-certificates")
        } else if PathBuf::from("/etc/pki/ca-trust/source/anchors").exists() {
            ("/etc/pki/ca-trust/source/anchors", "update-ca-trust extract")
        } else if PathBuf::from("/etc/ca-certificates/trust-source/anchors").exists() {
            ("/etc/ca-certificates/trust-source/anchors", "trust extract-compat")
        } else if PathBuf::from("/etc/pki/trust/anchors").exists() {
            ("/etc/pki/trust/anchors", "update-ca-certificates")
        } else {
            return Err(anyhow!("不支持当前 Linux 发行版,请手动安装 CA"));
        };

        let dest_path = format!("{dest_dir}/{cert_file_name}");

        // 第一步:Root 权限 - 更新系统级 CA
        let root_script = format!(
            "cp \"{}\" \"{dest_path}\" && chmod 644 \"{dest_path}\" && {update_cmd}",
            ca_path.to_string_lossy()
        );
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

        // 第二步:普通用户权限 - 注入浏览器 NSS 数据库
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
        let user_output = Command::new("sh")
            .args(["-c", &user_script])
            .output()
            .context("注入 NSS 数据库失败")?;
        if !user_output.status.success() {
            println!(
                "警告:部分 NSS 数据库注入失败: {}",
                String::from_utf8_lossy(&user_output.stderr)
            );
        }

        println!("CA 证书已安装(系统信任区 + 浏览器 NSS 数据库)。");
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let ca_path_str = ca_path.to_string_lossy().into_owned();
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
        println!("CA 证书已安装到 Windows 根证书库。");
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let ca_path_str = ca_path.to_string_lossy().into_owned();
        let script = format!(
            "do shell script \"security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain '{}'\" with administrator privileges",
            ca_path_str
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
        println!("CA 证书已安装到系统钥匙串。");
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err(anyhow!("暂不支持当前操作系统"))
}