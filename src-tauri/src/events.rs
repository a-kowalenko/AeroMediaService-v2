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
/// Snapshot of completed file count + currently active parallel upload slots.
pub const UPLOAD_PROGRESS_SLOTS: &str = "upload-progress-slots";
pub const UPLOAD_STATUS_UPDATE: &str = "upload-status-update";
/// Structured upload activity (phase / path / counters) for the status UI.
pub const UPLOAD_ACTIVITY: &str = "upload-activity";
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

/// One in-flight file among parallel upload workers (UI secondary row).
#[derive(Debug, Clone, Serialize)]
pub struct UploadActiveSlot {
    /// 1-based file index within the job.
    pub file_index: u32,
    pub name: String,
    pub percent: i32,
    pub current: u64,
    pub total: u64,
}

/// Primary counters + active parallel slots for the upload panel.
#[derive(Debug, Clone, Serialize)]
pub struct UploadSlotsProgress {
    pub files_done: u32,
    pub files_total: u32,
    pub slots: Vec<UploadActiveSlot>,
}

#[derive(Debug, Default)]
struct SlotsState {
    files_done: u32,
    files_total: u32,
    /// Active slots keyed by 0-based file index (insertion order preserved via Vec).
    slots: Vec<(usize, UploadActiveSlot)>,
}

static SLOTS: Lazy<Mutex<SlotsState>> = Lazy::new(|| Mutex::new(SlotsState::default()));

fn slot_percent(current: u64, total: u64) -> i32 {
    if total == 0 {
        0
    } else {
        ((current as f64 / total as f64) * 100.0) as i32
    }
}

fn emit_slots_locked(state: &SlotsState) {
    emit(
        UPLOAD_PROGRESS_SLOTS,
        UploadSlotsProgress {
            files_done: state.files_done,
            files_total: state.files_total,
            slots: state.slots.iter().map(|(_, s)| s.clone()).collect(),
        },
    );
}

/// Reset slot tracker for a new job (`files_done` for resume).
pub fn upload_slots_begin(files_total: u32, files_done: u32) {
    let Ok(mut guard) = SLOTS.lock() else {
        return;
    };
    guard.files_total = files_total;
    guard.files_done = files_done.min(files_total);
    guard.slots.clear();
    emit_slots_locked(&guard);
}

/// Mark how many files are already complete (resume without clearing active slots).
pub fn upload_slots_set_done(files_done: u32) {
    let Ok(mut guard) = SLOTS.lock() else {
        return;
    };
    guard.files_done = files_done.min(guard.files_total);
    emit_slots_locked(&guard);
}

/// Start or refresh an active slot (`file_index` is 0-based).
pub fn upload_slots_start(file_index: usize, name: impl Into<String>, size: u64) {
    let Ok(mut guard) = SLOTS.lock() else {
        return;
    };
    let name = name.into();
    let display = UploadActiveSlot {
        file_index: (file_index as u32).saturating_add(1),
        name,
        percent: 0,
        current: 0,
        total: size.max(1),
    };
    if let Some(pos) = guard.slots.iter().position(|(i, _)| *i == file_index) {
        guard.slots[pos].1 = display;
    } else {
        guard.slots.push((file_index, display));
    }
    emit_slots_locked(&guard);
}

/// Update bytes for an active slot (`file_index` is 0-based).
pub fn upload_slots_progress(file_index: usize, current: u64, total: u64) {
    let Ok(mut guard) = SLOTS.lock() else {
        return;
    };
    let Some(pos) = guard.slots.iter().position(|(i, _)| *i == file_index) else {
        return;
    };
    let slot = &mut guard.slots[pos].1;
    let total = total.max(1);
    slot.current = current.min(total);
    slot.total = total;
    slot.percent = slot_percent(slot.current, slot.total);
    emit_slots_locked(&guard);
}

/// Remove a slot and count the file as completed (`file_index` is 0-based).
pub fn upload_slots_finish(file_index: usize) {
    let Ok(mut guard) = SLOTS.lock() else {
        return;
    };
    let before = guard.slots.len();
    guard.slots.retain(|(i, _)| *i != file_index);
    if guard.slots.len() < before {
        guard.files_done = guard.files_done.saturating_add(1).min(guard.files_total);
    }
    emit_slots_locked(&guard);
}

/// Clear all slot state (job end / idle).
pub fn upload_slots_clear() {
    let Ok(mut guard) = SLOTS.lock() else {
        return;
    };
    *guard = SlotsState::default();
    emit_slots_locked(&guard);
}

/// Upload UI phase — path-free labels; identifiers live in optional fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadActivityPhase {
    Idle,
    Starting,
    Uploading,
    Finalizing,
    Registering,
    Linking,
    Paused,
    Pausing,
    Appending,
    Success,
    Failed,
    Cancelled,
}

/// Structured activity payload for the upload panel (prefer over free-form status).
#[derive(Debug, Clone, Serialize, Default)]
pub struct UploadActivity {
    pub phase: UploadActivityPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rel_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_count: Option<u32>,
    /// Short path-free phrase or error summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Default for UploadActivityPhase {
    fn default() -> Self {
        Self::Idle
    }
}

impl UploadActivity {
    pub fn phase(phase: UploadActivityPhase) -> Self {
        Self {
            phase,
            ..Default::default()
        }
    }

    pub fn with_dir_name(mut self, dir_name: impl Into<String>) -> Self {
        self.dir_name = Some(dir_name.into());
        self
    }

    pub fn with_rel_path(mut self, rel_path: impl Into<String>) -> Self {
        self.rel_path = Some(rel_path.into());
        self
    }

    pub fn with_file(mut self, index: u32, count: u32) -> Self {
        self.file_index = Some(index);
        self.file_count = Some(count);
        self
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
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

pub fn emit_activity(activity: UploadActivity) {
    emit(UPLOAD_ACTIVITY, activity);
}

pub fn emit_job_active(active: bool) {
    emit(UPLOAD_JOB_ACTIVE, active);
    if !active {
        upload_slots_clear();
    }
}

pub fn emit_started(file_count: i32) {
    emit_started_at(file_count, 0);
}

/// Announce job start and seed the slot tracker (`files_done` for resume).
pub fn emit_started_at(file_count: i32, files_done: u32) {
    emit(UPLOAD_STARTED, file_count);
    let total = file_count.max(0) as u32;
    upload_slots_begin(total, files_done.min(total));
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
        assert_eq!(UPLOAD_PROGRESS_SLOTS, "upload-progress-slots");
        assert_eq!(UPLOAD_STATUS_UPDATE, "upload-status-update");
        assert_eq!(UPLOAD_ACTIVITY, "upload-activity");
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

    #[test]
    fn upload_activity_serializes_phase_and_skips_none() {
        let json = serde_json::to_value(&UploadActivity::phase(UploadActivityPhase::Uploading))
            .unwrap();
        assert_eq!(json["phase"], "uploading");
        assert!(json.get("dir_name").is_none());
        assert!(json.get("rel_path").is_none());
        assert!(json.get("file_index").is_none());
        assert!(json.get("file_count").is_none());
        assert!(json.get("message").is_none());
    }

    #[test]
    fn upload_activity_serializes_optional_fields() {
        let json = serde_json::to_value(
            UploadActivity::phase(UploadActivityPhase::Uploading)
                .with_dir_name("20260825_Job")
                .with_rel_path("Outside_Video/clip.mp4")
                .with_file(1, 2)
                .with_message("Lädt hoch"),
        )
        .unwrap();
        assert_eq!(json["phase"], "uploading");
        assert_eq!(json["dir_name"], "20260825_Job");
        assert_eq!(json["rel_path"], "Outside_Video/clip.mp4");
        assert_eq!(json["file_index"], 1);
        assert_eq!(json["file_count"], 2);
        assert_eq!(json["message"], "Lädt hoch");
    }

    #[test]
    fn upload_slots_progress_tracks_active_and_done() {
        upload_slots_clear();
        upload_slots_begin(3, 1);
        upload_slots_start(1, "a.jpg", 100);
        upload_slots_start(2, "b.jpg", 200);
        upload_slots_progress(1, 50, 100);
        {
            let guard = SLOTS.lock().unwrap();
            assert_eq!(guard.files_done, 1);
            assert_eq!(guard.files_total, 3);
            assert_eq!(guard.slots.len(), 2);
            assert_eq!(guard.slots[0].1.percent, 50);
            assert_eq!(guard.slots[0].1.file_index, 2);
        }
        upload_slots_finish(1);
        {
            let guard = SLOTS.lock().unwrap();
            assert_eq!(guard.files_done, 2);
            assert_eq!(guard.slots.len(), 1);
            assert_eq!(guard.slots[0].1.name, "b.jpg");
        }
        upload_slots_clear();
        let guard = SLOTS.lock().unwrap();
        assert_eq!(guard.files_done, 0);
        assert!(guard.slots.is_empty());
    }
}
