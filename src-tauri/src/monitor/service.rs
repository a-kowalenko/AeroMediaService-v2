//! Folder monitor: scan interval, marker claim, enqueue into the upload pipeline.
//! Port of legacy `core/monitor.py`.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Notify;

use crate::cloud::custom_api::fetch_customer_as_kunde;
use crate::model::handoff::{
    evaluate_manifest_gate, is_handoff_scan_dir, manifest_path, peek_correlation_id,
    write_job_outbox, GateDecision, OutboxError, OutboxState, CODE_CUSTOMER_LOOKUP_FAILED,
    CODE_MANIFEST_MISSING_LEGACY, CODE_MARKER_INVALID,
};
use crate::model::kunde::Kunde;
use crate::model::marker::{
    claim_fertig_marker, discard_stale_fertig_marker, load_marker_data, marker_paths,
    parse_api_marker_data, read_marker_file, read_marker_raw, resolve_kunde_from_marker,
    should_use_dropbox_client_for_marker, ApiMarkerQuery, LookupMode, MarkerError,
};
use crate::monitor::stability::{has_uploadable_files, FolderStabilityTracker, ObserveResult};
use crate::storage::logging;
use crate::upload::registry::{UploadJob, UploadQueueRegistry};
use crate::util::archive::{self, handle_customer_lookup_failure, is_marker_format_failure};

const MISSING_PATH_WAIT_SECS: u64 = 60;
const DEFAULT_SCAN_INTERVAL_SECS: u64 = 10;
const DEFAULT_STABILITY_SECS: f64 = 15.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimResult {
    NotAFolder,
    AlreadyProcessing,
    NoFertigMarker,
    NoMedia,
    AlreadyClaimed,
    CustomerLookupFailed,
    /// Manifest gate rejected the folder; leave it for the next scan (no claim).
    ManifestRejected { code: String, message: String },
    MarkerError(String),
    IoError(String),
    Queued,
}

pub type CustomerLookup = fn(&ApiMarkerQuery, LookupMode) -> Result<Kunde, String>;

pub struct EnqueueContext<'a> {
    pub registry: &'a UploadQueueRegistry,
    pub jobs: &'a UnboundedSender<UploadJob>,
    pub selected_cloud: &'a str,
    pub archive_path: &'a str,
    pub customer_lookup: Option<CustomerLookup>,
    /// When true, jobs without `_ams_manifest.v1.json` are rejected (default false = legacy).
    pub manifest_required: bool,
}

pub struct MonitorState {
    running: Arc<AtomicBool>,
    wake: Arc<Notify>,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    registry: Arc<UploadQueueRegistry>,
    jobs: UnboundedSender<UploadJob>,
}

impl MonitorState {
    pub fn new(registry: Arc<UploadQueueRegistry>, jobs: UnboundedSender<UploadJob>) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            wake: Arc::new(Notify::new()),
            task: Mutex::new(None),
            registry,
            jobs,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn wake(&self) {
        self.wake.notify_waiters();
    }

    /// Shared wake callback for Bridge `POST /v1/handoff/ready` (monitor interrupt only).
    pub fn wake_fn(&self) -> Arc<dyn Fn() + Send + Sync> {
        let wake = Arc::clone(&self.wake);
        Arc::new(move || {
            wake.notify_waiters();
        })
    }

    pub fn start<F>(&self, get_setting: F) -> Result<bool, String>
    where
        F: Fn(&str) -> String + Send + Sync + 'static,
    {
        let mut task_guard = self.task.lock().map_err(|e| e.to_string())?;
        if self.running.load(Ordering::SeqCst) {
            logging::log_warn("Monitor-Thread läuft bereits.");
            return Ok(false);
        }

        let monitor_path = get_setting("monitor_path");
        if monitor_path.trim().is_empty() {
            logging::log_error("Monitoring nicht gestartet: Kein Überwachungsordner konfiguriert.");
            return Err("Kein Überwachungsordner konfiguriert.".into());
        }

        if let Some(previous) = task_guard.take() {
            previous.abort();
        }

        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        let wake = Arc::clone(&self.wake);
        let registry = Arc::clone(&self.registry);
        let jobs = self.jobs.clone();

        logging::log_info("Starte Monitor...");
        let handle = tauri::async_runtime::spawn(async move {
            run_loop(running, wake, registry, jobs, get_setting).await;
        });
        *task_guard = Some(handle);
        Ok(true)
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.wake.notify_waiters();
        let handle = self.task.lock().ok().and_then(|mut g| g.take());
        if let Some(handle) = handle {
            let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        }
        logging::log_info("Monitor-Thread gestoppt.");
    }
}

impl Drop for MonitorState {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.wake.notify_waiters();
        if let Ok(mut guard) = self.task.lock() {
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }
    }
}

async fn run_loop<F>(
    running: Arc<AtomicBool>,
    wake: Arc<Notify>,
    registry: Arc<UploadQueueRegistry>,
    jobs: UnboundedSender<UploadJob>,
    get_setting: F,
) where
    F: Fn(&str) -> String + Send + Sync,
{
    logging::log_info("Monitor-Thread gestartet.");
    let mut tracker = FolderStabilityTracker::new(DEFAULT_STABILITY_SECS);
    let mut recovered = false;

    while running.load(Ordering::SeqCst) {
        let scan_path = get_setting("monitor_path");
        let scan_interval = parse_scan_interval(&get_setting("scan_interval"));
        let stability_enabled = parse_stability_enabled(&get_setting("folder_stability_enabled"));
        let stability_seconds = parse_stability_seconds(&get_setting("folder_stability_seconds"));
        let manifest_required = parse_manifest_required(&get_setting("manifest_required"));
        let selected_cloud = get_setting("selected_cloud_service");
        let archive_path = get_setting("archive_path");

        if scan_path.trim().is_empty() {
            logging::log_info("Kein Überwachungsordner konfiguriert. Pausiere.");
            wait_interruptible(&running, &wake, Duration::from_secs(MISSING_PATH_WAIT_SECS)).await;
            continue;
        }

        let scan_path = Path::new(scan_path.trim());
        if !scan_path.is_dir() {
            logging::log_warn(&format!(
                "Überwachungsordner '{}' existiert nicht. Pausiere.",
                scan_path.display()
            ));
            wait_interruptible(&running, &wake, Duration::from_secs(MISSING_PATH_WAIT_SECS)).await;
            continue;
        }

        if stability_enabled {
            tracker.set_required_seconds(stability_seconds);
        } else {
            tracker.set_required_seconds(0.0);
        }

        let ctx = EnqueueContext {
            registry: &registry,
            jobs: &jobs,
            selected_cloud: selected_cloud.trim(),
            archive_path: archive_path.trim(),
            customer_lookup: None,
            manifest_required,
        };

        if !recovered {
            recover_stalled_folders(scan_path, &ctx).await;
            recovered = true;
        }

        logging::log_debug(&format!("Scanne Verzeichnis: {}", scan_path.display()));
        scan_once(scan_path, &mut tracker, stability_enabled, &ctx).await;

        if running.load(Ordering::SeqCst) {
            logging::log_debug(&format!("Scan beendet. Warte {scan_interval} Sekunden."));
            wait_interruptible(&running, &wake, Duration::from_secs(scan_interval)).await;
        }
    }

    tracker.clear();
    logging::log_info("Monitor-Thread beendet.");
}

async fn wait_interruptible(running: &AtomicBool, wake: &Notify, duration: Duration) {
    let notified = wake.notified();
    tokio::pin!(notified);
    tokio::select! {
        _ = tokio::time::sleep(duration) => {}
        _ = &mut notified => {}
        _ = wait_until_stopped(running) => {}
    }
}

async fn wait_until_stopped(running: &AtomicBool) {
    while running.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub async fn scan_once(
    scan_path: &Path,
    tracker: &mut FolderStabilityTracker,
    stability_enabled: bool,
    ctx: &EnqueueContext<'_>,
) -> usize {
    let entries = match fs::read_dir(scan_path) {
        Ok(rd) => rd,
        Err(e) => {
            logging::log_error(&format!(
                "Überwachungsordner '{}' wurde gelöscht oder ist nicht lesbar: {e}",
                scan_path.display()
            ));
            return 0;
        }
    };

    let mut found = 0usize;
    for entry in entries.flatten() {
        let full_dir_path = entry.path();
        if !full_dir_path.is_dir() {
            continue;
        }

        let dir_name = entry.file_name().to_string_lossy().into_owned();
        if is_handoff_scan_dir(&dir_name) {
            continue;
        }
        let (fertig_path, processing_path) = marker_paths(&full_dir_path);
        if !fertig_path.is_file() && !processing_path.is_file() {
            continue;
        }

        if processing_path.is_file() {
            tracker.discard(&full_dir_path);
        } else if fertig_path.is_file() && stability_enabled {
            // Valid handoff manifest replaces the stability wait (P1).
            // Incomplete/invalid manifests also skip the wait so the gate can reject promptly.
            let has_manifest = manifest_path(&full_dir_path).is_file();
            if !has_manifest {
                match tracker.observe(&full_dir_path) {
                    ObserveResult::Stable => {}
                    ObserveResult::Waiting | ObserveResult::Removed => continue,
                }
            } else {
                tracker.discard(&full_dir_path);
            }
        }

        if fertig_path.is_file() && !processing_path.is_file() {
            logging::log_info(&format!("Neues Verzeichnis gefunden: {dir_name}"));
        }

        match try_claim_and_enqueue(&full_dir_path, ctx).await {
            ClaimResult::Queued => found += 1,
            ClaimResult::AlreadyProcessing
            | ClaimResult::NoFertigMarker
            | ClaimResult::NoMedia
            | ClaimResult::AlreadyClaimed
            | ClaimResult::NotAFolder
            | ClaimResult::CustomerLookupFailed
            | ClaimResult::ManifestRejected { .. } => {}
            ClaimResult::MarkerError(msg) | ClaimResult::IoError(msg) => {
                logging::log_error(&format!("Fehler bei Verarbeitung von '{dir_name}': {msg}"));
            }
        }
    }
    found
}

pub async fn try_claim_and_enqueue(folder: &Path, ctx: &EnqueueContext<'_>) -> ClaimResult {
    if !folder.is_dir() {
        return ClaimResult::NotAFolder;
    }

    let dir_name = folder.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let (fertig_path, processing_path) = marker_paths(folder);

    if processing_path.is_file() {
        discard_stale_fertig_marker(folder);
        return ClaimResult::AlreadyProcessing;
    }
    if !fertig_path.is_file() {
        return ClaimResult::NoFertigMarker;
    }
    if !has_uploadable_files(folder) {
        logging::log_debug(&format!(
            "'{dir_name}': Marker vorhanden, aber keine Medien-Dateien — warte."
        ));
        return ClaimResult::NoMedia;
    }

    let handoff_cid: Option<String> = match evaluate_manifest_gate(folder, ctx.manifest_required) {
        GateDecision::Legacy => {
            logging::log_debug(&format!(
                "'{dir_name}': kein Manifest ({CODE_MANIFEST_MISSING_LEGACY}) — Legacy-Claim."
            ));
            None
        }
        GateDecision::Ready { correlation_id } => {
            logging::log_info(&format!(
                "'{dir_name}': Manifest OK (correlation_id={correlation_id}) — Claim ohne Stability-Wait."
            ));
            write_job_outbox(
                folder,
                Some(&correlation_id),
                OutboxState::Accepted,
                None,
                None,
            );
            Some(correlation_id)
        }
        GateDecision::Rejected {
            code,
            message,
            correlation_id,
        } => {
            logging::log_warn(&format!(
                "'{dir_name}': Manifest-Gate abgelehnt ({code}): {message}"
            ));
            if let Some(ref cid) = correlation_id {
                write_job_outbox(
                    folder,
                    Some(cid),
                    OutboxState::Rejected,
                    Some(OutboxError {
                        code: code.to_string(),
                        message: message.clone(),
                    }),
                    None,
                );
            }
            return ClaimResult::ManifestRejected {
                code: code.to_string(),
                message,
            };
        }
    };

    if !ctx.registry.register(folder) {
        logging::log_debug(&format!(
            "'{dir_name}' bereits in Upload-Warteschlange vorgemerkt."
        ));
        return ClaimResult::AlreadyClaimed;
    }

    let marker_raw = match read_marker_file(&fertig_path) {
        Ok(raw) => raw,
        Err(e) => {
            ctx.registry.unregister(Some(folder));
            return ClaimResult::IoError(e.to_string());
        }
    };
    logging::log_debug(&format!("Marker-Daten für '{dir_name}': {marker_raw}"));

    let kunde = match resolve_kunde_from_marker(&marker_raw) {
        Ok(k) => k,
        Err(MarkerError::ApiLookupRequired) => {
            match lookup_kunde_from_api_marker(&marker_raw, ctx).await {
                Ok(k) => k,
                Err(msg) => {
                    ctx.registry.unregister(Some(folder));
                    logging::log_error(&format!(
                        "Customer-Lookup für '{dir_name}' fehlgeschlagen: {msg}"
                    ));
                    write_job_outbox(
                        folder,
                        handoff_cid.as_deref(),
                        OutboxState::Failed,
                        Some(OutboxError {
                            code: CODE_CUSTOMER_LOOKUP_FAILED.into(),
                            message: msg.clone(),
                        }),
                        Some(archive::ARCHIVE_ERROR),
                    );
                    handle_customer_lookup_failure(
                        ctx.archive_path,
                        folder,
                        &msg,
                        Some(&marker_raw),
                    );
                    return ClaimResult::CustomerLookupFailed;
                }
            }
        }
        Err(e) => {
            ctx.registry.unregister(Some(folder));
            if is_marker_format_failure(&e) {
                logging::log_error(&format!(
                    "Ungültiger Marker für '{dir_name}', verschiebe nach Archiv/fehler: {e}"
                ));
                write_job_outbox(
                    folder,
                    handoff_cid.as_deref(),
                    OutboxState::Failed,
                    Some(OutboxError {
                        code: CODE_MARKER_INVALID.into(),
                        message: e.to_string(),
                    }),
                    Some(archive::ARCHIVE_ERROR),
                );
                archive::handle_marker_failure(ctx.archive_path, folder, &e.to_string(), Some(&marker_raw));
            }
            return ClaimResult::MarkerError(e.to_string());
        }
    };

    let use_dropbox = should_use_dropbox_client_for_marker(ctx.selected_cloud, &marker_raw)
        .unwrap_or(false);
    if use_dropbox {
        logging::log_info(&format!(
            "Reiner Kontakt-Marker für '{dir_name}' — Upload über DropboxClient (Custom API aktiv)."
        ));
    }

    logging::log_info(&format!(
        "Kundendaten erfolgreich geladen für '{dir_name}': {}",
        kunde_label(&kunde)
    ));
    emit_marker_history(dir_name, &marker_raw, &kunde);

    if let Err(e) = claim_fertig_marker(folder) {
        ctx.registry.unregister(Some(folder));
        return ClaimResult::IoError(format!(
            "Marker-Umbenennung fehlgeschlagen ({} -> {}): {e}",
            fertig_path.display(),
            processing_path.display()
        ));
    }

    let job = UploadJob {
        dir_path: folder.to_path_buf(),
        kunde,
        use_dropbox_client: use_dropbox,
        correlation_id: handoff_cid.clone(),
    };
    if ctx.registry.enqueue(ctx.jobs, job, true) {
        logging::log_info(&format!("'{dir_name}' zur Upload-Warteschlange hinzugefügt."));
        write_job_outbox(
            folder,
            handoff_cid.as_deref(),
            OutboxState::Queued,
            None,
            None,
        );
        ClaimResult::Queued
    } else {
        ctx.registry.unregister(Some(folder));
        ClaimResult::AlreadyClaimed
    }
}

async fn recover_stalled_folders(scan_path: &Path, ctx: &EnqueueContext<'_>) -> usize {
    let names = match fs::read_dir(scan_path) {
        Ok(rd) => rd,
        Err(e) => {
            logging::log_warn(&format!("Recovery: Konnte Überwachungsordner nicht lesen: {e}"));
            return 0;
        }
    };

    let mut recovered = 0usize;
    for entry in names.flatten() {
        let full_dir_path = entry.path();
        if !full_dir_path.is_dir() {
            continue;
        }
        let dir_name = entry.file_name().to_string_lossy().into_owned();
        if is_handoff_scan_dir(&dir_name) {
            continue;
        }
        let (_, processing_path) = marker_paths(&full_dir_path);
        if !processing_path.is_file() {
            continue;
        }
        discard_stale_fertig_marker(&full_dir_path);
        if !has_uploadable_files(&full_dir_path) {
            logging::log_error(&format!(
                "Recovery: '{dir_name}' ohne Medien-Dateien, verschiebe nach Archiv/fehler."
            ));
            archive::handle_marker_failure(
                ctx.archive_path,
                &full_dir_path,
                "Keine Medien-Dateien im Ordner gefunden.",
                read_marker_raw(&full_dir_path).as_deref(),
            );
            continue;
        }

        let marker_raw = match read_marker_file(&processing_path) {
            Ok(raw) => raw,
            Err(e) => {
                logging::log_error(&format!(
                    "Recovery: Verzeichnis '{dir_name}' konnte nicht wiederaufgenommen werden: {e}"
                ));
                continue;
            }
        };

        let kunde = match resolve_kunde_from_marker(&marker_raw) {
            Ok(k) => k,
            Err(MarkerError::ApiLookupRequired) => {
                match lookup_kunde_from_api_marker(&marker_raw, ctx).await {
                    Ok(k) => k,
                    Err(msg) => {
                        logging::log_error(&format!(
                            "Recovery: Customer-Lookup für '{dir_name}' fehlgeschlagen: {msg}"
                        ));
                        handle_customer_lookup_failure(
                            ctx.archive_path,
                            &full_dir_path,
                            &msg,
                            Some(&marker_raw),
                        );
                        continue;
                    }
                }
            }
            Err(e) => {
                logging::log_error(&format!(
                    "Recovery: Verzeichnis '{dir_name}' konnte nicht wiederaufgenommen werden: {e}"
                ));
                if is_marker_format_failure(&e) {
                    archive::handle_marker_failure(
                        ctx.archive_path,
                        &full_dir_path,
                        &e.to_string(),
                        Some(&marker_raw),
                    );
                }
                continue;
            }
        };

        let use_dropbox =
            should_use_dropbox_client_for_marker(ctx.selected_cloud, &marker_raw).unwrap_or(false);
        logging::log_info(&format!(
            "Recovery: unterbrochener Auftrag '{dir_name}', Kundendaten geladen."
        ));
        emit_marker_history(&dir_name, &marker_raw, &kunde);
        let job = UploadJob {
            dir_path: full_dir_path.clone(),
            kunde,
            use_dropbox_client: use_dropbox,
            correlation_id: peek_correlation_id(&full_dir_path),
        };
        if ctx.registry.enqueue(ctx.jobs, job, false) {
            recovered += 1;
            logging::log_info(&format!(
                "Recovery: '{dir_name}' erneut in Upload-Warteschlange gelegt."
            ));
        } else {
            logging::log_debug(&format!(
                "Recovery: '{dir_name}' bereits vorgemerkt, überspringe."
            ));
        }
    }

    if recovered > 0 {
        logging::log_info(&format!(
            "Recovery: {recovered} unterbrochene Aufträge erneut in die Queue gelegt."
        ));
    }
    recovered
}

async fn lookup_kunde_from_api_marker(
    marker_raw: &str,
    ctx: &EnqueueContext<'_>,
) -> Result<Kunde, String> {
    let data = load_marker_data(marker_raw).map_err(|e| e.to_string())?;
    let (query, mode) = parse_api_marker_data(&data).map_err(|e| e.to_string())?;
    if let Some(lookup) = ctx.customer_lookup {
        return lookup(&query, mode);
    }
    fetch_customer_as_kunde(&query, mode).await
}

fn emit_marker_history(dir_name: &str, marker_raw: &str, kunde: &Kunde) {
    crate::events::emit(
        crate::events::UPLOAD_HISTORY_UPDATE,
        serde_json::json!({
            "dir_name": dir_name,
            "marker_raw": marker_raw,
            "first_name": kunde.first_name.clone().unwrap_or_default(),
            "last_name": kunde.last_name.clone().unwrap_or_default(),
            "email": kunde.email.clone().unwrap_or_default(),
            "phone": kunde.phone.clone().unwrap_or_default(),
            "customer_number": kunde.customer_number.clone().unwrap_or_default(),
            "booking_number": kunde.booking_number.clone().unwrap_or_default(),
            "type": kunde.customer_type.clone().unwrap_or_default(),
        }),
    );
}

fn kunde_label(kunde: &Kunde) -> String {
    let first = kunde.first_name.as_deref().unwrap_or("");
    let last = kunde.last_name.as_deref().unwrap_or("");
    let email = kunde.email.as_deref().unwrap_or("");
    format!("{first} {last} <{email}>").trim().to_string()
}

fn parse_scan_interval(raw: &str) -> u64 {
    raw.trim()
        .parse::<u64>()
        .unwrap_or(DEFAULT_SCAN_INTERVAL_SECS)
        .max(1)
}

fn parse_stability_enabled(raw: &str) -> bool {
    raw.trim().to_ascii_lowercase() != "false"
}

fn parse_stability_seconds(raw: &str) -> f64 {
    raw.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(DEFAULT_STABILITY_SECS)
}

fn parse_manifest_required(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "on"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::marker::{write_fertig_marker, MARKER_FERTIG, MARKER_PROCESSING};
    use crate::util::archive::ARCHIVE_ERROR;
    use std::fs;
    use tempfile::tempdir;
    use tokio::sync::mpsc::unbounded_channel;

    fn contact_marker() -> &'static str {
        r#"{"vorname":"Anna","nachname":"Muster","email":"anna@example.de"}"#
    }

    fn api_marker() -> &'static str {
        r#"{"type":"Outside","kunden_id_hash":"abc","booking_id_hash":"def"}"#
    }

    fn write_media(dir: &Path) {
        fs::write(dir.join("photo.jpg"), b"jpeg-bytes").unwrap();
    }

    fn ctx<'a>(
        registry: &'a UploadQueueRegistry,
        jobs: &'a UnboundedSender<UploadJob>,
        cloud: &'a str,
        archive: &'a str,
    ) -> EnqueueContext<'a> {
        EnqueueContext {
            registry,
            jobs,
            selected_cloud: cloud,
            archive_path: archive,
            customer_lookup: None,
            manifest_required: false,
        }
    }

    fn mock_lookup(_query: &ApiMarkerQuery, _mode: LookupMode) -> Result<Kunde, String> {
        Ok(Kunde {
            first_name: Some("API".into()),
            last_name: Some("Kunde".into()),
            email: Some("api@example.de".into()),
            customer_number: Some("c1".into()),
            booking_number: Some("b2".into()),
            customer_type: Some("Outside".into()),
            ..Kunde::default()
        })
    }

    fn failing_lookup(_query: &ApiMarkerQuery, _mode: LookupMode) -> Result<Kunde, String> {
        Err("Customer-Lookup fehlgeschlagen: HTTP 404 - missing".into())
    }

    #[tokio::test]
    async fn claim_direct_contact_renames_marker_and_enqueues() {
        let dir = tempdir().unwrap();
        write_media(dir.path());
        write_fertig_marker(dir.path(), contact_marker()).unwrap();
        let registry = UploadQueueRegistry::new();
        let (tx, mut rx) = unbounded_channel();
        let context = ctx(&registry, &tx, "dropbox", "");

        assert_eq!(try_claim_and_enqueue(dir.path(), &context).await, ClaimResult::Queued);
        assert!(!dir.path().join(MARKER_FERTIG).is_file());
        assert!(dir.path().join(MARKER_PROCESSING).is_file());
        let job = rx.try_recv().unwrap();
        assert_eq!(job.kunde.first_name.as_deref(), Some("Anna"));
        assert_eq!(
            try_claim_and_enqueue(dir.path(), &context).await,
            ClaimResult::AlreadyProcessing
        );
    }

    #[tokio::test]
    async fn claim_api_lookup_success_enqueues() {
        let dir = tempdir().unwrap();
        write_media(dir.path());
        write_fertig_marker(dir.path(), api_marker()).unwrap();
        let registry = UploadQueueRegistry::new();
        let (tx, mut rx) = unbounded_channel();
        let mut context = ctx(&registry, &tx, "dropbox", "");
        context.customer_lookup = Some(mock_lookup);

        assert_eq!(try_claim_and_enqueue(dir.path(), &context).await, ClaimResult::Queued);
        assert!(dir.path().join(MARKER_PROCESSING).is_file());
        let job = rx.try_recv().unwrap();
        assert_eq!(job.kunde.first_name.as_deref(), Some("API"));
        assert_eq!(job.kunde.customer_number.as_deref(), Some("c1"));
        assert!(!job.use_dropbox_client);
    }

    #[tokio::test]
    async fn claim_api_lookup_failure_archives() {
        let root = tempdir().unwrap();
        let job = root.path().join("api-fail");
        let archive = root.path().join("archive");
        fs::create_dir(&job).unwrap();
        write_media(&job);
        write_fertig_marker(&job, api_marker()).unwrap();
        let registry = UploadQueueRegistry::new();
        let (tx, mut rx) = unbounded_channel();
        let archive_s = archive.to_string_lossy().into_owned();
        let mut context = ctx(&registry, &tx, "dropbox", &archive_s);
        context.customer_lookup = Some(failing_lookup);

        assert_eq!(
            try_claim_and_enqueue(&job, &context).await,
            ClaimResult::CustomerLookupFailed
        );
        assert!(!job.exists());
        assert!(archive.join(ARCHIVE_ERROR).join("api-fail").is_dir());
        assert!(rx.try_recv().is_err());
        assert!(!registry.is_registered(&job));
    }

    #[tokio::test]
    async fn claim_custom_api_extended_marker_enqueues() {
        let dir = tempdir().unwrap();
        write_media(dir.path());
        write_fertig_marker(
            dir.path(),
            r#"{"vorname":"Anna","nachname":"Muster","email":"anna@example.de","type":"Outside","handcam_foto":true}"#,
        )
        .unwrap();
        let registry = UploadQueueRegistry::new();
        let (tx, mut rx) = unbounded_channel();
        let context = ctx(&registry, &tx, "custom_api", "");
        assert_eq!(try_claim_and_enqueue(dir.path(), &context).await, ClaimResult::Queued);
        let job = rx.try_recv().unwrap();
        assert!(!job.use_dropbox_client);
    }

    #[tokio::test]
    async fn claim_pure_contact_uses_dropbox_when_custom_api_selected() {
        let dir = tempdir().unwrap();
        write_media(dir.path());
        write_fertig_marker(dir.path(), contact_marker()).unwrap();
        let registry = UploadQueueRegistry::new();
        let (tx, mut rx) = unbounded_channel();
        let context = ctx(&registry, &tx, "custom_api", "");
        assert_eq!(try_claim_and_enqueue(dir.path(), &context).await, ClaimResult::Queued);
        let job = rx.try_recv().unwrap();
        assert!(job.use_dropbox_client);
    }

    #[tokio::test]
    async fn claim_waits_when_no_media() {
        let dir = tempdir().unwrap();
        write_fertig_marker(dir.path(), contact_marker()).unwrap();
        let registry = UploadQueueRegistry::new();
        let (tx, _rx) = unbounded_channel();
        let context = ctx(&registry, &tx, "dropbox", "");
        assert_eq!(try_claim_and_enqueue(dir.path(), &context).await, ClaimResult::NoMedia);
        assert!(dir.path().join(MARKER_FERTIG).is_file());
        assert!(!registry.is_registered(dir.path()));
    }

    #[tokio::test]
    async fn invalid_marker_archives_to_fehler() {
        let root = tempdir().unwrap();
        let job = root.path().join("bad");
        let archive = root.path().join("archive");
        fs::create_dir(&job).unwrap();
        write_media(&job);
        write_fertig_marker(&job, r#"{"foo":"bar"}"#).unwrap();
        let registry = UploadQueueRegistry::new();
        let (tx, _rx) = unbounded_channel();
        let archive_s = archive.to_string_lossy().into_owned();
        let context = ctx(&registry, &tx, "dropbox", &archive_s);
        assert!(matches!(
            try_claim_and_enqueue(&job, &context).await,
            ClaimResult::MarkerError(_)
        ));
        assert!(!job.exists());
        assert!(archive.join(ARCHIVE_ERROR).join("bad").is_dir());
    }

    #[tokio::test]
    async fn scan_once_respects_stability_wait() {
        let root = tempdir().unwrap();
        let job = root.path().join("job1");
        fs::create_dir(&job).unwrap();
        write_media(&job);
        write_fertig_marker(&job, contact_marker()).unwrap();

        let mut tracker = FolderStabilityTracker::new(30.0);
        let registry = UploadQueueRegistry::new();
        let (tx, _rx) = unbounded_channel();
        let context = ctx(&registry, &tx, "dropbox", "");
        let queued = scan_once(root.path(), &mut tracker, true, &context).await;
        assert_eq!(queued, 0);
        assert!(job.join(MARKER_FERTIG).is_file());

        tracker.set_required_seconds(0.0);
        tracker.clear();
        let queued = scan_once(root.path(), &mut tracker, true, &context).await;
        assert_eq!(queued, 1);
        assert!(job.join(MARKER_PROCESSING).is_file());
    }

    #[tokio::test]
    async fn scan_once_without_stability_claims_immediately() {
        let root = tempdir().unwrap();
        let job = root.path().join("job2");
        fs::create_dir(&job).unwrap();
        write_media(&job);
        write_fertig_marker(&job, contact_marker()).unwrap();

        let mut tracker = FolderStabilityTracker::new(30.0);
        let registry = UploadQueueRegistry::new();
        let (tx, _rx) = unbounded_channel();
        let context = ctx(&registry, &tx, "dropbox", "");
        let queued = scan_once(root.path(), &mut tracker, false, &context).await;
        assert_eq!(queued, 1);
        assert!(job.join(MARKER_PROCESSING).is_file());
    }

    fn write_valid_manifest(dir: &Path, size: u64) {
        use crate::model::handoff::{
            HandoffManifestV1, IntegrityBlock, ManifestFileEntry, MarkerHint, ProducerInfo,
            ProducerRef, INTEGRITY_ALGO_SIZE, MANIFEST_FILENAME, PROTOCOL_NAME,
        };
        let m = HandoffManifestV1 {
            schema: 1,
            protocol: PROTOCOL_NAME.into(),
            correlation_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into(),
            producer: ProducerInfo {
                app: "AeroTandemStudio".into(),
                version: "0.0.0".into(),
            },
            producer_ref: ProducerRef::default(),
            created_at: "2026-08-15T00:00:00+02:00".into(),
            folder_name: "job".into(),
            integrity: IntegrityBlock {
                algo: INTEGRITY_ALGO_SIZE.into(),
                files: vec![ManifestFileEntry {
                    path: "photo.jpg".into(),
                    size,
                }],
            },
            marker_hint: MarkerHint::default(),
            extensions: serde_json::json!({}),
        };
        fs::write(
            dir.join(MANIFEST_FILENAME),
            serde_json::to_string_pretty(&m).unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn claim_with_valid_manifest_skips_legacy_path() {
        let dir = tempdir().unwrap();
        write_media(dir.path());
        write_fertig_marker(dir.path(), contact_marker()).unwrap();
        write_valid_manifest(dir.path(), 10);
        let registry = UploadQueueRegistry::new();
        let (tx, mut rx) = unbounded_channel();
        let context = ctx(&registry, &tx, "dropbox", "");
        assert_eq!(
            try_claim_and_enqueue(dir.path(), &context).await,
            ClaimResult::Queued
        );
        assert!(rx.try_recv().is_ok());
        assert!(dir.path().join(MARKER_PROCESSING).is_file());
    }

    #[tokio::test]
    async fn claim_rejects_size_mismatch_without_claiming() {
        let root = tempdir().unwrap();
        let job = root.path().join("job-bad");
        fs::create_dir(&job).unwrap();
        write_media(&job);
        write_fertig_marker(&job, contact_marker()).unwrap();
        write_valid_manifest(&job, 999);
        let registry = UploadQueueRegistry::new();
        let (tx, mut rx) = unbounded_channel();
        let context = ctx(&registry, &tx, "dropbox", "");
        match try_claim_and_enqueue(&job, &context).await {
            ClaimResult::ManifestRejected { code, .. } => {
                assert_eq!(code, crate::model::handoff::CODE_SIZE_MISMATCH);
            }
            other => panic!("expected ManifestRejected, got {other:?}"),
        }
        assert!(job.join(MARKER_FERTIG).is_file());
        assert!(!job.join(MARKER_PROCESSING).is_file());
        assert!(rx.try_recv().is_err());
        assert!(!registry.is_registered(&job));

        let outbox = crate::model::handoff::read_status_outbox(
            root.path(),
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        )
        .unwrap();
        assert_eq!(outbox.state, crate::model::handoff::OutboxState::Rejected);
        assert_eq!(
            outbox.error.as_ref().unwrap().code,
            crate::model::handoff::CODE_SIZE_MISMATCH
        );
    }

    #[tokio::test]
    async fn claim_with_valid_manifest_writes_queued_outbox() {
        let root = tempdir().unwrap();
        let job = root.path().join("job-ok");
        fs::create_dir(&job).unwrap();
        write_media(&job);
        write_fertig_marker(&job, contact_marker()).unwrap();
        write_valid_manifest(&job, 10);
        let registry = UploadQueueRegistry::new();
        let (tx, mut rx) = unbounded_channel();
        let context = ctx(&registry, &tx, "dropbox", "");
        assert_eq!(
            try_claim_and_enqueue(&job, &context).await,
            ClaimResult::Queued
        );
        let queued = rx.try_recv().unwrap();
        assert_eq!(
            queued.correlation_id.as_deref(),
            Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
        );
        let outbox = crate::model::handoff::read_status_outbox(
            root.path(),
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        )
        .unwrap();
        assert_eq!(outbox.state, crate::model::handoff::OutboxState::Queued);
    }

    #[tokio::test]
    async fn scan_once_ignores_ams_handoff_dir() {
        use crate::model::handoff::HANDOFF_DIRNAME;
        let root = tempdir().unwrap();
        let handoff = root.path().join(HANDOFF_DIRNAME);
        fs::create_dir(&handoff).unwrap();
        write_media(&handoff);
        write_fertig_marker(&handoff, contact_marker()).unwrap();

        let mut tracker = FolderStabilityTracker::new(0.0);
        let registry = UploadQueueRegistry::new();
        let (tx, _rx) = unbounded_channel();
        let context = ctx(&registry, &tx, "dropbox", "");
        let queued = scan_once(root.path(), &mut tracker, false, &context).await;
        assert_eq!(queued, 0);
        assert!(handoff.join(MARKER_FERTIG).is_file());
    }

    #[tokio::test]
    async fn scan_once_with_valid_manifest_skips_stability_wait() {
        let root = tempdir().unwrap();
        let job = root.path().join("job-m");
        fs::create_dir(&job).unwrap();
        write_media(&job);
        write_fertig_marker(&job, contact_marker()).unwrap();
        write_valid_manifest(&job, 10);

        let mut tracker = FolderStabilityTracker::new(30.0);
        let registry = UploadQueueRegistry::new();
        let (tx, _rx) = unbounded_channel();
        let context = ctx(&registry, &tx, "dropbox", "");
        let queued = scan_once(root.path(), &mut tracker, true, &context).await;
        assert_eq!(queued, 1);
        assert!(job.join(MARKER_PROCESSING).is_file());
    }

    #[test]
    fn parse_helpers_match_legacy() {
        assert_eq!(parse_scan_interval("15"), 15);
        assert_eq!(parse_scan_interval("nope"), 10);
        assert_eq!(parse_scan_interval("0"), 1);
        assert!(parse_stability_enabled("true"));
        assert!(parse_stability_enabled("0"));
        assert!(!parse_stability_enabled("false"));
        assert!(!parse_stability_enabled("FALSE"));
        assert_eq!(parse_stability_seconds("20"), 20.0);
        assert_eq!(parse_stability_seconds("x"), 15.0);
        assert!(!parse_manifest_required("false"));
        assert!(!parse_manifest_required(""));
        assert!(parse_manifest_required("true"));
        assert!(parse_manifest_required("1"));
    }
}
