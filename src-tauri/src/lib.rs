mod bridge;
mod cloud;
mod commands;
mod constants;
mod events;
#[allow(dead_code)]
mod model;
mod monitor;
mod notify;
mod storage;
mod updater;
mod upload;
mod util;

use std::sync::Arc;

use bridge::BridgeState;
use cloud::CloudState;
use commands::{
    apply_bridge_config, assign_customer_to_folder, assign_customers_batch, auto_connect_cloud,
    append_history_files, append_history_media, cancel_upload, channels_delivered, clear_history,
    connect_active_cloud, connect_custom_api, connect_dropbox, create_dropbox_account,
    delete_customer, delete_dropbox_account, delete_history_items, disconnect_active_cloud,
    disconnect_custom_api, disconnect_dropbox, expand_append_media_paths, finish_dropbox_oauth,
    get_app_version, get_assignment_history, get_ats_host_activity, get_ats_host_details,
    get_ats_hosts_summary, get_ats_jobs_by_host, get_bridge_status, get_cloud_connection_status,
    get_dropbox_account_info,
    get_history, get_history_entry, get_manual_status_warnings, get_monitoring_status,
    get_recent_logs, get_sandbox_warnings, get_secret, get_setting, get_sms_balance,
    get_stability_pending, get_upload_control_state, get_upload_queue, list_customers,
    list_dropbox_accounts, list_media_folders_cmd, lookup_share_link, migrate_legacy_settings,
    open_external_path, open_external_url,
    pause_upload, propose_customer_assignments, propose_default_dirs_cmd, remove_ats_host,
    remove_inactive_long_ats_hosts, rename_dropbox_account, resend_history_notifications,
    reset_setup, resolve_history_booking_flags, resume_upload, retry_upload, save_customer,
    save_history_contact, save_secret, save_setting, set_active_dropbox_account,
    set_customer_processed, set_dropbox_app_folder_name, set_manual_status, start_dropbox_oauth, start_monitoring,
    stop_monitoring, sync_sms_journal, test_link_shortener, update_customer, verify_dropbox_status,
    ensure_default_app_root_cmd, ensure_default_dir_cmd, ConfigState,
};
use monitor::MonitorState;
use storage::ats_presence::AtsPresenceState;
use storage::customers::CustomerState;
use storage::dropbox_accounts::{self, DropboxAccountState};
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
    let dropbox_account_state = DropboxAccountState::new().unwrap_or_else(|e| {
        panic!("failed to initialize dropbox account store: {e}");
    });
    // Import legacy secrets before creating Dropbox profiles (idempotent).
    let _ = config_state.with_store_mut(|store| {
        storage::legacy_migrate::migrate_from_legacy(store, false).map_err(|e| e.to_string())
    });
    let dropbox_migrate_report = config_state.with_store_mut(|store| {
        dropbox_account_state.with_store(|accounts| {
            dropbox_accounts::ensure_migrated(accounts, store)
        })
    });
    let cloud_state = config_state
        .with_store_mut(|store| Ok::<_, String>(CloudState::from_config(store)))
        .unwrap_or_else(|_| CloudState::new());
    let monitor_state = MonitorState::new(
        Arc::clone(&upload_state.registry),
        upload_state.jobs.clone(),
    );
    let ats_presence_state = AtsPresenceState::new().unwrap_or_else(|e| {
        panic!("failed to initialize ATS presence store: {e}");
    });
    let bridge_state = BridgeState::new(
        monitor_state.wake_fn(),
        monitor_state.cancel_fn(),
        ats_presence_state.clone(),
    );
    let history_state = HistoryState::new().unwrap_or_else(|e| {
        panic!("failed to initialize history store: {e}");
    });
    let customer_state = CustomerState::new().unwrap_or_else(|e| {
        panic!("failed to initialize customer store: {e}");
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(config_state.clone())
        .manage(cloud_state.clone())
        .manage(dropbox_account_state)
        .manage(bridge_state.clone())
        .manage(ats_presence_state)
        .manage(monitor_state)
        .manage(upload_state)
        .manage(history_state)
        .manage(customer_state)
        .invoke_handler(tauri::generate_handler![
            get_app_version,
            open_external_url,
            open_external_path,
            get_setting,
            save_setting,
            get_secret,
            save_secret,
            migrate_legacy_settings,
            reset_setup,
            propose_default_dirs_cmd,
            ensure_default_dir_cmd,
            ensure_default_app_root_cmd,
            get_recent_logs,
            get_monitoring_status,
            get_stability_pending,
            start_monitoring,
            stop_monitoring,
            get_bridge_status,
            apply_bridge_config,
            get_ats_hosts_summary,
            get_ats_host_details,
            get_ats_jobs_by_host,
            get_ats_host_activity,
            remove_ats_host,
            remove_inactive_long_ats_hosts,
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
            resolve_history_booking_flags,
            expand_append_media_paths,
            append_history_media,
            append_history_files,
            get_sandbox_warnings,
            lookup_share_link,
            resend_history_notifications,
            save_history_contact,
            get_manual_status_warnings,
            set_manual_status,
            channels_delivered,
            sync_sms_journal,
            list_customers,
            save_customer,
            update_customer,
            delete_customer,
            set_customer_processed,
            list_media_folders_cmd,
            assign_customer_to_folder,
            propose_customer_assignments,
            assign_customers_batch,
            get_assignment_history,
            get_cloud_connection_status,
            verify_dropbox_status,
            get_dropbox_account_info,
            list_dropbox_accounts,
            create_dropbox_account,
            set_active_dropbox_account,
            rename_dropbox_account,
            set_dropbox_app_folder_name,
            delete_dropbox_account,
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
                Err(e) => log_warn(&format!(
                    "Legacy-Historie konnte nicht importiert werden: {e}"
                )),
            }
            match config_state.with_store_mut(|store| {
                storage::legacy_migrate::migrate_from_legacy(store, false)
                    .map_err(|e| e.to_string())
            }) {
                Ok(report) if report.skipped => {}
                Ok(report) => log_info(&report.message),
                Err(e) => log_warn(&format!(
                    "Legacy-QSettings/Keyring-Migration fehlgeschlagen: {e}"
                )),
            }
            match &dropbox_migrate_report {
                Ok(report) if report.skipped => {}
                Ok(report) => log_info(&report.message),
                Err(e) => log_warn(&format!(
                    "Dropbox-Multi-Account-Migration fehlgeschlagen: {e}"
                )),
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
            {
                let bridge = app.state::<BridgeState>().inner().clone();
                let config_for_bridge = config_state.clone();
                tauri::async_runtime::spawn(async move {
                    match bridge.apply_from_config(&config_for_bridge).await {
                        Ok(status) if status.running => {
                            log_info(&format!("AMS-Bridge aktiv: {}", status.bind_addr));
                        }
                        Ok(_) => {}
                        Err(e) => log_warn(&format!("AMS-Bridge nicht gestartet: {e}")),
                    }
                });
            }
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
            // macOS: tauri.macos.conf.json creates decorations + Overlay + hiddenTitle
            // (do not toggle decorations false→true — that restores a normal title bar).
            // Win/Linux: conf starts frameless; React AppChrome draws Min/Max/Close.
            // Then clamp to the monitor work area so the bottom edge stays on-screen.
            #[cfg(desktop)]
            {
                if let Some(window) = app.get_webview_window("main") {
                    #[cfg(any(target_os = "windows", target_os = "linux"))]
                    if let Err(e) = window.set_decorations(false) {
                        eprintln!("set_decorations(false) failed: {e}");
                    }
                    crate::util::window_fit::fit_main_window(&window);
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
