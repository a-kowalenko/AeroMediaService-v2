//! Tauri IPC for retry, resend, manual status, and SMS journal sync.

use serde::Serialize;
use tauri::State;

use crate::cloud::CloudState;
use crate::commands::ConfigState;
use crate::events;
use crate::model::manual_status::{build_manual_status_update, collect_manual_status_warnings};
use crate::model::validation::is_valid_email;
use crate::notify::resend::{
    build_contact_update_payload, can_resend_notifications, channels_already_delivered,
    format_resend_result_message, get_sandbox_warnings as collect_sandbox_warnings,
    lookup_share_link_from_cloud, normalize_contact, resend_had_failures, resend_notifications,
    validate_contact_for_channels,
};
use crate::notify::sms_sync;
use crate::storage::history::{HistoryEntry, HistoryState};
use crate::model::marker::{history_has_booked_option, merge_kunde_media_flags};
use crate::model::kunde::Kunde;
use crate::upload::append::{append_media_from_files, append_media_from_history, AppendFileItem};
use crate::upload::retry::{resolve_kunde_from_history_entry, retry_upload_from_history};
use crate::upload::UploadState;

fn load_entry_json(
    history: &HistoryState,
    id: &str,
) -> Result<(HistoryEntry, serde_json::Value), String> {
    let entry = history
        .get_by_id(id)?
        .ok_or_else(|| "Historieneintrag nicht gefunden.".to_string())?;
    let json = entry.to_json();
    Ok((entry, json))
}

#[tauri::command]
pub async fn retry_upload(
    config: State<'_, ConfigState>,
    upload: State<'_, UploadState>,
    history: State<'_, HistoryState>,
    id: String,
) -> Result<String, String> {
    let (_entry, json) = load_entry_json(&history, &id)?;
    let monitor_path = config.get("monitor_path", Some(""))?;
    let archive_path = config.get("archive_path", Some(""))?;
    let selected_cloud = config.get("selected_cloud_service", Some("dropbox"))?;
    retry_upload_from_history(
        &json,
        &monitor_path,
        &archive_path,
        &selected_cloud,
        &upload.jobs,
        &upload.registry,
    )
    .await
}

#[tauri::command]
pub async fn append_history_media(
    config: State<'_, ConfigState>,
    upload: State<'_, UploadState>,
    history: State<'_, HistoryState>,
    cloud: State<'_, CloudState>,
    id: String,
    local_dir: String,
) -> Result<String, String> {
    let (_entry, json) = load_entry_json(&history, &id)?;
    let selected_cloud = config.get("selected_cloud_service", Some("dropbox"))?;
    let updates = append_media_from_history(
        &json,
        std::path::Path::new(&local_dir),
        &selected_cloud,
        &cloud,
        &upload.control,
        &upload.registry,
    )
    .await?;
    history.add_or_update_from_value(&updates)?;
    let count = updates
        .get("append_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(1);
    let remote = updates
        .get("remote_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    Ok(format!(
        "{count}× nachgeladen nach {remote}. Der bestehende Download-Link bleibt gültig."
    ))
}

#[tauri::command]
pub async fn append_history_files(
    config: State<'_, ConfigState>,
    upload: State<'_, UploadState>,
    history: State<'_, HistoryState>,
    cloud: State<'_, CloudState>,
    id: String,
    items: Vec<AppendFileItem>,
) -> Result<String, String> {
    let (_entry, json) = load_entry_json(&history, &id)?;
    let selected_cloud = config.get("selected_cloud_service", Some("dropbox"))?;
    let updates = append_media_from_files(
        &json,
        &items,
        &selected_cloud,
        &cloud,
        &upload.control,
        &upload.registry,
    )
    .await?;
    history.add_or_update_from_value(&updates)?;
    let count = updates
        .get("append_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(1);
    let remote = updates
        .get("remote_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    Ok(format!(
        "{count}× nachgeladen nach {remote} ({} Datei(en)). Der bestehende Download-Link bleibt gültig.",
        items.len()
    ))
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryBookingFlags {
    pub handcam_foto: bool,
    pub handcam_video: bool,
    pub outside_foto: bool,
    pub outside_video: bool,
    pub ist_bezahlt_handcam_foto: bool,
    pub ist_bezahlt_handcam_video: bool,
    pub ist_bezahlt_outside_foto: bool,
    pub ist_bezahlt_outside_video: bool,
}

impl From<&Kunde> for HistoryBookingFlags {
    fn from(k: &Kunde) -> Self {
        Self {
            handcam_foto: k.handcam_foto,
            handcam_video: k.handcam_video,
            outside_foto: k.outside_foto,
            outside_video: k.outside_video,
            ist_bezahlt_handcam_foto: k.ist_bezahlt_handcam_foto,
            ist_bezahlt_handcam_video: k.ist_bezahlt_handcam_video,
            ist_bezahlt_outside_foto: k.ist_bezahlt_outside_foto,
            ist_bezahlt_outside_video: k.ist_bezahlt_outside_video,
        }
    }
}

#[tauri::command]
pub async fn resolve_history_booking_flags(
    history: State<'_, HistoryState>,
    id: String,
) -> Result<HistoryBookingFlags, String> {
    let (_entry, json) = load_entry_json(&history, &id)?;
    let kunde = resolve_kunde_from_history_entry(&json).await?;
    if !history_has_booked_option(&json) {
        let dir_name = json
            .get("dir_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if !dir_name.is_empty() {
            let mut patch = serde_json::json!({ "dir_name": dir_name });
            merge_kunde_media_flags(&mut patch, &kunde);
            history.add_or_update_from_value(&patch)?;
            events::emit(events::UPLOAD_HISTORY_UPDATE, &patch);
        }
    }
    Ok(HistoryBookingFlags::from(&kunde))
}

#[tauri::command]
pub fn expand_append_media_paths(paths: Vec<String>) -> Result<Vec<String>, String> {
    crate::upload::append::expand_append_media_paths(&paths)
}

#[derive(Debug, Clone, Serialize)]
pub struct ResendCommandResult {
    pub message: String,
    pub had_failures: bool,
    pub email_status: Option<String>,
    pub sms_status: Option<String>,
}

#[tauri::command]
pub fn get_sandbox_warnings() -> Vec<String> {
    collect_sandbox_warnings()
}

#[tauri::command]
pub async fn lookup_share_link(
    config: State<'_, ConfigState>,
    history: State<'_, HistoryState>,
    id: String,
) -> Result<String, String> {
    let (_entry, json) = load_entry_json(&history, &id)?;
    let selected_cloud = config.get("selected_cloud_service", Some("dropbox"))?;
    lookup_share_link_from_cloud(&json, &selected_cloud).await
}

#[tauri::command]
pub async fn resend_history_notifications(
    history: State<'_, HistoryState>,
    id: String,
    email: String,
    phone: String,
    share_link: String,
    send_email: bool,
    send_sms: bool,
) -> Result<ResendCommandResult, String> {
    let (_entry, mut json) = load_entry_json(&history, &id)?;
    if !can_resend_notifications(&json) {
        return Err("Nur erfolgreiche Uploads unterstützen einen erneuten Versand.".into());
    }

    let (email, phone) = normalize_contact(&email, &phone);
    validate_contact_for_channels(&email, phone.as_deref(), send_email, send_sms)?;

    let mut contact = build_contact_update_payload(&json, &email, phone.as_deref());
    if let Some(obj) = contact.as_object_mut() {
        obj.insert(
            "share_link".into(),
            serde_json::Value::String(share_link.clone()),
        );
    }
    if let Some(updated) = history.add_or_update_from_value(&contact)? {
        json = updated.to_json();
    }

    let result = resend_notifications(
        &json,
        &email,
        phone.as_deref(),
        &share_link,
        send_email,
        send_sms,
    )
    .await?;
    history.add_or_update_from_value(&result.history_updates)?;
    events::emit(events::UPLOAD_HISTORY_UPDATE, &result.history_updates);

    if sms_sync::history_needs_sms_journal_check(&history) {
        let _ = sms_sync::sync_history_with_journal(&history).await;
    }

    Ok(ResendCommandResult {
        message: format_resend_result_message(&result),
        had_failures: resend_had_failures(&result),
        email_status: result.email_result.map(|r| r.status),
        sms_status: result.sms_result.map(|r| r.status),
    })
}

#[tauri::command]
pub fn save_history_contact(
    history: State<'_, HistoryState>,
    id: String,
    email: String,
    phone: String,
) -> Result<HistoryEntry, String> {
    let (_entry, json) = load_entry_json(&history, &id)?;
    let (email, phone) = normalize_contact(&email, &phone);
    if !email.is_empty() && !is_valid_email(&email) {
        return Err("E-Mail-Adresse ist ungültig.".into());
    }
    let payload = build_contact_update_payload(&json, &email, phone.as_deref());
    history
        .add_or_update_from_value(&payload)?
        .ok_or_else(|| "Kontakt konnte nicht gespeichert werden.".to_string())
}

#[tauri::command]
pub fn get_manual_status_warnings(
    history: State<'_, HistoryState>,
    id: String,
    action: String,
) -> Result<Vec<String>, String> {
    let (_entry, json) = load_entry_json(&history, &id)?;
    Ok(collect_manual_status_warnings(&json, &action))
}

#[tauri::command]
pub fn set_manual_status(
    history: State<'_, HistoryState>,
    id: String,
    action: String,
    reason: Option<String>,
) -> Result<HistoryEntry, String> {
    let (_entry, json) = load_entry_json(&history, &id)?;
    let payload = build_manual_status_update(&json, &action, reason.as_deref().unwrap_or(""))?;
    let updated = history
        .add_or_update_from_value(&payload)?
        .ok_or_else(|| "Status konnte nicht gespeichert werden.".to_string())?;
    events::emit(events::UPLOAD_HISTORY_UPDATE, &payload);
    Ok(updated)
}

#[tauri::command]
pub fn channels_delivered(
    history: State<'_, HistoryState>,
    id: String,
    send_email: bool,
    send_sms: bool,
) -> Result<Vec<String>, String> {
    let (_entry, json) = load_entry_json(&history, &id)?;
    Ok(channels_already_delivered(&json, send_email, send_sms))
}

#[tauri::command]
pub async fn sync_sms_journal(history: State<'_, HistoryState>) -> Result<usize, String> {
    sms_sync::sync_history_with_journal(&history).await
}
