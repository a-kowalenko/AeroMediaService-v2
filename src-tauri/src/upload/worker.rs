//! Upload queue worker: Dropbox / Custom API upload, share link, archive, notifications.
//! Port of legacy `core/uploader.py`.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedReceiver;

use crate::cloud::binding::{
    client_for_binding, merge_binding_into_history, pool_for_new_job, CustomDropboxPin,
    DropboxAccountBinding,
};
use crate::cloud::guards::assert_upload_has_binding_when_required;
use crate::cloud::{CloudClient, CloudError, CloudState, DropboxPool};
use crate::events;
use crate::model::handoff::{write_job_outbox, OutboxError, OutboxState, CODE_CANCELLED};
use crate::model::kunde::Kunde;
use crate::model::marker::remove_upload_markers;
use crate::storage::dropbox_accounts::DropboxAccountStore;
use crate::storage::logging;
use crate::upload::append::{
    build_append_parent_history_update, APPEND_EVENT_CANCELLED, APPEND_EVENT_COMPLETED,
    APPEND_EVENT_FAILED, APPEND_EVENT_UPLOADING,
};
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

/// Pure-contact markers with Custom API selected upload via the Custom-API Dropbox
/// account (`custom_db_*` secrets), not the hidden native Dropbox tab credentials.
pub fn uses_custom_dropbox_client(job: &UploadJob, selected_cloud: &str) -> bool {
    selected_cloud.trim() == "custom_api" && job.use_dropbox_client
}

#[allow(dead_code)]
pub fn select_upload_client<'a>(
    job: &UploadJob,
    selected_cloud: &str,
    dropbox: &'a dyn CloudClient,
    custom_api: &'a dyn CloudClient,
    custom_dropbox: &'a dyn CloudClient,
) -> &'a dyn CloudClient {
    if uses_custom_api_client(job, selected_cloud) {
        custom_api
    } else if uses_custom_dropbox_client(job, selected_cloud) {
        custom_dropbox
    } else {
        dropbox
    }
}

pub async fn run_loop<F>(
    mut rx: UnboundedReceiver<UploadJob>,
    cloud: CloudState,
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
        let (_pin, client) = match resolve_job_cloud_client(&cloud, &job, &selected_cloud) {
            Ok(v) => v,
            Err(e) => {
                logging::log_error(&format!(
                    "Upload abgebrochen — Dropbox-Konto-Auflösung: {e}"
                ));
                let dir_name = job
                    .dir_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unbekannt");
                events::emit(
                    events::UPLOAD_HISTORY_UPDATE,
                    failure_history(dir_name, &e, job.dropbox_binding.as_ref()),
                );
                registry.unregister(Some(&job.dir_path));
                continue;
            }
        };
        process_job(&job, client.as_ref(), &control, &registry, &archive_path).await;
    }
    logging::log_info("Uploader-Thread beendet.");
}

/// Resolve Dropbox clients from job binding (or active/legacy when unbound).
/// Returns an optional pin (Custom-API Direct-Dropbox) that must outlive the upload.
fn resolve_job_cloud_client(
    cloud: &CloudState,
    job: &UploadJob,
    selected_cloud: &str,
) -> Result<(Option<CustomDropboxPin>, Arc<dyn CloudClient>), String> {
    let pool = pool_for_new_job(selected_cloud, job.use_dropbox_client);
    // Invariant (16c): no multi-account upload without frozen binding (Settings-Verify exempt).
    if let Ok(accounts) = DropboxAccountStore::open_default() {
        assert_upload_has_binding_when_required(
            pool,
            job.dropbox_binding.as_ref().map(|b| b.ams_id.as_str()),
            &accounts,
        )?;
    }

    if uses_custom_api_client(job, selected_cloud) {
        let pin = match &job.dropbox_binding {
            Some(b) if b.pool == DropboxPool::CustomApi => {
                // Temporary slot pin only with job binding (history already carries it).
                Some(CustomDropboxPin::pin(cloud, &b.ams_id))
            }
            Some(b) => {
                return Err(format!(
                    "Job ist an Pool „{}“ gebunden, Custom-API-Pfad erwartet „custom_api“.",
                    b.pool.as_str()
                ));
            }
            None => None,
        };
        let client: Arc<dyn CloudClient> = cloud.custom_api.clone();
        return Ok((pin, client));
    }

    if uses_custom_dropbox_client(job, selected_cloud) {
        let client: Arc<dyn CloudClient> = match &job.dropbox_binding {
            Some(b) => client_for_binding(cloud, b),
            None => cloud.custom_dropbox(),
        };
        return Ok((None, client));
    }

    // Native Dropbox path
    let client: Arc<dyn CloudClient> = match &job.dropbox_binding {
        Some(b) => client_for_binding(cloud, b),
        None => cloud.dropbox(),
    };
    Ok((None, client))
}

fn failure_history(
    dir_name: &str,
    error: &str,
    binding: Option<&DropboxAccountBinding>,
) -> serde_json::Value {
    let mut history = serde_json::json!({
        "dir_name": dir_name,
        "status": "Fehler",
        "error_msg": error,
    });
    merge_binding_into_history(&mut history, binding);
    crate::storage::history::touch_last_updated(&mut history);
    history
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

    if let Some(append) = job.append.clone() {
        process_append_job(job, &append, client, control, registry, archive_path).await;
        return;
    }

    if job.use_dropbox_client {
        logging::log_info(&format!(
            "Beginne Verarbeitung von: {dir_name} (DropboxClient — reiner Kontakt-Marker)"
        ));
    } else {
        logging::log_info(&format!("Beginne Verarbeitung von: {dir_name}"));
    }

    write_job_outbox(
        local_dir_path,
        job.correlation_id.as_deref(),
        OutboxState::Uploading,
        None,
        None,
    );

    events::emit_job_active(true);
    let outcome = run_single_job(local_dir_path, dir_name, kunde, client, control).await;
    events::emit_job_active(false);

    match outcome {
        JobOutcome::Success {
            remote_path,
            share_link,
        } => {
            logging::log_info(&format!("Upload für {dir_name} erfolgreich abgeschlossen."));
            let notify =
                crate::notify::notify_after_upload(dir_name, share_link.as_deref(), Some(kunde))
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
                    client.last_order_id().as_deref(),
                    job.correlation_id.as_deref(),
                    job.dropbox_binding.as_ref(),
                ),
            );
            // Final outbox status before archive move (P1b); outbox file stays on share.
            write_job_outbox(
                local_dir_path,
                job.correlation_id.as_deref(),
                OutboxState::Completed,
                None,
                Some(ARCHIVE_SUCCESS),
            );
            if let Some(moved) =
                archive::archive_directory(archive_path, local_dir_path, ARCHIVE_SUCCESS)
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
                kunde_history(dir_name, "Abgebrochen", kunde, job.dropbox_binding.as_ref()),
            );
            write_job_outbox(
                local_dir_path,
                job.correlation_id.as_deref(),
                OutboxState::Failed,
                Some(OutboxError {
                    code: CODE_CANCELLED.into(),
                    message: "Upload abgebrochen.".into(),
                }),
                Some(ARCHIVE_CANCELLED),
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
                failure_history(dir_name, &err, job.dropbox_binding.as_ref()),
            );
            write_job_outbox(
                local_dir_path,
                job.correlation_id.as_deref(),
                OutboxState::Failed,
                Some(OutboxError {
                    code: "upload_failed".into(),
                    message: err.clone(),
                }),
                Some(ARCHIVE_ERROR),
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

async fn process_append_job(
    job: &UploadJob,
    append: &crate::upload::registry::AppendTarget,
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
    let parent = &append.parent_dir_name;

    logging::log_info(&format!(
        "Beginne Nachreichung '{dir_name}' → {parent} ({})",
        append.remote_path
    ));
    write_job_outbox(
        local_dir_path,
        job.correlation_id.as_deref(),
        OutboxState::Uploading,
        None,
        None,
    );
    events::emit_job_active(true);
    events::emit_status(format!("Nachreichen: {parent}"));
    events::emit(
        events::UPLOAD_HISTORY_UPDATE,
        build_append_parent_history_update(
            append,
            dir_name,
            APPEND_EVENT_UPLOADING,
            job.correlation_id.as_deref(),
            Some(kunde),
            None,
            None,
            None,
            None,
            None,
        ),
    );

    client.set_append_order_id(append.order_id.clone());
    let outcome = match upload_with_retries(
        client,
        local_dir_path,
        &append.remote_path,
        dir_name,
        kunde,
        control,
    )
    .await
    {
        Ok(()) => JobOutcome::Success {
            remote_path: append.remote_path.clone(),
            share_link: append.share_link.clone(),
        },
        Err(e) if e.is_cancelled() => JobOutcome::Cancelled,
        Err(e) => JobOutcome::Failed(e.to_string()),
    };
    client.set_append_order_id(None);
    events::emit_job_active(false);

    match outcome {
        JobOutcome::Success {
            remote_path,
            share_link,
        } => {
            logging::log_info(&format!(
                "Nachreichung '{dir_name}' in {parent} abgeschlossen."
            ));
            crate::model::marker::remove_upload_markers(local_dir_path);
            write_job_outbox(
                local_dir_path,
                job.correlation_id.as_deref(),
                OutboxState::Completed,
                None,
                Some(ARCHIVE_SUCCESS),
            );
            let moved = archive::archive_directory(archive_path, local_dir_path, ARCHIVE_SUCCESS);
            events::emit(
                events::UPLOAD_HISTORY_UPDATE,
                build_append_parent_history_update(
                    append,
                    dir_name,
                    APPEND_EVENT_COMPLETED,
                    job.correlation_id.as_deref(),
                    Some(kunde),
                    None,
                    moved.as_ref().map(|m| m.archived_path.as_path()),
                    None,
                    share_link.as_deref(),
                    client.last_order_id().as_deref(),
                ),
            );
            events::emit_status(format!("Nachgereicht: {remote_path}"));
            events::emit_finished(dir_name.to_string());
        }
        JobOutcome::Cancelled => {
            logging::log_info(&format!("Nachreichung abgebrochen: {dir_name}"));
            events::emit_status(format!("Abgebrochen: {dir_name}"));
            write_job_outbox(
                local_dir_path,
                job.correlation_id.as_deref(),
                OutboxState::Failed,
                Some(OutboxError {
                    code: CODE_CANCELLED.into(),
                    message: "Nachreichung abgebrochen.".into(),
                }),
                Some(ARCHIVE_CANCELLED),
            );
            let moved = archive::archive_directory(archive_path, local_dir_path, ARCHIVE_CANCELLED);
            events::emit(
                events::UPLOAD_HISTORY_UPDATE,
                build_append_parent_history_update(
                    append,
                    dir_name,
                    APPEND_EVENT_CANCELLED,
                    job.correlation_id.as_deref(),
                    Some(kunde),
                    None,
                    moved.as_ref().map(|m| m.archived_path.as_path()),
                    Some("Nachreichung abgebrochen."),
                    None,
                    None,
                ),
            );
        }
        JobOutcome::Failed(err) => {
            logging::log_error(&format!(
                "Nachreichung '{dir_name}' fehlgeschlagen: {err}"
            ));
            events::emit_status(format!("Fehler: {dir_name}"));
            events::emit_failed(err.clone());
            write_job_outbox(
                local_dir_path,
                job.correlation_id.as_deref(),
                OutboxState::Failed,
                Some(OutboxError {
                    code: "upload_failed".into(),
                    message: err.clone(),
                }),
                Some(ARCHIVE_ERROR),
            );
            let moved = archive::archive_directory(archive_path, local_dir_path, ARCHIVE_ERROR);
            events::emit(
                events::UPLOAD_HISTORY_UPDATE,
                build_append_parent_history_update(
                    append,
                    dir_name,
                    APPEND_EVENT_FAILED,
                    job.correlation_id.as_deref(),
                    Some(kunde),
                    None,
                    moved.as_ref().map(|m| m.archived_path.as_path()),
                    Some(err.as_str()),
                    None,
                    None,
                ),
            );
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
        kunde_history(dir_name, "Gestartet", kunde, None),
    );

    let remote_path = format!("/{dir_name}");

    match upload_with_retries(
        client,
        local_dir_path,
        &remote_path,
        dir_name,
        kunde,
        control,
    )
    .await
    {
        Ok(()) => {}
        Err(e) if e.is_cancelled() => return JobOutcome::Cancelled,
        Err(e) => return JobOutcome::Failed(e.to_string()),
    }

    remove_upload_markers(local_dir_path);

    let share_link = match fetch_share_link(client, &remote_path, control).await {
        Ok(link) => link,
        Err(e) if e.is_cancelled() => return JobOutcome::Cancelled,
        Err(e) => {
            logging::log_error(&format!(
                "Konnte Freigabelink für {dir_name} nicht erstellen: {e}"
            ));
            None
        }
    };
    if share_link.is_none() {
        logging::log_error(&format!(
            "Konnte Freigabelink für {dir_name} nicht erstellen."
        ));
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

fn kunde_history(
    dir_name: &str,
    status: &str,
    kunde: &Kunde,
    binding: Option<&DropboxAccountBinding>,
) -> serde_json::Value {
    let mut history = serde_json::json!({
        "dir_name": dir_name,
        "status": status,
        "first_name": kunde.first_name.clone().unwrap_or_default(),
        "last_name": kunde.last_name.clone().unwrap_or_default(),
        "email": kunde.email.clone().unwrap_or_default(),
        "phone": kunde.phone.clone().unwrap_or_default(),
    });
    crate::model::marker::merge_kunde_media_flags(&mut history, kunde);
    merge_binding_into_history(&mut history, binding);
    crate::storage::history::touch_last_updated(&mut history);
    history
}

fn success_history(
    dir_name: &str,
    remote_path: &str,
    share_link: Option<&str>,
    email_status: &str,
    sms_status: &str,
    sms_id: Option<&str>,
    order_id: Option<&str>,
    correlation_id: Option<&str>,
    binding: Option<&DropboxAccountBinding>,
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
    if let Some(order_id) = order_id.map(str::trim).filter(|s| !s.is_empty()) {
        history["order_id"] = serde_json::Value::String(order_id.to_string());
    }
    if let Some(cid) = correlation_id.map(str::trim).filter(|s| !s.is_empty()) {
        history["correlation_id"] = serde_json::Value::String(cid.to_string());
    }
    merge_binding_into_history(&mut history, binding);
    crate::storage::history::touch_last_updated(&mut history);
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
        async fn get_shareable_link(
            &self,
            _remote_path: &str,
        ) -> Result<Option<String>, CloudError> {
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
            correlation_id: None,
            append: None,
            dropbox_binding: None,
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
        assert!(archive
            .join(ARCHIVE_SUCCESS)
            .join("job-ok")
            .join("photo.jpg")
            .is_file());
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
        assert!(uses_custom_dropbox_client(&job, "custom_api"));
        assert!(!uses_custom_dropbox_client(&job, "dropbox"));
    }

    #[test]
    fn select_upload_client_routes_pure_contact_to_custom_dropbox() {
        let dir = tempfile::tempdir().unwrap();
        let mut job = make_job(dir.path());
        job.use_dropbox_client = true;

        let native = MockClient::ok();
        let custom = MockClient::ok();
        let custom_db = MockClient::ok();

        assert!(std::ptr::eq(
            select_upload_client(&job, "custom_api", &native, &custom, &custom_db),
            &custom_db as &dyn CloudClient
        ));
        assert!(std::ptr::eq(
            select_upload_client(&job, "dropbox", &native, &custom, &custom_db),
            &native as &dyn CloudClient
        ));

        job.use_dropbox_client = false;
        assert!(std::ptr::eq(
            select_upload_client(&job, "custom_api", &native, &custom, &custom_db),
            &custom as &dyn CloudClient
        ));
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
            None,
            None,
            None,
        );
        assert_eq!(without_id["email_status"], "Gesendet");
        assert_eq!(without_id["sms_status"], "Gesendet");
        assert_eq!(without_id["share_link"], "https://dropbox.com/s/x");
        assert!(without_id.get("sms_id").is_none());
        assert!(without_id.get("order_id").is_none());

        let with_id = success_history(
            "job-ok",
            "/job-ok",
            Some("https://dropbox.com/s/x"),
            "Fehler: Versand fehlgeschlagen",
            "Gesendet",
            Some("12345"),
            Some("order_abc"),
            Some("cid-1"),
            None,
        );
        assert_eq!(with_id["sms_id"], "12345");
        assert_eq!(with_id["email_status"], "Fehler: Versand fehlgeschlagen");
        assert_eq!(with_id["order_id"], "order_abc");
        assert_eq!(with_id["correlation_id"], "cid-1");
    }
}
