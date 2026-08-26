//! Folder monitor: scan interval, marker claim, enqueue into the upload pipeline.
//! Port of legacy `core/monitor.py`.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Notify;

use crate::cloud::binding::{
    binding_from_append_fields, freeze_active_binding, merge_binding_into_history,
    pool_for_new_job, resolve_binding_for_history, DropboxAccountBinding,
};
use crate::cloud::custom_api::fetch_customer_as_kunde;
use crate::cloud::DropboxPool;
use crate::events;
use crate::model::handoff::{
    evaluate_manifest_gate, is_handoff_scan_dir, manifest_path, peek_correlation_id,
    write_job_outbox, GateDecision, OutboxError, OutboxState, CODE_CUSTOMER_LOOKUP_FAILED,
    CODE_HANDOFF_FOLDER_MISSING, CODE_HANDOFF_NO_FERTIG, CODE_HANDOFF_NO_MEDIA,
    CODE_HANDOFF_TIMEOUT, CODE_MANIFEST_MISSING_LEGACY, CODE_MARKER_INVALID,
};
use crate::model::kunde::Kunde;
use crate::model::marker::{
    apply_media_flags_if_present, claim_fertig_marker, discard_stale_fertig_marker, load_marker_data,
    marker_paths, parse_api_marker_data, read_marker_file, read_marker_raw, resolve_kunde_from_marker,
    should_use_dropbox_client_for_marker, ApiMarkerQuery, LookupMode, MarkerError,
};
use crate::monitor::stability::{
    folder_key, has_uploadable_files, FolderStabilityTracker, ObserveResult, StabilityPendingItem,
    HANDOFF_PHASE_REJECTED, HANDOFF_PHASE_SIGNALED, HANDOFF_PHASE_WAITING_FERTIG,
    HANDOFF_PHASE_WAITING_FOLDER, HANDOFF_PHASE_WAITING_MEDIA, PENDING_KIND_HANDOFF,
};
use crate::storage::dropbox_accounts::DropboxAccountStore;
use crate::storage::history::HistoryStore;
use crate::storage::logging;
use crate::upload::append::{
    build_append_parent_history_update, resolve_claimed_append_target, APPEND_EVENT_QUEUED,
};
use crate::upload::registry::{AppendTarget, UploadJob, UploadQueueRegistry};
use crate::util::archive::{self, handle_customer_lookup_failure, is_marker_format_failure};

const MISSING_PATH_WAIT_SECS: u64 = 60;
const DEFAULT_SCAN_INTERVAL_SECS: u64 = 10;
const DEFAULT_STABILITY_SECS: f64 = 15.0;
/// Drop a Bridge `handoff/ready` row only if the folder never appeared on the share.
const HANDOFF_MISSING_FOLDER_TTL: Duration = Duration::from_secs(600);
/// After this duration without a successful claim, write a rejected outbox for ATS.
const HANDOFF_CLAIM_TIMEOUT: Duration = Duration::from_secs(90);
/// Keep rejected handoff rows in the UI briefly after outbox was written.
const HANDOFF_REJECTED_UI_TTL: Duration = Duration::from_secs(120);
/// Re-run stalled-folder recovery every N monitor scans (~10s interval → ~60s).
const RECOVERY_EVERY_N_SCANS: u64 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimResult {
    NotAFolder,
    AlreadyProcessing,
    NoFertigMarker,
    NoMedia,
    AlreadyClaimed,
    CustomerLookupFailed,
    /// Manifest gate rejected the folder; leave it for the next scan (no claim).
    ManifestRejected {
        code: String,
        message: String,
    },
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
    pub active_dropbox_account_id: &'a str,
    pub active_custom_dropbox_account_id: &'a str,
}

#[derive(Debug, Clone)]
struct HandoffPendingEntry {
    dir_name: String,
    correlation_id: String,
    since: Instant,
    phase: String,
    error_code: String,
    error_message: String,
    outbox_written: bool,
    outbox_at: Option<Instant>,
}

pub struct MonitorState {
    running: Arc<AtomicBool>,
    wake: Arc<Notify>,
    /// Set when Bridge/UI requests a scan so a wake during `scan_once` is not lost.
    scan_requested: Arc<AtomicBool>,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    registry: Arc<UploadQueueRegistry>,
    jobs: UnboundedSender<UploadJob>,
    stability_pending: Arc<Mutex<Vec<StabilityPendingItem>>>,
    handoff_pending: Arc<Mutex<Vec<HandoffPendingEntry>>>,
}

fn incoming_snapshot(
    stability: &[StabilityPendingItem],
    handoff: &[HandoffPendingEntry],
    _now: Instant,
) -> Vec<StabilityPendingItem> {
    let mut items: Vec<StabilityPendingItem> = handoff
        .iter()
        .filter(|entry| {
            !stability
                .iter()
                .any(|s| s.dir_name.eq_ignore_ascii_case(&entry.dir_name))
        })
        .map(|entry| StabilityPendingItem {
            dir_name: entry.dir_name.clone(),
            remaining_seconds: 0.0,
            required_seconds: 0.0,
            waiting_for_media: entry.phase == HANDOFF_PHASE_WAITING_MEDIA,
            kind: PENDING_KIND_HANDOFF.to_string(),
            correlation_id: entry.correlation_id.clone(),
            handoff_phase: entry.phase.clone(),
            handoff_error_code: entry.error_code.clone(),
            handoff_error_message: entry.error_message.clone(),
        })
        .collect();
    items.extend(stability.iter().cloned());
    items.sort_by(|a, b| a.dir_name.cmp(&b.dir_name));
    items
}

fn publish_incoming(
    stability_store: &Mutex<Vec<StabilityPendingItem>>,
    handoff_store: &Mutex<Vec<HandoffPendingEntry>>,
    stability_items: Option<Vec<StabilityPendingItem>>,
) {
    if let Some(items) = stability_items {
        let mut guard = stability_store.lock().unwrap_or_else(|e| e.into_inner());
        *guard = items;
    }
    let stability = stability_store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let handoff = handoff_store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    events::emit(
        events::STABILITY_PENDING_CHANGED,
        incoming_snapshot(&stability, &handoff, Instant::now()),
    );
}

fn note_handoff_entry(
    store: &Mutex<Vec<HandoffPendingEntry>>,
    folder_name: &str,
    correlation_id: &str,
) {
    let dir_name = folder_name.trim();
    if dir_name.is_empty() {
        return;
    }
    let cid = correlation_id.trim().to_string();
    let mut guard = store.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = guard
        .iter_mut()
        .find(|e| e.dir_name.eq_ignore_ascii_case(dir_name))
    {
        existing.since = Instant::now();
        if !cid.is_empty() {
            existing.correlation_id = cid;
        }
        if existing.outbox_written {
            existing.outbox_written = false;
            existing.outbox_at = None;
            existing.phase = HANDOFF_PHASE_SIGNALED.to_string();
            existing.error_code.clear();
            existing.error_message.clear();
        }
        return;
    }
    guard.push(HandoffPendingEntry {
        dir_name: dir_name.to_string(),
        correlation_id: cid,
        since: Instant::now(),
        phase: HANDOFF_PHASE_SIGNALED.to_string(),
        error_code: String::new(),
        error_message: String::new(),
        outbox_written: false,
        outbox_at: None,
    });
}

fn remove_handoff_entries(
    store: &Mutex<Vec<HandoffPendingEntry>>,
    folder_name: &str,
    correlation_id: &str,
) {
    let dir = folder_name.trim();
    let cid = correlation_id.trim();
    let mut guard = store.lock().unwrap_or_else(|e| e.into_inner());
    guard.retain(|e| {
        let by_name = !dir.is_empty() && e.dir_name.eq_ignore_ascii_case(dir);
        let by_cid = !cid.is_empty() && e.correlation_id.eq_ignore_ascii_case(cid);
        !(by_name || by_cid)
    });
}

fn prune_handoff_entries(
    store: &Mutex<Vec<HandoffPendingEntry>>,
    scan_path: &Path,
    registry: &UploadQueueRegistry,
) {
    let now = Instant::now();
    let mut guard = store.lock().unwrap_or_else(|e| e.into_inner());
    guard.retain(|entry| {
        if entry.outbox_written {
            if let Some(at) = entry.outbox_at {
                return now.saturating_duration_since(at) < HANDOFF_REJECTED_UI_TTL;
            }
            return false;
        }
        let path = scan_path.join(&entry.dir_name);
        if registry.is_registered(&path) {
            return false;
        }
        let (_, processing_path) = marker_paths(&path);
        if processing_path.is_file() {
            return false;
        }
        if path.is_dir() {
            return true;
        }
        now.saturating_duration_since(entry.since) < HANDOFF_MISSING_FOLDER_TTL
    });
}

fn handoff_timeout_error(phase: &str) -> (&'static str, String) {
    match phase {
        HANDOFF_PHASE_WAITING_FOLDER | HANDOFF_PHASE_SIGNALED => (
            CODE_HANDOFF_FOLDER_MISSING,
            "Handoff-Ordner auf dem Share nicht sichtbar.".into(),
        ),
        HANDOFF_PHASE_WAITING_FERTIG => (
            CODE_HANDOFF_NO_FERTIG,
            "_fertig.txt fehlt — Ordner nicht bereit für AMS.".into(),
        ),
        HANDOFF_PHASE_WAITING_MEDIA => (
            CODE_HANDOFF_NO_MEDIA,
            "Keine Medien-Dateien im Handoff-Ordner.".into(),
        ),
        _ => (
            CODE_HANDOFF_TIMEOUT,
            "AMS konnte den Handoff-Auftrag nicht übernehmen (Timeout).".into(),
        ),
    }
}

fn write_handoff_rejection_outbox(
    folder: &Path,
    correlation_id: &str,
    code: &str,
    message: &str,
) {
    let cid = correlation_id.trim();
    if cid.is_empty() {
        return;
    }
    write_job_outbox(
        folder,
        Some(cid),
        OutboxState::Rejected,
        Some(OutboxError {
            code: code.to_string(),
            message: message.to_string(),
        }),
        None,
    );
}

impl MonitorState {
    pub fn new(registry: Arc<UploadQueueRegistry>, jobs: UnboundedSender<UploadJob>) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            wake: Arc::new(Notify::new()),
            scan_requested: Arc::new(AtomicBool::new(false)),
            task: Mutex::new(None),
            registry,
            jobs,
            stability_pending: Arc::new(Mutex::new(Vec::new())),
            handoff_pending: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn stability_snapshot(&self) -> Vec<StabilityPendingItem> {
        let stability = self
            .stability_pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let handoff = self
            .handoff_pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        incoming_snapshot(&stability, &handoff, Instant::now())
    }

    pub fn wake(&self) {
        self.scan_requested.store(true, Ordering::SeqCst);
        self.wake.notify_waiters();
    }

    /// Shared wake callback for Bridge `POST /v1/handoff/ready` (monitor interrupt only).
    /// `folder_name` / `correlation_id` may be empty; a non-empty folder is shown in the left panel.
    pub fn wake_fn(&self) -> Arc<dyn Fn(String, String) + Send + Sync> {
        let wake = Arc::clone(&self.wake);
        let scan_requested = Arc::clone(&self.scan_requested);
        let handoff_pending = Arc::clone(&self.handoff_pending);
        let stability_pending = Arc::clone(&self.stability_pending);
        Arc::new(move |folder_name: String, correlation_id: String| {
            note_handoff_entry(&handoff_pending, &folder_name, &correlation_id);
            publish_incoming(&stability_pending, &handoff_pending, None);
            scan_requested.store(true, Ordering::SeqCst);
            wake.notify_waiters();
        })
    }

    /// Shared cancel callback for Bridge `POST /v1/handoff/cancel` (ATS upload abort).
    pub fn cancel_fn(&self) -> Arc<dyn Fn(String, String) + Send + Sync> {
        let wake = Arc::clone(&self.wake);
        let scan_requested = Arc::clone(&self.scan_requested);
        let handoff_pending = Arc::clone(&self.handoff_pending);
        let stability_pending = Arc::clone(&self.stability_pending);
        Arc::new(move |folder_name: String, correlation_id: String| {
            remove_handoff_entries(&handoff_pending, &folder_name, &correlation_id);
            publish_incoming(&stability_pending, &handoff_pending, None);
            scan_requested.store(true, Ordering::SeqCst);
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
        let scan_requested = Arc::clone(&self.scan_requested);
        let registry = Arc::clone(&self.registry);
        let jobs = self.jobs.clone();
        let stability_pending = Arc::clone(&self.stability_pending);
        let handoff_pending = Arc::clone(&self.handoff_pending);

        logging::log_info("Starte Monitor...");
        let handle = tauri::async_runtime::spawn(async move {
            run_loop(
                running,
                wake,
                scan_requested,
                registry,
                jobs,
                stability_pending,
                handoff_pending,
                get_setting,
            )
            .await;
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
        {
            self.handoff_pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
        }
        publish_incoming(
            &self.stability_pending,
            &self.handoff_pending,
            Some(Vec::new()),
        );
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
    scan_requested: Arc<AtomicBool>,
    registry: Arc<UploadQueueRegistry>,
    jobs: UnboundedSender<UploadJob>,
    stability_pending: Arc<Mutex<Vec<StabilityPendingItem>>>,
    handoff_pending: Arc<Mutex<Vec<HandoffPendingEntry>>>,
    get_setting: F,
) where
    F: Fn(&str) -> String + Send + Sync,
{
    logging::log_info("Monitor-Thread gestartet.");
    let mut tracker = FolderStabilityTracker::new(DEFAULT_STABILITY_SECS);
    let mut recovered = false;
    let mut scan_counter: u64 = 0;

    while running.load(Ordering::SeqCst) {
        let scan_path = get_setting("monitor_path");
        let scan_interval = parse_scan_interval(&get_setting("scan_interval"));
        let stability_enabled = parse_stability_enabled(&get_setting("folder_stability_enabled"));
        let stability_seconds = parse_stability_seconds(&get_setting("folder_stability_seconds"));
        let manifest_required = parse_manifest_required(&get_setting("manifest_required"));
        let selected_cloud = get_setting("selected_cloud_service");
        let archive_path = get_setting("archive_path");
        let active_dropbox_account_id = get_setting("active_dropbox_account_id");
        let active_custom_dropbox_account_id = get_setting("active_custom_dropbox_account_id");

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
            active_dropbox_account_id: active_dropbox_account_id.trim(),
            active_custom_dropbox_account_id: active_custom_dropbox_account_id.trim(),
        };

        if !recovered {
            recover_stalled_folders(scan_path, &ctx).await;
            recovered = true;
        } else if scan_counter > 0 && scan_counter % RECOVERY_EVERY_N_SCANS == 0 {
            let n = recover_stalled_folders(scan_path, &ctx).await;
            if n > 0 {
                logging::log_info(&format!("Recovery: {n} unterbrochene Aufträge wiederaufgenommen."));
            }
        }
        scan_counter = scan_counter.saturating_add(1);

        logging::log_debug(&format!("Scanne Verzeichnis: {}", scan_path.display()));
        let handoff_found =
            process_handoff_pending(scan_path, &handoff_pending, &ctx).await;
        let dir_found = scan_once(scan_path, &mut tracker, stability_enabled, &ctx).await;
        if handoff_found + dir_found > 0 {
            logging::log_info(&format!(
                "Scan: {} Auftrag/Aufträge übernommen (Handoff: {handoff_found}, Ordner: {dir_found}).",
                handoff_found + dir_found
            ));
        }
        prune_handoff_entries(&handoff_pending, scan_path, &registry);
        publish_incoming(
            &stability_pending,
            &handoff_pending,
            Some(tracker.snapshot()),
        );

        if running.load(Ordering::SeqCst) && !scan_requested.swap(false, Ordering::SeqCst) {
            logging::log_debug(&format!("Scan beendet. Warte {scan_interval} Sekunden."));
            wait_interruptible(&running, &wake, Duration::from_secs(scan_interval)).await;
        }
    }

    tracker.clear();
    {
        handoff_pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
    publish_incoming(&stability_pending, &handoff_pending, Some(Vec::new()));
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

/// Claim Bridge-notified folders by explicit path; update UI phase + timeout outbox for ATS.
async fn process_handoff_pending(
    scan_path: &Path,
    handoff_pending: &Mutex<Vec<HandoffPendingEntry>>,
    ctx: &EnqueueContext<'_>,
) -> usize {
    let now = Instant::now();
    let mut entries = handoff_pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let mut found = 0usize;
    for entry in entries.iter_mut() {
        if entry.outbox_written {
            continue;
        }
        let dir_name = entry.dir_name.trim();
        if dir_name.is_empty() {
            continue;
        }
        let folder = scan_path.join(dir_name);

        if !folder.is_dir() {
            entry.phase = HANDOFF_PHASE_WAITING_FOLDER.to_string();
        } else {
            let (fertig_path, processing_path) = marker_paths(&folder);
            if processing_path.is_file() {
                continue;
            }
            if !fertig_path.is_file() {
                entry.phase = HANDOFF_PHASE_WAITING_FERTIG.to_string();
            } else if !has_uploadable_files(&folder) {
                entry.phase = HANDOFF_PHASE_WAITING_MEDIA.to_string();
            } else if entry.phase != HANDOFF_PHASE_REJECTED {
                entry.phase = HANDOFF_PHASE_SIGNALED.to_string();
            }
        }

        if now.saturating_duration_since(entry.since) >= HANDOFF_CLAIM_TIMEOUT {
            if !entry.correlation_id.trim().is_empty() {
                let (code, message) = handoff_timeout_error(&entry.phase);
                write_handoff_rejection_outbox(&folder, &entry.correlation_id, code, &message);
                logging::log_warn(&format!(
                    "Handoff '{dir_name}' Timeout ({code}): {message}"
                ));
                entry.phase = HANDOFF_PHASE_REJECTED.to_string();
                entry.error_code = code.to_string();
                entry.error_message = message;
                entry.outbox_written = true;
                entry.outbox_at = Some(now);
            }
            continue;
        }

        if !folder.is_dir() {
            continue;
        }

        match try_claim_and_enqueue(&folder, ctx).await {
            ClaimResult::Queued => {
                logging::log_info(&format!(
                    "Handoff-Ordner '{dir_name}' zur Upload-Warteschlange hinzugefügt."
                ));
                found += 1;
            }
            ClaimResult::NoFertigMarker => {
                entry.phase = HANDOFF_PHASE_WAITING_FERTIG.to_string();
            }
            ClaimResult::NoMedia => {
                entry.phase = HANDOFF_PHASE_WAITING_MEDIA.to_string();
            }
            ClaimResult::ManifestRejected { code, message } => {
                entry.phase = HANDOFF_PHASE_REJECTED.to_string();
                entry.error_code = code.clone();
                entry.error_message = message.clone();
                entry.outbox_written = true;
                entry.outbox_at = Some(now);
                logging::log_warn(&format!(
                    "Handoff '{dir_name}' abgelehnt ({code}): {message}"
                ));
            }
            ClaimResult::CustomerLookupFailed => {
                entry.phase = HANDOFF_PHASE_REJECTED.to_string();
                entry.error_code = CODE_CUSTOMER_LOOKUP_FAILED.to_string();
                entry.error_message = "Customer-Lookup fehlgeschlagen.".into();
                entry.outbox_written = true;
                entry.outbox_at = Some(now);
            }
            ClaimResult::MarkerError(msg) | ClaimResult::IoError(msg) => {
                entry.phase = HANDOFF_PHASE_REJECTED.to_string();
                entry.error_code = CODE_MARKER_INVALID.to_string();
                entry.error_message = msg.clone();
                entry.outbox_written = true;
                entry.outbox_at = Some(now);
                logging::log_error(&format!(
                    "Handoff '{dir_name}' konnte nicht übernommen werden: {msg}"
                ));
            }
            ClaimResult::AlreadyProcessing | ClaimResult::AlreadyClaimed | ClaimResult::NotAFolder => {
            }
        }
    }
    {
        let mut guard = handoff_pending.lock().unwrap_or_else(|e| e.into_inner());
        for updated in entries {
            if let Some(slot) = guard
                .iter_mut()
                .find(|e| e.dir_name.eq_ignore_ascii_case(&updated.dir_name))
            {
                *slot = updated;
            }
        }
    }
    found
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
    let mut keep = HashSet::new();
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
                    ObserveResult::Waiting => {
                        keep.insert(folder_key(&full_dir_path));
                        continue;
                    }
                    ObserveResult::Removed => continue,
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
    tracker.retain_keys(&keep);
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

    let mut append_target: Option<AppendTarget> = None;
    let handoff_cid: Option<String> = match evaluate_manifest_gate(folder, ctx.manifest_required) {
        GateDecision::Legacy => {
            logging::log_debug(&format!(
                "'{dir_name}': kein Manifest ({CODE_MANIFEST_MISSING_LEGACY}) — Legacy-Claim."
            ));
            None
        }
        GateDecision::Ready { correlation_id } => {
            match resolve_claimed_append_target(folder) {
                Ok(target) => {
                    if let Some(ref t) = target {
                        logging::log_info(&format!(
                            "'{dir_name}': Nachreichung an {} ({})",
                            t.parent_dir_name, t.remote_path
                        ));
                    } else {
                        logging::log_info(&format!(
                            "'{dir_name}': Manifest OK (correlation_id={correlation_id}) — Claim ohne Stability-Wait."
                        ));
                    }
                    append_target = target;
                    write_job_outbox(
                        folder,
                        Some(&correlation_id),
                        OutboxState::Accepted,
                        None,
                        None,
                    );
                    Some(correlation_id)
                }
                Err((code, message)) => {
                    logging::log_warn(&format!(
                        "'{dir_name}': Nachreichung abgelehnt ({code}): {message}"
                    ));
                    write_job_outbox(
                        folder,
                        Some(&correlation_id),
                        OutboxState::Rejected,
                        Some(OutboxError {
                            code: code.clone(),
                            message: message.clone(),
                        }),
                        None,
                    );
                    return ClaimResult::ManifestRejected { code, message };
                }
            }
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
                archive::handle_marker_failure(
                    ctx.archive_path,
                    folder,
                    &e.to_string(),
                    Some(&marker_raw),
                );
            }
            return ClaimResult::MarkerError(e.to_string());
        }
    };

    let use_dropbox =
        should_use_dropbox_client_for_marker(ctx.selected_cloud, &marker_raw).unwrap_or(false);
    if use_dropbox {
        logging::log_info(&format!(
            "Reiner Kontakt-Marker für '{dir_name}' — Upload über DropboxClient (Custom API aktiv)."
        ));
    }

    let dropbox_binding = match resolve_enqueue_binding(ctx, use_dropbox, append_target.as_ref()) {
        Ok(b) => b,
        Err(e) => {
            ctx.registry.unregister(Some(folder));
            logging::log_error(&format!(
                "Dropbox-Konto-Bindung für '{dir_name}' fehlgeschlagen: {e}"
            ));
            return ClaimResult::MarkerError(e);
        }
    };

    logging::log_info(&format!(
        "Kundendaten erfolgreich geladen für '{dir_name}': {}",
        kunde_label(&kunde)
    ));
    if let Some(ref append) = append_target {
        events::emit(
            events::UPLOAD_HISTORY_UPDATE,
            build_append_parent_history_update(
                append,
                dir_name,
                APPEND_EVENT_QUEUED,
                handoff_cid.as_deref(),
                Some(&kunde),
                Some(&marker_raw),
                None,
                None,
                None,
                None,
            ),
        );
    } else {
        emit_marker_history(dir_name, &marker_raw, &kunde, dropbox_binding.as_ref());
    }

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
        append: append_target,
        dropbox_binding,
    };
    if ctx.registry.enqueue(ctx.jobs, job, true) {
        logging::log_info(&format!(
            "'{dir_name}' zur Upload-Warteschlange hinzugefügt."
        ));
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
            logging::log_warn(&format!(
                "Recovery: Konnte Überwachungsordner nicht lesen: {e}"
            ));
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

        let append = match resolve_claimed_append_target(&full_dir_path) {
            Ok(t) => t,
            Err((code, msg)) => {
                logging::log_error(&format!(
                    "Recovery: Nachreichung '{dir_name}' abgelehnt ({code}): {msg}"
                ));
                write_job_outbox(
                    &full_dir_path,
                    peek_correlation_id(&full_dir_path).as_deref(),
                    OutboxState::Failed,
                    Some(OutboxError {
                        code,
                        message: msg.clone(),
                    }),
                    Some(archive::ARCHIVE_ERROR),
                );
                archive::handle_marker_failure(
                    ctx.archive_path,
                    &full_dir_path,
                    &msg,
                    Some(&marker_raw),
                );
                continue;
            }
        };
        let use_dropbox =
            should_use_dropbox_client_for_marker(ctx.selected_cloud, &marker_raw).unwrap_or(false);
        logging::log_info(&format!(
            "Recovery: unterbrochener Auftrag '{dir_name}', Kundendaten geladen."
        ));
        let dropbox_binding =
            match resolve_recovery_binding(ctx, &dir_name, use_dropbox, append.as_ref()) {
                Ok(b) => b,
                Err(e) => {
                    logging::log_error(&format!(
                        "Recovery: Dropbox-Konto-Bindung für '{dir_name}' fehlgeschlagen: {e}"
                    ));
                    continue;
                }
            };
        let correlation_id = peek_correlation_id(&full_dir_path);
        if let Some(ref append_target) = append {
            events::emit(
                events::UPLOAD_HISTORY_UPDATE,
                build_append_parent_history_update(
                    append_target,
                    &dir_name,
                    APPEND_EVENT_QUEUED,
                    correlation_id.as_deref(),
                    Some(&kunde),
                    Some(&marker_raw),
                    None,
                    None,
                    None,
                    None,
                ),
            );
        } else {
            emit_marker_history(&dir_name, &marker_raw, &kunde, dropbox_binding.as_ref());
        }
        let job = UploadJob {
            dir_path: full_dir_path.clone(),
            kunde,
            use_dropbox_client: use_dropbox,
            correlation_id,
            append,
            dropbox_binding,
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
    let mut kunde = if let Some(lookup) = ctx.customer_lookup {
        lookup(&query, mode)?
    } else {
        fetch_customer_as_kunde(&query, mode).await?
    };
    apply_media_flags_if_present(&mut kunde, &data);
    Ok(kunde)
}

fn emit_marker_history(
    dir_name: &str,
    marker_raw: &str,
    kunde: &Kunde,
    binding: Option<&DropboxAccountBinding>,
) {
    let mut payload = serde_json::json!({
        "dir_name": dir_name,
        "marker_raw": marker_raw,
        "first_name": kunde.first_name.clone().unwrap_or_default(),
        "last_name": kunde.last_name.clone().unwrap_or_default(),
        "email": kunde.email.clone().unwrap_or_default(),
        "phone": kunde.phone.clone().unwrap_or_default(),
        "customer_number": kunde.customer_number.clone().unwrap_or_default(),
        "booking_number": kunde.booking_number.clone().unwrap_or_default(),
        "type": kunde.customer_type.clone().unwrap_or_default(),
    });
    crate::model::marker::merge_kunde_media_flags(&mut payload, kunde);
    merge_binding_into_history(&mut payload, binding);
    crate::events::emit(crate::events::UPLOAD_HISTORY_UPDATE, payload);
}

/// Freeze active account for new jobs, or inherit parent binding for append.
fn resolve_enqueue_binding(
    ctx: &EnqueueContext<'_>,
    use_dropbox_client: bool,
    append: Option<&AppendTarget>,
) -> Result<Option<DropboxAccountBinding>, String> {
    let pool = pool_for_new_job(ctx.selected_cloud, use_dropbox_client);
    let accounts = DropboxAccountStore::open_default().map_err(|e| e.to_string())?;
    if let Some(target) = append {
        if let Some(binding) = binding_from_append_fields(
            target.dropbox_account_ams_id.as_deref(),
            target.dropbox_account_pool.as_deref(),
            target.dropbox_account_id.as_deref(),
            target.dropbox_account_email.as_deref(),
        ) {
            if binding.pool != pool {
                return Err(format!(
                    "Parent-Job ist an Pool „{}“ gebunden, aktueller Cloud-Pfad erwartet „{}“.",
                    binding.pool.as_str(),
                    pool.as_str()
                ));
            }
            return Ok(Some(binding));
        }
        // Legacy parent without binding: sole profile, empty pool → None, else error.
        let rows = accounts.list(pool).map_err(|e| e.to_string())?;
        return match rows.len() {
            0 => Ok(None),
            1 => Ok(Some(DropboxAccountBinding::from_row(&rows[0])?)),
            n => Err(format!(
                "Parent-Job ohne Konto-Bindung und {n} Profile im Pool „{}“.",
                pool.as_str()
            )),
        };
    }
    let active = match pool {
        DropboxPool::Native => ctx.active_dropbox_account_id,
        DropboxPool::CustomApi => ctx.active_custom_dropbox_account_id,
    };
    freeze_active_binding(pool, active, &accounts)
}

/// Prefer history binding on recovery so Soft-Active switches cannot rebind mid-flight.
fn resolve_recovery_binding(
    ctx: &EnqueueContext<'_>,
    dir_name: &str,
    use_dropbox_client: bool,
    append: Option<&AppendTarget>,
) -> Result<Option<DropboxAccountBinding>, String> {
    let pool = pool_for_new_job(ctx.selected_cloud, use_dropbox_client);
    if let Ok(store) = HistoryStore::open_default() {
        if let Ok(Some(entry)) = store.find_by_dir_name(dir_name) {
            let json = entry.to_json();
            let ams = json
                .get("dropbox_account_ams_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if !ams.is_empty() {
                let accounts = DropboxAccountStore::open_default().map_err(|e| e.to_string())?;
                return resolve_binding_for_history(&json, pool, &accounts).map(Some);
            }
        }
    }
    resolve_enqueue_binding(ctx, use_dropbox_client, append)
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

    fn sample_handoff(dir_name: &str, cid: &str) -> HandoffPendingEntry {
        HandoffPendingEntry {
            dir_name: dir_name.into(),
            correlation_id: cid.into(),
            since: Instant::now(),
            phase: HANDOFF_PHASE_SIGNALED.to_string(),
            error_code: String::new(),
            error_message: String::new(),
            outbox_written: false,
            outbox_at: None,
        }
    }

    #[test]
    fn incoming_snapshot_shows_handoff_until_stability_takes_over() {
        let handoff = vec![sample_handoff("JobA", "cid-1")];
        let merged = incoming_snapshot(&[], &handoff, Instant::now());
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].kind, PENDING_KIND_HANDOFF);
        assert_eq!(merged[0].correlation_id, "cid-1");

        let stability = vec![StabilityPendingItem {
            dir_name: "JobA".into(),
            remaining_seconds: 10.0,
            required_seconds: 15.0,
            waiting_for_media: false,
            kind: "stability".into(),
            correlation_id: String::new(),
            handoff_phase: String::new(),
            handoff_error_code: String::new(),
            handoff_error_message: String::new(),
        }];
        let merged = incoming_snapshot(&stability, &handoff, Instant::now());
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].kind, "stability");
    }

    #[test]
    fn prune_handoff_drops_registered_or_processing_folders() {
        let root = tempdir().unwrap();
        let job = root.path().join("JobB");
        fs::create_dir(&job).unwrap();
        fs::write(job.join(MARKER_PROCESSING), b"x").unwrap();
        let store = Mutex::new(vec![sample_handoff("JobB", "c")]);
        let registry = UploadQueueRegistry::new();
        prune_handoff_entries(&store, root.path(), &registry);
        assert!(store.lock().unwrap().is_empty());
    }

    #[test]
    fn handoff_timeout_error_maps_phases() {
        use crate::model::handoff::{
            CODE_HANDOFF_FOLDER_MISSING, CODE_HANDOFF_NO_FERTIG, CODE_HANDOFF_NO_MEDIA,
            CODE_HANDOFF_TIMEOUT,
        };

        assert_eq!(
            handoff_timeout_error(HANDOFF_PHASE_WAITING_FOLDER).0,
            CODE_HANDOFF_FOLDER_MISSING
        );
        assert_eq!(
            handoff_timeout_error(HANDOFF_PHASE_WAITING_FERTIG).0,
            CODE_HANDOFF_NO_FERTIG
        );
        assert_eq!(
            handoff_timeout_error(HANDOFF_PHASE_WAITING_MEDIA).0,
            CODE_HANDOFF_NO_MEDIA
        );
        assert_eq!(
            handoff_timeout_error("unknown").0,
            CODE_HANDOFF_TIMEOUT
        );
    }

    #[tokio::test]
    async fn handoff_timeout_writes_rejected_outbox() {
        let root = tempdir().unwrap();
        let share = root.path().join("aktuell");
        fs::create_dir_all(&share).unwrap();
        let mut entry = sample_handoff("JobTimeout", "dddddddd-dddd-dddd-dddd-dddddddddddd");
        entry.since = Instant::now()
            .checked_sub(HANDOFF_CLAIM_TIMEOUT + Duration::from_secs(5))
            .unwrap();
        entry.phase = HANDOFF_PHASE_WAITING_FOLDER.to_string();
        let store = Mutex::new(vec![entry]);
        let registry = UploadQueueRegistry::new();
        let (jobs_tx, _jobs_rx) = unbounded_channel();
        let ctx = ctx(&registry, &jobs_tx, "custom_api", root.path().to_str().unwrap());
        assert_eq!(process_handoff_pending(&share, &store, &ctx).await, 0);
        let outbox = share
            .join(".ams-handoff")
            .join("dddddddd-dddd-dddd-dddd-dddddddddddd.json");
        assert!(outbox.is_file(), "expected rejected outbox at {}", outbox.display());
        let guard = store.lock().unwrap();
        assert!(guard[0].outbox_written);
        assert_eq!(guard[0].phase, HANDOFF_PHASE_REJECTED);
    }

    fn ctx<'a>(
        registry: &'a UploadQueueRegistry,
        jobs: &'a UnboundedSender<UploadJob>,
        cloud: &'a str,
        archive: &'a str,
    ) -> EnqueueContext<'a> {
        ctx_with_active(registry, jobs, cloud, archive, "", "")
    }

    fn ctx_with_active<'a>(
        registry: &'a UploadQueueRegistry,
        jobs: &'a UnboundedSender<UploadJob>,
        cloud: &'a str,
        archive: &'a str,
        active_dropbox: &'a str,
        active_custom: &'a str,
    ) -> EnqueueContext<'a> {
        EnqueueContext {
            registry,
            jobs,
            selected_cloud: cloud,
            archive_path: archive,
            customer_lookup: None,
            manifest_required: false,
            active_dropbox_account_id: active_dropbox,
            active_custom_dropbox_account_id: active_custom,
        }
    }

    /// Prefer a real Custom-API profile so freeze_active_binding works on developer machines.
    fn first_custom_dropbox_ams_id() -> String {
        DropboxAccountStore::open_default()
            .ok()
            .and_then(|store| store.list(DropboxPool::CustomApi).ok())
            .and_then(|rows| rows.into_iter().next().map(|r| r.id))
            .unwrap_or_default()
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

        assert_eq!(
            try_claim_and_enqueue(dir.path(), &context).await,
            ClaimResult::Queued
        );
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

        assert_eq!(
            try_claim_and_enqueue(dir.path(), &context).await,
            ClaimResult::Queued
        );
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
        let active_custom = first_custom_dropbox_ams_id();
        let context = ctx_with_active(
            &registry,
            &tx,
            "custom_api",
            "",
            "",
            active_custom.as_str(),
        );
        assert_eq!(
            try_claim_and_enqueue(dir.path(), &context).await,
            ClaimResult::Queued
        );
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
        let active_custom = first_custom_dropbox_ams_id();
        let context = ctx_with_active(
            &registry,
            &tx,
            "custom_api",
            "",
            "",
            active_custom.as_str(),
        );
        assert_eq!(
            try_claim_and_enqueue(dir.path(), &context).await,
            ClaimResult::Queued
        );
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
        assert_eq!(
            try_claim_and_enqueue(dir.path(), &context).await,
            ClaimResult::NoMedia
        );
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
        let pending = tracker.snapshot();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].dir_name, "job1");
        assert!(!pending[0].waiting_for_media);

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
