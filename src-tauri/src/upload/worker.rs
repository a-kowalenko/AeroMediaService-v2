//! Upload queue worker: Dropbox / Custom API upload, share link, archive, notifications.
//! Port of legacy `core/uploader.py`.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedReceiver;

use crate::cloud::{CloudClient, CloudError};
use crate::events;
use crate::model::kunde::Kunde;
use crate::model::marker::remove_upload_markers;
use crate::storage::logging;
use crate::upload::control::{UploadCancelled, UploadControl};
use crate::upload::registry::{UploadJob, UploadQueueRegistry};
use crate::util::archive::{self, ARCHIVE_CANCELLED, ARCHIVE_ERROR, ARCHIVE_SUCCESS};

fn job_retry_delays() -> [u64; 2] {
    if cfg!(test) {
        [0, 0]
    } else {
        [5, 15]
    }
}

fn share_link_retry_secs() -> u64 {
    if cfg!(test) {
        0
    } else {
        2
    }
}

pub fn uses_custom_api_client(job: &UploadJob, selected_cloud: &str) -> bool {
    selected_cloud.trim() == "custom_api" && !job.use_dropbox_client
}

pub async fn run_loop<F>(
    mut rx: UnboundedReceiver<UploadJob>,
    dropbox: Arc<dyn CloudClient>,
    custom_api: Arc<dyn CloudClient>,
    control: UploadControl,
    registry: Arc<UploadQueueRegistry>,
    get_setting: F,
) where
    F: Fn(&str) -> String + Send + Sync,
{
    logging::log_info("Uploader-Thread gestartet. Warte auf Upload-Aufträge.");
    while let Some(job) = rx.recv().await {
        let archive_path = get_setting("archive_path");
        let selected_cloud = get_setting("selected_cloud_service");
        let client: &dyn CloudClient = if uses_custom_api_client(&job, &selected_cloud) {
            custom_api.as_ref()
        } else {
            dropbox.as_ref()
        };
        process_job(&job, client, &control, &registry, &archive_path).await;
    }
    logging::log_info("Uploader-Thread beendet.");
}

pub async fn process_job(
    job: &UploadJob,
    client: &dyn CloudClient,
    control: &UploadControl,
    registry: &UploadQueueRegistry,
    archive_path: &str,
) {
    let local_dir_path = job.dir_path.as_path();
    let dir_name = local_dir_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unbekannt");
    let kunde = &job.kunde;

    registry.mark_active(local_dir_path);

    if job.use_dropbox_client {
        logging::log_info(&format!(
            "Beginne Verarbeitung von: {dir_name} (DropboxClient — reiner Kontakt-Marker)"
        ));
    } else {
        logging::log_info(&format!("Beginne Verarbeitung von: {dir_name}"));
    }

    events::emit_job_active(true);
    let outcome = run_single_job(local_dir_path, dir_name, kunde, client, control).await;
    events::emit_job_active(false);

    match outcome {
        JobOutcome::Success { remote_path, share_link } => {
            logging::log_info(&format!("Upload für {dir_name} erfolgreich abgeschlossen."));
            let notify = crate::notify::notify_after_upload(
                dir_name,
                share_link.as_deref(),
                Some(kunde),
            )
            .await;
            events::emit(
                events::UPLOAD_HISTORY_UPDATE,
                success_history(
                    dir_name,
                    &remote_path,
                    share_link.as_deref(),
                    &notify.email_status,
                    &notify.sms_status,
                    notify.sms_id.as_deref(),
                ),
            );
            if let Some(moved) = archive::archive_directory(archive_path, local_dir_path, ARCHIVE_SUCCESS)
            {
                emit_archive_history(&moved);
            }
            events::emit_status(format!("Erfolgreich: {dir_name}"));
            events::emit_finished(dir_name.to_string());
        }
        JobOutcome::Cancelled => {
            logging::log_info(&format!("Upload abgebrochen: {dir_name}"));
            events::emit_status(format!("Abgebrochen: {dir_name}"));
            events::emit(
                events::UPLOAD_HISTORY_UPDATE,
                kunde_history(dir_name, "Abgebrochen", kunde),
            );
            if let Some(moved) =
                archive::archive_directory(archive_path, local_dir_path, ARCHIVE_CANCELLED)
            {
                emit_archive_history(&moved);
            }
        }
        JobOutcome::Failed(err) => {
            logging::log_error(&format!(
                "Fehler bei der Verarbeitung von '{dir_name}' (Pfad: {}): {err}",
                local_dir_path.display()
            ));
            events::emit_status(format!("Fehler: {dir_name}"));
            events::emit_failed(err.clone());
            events::emit(
                events::UPLOAD_HISTORY_UPDATE,
                serde_json::json!({
                    "dir_name": dir_name,
                    "status": "Fehler",
                    "error_msg": err,
                }),
            );
            if let Some(moved) =
                archive::archive_directory(archive_path, local_dir_path, ARCHIVE_ERROR)
            {
                emit_archive_history(&moved);
            }
        }
    }

    registry.unregister(Some(local_dir_path));
    events::emit_progress_file(0, 0, 0);
    events::emit_progress_total(0, 0, 0);
    logging::log_info("Warte auf nächsten Upload-Auftrag...");
    events::emit_status("Warte auf nächsten Auftrag...");
}

enum JobOutcome {
    Success {
        remote_path: String,
        share_link: Option<String>,
    },
    Cancelled,
    Failed(String),
}

async fn run_single_job(
    local_dir_path: &Path,
    dir_name: &str,
    kunde: &Kunde,
    client: &dyn CloudClient,
    control: &UploadControl,
) -> JobOutcome {
    control.reset_for_new_job();
    events::emit_status(format!("Starte Upload: {dir_name}"));
    events::emit_progress_file(0, 0, 0);
    events::emit_progress_total(0, 0, 0);
    events::emit(
        events::UPLOAD_HISTORY_UPDATE,
        kunde_history(dir_name, "Gestartet", kunde),
    );

    let remote_path = format!("/{dir_name}");

    match upload_with_retries(client, local_dir_path, &remote_path, dir_name, kunde, control).await {
        Ok(()) => {}
        Err(e) if e.is_cancelled() => return JobOutcome::Cancelled,
        Err(e) => return JobOutcome::Failed(e.to_string()),
    }

    remove_upload_markers(local_dir_path);

    let share_link = match fetch_share_link(client, &remote_path, control).await {
        Ok(link) => link,
        Err(e) if e.is_cancelled() => return JobOutcome::Cancelled,
        Err(e) => {
            logging::log_error(&format!("Konnte Freigabelink für {dir_name} nicht erstellen: {e}"));
            None
        }
    };
    if share_link.is_none() {
        logging::log_error(&format!("Konnte Freigabelink für {dir_name} nicht erstellen."));
    }

    JobOutcome::Success {
        remote_path,
        share_link,
    }
}

async fn upload_with_retries(
    client: &dyn CloudClient,
    local_dir_path: &Path,
    remote_path: &str,
    dir_name: &str,
    kunde: &Kunde,
    control: &UploadControl,
) -> Result<(), CloudError> {
    for job_try in 1..=3 {
        let result = client
            .upload_directory(local_dir_path, remote_path, control, kunde)
            .await;
        match result {
            Ok(true) => return Ok(()),
            Err(e) if e.is_cancelled() => return Err(e),
            Ok(false) | Err(_) => {
                if job_try < 3 {
                    let wait_s = job_retry_delays()[job_try - 1];
                    logging::log_warn(&format!(
                        "Upload '{dir_name}' meldete Fehler (Versuch {job_try}/3). Erneuter Versuch in {wait_s}s."
                    ));
                    sleep_cancellable(control, wait_s).await?;
                }
            }
        }
    }
    Err(CloudError::Message(
        "Upload-Funktion des Clients meldete nach 3 Versuchen weiterhin einen Fehler.".into(),
    ))
}

async fn fetch_share_link(
    client: &dyn CloudClient,
    remote_path: &str,
    control: &UploadControl,
) -> Result<Option<String>, CloudError> {
    for attempt in 1..=3 {
        control.check_cancelled()?;
        match client.get_shareable_link(remote_path).await {
            Ok(Some(link)) => return Ok(Some(link)),
            Ok(None) | Err(_) if attempt < 3 => {
                logging::log_warn(&format!(
                    "Freigabelink noch nicht verfuegbar (Versuch {attempt}/3), warte {}s...",
                    share_link_retry_secs()
                ));
                sleep_cancellable(control, share_link_retry_secs()).await?;
            }
            Ok(None) => return Ok(None),
            Err(e) if e.is_cancelled() => return Err(e),
            Err(e) => {
                if attempt >= 3 {
                    logging::log_error(&format!("Freigabelink fehlgeschlagen: {e}"));
                    return Ok(None);
                }
            }
        }
    }
    Ok(None)
}

async fn sleep_cancellable(control: &UploadControl, secs: u64) -> Result<(), UploadCancelled> {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        control.check_cancelled()?;
        let rem = end.saturating_duration_since(Instant::now());
        if rem.is_zero() {
            return Ok(());
        }
        tokio::time::sleep(rem.min(Duration::from_millis(500))).await;
    }
}

fn kunde_history(dir_name: &str, status: &str, kunde: &Kunde) -> serde_json::Value {
    serde_json::json!({
        "dir_name": dir_name,
        "status": status,
        "first_name": kunde.first_name.clone().unwrap_or_default(),
        "last_name": kunde.last_name.clone().unwrap_or_default(),
        "email": kunde.email.clone().unwrap_or_default(),
        "phone": kunde.phone.clone().unwrap_or_default(),
    })
}

fn success_history(
    dir_name: &str,
    remote_path: &str,
    share_link: Option<&str>,
    email_status: &str,
    sms_status: &str,
    sms_id: Option<&str>,
) -> serde_json::Value {
    let mut history = serde_json::json!({
        "dir_name": dir_name,
        "status": "Erfolgreich",
        "email_status": email_status,
        "sms_status": sms_status,
        "remote_path": remote_path,
    });
    if let Some(link) = share_link {
        history["share_link"] = serde_json::Value::String(link.to_string());
    }
    if let Some(sms_id) = sms_id.filter(|s| !s.is_empty()) {
        history["sms_id"] = serde_json::Value::String(sms_id.to_string());
    }
    history
}

fn emit_archive_history(moved: &archive::ArchiveMove) {
    events::emit(
        events::UPLOAD_HISTORY_UPDATE,
        serde_json::json!({
            "dir_name": moved.dir_name,
            "archived_path": moved.archived_path.to_string_lossy(),
            "archive_subfolder": moved.archive_subfolder,
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::kunde::Kunde;
    use crate::model::marker::{write_processing_marker, MARKER_PROCESSING};
    use crate::upload::registry::UploadQueueRegistry;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tempfile::tempdir;
    use tokio::sync::mpsc::unbounded_channel;

    struct MockClient {
        uploads: AtomicUsize,
        fail_times: usize,
        cancel: bool,
        share: Option<String>,
        uploaded_paths: Mutex<Vec<PathBuf>>,
    }

    impl MockClient {
        fn ok() -> Self {
            Self {
                uploads: AtomicUsize::new(0),
                fail_times: 0,
                cancel: false,
                share: Some("https://dropbox.com/s/x".into()),
                uploaded_paths: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl CloudClient for MockClient {
        async fn connect(&self) -> Result<bool, CloudError> {
            Ok(true)
        }
        async fn disconnect(&self) -> Result<(), CloudError> {
            Ok(())
        }
        fn connection_status(&self) -> String {
            "Verbunden".into()
        }
        async fn upload_directory(
            &self,
            local_dir_path: &Path,
            _remote_base_path: &str,
            control: &UploadControl,
            _kunde: &crate::model::kunde::Kunde,
        ) -> Result<bool, CloudError> {
            control.wait_if_paused().await?;
            let n = self.uploads.fetch_add(1, Ordering::SeqCst) + 1;
            self.uploaded_paths
                .lock()
                .unwrap()
                .push(local_dir_path.to_path_buf());
            if self.cancel {
                return Err(CloudError::Cancelled(UploadCancelled));
            }
            Ok(n > self.fail_times)
        }
        async fn get_shareable_link(&self, _remote_path: &str) -> Result<Option<String>, CloudError> {
            Ok(self.share.clone())
        }
    }

    fn sample_kunde() -> Kunde {
        Kunde {
            first_name: Some("Anna".into()),
            last_name: Some("Muster".into()),
            email: Some("anna@example.de".into()),
            ..Kunde::default()
        }
    }

    fn make_job(dir: &Path) -> UploadJob {
        write_processing_marker(
            dir,
            r#"{"vorname":"Anna","nachname":"Muster","email":"anna@example.de"}"#,
        )
        .unwrap();
        std::fs::write(dir.join("photo.jpg"), b"jpeg").unwrap();
        UploadJob {
            dir_path: dir.to_path_buf(),
            kunde: sample_kunde(),
            use_dropbox_client: false,
        }
    }

    #[tokio::test]
    async fn success_archives_to_erfolg() {
        let root = tempdir().unwrap();
        let job_dir = root.path().join("job-ok");
        std::fs::create_dir(&job_dir).unwrap();
        let archive = root.path().join("archive");
        let job = make_job(&job_dir);
        let registry = UploadQueueRegistry::new();
        let (tx, _rx) = unbounded_channel();
        assert!(registry.enqueue(&tx, job.clone(), false));

        let client = MockClient::ok();
        process_job(
            &job,
            &client,
            &UploadControl::new(),
            &registry,
            archive.to_str().unwrap(),
        )
        .await;

        assert!(!job_dir.exists());
        assert!(archive.join(ARCHIVE_SUCCESS).join("job-ok").join("photo.jpg").is_file());
        assert!(!archive
            .join(ARCHIVE_SUCCESS)
            .join("job-ok")
            .join(MARKER_PROCESSING)
            .exists());
        assert!(!registry.is_registered(&job.dir_path));
        assert_eq!(client.uploads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failure_archives_to_fehler() {
        let root = tempdir().unwrap();
        let job_dir = root.path().join("job-fail");
        std::fs::create_dir(&job_dir).unwrap();
        let archive = root.path().join("archive");
        let job = make_job(&job_dir);
        let registry = UploadQueueRegistry::new();
        let (tx, _rx) = unbounded_channel();
        registry.enqueue(&tx, job.clone(), false);

        let client = MockClient {
            fail_times: 99,
            ..MockClient::ok()
        };
        process_job(
            &job,
            &client,
            &UploadControl::new(),
            &registry,
            archive.to_str().unwrap(),
        )
        .await;

        assert!(archive.join(ARCHIVE_ERROR).join("job-fail").is_dir());
        assert!(!job_dir.exists());
    }

    #[tokio::test]
    async fn cancel_archives_to_abgebrochen() {
        let root = tempdir().unwrap();
        let job_dir = root.path().join("job-cancel");
        std::fs::create_dir(&job_dir).unwrap();
        let archive = root.path().join("archive");
        let job = make_job(&job_dir);
        let registry = UploadQueueRegistry::new();
        let (tx, _rx) = unbounded_channel();
        registry.enqueue(&tx, job.clone(), false);

        let client = MockClient {
            cancel: true,
            ..MockClient::ok()
        };
        process_job(
            &job,
            &client,
            &UploadControl::new(),
            &registry,
            archive.to_str().unwrap(),
        )
        .await;

        assert!(archive.join(ARCHIVE_CANCELLED).join("job-cancel").is_dir());
        assert!(!job_dir.exists());
    }

    #[test]
    fn custom_api_client_selection_respects_pure_contact() {
        let dir = tempfile::tempdir().unwrap();
        let mut job = make_job(dir.path());
        job.use_dropbox_client = false;
        assert!(uses_custom_api_client(&job, "custom_api"));
        assert!(!uses_custom_api_client(&job, "dropbox"));
        job.use_dropbox_client = true;
        assert!(!uses_custom_api_client(&job, "custom_api"));
    }

    #[test]
    fn success_history_includes_notify_fields_and_optional_sms_id() {
        let without_id = success_history(
            "job-ok",
            "/job-ok",
            Some("https://dropbox.com/s/x"),
            "Gesendet",
            "Gesendet",
            None,
        );
        assert_eq!(without_id["email_status"], "Gesendet");
        assert_eq!(without_id["sms_status"], "Gesendet");
        assert_eq!(without_id["share_link"], "https://dropbox.com/s/x");
        assert!(without_id.get("sms_id").is_none());

        let with_id = success_history(
            "job-ok",
            "/job-ok",
            Some("https://dropbox.com/s/x"),
            "Fehler: Versand fehlgeschlagen",
            "Gesendet",
            Some("12345"),
        );
        assert_eq!(with_id["sms_id"], "12345");
        assert_eq!(with_id["email_status"], "Fehler: Versand fehlgeschlagen");
    }
}
