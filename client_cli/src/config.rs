// 与 GUI 客户端(tauri-plugin-store)共用同一份配置:
//   app_data_dir/settings.json  =>  { "settings": { ...ProxySettings } }
// 字段为 camelCase,默认值与 client_tauri/src-tauri/src/commands/settings.rs 保持一致。
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Tauri 应用 identifier(决定 app_data_dir 子目录名,与 GUI 共用目录的关键)
pub const IDENTIFIER: &str = "com.zz.freeproxy";
pub const STORE_FILE: &str = "settings.json";
const SETTINGS_KEY: &str = "settings";

/// app_data_dir:
///   Linux   -> ~/.local/share/com.zz.freeproxy
///   Windows -> %APPDATA%\com.zz.freeproxy
///   macOS   -> ~/Library/Application Support/com.zz.freeproxy
pub fn app_data_dir() -> Result<PathBuf> {
    let dir = dirs::data_dir().context("无法确定应用数据目录")?;
    Ok(dir.join(IDENTIFIER))
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(app_data_dir()?.join(STORE_FILE))
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ProxySettings {
    pub domain: String,
    pub use_https: bool,
    pub auth_key: String,
    pub local_port: u16,
    pub compressor: String,
    pub aead: String,
    pub pref_ip: Option<String>,
}

impl ProxySettings {
    pub fn defaults() -> Self {
        Self {
            domain: String::new(),
            use_https: false,
            auth_key: String::new(),
            local_port: 8080,
            compressor: "zstd".into(),
            aead: "aes128gcm".into(),
            pref_ip: None,
        }
    }

    /// 非空校验(与 GUI 前端提示一致)
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.domain.trim().is_empty(),
            "domain(Worker 域名)不能为空"
        );
        anyhow::ensure!(!self.auth_key.is_empty(), "认证密钥(auth_key)不能为空");
        Ok(())
    }
}

/// 加载配置:文件缺失或尚无 settings 键(首次运行)返回默认值;
/// 仅解析失败报错(与 GUI load_settings 语义一致)。
pub fn load() -> Result<ProxySettings> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(ProxySettings::defaults());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("读取配置失败: {}", path.display()))?;
    let root: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("配置文件损坏: {}", path.display()))?;
    match root.get(SETTINGS_KEY) {
        Some(v) => serde_json::from_value(v.clone())
            .with_context(|| format!("配置内容解析失败: {}", path.display())),
        None => Ok(ProxySettings::defaults()),
    }
}

pub fn save(s: &ProxySettings) -> Result<()> {
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("创建配置目录失败: {}", parent.display()))?;
    }
    let root = serde_json::json!({ SETTINGS_KEY: serde_json::to_value(s)? });
    let text = serde_json::to_string_pretty(&root)?;
    std::fs::write(&path, text).with_context(|| format!("写入配置失败: {}", path.display()))?;
    Ok(())
}

/// 将 CLI 参数覆盖到配置上(不写回文件)。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_roundtrip() {
        let s = ProxySettings::defaults();
        let json = serde_json::to_value(&s).unwrap();
        // 与 GUI ProxySettings(serde rename_all = "camelCase")字段名契约
        for key in ["domain", "useHttps", "authKey", "localPort", "compressor", "aead"] {
            assert!(json.get(key).is_some(), "缺少字段: {key}");
        }
        let back: ProxySettings = serde_json::from_value(json).unwrap();
        assert_eq!(s.local_port, back.local_port);
        assert_eq!(s.compressor, back.compressor);
        assert_eq!(s.aead, back.aead);
    }

    #[test]
    fn store_shape_matches_tauri_plugin_store() {
        // tauri-plugin-store: store.get("settings") -> ProxySettings
        let s = ProxySettings::defaults();
        let root = serde_json::json!({ "settings": serde_json::to_value(&s).unwrap() });
        let got = root.get("settings").unwrap();
        let parsed: ProxySettings = serde_json::from_value(got.clone()).unwrap();
        assert_eq!(parsed.local_port, 8080);
    }
}