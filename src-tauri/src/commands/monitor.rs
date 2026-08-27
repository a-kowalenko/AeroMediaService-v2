//! Tauri IPC for folder monitoring start/stop.

use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::ConfigState;
use crate::events;
use crate::monitor::stability::StabilityPendingItem;
use crate::monitor::MonitorState;
use crate::storage::logging;

const MONITORING_ENABLED_KEY: &str = "monitoring_enabled";

fn persist_monitoring_enabled(config: &ConfigState, enabled: bool) -> Result<(), String> {
    let value = if enabled { "true" } else { "false" };
    config.with_store_mut(|store| {
        store
            .save(MONITORING_ENABLED_KEY, value)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn get_monitoring_status(monitor: State<'_, MonitorState>) -> bool {
    monitor.is_running()
}

#[tauri::command]
pub fn get_stability_pending(monitor: State<'_, MonitorState>) -> Vec<StabilityPendingItem> {
    monitor.stability_snapshot()
}

#[tauri::command]
pub fn start_monitoring(
    app: AppHandle,
    config: State<'_, ConfigState>,
    monitor: State<'_, MonitorState>,
) -> Result<bool, String> {
    let config_for_monitor = config.inner().clone();
    let _started =
        monitor.start(move |key| config_for_monitor.get(key, None).unwrap_or_default())?;
    // Intentional start — restore this preference after app restart.
    persist_monitoring_enabled(&config, true)?;
    let running = monitor.is_running();
    let _ = app.emit(events::MONITORING_STATUS_CHANGED, running);
    Ok(running)
}

#[tauri::command]
pub async fn stop_monitoring(
    app: AppHandle,
    config: State<'_, ConfigState>,
    monitor: State<'_, MonitorState>,
) -> Result<(), String> {
    monitor.stop().await;
    // Intentional stop — do not auto-start on next launch.
    persist_monitoring_enabled(&config, false)?;
    let _ = app.emit(events::STOP_MONITORING, ());
    let _ = app.emit(events::MONITORING_STATUS_CHANGED, false);
    Ok(())
}

/// Transient stop (e.g. cloud disconnect). Does not clear `monitoring_enabled`.
pub async fn stop_from_event(app: &AppHandle) {
    let monitor = app.state::<MonitorState>();
    if monitor.is_running() {
        logging::log_info("stop-monitoring Event empfangen — Monitor wird beendet.");
        monitor.stop().await;
        let _ = app.emit(events::MONITORING_STATUS_CHANGED, false);
    }
}
