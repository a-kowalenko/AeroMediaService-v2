//! Tauri IPC for folder monitoring start/stop.

use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::ConfigState;
use crate::events;
use crate::monitor::stability::StabilityPendingItem;
use crate::monitor::MonitorState;
use crate::storage::logging;

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
    let config = config.inner().clone();
    let started = monitor.start(move |key| config.get(key, None).unwrap_or_default())?;
    if started {
        let _ = app.emit(events::MONITORING_STATUS_CHANGED, true);
    }
    Ok(started)
}

#[tauri::command]
pub async fn stop_monitoring(
    app: AppHandle,
    monitor: State<'_, MonitorState>,
) -> Result<(), String> {
    monitor.stop().await;
    let _ = app.emit(events::STOP_MONITORING, ());
    let _ = app.emit(events::MONITORING_STATUS_CHANGED, false);
    Ok(())
}

pub async fn stop_from_event(app: &AppHandle) {
    let monitor = app.state::<MonitorState>();
    if monitor.is_running() {
        logging::log_info("stop-monitoring Event empfangen — Monitor wird beendet.");
        monitor.stop().await;
        let _ = app.emit(events::MONITORING_STATUS_CHANGED, false);
    }
}
