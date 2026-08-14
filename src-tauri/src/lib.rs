mod commands;
mod constants;
mod events;
#[allow(dead_code)]
mod model;
mod monitor;
mod storage;
mod cloud;
mod notify;
mod upload;
mod util;
mod updater;

use std::sync::Arc;

use commands::{
    auto_connect_cloud, cancel_upload, channels_delivered, clear_history, connect_active_cloud,
    connect_custom_api, connect_dropbox, delete_history_items, disconnect_active_cloud,
    disconnect_custom_api, disconnect_dropbox, finish_dropbox_oauth, get_app_version,
    get_cloud_connection_status, get_history, get_history_entry, get_manual_status_warnings,
    get_monitoring_status, get_recent_logs, get_sandbox_warnings, get_secret, get_setting,
    get_sms_balance, get_upload_control_state, get_upload_queue, lookup_share_link,
    migrate_legacy_settings, pause_upload, resend_history_notifications, reset_setup,
    resume_upload, retry_upload, save_history_contact, save_secret, save_setting,
    set_manual_status, start_dropbox_oauth, start_monitoring, stop_monitoring, sync_sms_journal,
    test_link_shortener, verify_dropbox_status, ConfigState,
};
use cloud::CloudState;
use monitor::MonitorState;
use storage::history::HistoryState;
use storage::logging::{init_logging, log_info, log_warn, set_log_emitter};
use tauri::{Emitter, Listener, Manager};
use updater::{
    cancel_update_install, check_for_updates, get_updater_install_hint, get_updater_status,
    install_specific_version, install_update, list_available_versions,
};
use upload::UploadState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config_state = ConfigState::new().unwrap_or_else(|e| {
        panic!("failed to initialize config store: {e}");
    });
    let log_dir = config_state
        .get("log_file_path", None)
        .ok()
        .filter(|s| !s.trim().is_empty());

    let upload_state = UploadState::new();
    let cloud_state = CloudState::new();
    let monitor_state = MonitorState::new(
        Arc::clone(&upload_state.registry),
        upload_state.jobs.clone(),
    );
    let history_state = HistoryState::new().unwrap_or_else(|e| {
        panic!("failed to initialize history store: {e}");
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(config_state.clone())
        .manage(cloud_state.clone())
        .manage(monitor_state)
        .manage(upload_state)
        .manage(history_state)
        .invoke_handler(tauri::generate_handler![
            get_app_version,
            get_setting,
            save_setting,
            get_secret,
            save_secret,
            migrate_legacy_settings,
            reset_setup,
            get_recent_logs,
            get_monitoring_status,
            start_monitoring,
            stop_monitoring,
            pause_upload,
            resume_upload,
            cancel_upload,
            get_upload_queue,
            get_upload_control_state,
            get_history,
            get_history_entry,
            delete_history_items,
            clear_history,
            retry_upload,
            get_sandbox_warnings,
            lookup_share_link,
            resend_history_notifications,
            save_history_contact,
            get_manual_status_warnings,
            set_manual_status,
            channels_delivered,
            sync_sms_journal,
            get_cloud_connection_status,
            verify_dropbox_status,
            start_dropbox_oauth,
            finish_dropbox_oauth,
            connect_dropbox,
            disconnect_dropbox,
            connect_custom_api,
            disconnect_custom_api,
            connect_active_cloud,
            disconnect_active_cloud,
            auto_connect_cloud,
            get_sms_balance,
            test_link_shortener,
            get_updater_status,
            get_updater_install_hint,
            check_for_updates,
            install_update,
            cancel_update_install,
            list_available_versions,
            install_specific_version,
        ])
        .setup(move |app| {
            match init_logging(log_dir.as_deref()) {
                Ok(path) => {
                    log_info(&format!("Log-Datei: {}", path.display()));
                }
                Err(e) => {
                    eprintln!("failed to init logging: {e}");
                }
            }
            set_log_emitter({
                let handle = app.handle().clone();
                move |entry| {
                    let _ = handle.emit(events::LOG_MESSAGE, entry);
                }
            });
            match app.state::<HistoryState>().import_legacy_json_if_needed() {
                Ok(0) => {}
                Ok(n) => log_info(&format!(
                    "Legacy-Historie importiert: {n} Einträge aus upload_history.json"
                )),
                Err(e) => log_warn(&format!("Legacy-Historie konnte nicht importiert werden: {e}")),
            }
            match config_state.with_store_mut(|store| {
                storage::legacy_migrate::migrate_from_legacy(store, false)
                    .map_err(|e| e.to_string())
            }) {
                Ok(report) if report.skipped => {}
                Ok(report) => log_info(&report.message),
                Err(e) => log_warn(&format!("Legacy-QSettings/Keyring-Migration fehlgeschlagen: {e}")),
            }
            events::set_event_emitter({
                let handle = app.handle().clone();
                let history = app.state::<HistoryState>().inner().clone();
                move |name, payload| {
                    if name == events::UPLOAD_HISTORY_UPDATE {
                        if let Err(e) = history.add_or_update_from_value(&payload) {
                            log_warn(&format!("History-Update fehlgeschlagen: {e}"));
                        }
                    }
                    let _ = handle.emit(name, payload);
                }
            });
            let upload = app.state::<UploadState>();
            let cloud = app.state::<CloudState>().inner().clone();
            let config_for_upload = config_state.clone();
            let config_for_runtime = config_state.clone();
            storage::config::install_runtime_getter(std::sync::Arc::new(move |key: &str| {
                config_for_runtime.get(key, None).unwrap_or_default()
            }));
            upload.spawn_worker(&cloud, move |key| {
                config_for_upload.get(key, None).unwrap_or_default()
            });
            let handle = app.handle().clone();
            app.listen(events::STOP_MONITORING, move |_| {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    commands::monitor::stop_from_event(&handle).await;
                });
            });
            {
                let history = app.state::<HistoryState>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(90));
                    ticker.tick().await;
                    loop {
                        ticker.tick().await;
                        if !crate::notify::sms_sync::history_needs_sms_journal_check(&history) {
                            continue;
                        }
                        match crate::notify::sms_sync::sync_history_with_journal(&history).await {
                            Ok(0) => {}
                            Ok(n) => storage::logging::log_info(&format!(
                                "SMS-Journal: {n} Einträge aktualisiert"
                            )),
                            Err(e) => storage::logging::log_warn(&format!(
                                "SMS-Journal-Abgleich fehlgeschlagen: {e}"
                            )),
                        }
                    }
                });
            }
            #[cfg(desktop)]
            {
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
                app.handle().plugin(tauri_plugin_process::init())?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
