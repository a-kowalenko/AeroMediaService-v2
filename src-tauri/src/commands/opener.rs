//! Tauri IPC for opening URLs and paths in the host browser / file manager.

use std::path::Path;

use crate::util::host_opener;

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    host_opener::open_url(&url)
}

#[tauri::command]
pub fn open_external_path(path: String) -> Result<(), String> {
    host_opener::open_path(Path::new(path.trim()))
}
