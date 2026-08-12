use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

use super::Result;

pub const STORE_FILE: &str = "settings.json";
const SETTINGS_KEY: &str = "settings";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettings {
    #[serde(default)]
    pub domain: String,
    #[serde(default = "default_use_https")]
    pub use_https: bool,
    #[serde(default)]
    pub auth_key: String,
    #[serde(default = "default_local_port")]
    pub local_port: u16,
    #[serde(default = "default_compressor")]
    pub compressor: String,
    #[serde(default = "default_aead")]
    pub aead: String,
    #[serde(default)]
    pub pref_ip: Option<String>,
}

// 以下默认值必须与前端 client_tauri/src/lib/types.ts 的 DEFAULT_SETTINGS 保持一致
// （跨语言契约，改动两端需同步）
fn default_use_https() -> bool {
    false
}

fn default_local_port() -> u16 {
    8080
}

fn default_compressor() -> String {
    "zstd".into()
}

fn default_aead() -> String {
    "aes128gcm".into()
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            domain: String::new(),
            use_https: default_use_https(),
            auth_key: String::new(),
            local_port: default_local_port(),
            compressor: default_compressor(),
            aead: default_aead(),
            pref_ip: None,
        }
    }
}

#[tauri::command]
pub fn load_settings<R: Runtime>(app: AppHandle<R>) -> Result<ProxySettings> {
    let store = app.store(STORE_FILE).map_err(super::err_str)?;
    match store
        .get(SETTINGS_KEY)
        .and_then(|v| serde_json::from_value(v).ok())
    {
        Some(settings) => Ok(settings),
        None => Err("no such settings".into()),
    }
}

#[tauri::command]
pub fn save_settings<R: Runtime>(app: AppHandle<R>, s: ProxySettings) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(super::err_str)?;
    let value = serde_json::to_value(&s).map_err(super::err_str)?;
    store.set(SETTINGS_KEY, value);
    store.save().map_err(super::err_str)?;
    Ok(())
}
