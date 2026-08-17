//! Append extra media into an already uploaded Dropbox / Cloud order folder.
//! Does not create a new monitor directory, share link, or customer notification.

use std::path::Path;

use chrono::Local;
use serde_json::{json, Value};

use crate::cloud::dropbox::DropboxClient;
use crate::cloud::traits::CloudClient;
use crate::cloud::{CloudState, CustomApiClient};
use crate::constants::is_direct_dropbox_upload_mode;
use crate::events;
use crate::monitor::stability::has_uploadable_files;
use crate::notify::resend::remote_path_for_entry;
use crate::storage::config::runtime_setting;
use crate::storage::logging;
use crate::upload::retry::resolve_kunde_from_history_entry;
use crate::upload::UploadControl;
use crate::upload::UploadQueueRegistry;

pub const APPENDABLE_STATUS: &str = "Erfolgreich";

fn json_str<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry.get(key).and_then(Value::as_str).unwrap_or("")
}

fn json_int(entry: &Value, key: &str) -> i64 {
    match entry.get(key) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

pub fn can_append_media(status: &str) -> bool {
    status.trim() == APPENDABLE_STATUS
}

pub fn existing_order_id(entry: &Value) -> Option<String> {
    let value = json_str(entry, "order_id").trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Upload files from `local_dir` into the history entry's existing remote folder.
pub async fn append_media_from_history(
    history_entry: &Value,
    local_dir: &Path,
    selected_cloud: &str,
    cloud: &CloudState,
    control: &UploadControl,
    registry: &UploadQueueRegistry,
) -> Result<Value, String> {
    let status = json_str(history_entry, "status").trim();
    if !can_append_media(status) {
        return Err(format!(
            "Status „{status}“ unterstützt kein Nachladen (nur Erfolgreich)."
        ));
    }

    if !registry.snapshot_dicts().is_empty() {
        return Err(
            "Ein Upload läuft bereits. Bitte warten, bis die Warteschlange leer ist.".into(),
        );
    }

    if !local_dir.is_dir() {
        return Err(format!(
            "Ordner existiert nicht: {}",
            local_dir.display()
        ));
    }
    if !has_uploadable_files(local_dir) {
        return Err("Der gewählte Ordner enthält keine Medien-Dateien.".into());
    }

    let remote_path = remote_path_for_entry(history_entry);
    if remote_path.trim().is_empty() {
        return Err("Historieneintrag ohne Dropbox-Pfad (remote_path / dir_name).".into());
    }

    let dir_name = json_str(history_entry, "dir_name").trim();
    let kunde = resolve_kunde_from_history_entry(history_entry).await?;
    let order_id = existing_order_id(history_entry);
    let use_custom = selected_cloud.trim() == "custom_api";

    if use_custom && !is_direct_dropbox_upload_mode(&runtime_setting("custom_api_upload_mode")) {
        return Err(
            "Nachladen über die Custom API ist nur im Modus „Dropbox + Manifest“ möglich."
                .into(),
        );
    }

    logging::log_info(&format!(
        "Nachladen in {remote_path} aus {} (dir_name={dir_name})",
        local_dir.display()
    ));
    events::emit_status(format!("Lade Dateien nach: {remote_path}"));

    control.reset_for_new_job();

    let ok = if use_custom {
        append_via_custom_api(
            &cloud.custom_api,
            local_dir,
            &remote_path,
            control,
            &kunde,
            order_id.as_deref(),
        )
        .await?
    } else {
        append_via_dropbox(&cloud.dropbox, local_dir, &remote_path, control, &kunde).await?
    };

    if !ok {
        return Err("Nachladen fehlgeschlagen (siehe Log).".into());
    }

    let now = Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let append_count = json_int(history_entry, "append_count") + 1;
    let mut updates = json!({
        "dir_name": dir_name,
        "status": "Erfolgreich",
        "remote_path": remote_path,
        "last_append_at": now,
        "append_count": append_count,
    });
    let stored_link = json_str(history_entry, "share_link").trim();
    if !stored_link.is_empty() {
        updates["share_link"] = json!(stored_link);
    }
    let resolved_order = if use_custom {
        CloudClient::last_order_id(cloud.custom_api.as_ref()).or(order_id)
    } else {
        order_id
    };
    if let Some(oid) = resolved_order.filter(|s| !s.is_empty()) {
        updates["order_id"] = json!(oid);
    }

    events::emit(events::UPLOAD_HISTORY_UPDATE, &updates);
    events::emit_status(format!("Nachgeladen: {remote_path}"));
    Ok(updates)
}

async fn append_via_dropbox(
    client: &DropboxClient,
    local_dir: &Path,
    remote_path: &str,
    control: &UploadControl,
    kunde: &crate::model::kunde::Kunde,
) -> Result<bool, String> {
    client
        .upload_directory(local_dir, remote_path, control, kunde)
        .await
        .map_err(|e| e.to_string())
}

async fn append_via_custom_api(
    client: &CustomApiClient,
    local_dir: &Path,
    remote_path: &str,
    control: &UploadControl,
    kunde: &crate::model::kunde::Kunde,
    order_id: Option<&str>,
) -> Result<bool, String> {
    client.set_append_order_id(order_id.map(str::to_string));
    let result = client
        .upload_directory(local_dir, remote_path, control, kunde)
        .await;
    client.set_append_order_id(None);
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_successful_jobs_can_append() {
        assert!(can_append_media("Erfolgreich"));
        assert!(!can_append_media("Fehler"));
        assert!(!can_append_media("Gestartet"));
    }

    #[test]
    fn order_id_from_history() {
        assert_eq!(
            existing_order_id(&json!({"order_id": "order_abc"})).as_deref(),
            Some("order_abc")
        );
        assert_eq!(existing_order_id(&json!({"order_id": "  "})), None);
        assert_eq!(existing_order_id(&json!({})), None);
    }
}
