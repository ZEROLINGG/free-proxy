use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

use super::Result;

// 复用 lib 单一真源，端口已统一 8001
pub use lib::client::config::ProxySettings;
use lib::client::config::{SETTINGS_KEY, STORE_FILE};

#[tauri::command]
pub fn load_settings<R: Runtime>(app: AppHandle<R>) -> Result<ProxySettings> {
    let store = app.store(STORE_FILE).map_err(super::err_str)?;
    match store.get(SETTINGS_KEY) {
        Some(value) => serde_json::from_value(value).map_err(super::err_str),
        None => Ok(ProxySettings::default()),
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
