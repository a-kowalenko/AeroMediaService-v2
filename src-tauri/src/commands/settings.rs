//! Tauri IPC for settings, secrets, version, and recent logs.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::events;
use crate::monitor::MonitorState;
use crate::storage::legacy_migrate::{self, MigrateReport};
use crate::storage::logging::{self, LogMessage};
use crate::storage::secrets;
use crate::storage::ConfigStore;

#[derive(Clone)]
pub struct ConfigState {
    store: Arc<Mutex<ConfigStore>>,
}

impl ConfigState {
    pub fn new() -> Result<Self, String> {
        let store = ConfigStore::open_default().map_err(|e| e.to_string())?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
        })
    }

    pub fn get(&self, key: &str, default: Option<&str>) -> Result<String, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        Ok(store.get(key, default))
    }

    pub fn with_store_mut<T>(
        &self,
        f: impl FnOnce(&mut ConfigStore) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut store = self.store.lock().map_err(|e| e.to_string())?;
        f(&mut store)
    }
}

#[derive(Debug, Serialize, Clone)]
struct SettingsChangedPayload {
    key: String,
}

#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub fn get_setting(
    state: State<'_, ConfigState>,
    key: String,
    default: Option<String>,
) -> Result<String, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    Ok(store.get(&key, default.as_deref()))
}

#[tauri::command]
pub fn save_setting(
    app: AppHandle,
    state: State<'_, ConfigState>,
    monitor: State<'_, MonitorState>,
    key: String,
    value: String,
) -> Result<(), String> {
    if secrets::is_secret_key(&key) {
        return Err(format!(
            "Schlüssel '{key}' ist ein Secret und muss über save_secret gespeichert werden."
        ));
    }
    {
        let mut store = state.store.lock().map_err(|e| e.to_string())?;
        store.save(&key, &value).map_err(|e| e.to_string())?;
    }

    if key == "log_file_path" {
        logging::set_log_dir(&value).map_err(|e| e.to_string())?;
    }

    if matches!(
        key.as_str(),
        "monitor_path"
            | "scan_interval"
            | "folder_stability_enabled"
            | "folder_stability_seconds"
    ) {
        monitor.wake();
    }

    logging::log_info(&format!("Einstellung gespeichert: {key}"));
    let _ = app.emit(events::SETTINGS_CHANGED, SettingsChangedPayload { key });
    Ok(())
}

#[tauri::command]
pub fn get_secret(key: String) -> Result<Option<String>, String> {
    secrets::get_secret(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_secret(app: AppHandle, key: String, value: String) -> Result<(), String> {
    secrets::save_secret(&key, &value).map_err(|e| e.to_string())?;
    if value.is_empty() {
        logging::log_info(&format!("Geheimnis für '{key}' gelöscht."));
    } else {
        logging::log_info(&format!("Geheimnis für '{key}' sicher gespeichert."));
    }
    let _ = app.emit(events::SETTINGS_CHANGED, SettingsChangedPayload { key });
    Ok(())
}

#[tauri::command]
pub fn get_recent_logs(limit: Option<usize>) -> Vec<LogMessage> {
    logging::recent_logs(limit)
}

/// One-shot (or forced) import from Legacy QSettings + Keyring.
#[tauri::command]
pub fn migrate_legacy_settings(
    app: AppHandle,
    state: State<'_, ConfigState>,
    force: Option<bool>,
) -> Result<MigrateReport, String> {
    let report = state.with_store_mut(|store| {
        legacy_migrate::migrate_from_legacy(store, force.unwrap_or(false))
            .map_err(|e| e.to_string())
    })?;
    if !report.skipped {
        logging::log_info(&report.message);
        let _ = app.emit(
            events::SETTINGS_CHANGED,
            SettingsChangedPayload {
                key: "legacy_migration_done".into(),
            },
        );
    }
    Ok(report)
}

/// Clear first-run flag (and optionally core paths) so the wizard can run again.
#[tauri::command]
pub fn reset_setup(
    app: AppHandle,
    state: State<'_, ConfigState>,
    clear_paths: Option<bool>,
) -> Result<(), String> {
    let clear_paths = clear_paths.unwrap_or(false);
    {
        let mut store = state.store.lock().map_err(|e| e.to_string())?;
        store
            .save("setup_completed", "false")
            .map_err(|e| e.to_string())?;
        if clear_paths {
            for key in ["monitor_path", "archive_path", "log_file_path"] {
                store.save(key, "").map_err(|e| e.to_string())?;
            }
        }
    }
    logging::log_info(if clear_paths {
        "Setup zurückgesetzt (inkl. Pfade)."
    } else {
        "Setup-Flag zurückgesetzt — Wizard kann erneut geöffnet werden."
    });
    let _ = app.emit(
        events::SETTINGS_CHANGED,
        SettingsChangedPayload {
            key: "setup_completed".into(),
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_version_is_semver_like() {
        let v = env!("CARGO_PKG_VERSION");
        assert!(!v.is_empty());
        assert!(v.contains('.'));
    }
}
