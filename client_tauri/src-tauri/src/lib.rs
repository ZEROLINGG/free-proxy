pub mod commands;

#[cfg(desktop)]
mod tray;


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // tracing: 默认 WARN，RUST_LOG=debug 可开启
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .try_init();
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
