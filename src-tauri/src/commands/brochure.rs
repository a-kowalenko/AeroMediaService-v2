//! Phase 17 — Infobroschüre PDF settings / import commands.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::events;
use crate::storage::logging;
use crate::upload::brochure::{
    brochure_source_info, brochure_source_path, import_brochure_pdf, remove_brochure_pdf,
    BrochureSourceInfo,
};
use crate::util::host_opener;

use super::settings::SettingsChangedPayload;

#[derive(Debug, Serialize)]
pub struct BrochureStatus {
    pub source: BrochureSourceInfo,
}

#[tauri::command]
pub fn get_brochure_status() -> Result<BrochureStatus, String> {
    Ok(BrochureStatus {
        source: brochure_source_info()?,
    })
}

#[tauri::command]
pub fn import_brochure(app: AppHandle, path: String) -> Result<BrochureStatus, String> {
    let info = import_brochure_pdf(std::path::Path::new(path.trim()))?;
    logging::log_info("Broschüre gesetzt");
    let _ = app.emit(
        events::SETTINGS_CHANGED,
        SettingsChangedPayload {
            key: "brochure_source".into(),
        },
    );
    Ok(BrochureStatus { source: info })
}

#[tauri::command]
pub fn remove_brochure(app: AppHandle) -> Result<BrochureStatus, String> {
    remove_brochure_pdf()?;
    let _ = app.emit(
        events::SETTINGS_CHANGED,
        SettingsChangedPayload {
            key: "brochure_source".into(),
        },
    );
    Ok(BrochureStatus {
        source: brochure_source_info()?,
    })
}

#[tauri::command]
pub fn open_brochure() -> Result<(), String> {
    let path = brochure_source_path()?;
    if !path.is_file() {
        return Err("Keine Infobroschüre hinterlegt.".into());
    }
    host_opener::open_path(&path)
}
