pub mod commands;

#[cfg(desktop)]
mod tray;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let log_cfg = if cfg!(debug_assertions) {
        lib::log::LogConfig {
            tag: "[PROXY]".into(),
            ..Default::default()
        }
    } else {
        match lib::client::config::app_data_dir() {
            Ok(dir) => lib::log::LogConfig {
                tag: "[PROXY]".into(),
                log_dir: Some(dir.join("logs")),
                with_ansi: false,
                ..Default::default()
            },
            Err(_) => lib::log::LogConfig {
                tag: "[PROXY]".into(),
                with_ansi: false,
                ..Default::default()
            },
        }
    };
    let _ = lib::log::init(log_cfg);
    #[allow(unused)]
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_os::init());
    #[cfg(desktop)]
    {
        use tauri::Manager;
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }));
    }

    let app = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_machine_uid::init())
        .setup(|_app| {
            #[cfg(desktop)]
            tray::init(_app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings::load_settings,
            commands::settings::save_settings,
            commands::proxy::proxy_start,
            commands::proxy::proxy_stop,
            commands::proxy::proxy_status,
            commands::proxy::proxy_set_aead,
            commands::proxy::proxy_set_compressor,
            commands::proxy::proxy_set_ip,
            commands::proxy::open_ca_dir,
            commands::proxy::ca_info,
            commands::proxy::install_ca,
            commands::proxy::proxy_check_availability,
            commands::speed::speed_test_start,
            commands::speed::speed_test_cancel,
            commands::speed::speed_test_state,
            commands::speed::worker_health,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            commands::speed::shutdown();
            commands::proxy::shutdown();
        }
    });
}
