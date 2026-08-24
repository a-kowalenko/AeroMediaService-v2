//! Tauri event names — kebab-case ports of legacy `core/signals.py` (+ `settings_changed`).
//! Unused names are reserved for later phases (upload, monitor, connection).

#![allow(dead_code)]

use std::sync::Mutex;

use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::Value;

type EventEmitter = Box<dyn Fn(&str, Value) + Send + Sync>;

static EMITTER: Lazy<Mutex<Option<EventEmitter>>> = Lazy::new(|| Mutex::new(None));

/// Log line for the UI (`Signal(int, str)` → structured payload).
pub const LOG_MESSAGE: &str = "log-message";

/// Non-secret or secret setting was persisted.
pub const SETTINGS_CHANGED: &str = "settings-changed";

pub const UPLOAD_HISTORY_UPDATE: &str = "upload-history-update";
pub const UPLOAD_PROGRESS_FILE: &str = "upload-progress-file";
pub const UPLOAD_PROGRESS_TOTAL: &str = "upload-progress-total";
pub const UPLOAD_STATUS_UPDATE: &str = "upload-status-update";
pub const UPLOAD_JOB_ACTIVE: &str = "upload-job-active";
pub const UPLOAD_QUEUE_CHANGED: &str = "upload-queue-changed";
pub const UPLOAD_STARTED: &str = "upload-started";
pub const UPLOAD_PROGRESS: &str = "upload-progress";
pub const UPLOAD_FINISHED: &str = "upload-finished";
pub const UPLOAD_FAILED: &str = "upload-failed";
/// Cooperative pause/resume flags (`paused` requested, `holding` actually blocked).
pub const UPLOAD_CONTROL_CHANGED: &str = "upload-control-changed";
pub const MONITORING_STATUS_CHANGED: &str = "monitoring-status-changed";
/// Folders waiting for unchanged content before claim (left-panel snapshot).
pub const STABILITY_PENDING_CHANGED: &str = "stability-pending-changed";
pub const CONNECTION_STATUS_CHANGED: &str = "connection-status-changed";
pub const STOP_MONITORING: &str = "stop-monitoring";
/// Updater download/install progress (Tauri updater plugin helpers).
pub const UPDATE_INSTALL_PROGRESS: &str = "update-install-progress";

/// Install the app-wide event emitter (typically wrapping `AppHandle::emit`).
pub fn set_event_emitter<F>(f: F)
where
    F: Fn(&str, Value) + Send + Sync + 'static,
{
    if let Ok(mut guard) = EMITTER.lock() {
        *guard = Some(Box::new(f));
    }
}

/// Emit a structured payload to the frontend. No-op when no emitter is installed.
pub fn emit(event: &str, payload: impl Serialize) {
    let Ok(value) = serde_json::to_value(&payload) else {
        return;
    };
    if let Ok(guard) = EMITTER.lock() {
        if let Some(emit) = guard.as_ref() {
            emit(event, value);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ByteProgress {
    pub percent: i32,
    pub current: u64,
    pub total: u64,
}

pub fn emit_progress_file(percent: i32, current: u64, total: u64) {
    emit(
        UPLOAD_PROGRESS_FILE,
        ByteProgress {
            percent,
            current,
            total,
        },
    );
}

pub fn emit_progress_total(percent: i32, current: u64, total: u64) {
    emit(
        UPLOAD_PROGRESS_TOTAL,
        ByteProgress {
            percent,
            current,
            total,
        },
    );
}

pub fn emit_status(message: impl Into<String>) {
    emit(UPLOAD_STATUS_UPDATE, message.into());
}

pub fn emit_job_active(active: bool) {
    emit(UPLOAD_JOB_ACTIVE, active);
}

pub fn emit_started(file_count: i32) {
    emit(UPLOAD_STARTED, file_count);
}

pub fn emit_progress_message(message: impl Into<String>) {
    emit(UPLOAD_PROGRESS, message.into());
}

pub fn emit_finished(message: impl Into<String>) {
    emit(UPLOAD_FINISHED, message.into());
}

pub fn emit_failed(message: impl Into<String>) {
    emit(UPLOAD_FAILED, message.into());
}

pub fn emit_connection_status(status: impl Into<String>) {
    emit(CONNECTION_STATUS_CHANGED, status.into());
}

pub fn emit_upload_control(payload: impl Serialize) {
    emit(UPLOAD_CONTROL_CHANGED, payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_match_legacy_signals() {
        assert_eq!(LOG_MESSAGE, "log-message");
        assert_eq!(SETTINGS_CHANGED, "settings-changed");
        assert_eq!(UPLOAD_HISTORY_UPDATE, "upload-history-update");
        assert_eq!(UPLOAD_PROGRESS_FILE, "upload-progress-file");
        assert_eq!(UPLOAD_PROGRESS_TOTAL, "upload-progress-total");
        assert_eq!(UPLOAD_STATUS_UPDATE, "upload-status-update");
        assert_eq!(UPLOAD_JOB_ACTIVE, "upload-job-active");
        assert_eq!(UPLOAD_QUEUE_CHANGED, "upload-queue-changed");
        assert_eq!(UPLOAD_STARTED, "upload-started");
        assert_eq!(UPLOAD_PROGRESS, "upload-progress");
        assert_eq!(UPLOAD_FINISHED, "upload-finished");
        assert_eq!(UPLOAD_FAILED, "upload-failed");
        assert_eq!(UPLOAD_CONTROL_CHANGED, "upload-control-changed");
        assert_eq!(MONITORING_STATUS_CHANGED, "monitoring-status-changed");
        assert_eq!(STABILITY_PENDING_CHANGED, "stability-pending-changed");
        assert_eq!(CONNECTION_STATUS_CHANGED, "connection-status-changed");
        assert_eq!(STOP_MONITORING, "stop-monitoring");
        assert_eq!(UPDATE_INSTALL_PROGRESS, "update-install-progress");
    }

    #[test]
    fn byte_progress_serializes_u64_fields() {
        let json = serde_json::to_value(&ByteProgress {
            percent: 50,
            current: 3_000_000_000,
            total: 6_000_000_000,
        })
        .unwrap();
        assert_eq!(json["percent"], 50);
        assert_eq!(json["current"], 3_000_000_000u64);
        assert_eq!(json["total"], 6_000_000_000u64);
    }
}
