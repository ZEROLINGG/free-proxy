pub mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
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
