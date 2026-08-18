//! Re-queue an archived upload from history.
//! Port of legacy `core/retry_upload.py`.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

use crate::events;
use crate::model::handoff::peek_correlation_id;
use crate::model::marker::{
    should_use_dropbox_client_for_marker, write_processing_marker, MARKER_PROCESSING,
};
use crate::upload::booking_flags::{self, BookingFlagsPolicy};
use crate::monitor::stability::has_uploadable_files;
use crate::storage::logging;
use crate::upload::registry::{UploadJob, UploadQueueRegistry};
use crate::util::archive::{self, ARCHIVE_CANCELLED, ARCHIVE_ERROR};

pub const RETRYABLE_STATUSES: [&str; 2] = ["Fehler", "Abgebrochen"];

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

pub fn is_retryable_status(status: &str) -> bool {
    RETRYABLE_STATUSES.contains(&status.trim())
}

pub async fn resolve_kunde_from_history_entry(entry: &Value) -> Result<crate::model::kunde::Kunde, String> {
    let resolved = booking_flags::resolve_booking_flags(entry, BookingFlagsPolicy::Auto).await?;
    let k = &resolved.kunde;
    let has_contact = nonempty_opt(k.first_name.as_deref())
        && nonempty_opt(k.last_name.as_deref())
        && nonempty_opt(k.email.as_deref());
    if !has_contact {
        return Err(
            "Weder Marker (marker_raw) noch vollständige Kundendaten in der Historie. \
             Erneuter Upload ist nicht möglich."
                .into(),
        );
    }
    Ok(resolved.kunde)
}

fn nonempty_opt(value: Option<&str>) -> bool {
    value.map(|s| !s.trim().is_empty()).unwrap_or(false)
}

/// Restore an archived job into the monitor folder and enqueue it.
pub async fn retry_upload_from_history(
    history_entry: &Value,
    monitor_path: &str,
    archive_path: &str,
    selected_cloud: &str,
    jobs: &UnboundedSender<UploadJob>,
    registry: &UploadQueueRegistry,
) -> Result<String, String> {
    let status = json_str(history_entry, "status").trim();
    if !is_retryable_status(status) {
        return Err(format!(
            "Status „{status}“ unterstützt keinen erneuten Upload."
        ));
    }

    let dir_name = json_str(history_entry, "dir_name").trim();
    if dir_name.is_empty() {
        return Err("Historieneintrag ohne Verzeichnisname.".into());
    }

    if monitor_path.trim().is_empty() {
        return Err("Kein Überwachungsordner konfiguriert.".into());
    }
    if archive_path.trim().is_empty() {
        return Err("Kein Archiv-Ordner konfiguriert.".into());
    }
    if !Path::new(monitor_path).is_dir() {
        return Err(format!(
            "Überwachungsordner existiert nicht: {monitor_path}"
        ));
    }

    let archived_hint = {
        let hint = json_str(history_entry, "archived_path").trim();
        if hint.is_empty() {
            None
        } else {
            Some(PathBuf::from(hint))
        }
    };
    let archived_path = archive::find_archived_folder(
        archive_path,
        dir_name,
        &[ARCHIVE_ERROR, ARCHIVE_CANCELLED],
        archived_hint.as_deref(),
    )
    .ok_or_else(|| {
        format!(
            "Ordner „{dir_name}“ wurde unter archiv/fehler oder archiv/abgebrochen nicht gefunden."
        )
    })?;

    let target_path = Path::new(monitor_path).join(dir_name);
    if target_path.exists() {
        return Err(format!(
            "Im Überwachungsordner existiert bereits „{dir_name}“. \
             Bitte den Konflikt manuell lösen."
        ));
    }

    if registry.is_registered(&target_path) {
        return Err(format!(
            "„{dir_name}“ ist bereits in der Upload-Warteschlange."
        ));
    }

    if !has_uploadable_files(&archived_path) {
        return Err(format!(
            "Ordner „{dir_name}“ enthält keine Medien-Dateien. Erneuter Upload ist nicht möglich."
        ));
    }

    let kunde = resolve_kunde_from_history_entry(history_entry).await?;
    let marker_raw = json_str(history_entry, "marker_raw").trim().to_string();

    logging::log_info(&format!(
        "Retry: verschiebe '{dir_name}' von '{}' nach '{}'.",
        archived_path.display(),
        target_path.display()
    ));
    archive::move_directory(&archived_path, &target_path).map_err(|e| {
        format!(
            "Ordner konnte nicht nach {} verschoben werden: {e}",
            target_path.display()
        )
    })?;

    if !marker_raw.is_empty() {
        write_processing_marker(&target_path, &marker_raw).map_err(|e| e.to_string())?;
        logging::log_info(&format!(
            "Marker {MARKER_PROCESSING} für Retry geschrieben."
        ));
    }

    let use_dropbox_client = if marker_raw.is_empty() {
        false
    } else {
        should_use_dropbox_client_for_marker(selected_cloud, &marker_raw).unwrap_or(false)
    };

    let job = UploadJob {
        dir_path: target_path.clone(),
        kunde: kunde.clone(),
        use_dropbox_client,
        correlation_id: peek_correlation_id(&target_path),
        append: None,
    };
    if !registry.enqueue(jobs, job, false) {
        return Err(format!(
            "„{dir_name}“ konnte nicht in die Warteschlange eingereiht werden."
        ));
    }

    let retry_count = json_int(history_entry, "retry_count") + 1;
    events::emit(
        events::UPLOAD_HISTORY_UPDATE,
        json!({
            "dir_name": dir_name,
            "status": "Gestartet",
            "retry_count": retry_count,
            "first_name": kunde.first_name.unwrap_or_default(),
            "last_name": kunde.last_name.unwrap_or_default(),
            "email": kunde.email.unwrap_or_default(),
            "phone": kunde.phone.unwrap_or_default(),
            "customer_number": kunde.customer_number.unwrap_or_default(),
            "booking_number": kunde.booking_number.unwrap_or_default(),
            "type": kunde.customer_type.unwrap_or_default(),
            "handcam_foto": kunde.handcam_foto,
            "handcam_video": kunde.handcam_video,
            "outside_foto": kunde.outside_foto,
            "outside_video": kunde.outside_video,
            "ist_bezahlt_handcam_foto": kunde.ist_bezahlt_handcam_foto,
            "ist_bezahlt_handcam_video": kunde.ist_bezahlt_handcam_video,
            "ist_bezahlt_outside_foto": kunde.ist_bezahlt_outside_foto,
            "ist_bezahlt_outside_video": kunde.ist_bezahlt_outside_video,
        }),
    );
    events::emit_status(format!("Erneut eingereiht: {dir_name}"));

    Ok(format!(
        "„{dir_name}“ wurde in die Upload-Warteschlange eingereiht."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upload::booking_flags::kunde_from_history_fields;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;
    use tokio::sync::mpsc::unbounded_channel;

    fn write_media(dir: &Path) {
        fs::write(dir.join("clip.mp4"), b"media").unwrap();
    }

    #[test]
    fn retryable_statuses_match_legacy() {
        assert!(is_retryable_status("Fehler"));
        assert!(is_retryable_status("Abgebrochen"));
        assert!(!is_retryable_status("Erfolgreich"));
        assert!(!is_retryable_status("Gestartet"));
    }

    #[test]
    fn kunde_from_history_requires_name_and_email() {
        assert!(kunde_from_history_fields(&json!({
            "first_name": "Ada",
            "last_name": "Lovelace",
            "email": "ada@example.de",
            "phone": "0160",
        }))
        .is_some());
        assert!(kunde_from_history_fields(&json!({
            "first_name": "Ada",
            "last_name": "Lovelace",
        }))
        .is_none());
    }

    #[test]
    fn kunde_from_history_applies_media_flags() {
        let k = kunde_from_history_fields(&json!({
            "first_name": "Ada",
            "last_name": "Lovelace",
            "email": "ada@example.de",
            "handcam_video": true,
            "ist_bezahlt_handcam_video": true,
            "outside_foto": "JA",
        }))
        .unwrap();
        assert!(k.handcam_video);
        assert!(k.ist_bezahlt_handcam_video);
        assert!(k.outside_foto);
        assert!(!k.handcam_foto);
    }

    #[tokio::test]
    async fn resolve_uses_persisted_flags_without_marker() {
        let k = resolve_kunde_from_history_entry(&json!({
            "first_name": "Ada",
            "last_name": "Lovelace",
            "email": "ada@example.de",
            "handcam_video": true,
            "ist_bezahlt_handcam_video": true,
            "marker_raw": "{\"type\":\"Handcam\",\"kunden_id_hash\":\"a\",\"booking_id_hash\":\"b\"}",
        }))
        .await
        .unwrap();
        assert!(k.handcam_video);
        assert!(k.ist_bezahlt_handcam_video);
        assert!(!k.outside_foto);
    }

    #[tokio::test]
    async fn resolve_reads_contact_marker_flags_when_history_lacks_them() {
        let k = resolve_kunde_from_history_entry(&json!({
            "dir_name": "Flug_1",
            "first_name": "Ada",
            "last_name": "Lovelace",
            "email": "ada@example.de",
            "marker_raw": "{\"vorname\":\"Ada\",\"nachname\":\"Lovelace\",\"email\":\"ada@example.de\",\"outside_foto\":true,\"ist_bezahlt_outside_foto\":false}",
        }))
        .await
        .unwrap();
        assert!(k.outside_foto);
        assert!(!k.ist_bezahlt_outside_foto);
    }

    #[tokio::test]
    async fn resolve_applies_flags_from_api_marker_when_lookup_fails() {
        let k = resolve_kunde_from_history_entry(&json!({
            "dir_name": "Flug_1",
            "first_name": "Ada",
            "last_name": "Lovelace",
            "email": "ada@example.de",
            "marker_raw": "{\"type\":\"Handcam\",\"kunden_id_hash\":\"a\",\"booking_id_hash\":\"b\",\"handcam_video\":true,\"ist_bezahlt_handcam_video\":true}",
        }))
        .await
        .unwrap();
        assert!(k.handcam_video);
        assert!(k.ist_bezahlt_handcam_video);
        assert!(!k.handcam_foto);
    }

    #[tokio::test]
    async fn retry_rejects_non_retryable_and_missing_dir() {
        let (tx, _rx) = unbounded_channel();
        let registry = UploadQueueRegistry::new();
        let err = retry_upload_from_history(
            &json!({"status": "Erfolgreich", "dir_name": "x"}),
            "/tmp",
            "/tmp",
            "dropbox",
            &tx,
            &registry,
        )
        .await
        .unwrap_err();
        assert!(err.contains("unterstützt keinen"));

        let err = retry_upload_from_history(
            &json!({"status": "Fehler"}),
            "/tmp",
            "/tmp",
            "dropbox",
            &tx,
            &registry,
        )
        .await
        .unwrap_err();
        assert!(err.contains("Verzeichnisname"));
    }

    #[tokio::test]
    async fn retry_moves_archive_and_enqueues() {
        let root = tempdir().unwrap();
        let monitor = root.path().join("monitor");
        let archive = root.path().join("archive");
        fs::create_dir_all(&monitor).unwrap();
        let archived = archive.join(ARCHIVE_ERROR).join("Flug_1");
        fs::create_dir_all(&archived).unwrap();
        write_media(&archived);
        fs::write(
            archived.join("_in_verarbeitung.txt"),
            r#"{"vorname":"Ada","nachname":"Lovelace","email":"ada@example.de"}"#,
        )
        .unwrap();

        let (tx, mut rx) = unbounded_channel();
        let registry = UploadQueueRegistry::new();
        let entry = json!({
            "status": "Fehler",
            "dir_name": "Flug_1",
            "archived_path": archived.to_string_lossy(),
            "first_name": "Ada",
            "last_name": "Lovelace",
            "email": "ada@example.de",
            "phone": "0160",
            "marker_raw": "{\"vorname\":\"Ada\",\"nachname\":\"Lovelace\",\"email\":\"ada@example.de\"}",
        });
        let message = retry_upload_from_history(
            &entry,
            monitor.to_str().unwrap(),
            archive.to_str().unwrap(),
            "dropbox",
            &tx,
            &registry,
        )
        .await
        .unwrap();
        assert!(message.contains("Flug_1"));
        assert!(!archived.exists());
        let restored = monitor.join("Flug_1");
        assert!(restored.join("clip.mp4").is_file());
        assert!(restored.join(MARKER_PROCESSING).is_file());
        let job = rx.try_recv().unwrap();
        assert_eq!(job.dir_path, restored);
        assert_eq!(job.kunde.first_name.as_deref(), Some("Ada"));
        assert!(registry.is_registered(&restored));
    }

    #[tokio::test]
    async fn retry_rejects_existing_monitor_folder() {
        let root = tempdir().unwrap();
        let monitor = root.path().join("monitor");
        let archive = root.path().join("archive");
        fs::create_dir_all(monitor.join("Flug_1")).unwrap();
        let archived = archive.join(ARCHIVE_ERROR).join("Flug_1");
        fs::create_dir_all(&archived).unwrap();
        write_media(&archived);

        let (tx, _rx) = unbounded_channel();
        let registry = UploadQueueRegistry::new();
        let err = retry_upload_from_history(
            &json!({
                "status": "Abgebrochen",
                "dir_name": "Flug_1",
                "first_name": "A",
                "last_name": "B",
                "email": "a@b.de",
            }),
            monitor.to_str().unwrap(),
            archive.to_str().unwrap(),
            "dropbox",
            &tx,
            &registry,
        )
        .await
        .unwrap_err();
        assert!(err.contains("existiert bereits"));
    }
}
