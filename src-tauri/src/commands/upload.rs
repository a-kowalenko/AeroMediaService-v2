//! Tauri IPC for upload pause / resume / cancel and queue snapshot.

use tauri::State;

use crate::upload::registry::QueueSnapshotItem;
use crate::upload::UploadState;

#[tauri::command]
pub fn pause_upload(upload: State<'_, UploadState>) {
    upload.control.request_pause();
}

#[tauri::command]
pub fn resume_upload(upload: State<'_, UploadState>) {
    upload.control.request_resume();
}

#[tauri::command]
pub fn cancel_upload(upload: State<'_, UploadState>) {
    upload.control.request_cancel();
}

#[tauri::command]
pub fn get_upload_queue(upload: State<'_, UploadState>) -> Vec<QueueSnapshotItem> {
    upload.registry.snapshot_dicts()
}

#[tauri::command]
pub fn get_upload_control_state(upload: State<'_, UploadState>) -> UploadControlState {
    UploadControlState {
        paused: upload.control.is_paused(),
        cancelled: upload.control.is_cancelled(),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UploadControlState {
    pub paused: bool,
    pub cancelled: bool,
}
