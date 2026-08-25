//! Prevents duplicate queue entries and publishes an ordered snapshot.
//! Port of legacy `core/upload_queue_registry.py`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;
use tokio::sync::mpsc::UnboundedSender;

use crate::cloud::DropboxAccountBinding;
use crate::events;
use crate::model::kunde::Kunde;
use crate::monitor::stability::folder_key;
use crate::storage::logging;

/// One monitor-claimed folder waiting for (or in) the upload worker.
#[derive(Debug, Clone)]
pub struct UploadJob {
    pub dir_path: PathBuf,
    pub kunde: Kunde,
    pub use_dropbox_client: bool,
    /// ATS handoff correlation id when a valid manifest was present (P1b outbox).
    pub correlation_id: Option<String>,
    /// When set, worker appends into the parent order instead of creating a new one.
    pub append: Option<AppendTarget>,
    /// Dropbox profile frozen at claim/enqueue (native or custom_api pool).
    pub dropbox_binding: Option<DropboxAccountBinding>,
}

/// Existing successful upload that an ATS Nachreichung should merge into.
#[derive(Debug, Clone)]
pub struct AppendTarget {
    pub parent_dir_name: String,
    pub remote_path: String,
    pub order_id: Option<String>,
    pub share_link: Option<String>,
    /// Parent job's bound AMS Dropbox profile (Phase 16b).
    pub dropbox_account_ams_id: Option<String>,
    pub dropbox_account_pool: Option<String>,
    pub dropbox_account_id: Option<String>,
    pub dropbox_account_email: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueState {
    Waiting,
    Active,
}

impl QueueState {
    fn as_str(self) -> &'static str {
        match self {
            QueueState::Waiting => "waiting",
            QueueState::Active => "active",
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub dir_path: PathBuf,
    pub dir_name: String,
    pub customer_label: String,
    pub enqueued_at: Instant,
    pub state: QueueState,
    /// AMS Dropbox profile frozen on the job (Hard-Guard / Soft-Active).
    pub dropbox_binding: Option<DropboxAccountBinding>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct QueueSnapshotItem {
    pub position: usize,
    pub dir_name: String,
    pub customer_label: String,
    pub state: String,
    pub wait_seconds: f64,
}

pub fn format_customer_label(kunde: Option<&Kunde>) -> String {
    let Some(kunde) = kunde else {
        return "—".into();
    };
    let first = kunde.first_name.as_deref().unwrap_or("").trim();
    let last = kunde.last_name.as_deref().unwrap_or("").trim();
    let name = format!("{first} {last}").trim().to_string();
    if !name.is_empty() {
        return name;
    }
    let email = kunde.email.as_deref().unwrap_or("").trim();
    if !email.is_empty() {
        return email.to_string();
    }
    "—".into()
}

struct Inner {
    pending: HashSet<String>,
    entries: Vec<QueueEntry>,
}

pub struct UploadQueueRegistry {
    inner: Mutex<Inner>,
}

impl Default for UploadQueueRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl UploadQueueRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                pending: HashSet::new(),
                entries: Vec::new(),
            }),
        }
    }

    fn with_lock<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    fn find_entry_index(entries: &[QueueEntry], dir_path: &Path) -> Option<usize> {
        let key = folder_key(dir_path);
        entries
            .iter()
            .position(|entry| folder_key(&entry.dir_path) == key)
    }

    fn emit_changed(&self) {
        events::emit(events::UPLOAD_QUEUE_CHANGED, self.snapshot_dicts());
    }

    #[allow(dead_code)]
    pub fn snapshot(&self) -> Vec<QueueEntry> {
        self.with_lock(|inner| inner.entries.clone())
    }

    pub fn snapshot_dicts(&self) -> Vec<QueueSnapshotItem> {
        let now = Instant::now();
        self.with_lock(|inner| {
            inner
                .entries
                .iter()
                .enumerate()
                .map(|(i, entry)| QueueSnapshotItem {
                    position: i + 1,
                    dir_name: entry.dir_name.clone(),
                    customer_label: entry.customer_label.clone(),
                    state: entry.state.as_str().to_string(),
                    wait_seconds: now
                        .saturating_duration_since(entry.enqueued_at)
                        .as_secs_f64(),
                })
                .collect()
        })
    }

    /// Reserve a folder so the monitor cannot enqueue it twice.
    pub fn register(&self, dir_path: &Path) -> bool {
        let key = folder_key(dir_path);
        self.with_lock(|inner| inner.pending.insert(key))
    }

    pub fn unregister(&self, dir_path: Option<&Path>) {
        let Some(dir_path) = dir_path else {
            return;
        };
        let key = folder_key(dir_path);
        let changed = self.with_lock(|inner| {
            let mut changed = inner.pending.remove(&key);
            if let Some(idx) = Self::find_entry_index(&inner.entries, dir_path) {
                inner.entries.remove(idx);
                changed = true;
            }
            changed
        });
        if changed {
            self.emit_changed();
        }
    }

    #[allow(dead_code)]
    pub fn is_registered(&self, dir_path: &Path) -> bool {
        let key = folder_key(dir_path);
        self.with_lock(|inner| inner.pending.contains(&key))
    }

    pub fn mark_active(&self, dir_path: &Path) {
        let changed = self.with_lock(|inner| {
            if let Some(idx) = Self::find_entry_index(&inner.entries, dir_path) {
                inner.entries[idx].state = QueueState::Active;
                true
            } else {
                false
            }
        });
        if changed {
            self.emit_changed();
        }
    }

    fn append_entry(inner: &mut Inner, job: &UploadJob) {
        let dir_name = job
            .dir_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        inner.entries.push(QueueEntry {
            dir_path: job.dir_path.clone(),
            dir_name,
            customer_label: format_customer_label(Some(&job.kunde)),
            enqueued_at: Instant::now(),
            state: QueueState::Waiting,
            dropbox_binding: job.dropbox_binding.clone(),
        });
    }

    /// Waiting + active jobs bound to this AMS Dropbox profile (any pool).
    pub fn bound_job_count(&self, ams_id: &str) -> usize {
        let id = ams_id.trim();
        if id.is_empty() {
            return 0;
        }
        self.with_lock(|inner| {
            inner
                .entries
                .iter()
                .filter(|e| {
                    e.dropbox_binding
                        .as_ref()
                        .is_some_and(|b| b.ams_id == id)
                })
                .count()
        })
    }

    /// Hard-Guard: block delete/disconnect while queue still holds jobs for this profile.
    pub fn assert_can_remove_account(&self, ams_id: &str) -> Result<(), String> {
        let n = self.bound_job_count(ams_id);
        if n == 0 {
            return Ok(());
        }
        Err(format!(
            "Dropbox-Profil kann nicht gelöscht/getrennt werden: {n} offene(r) Upload-Job(s) sind daran gebunden. \
             Bitte Uploads abwarten oder abbrechen."
        ))
    }

    /// Register (unless already reserved), send to the worker, and append a snapshot row.
    pub fn enqueue(
        &self,
        jobs: &UnboundedSender<UploadJob>,
        job: UploadJob,
        already_registered: bool,
    ) -> bool {
        let dir_path = job.dir_path.clone();
        if dir_path.as_os_str().is_empty() {
            return false;
        }
        let dir_name = dir_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let key = folder_key(&dir_path);
        let ok = self.with_lock(|inner| {
            if !already_registered {
                if inner.pending.contains(&key) {
                    logging::log_info(&format!(
                        "Upload bereits vorgemerkt, überspringe Queue: {dir_name}"
                    ));
                    return false;
                }
                inner.pending.insert(key);
            } else if !inner.pending.contains(&key) {
                return false;
            }
            if jobs.send(job.clone()).is_err() {
                logging::log_error("Upload-Queue ist geschlossen — Auftrag verworfen.");
                inner.pending.remove(&folder_key(&dir_path));
                return false;
            }
            Self::append_entry(inner, &job);
            true
        });
        if ok {
            self.emit_changed();
        }
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::sync::mpsc::unbounded_channel;

    fn job(path: &Path, first: &str) -> UploadJob {
        UploadJob {
            dir_path: path.to_path_buf(),
            kunde: Kunde {
                first_name: Some(first.into()),
                last_name: Some("Test".into()),
                email: Some("a@b.de".into()),
                ..Kunde::default()
            },
            use_dropbox_client: false,
            correlation_id: None,
            append: None,
            dropbox_binding: None,
        }
    }

    #[test]
    fn format_customer_label_prefers_name_then_email() {
        let named = Kunde {
            first_name: Some("Anna".into()),
            last_name: Some("Muster".into()),
            ..Kunde::default()
        };
        assert_eq!(format_customer_label(Some(&named)), "Anna Muster");
        let email_only = Kunde {
            email: Some("x@y.de".into()),
            ..Kunde::default()
        };
        assert_eq!(format_customer_label(Some(&email_only)), "x@y.de");
        assert_eq!(format_customer_label(None), "—");
        assert_eq!(format_customer_label(Some(&Kunde::default())), "—");
    }

    #[test]
    fn register_rejects_duplicates() {
        let dir = tempdir().unwrap();
        let registry = UploadQueueRegistry::new();
        assert!(registry.register(dir.path()));
        assert!(!registry.register(dir.path()));
        assert!(registry.is_registered(dir.path()));
        registry.unregister(Some(dir.path()));
        assert!(!registry.is_registered(dir.path()));
    }

    #[test]
    fn enqueue_snapshot_and_mark_active() {
        let dir = tempdir().unwrap();
        let registry = UploadQueueRegistry::new();
        let (tx, mut rx) = unbounded_channel();
        let item = job(dir.path(), "Anna");
        assert!(registry.enqueue(&tx, item, false));
        assert!(rx.try_recv().is_ok());
        let snap = registry.snapshot_dicts();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].position, 1);
        assert_eq!(snap[0].state, "waiting");
        assert_eq!(snap[0].customer_label, "Anna Test");

        registry.mark_active(dir.path());
        assert_eq!(registry.snapshot_dicts()[0].state, "active");

        assert!(!registry.enqueue(&tx, job(dir.path(), "Anna"), false));
        registry.unregister(Some(dir.path()));
        assert!(registry.snapshot_dicts().is_empty());
    }

    #[test]
    fn enqueue_already_registered_requires_pending() {
        let dir = tempdir().unwrap();
        let registry = UploadQueueRegistry::new();
        let (tx, _rx) = unbounded_channel();
        assert!(!registry.enqueue(&tx, job(dir.path(), "A"), true));
        assert!(registry.register(dir.path()));
        assert!(registry.enqueue(&tx, job(dir.path(), "A"), true));
    }

    #[test]
    fn bound_job_count_tracks_ams_binding() {
        use crate::cloud::dropbox::DropboxPool;
        let dir = tempdir().unwrap();
        let registry = UploadQueueRegistry::new();
        let (tx, _rx) = unbounded_channel();
        let mut item = job(dir.path(), "Anna");
        item.dropbox_binding = Some(DropboxAccountBinding {
            ams_id: "ams-bound".into(),
            pool: DropboxPool::Native,
            dropbox_account_id: String::new(),
            email: String::new(),
        });
        assert!(registry.enqueue(&tx, item, false));
        assert_eq!(registry.bound_job_count("ams-bound"), 1);
        assert_eq!(registry.bound_job_count("other"), 0);
        assert!(registry.assert_can_remove_account("ams-bound").is_err());
        assert!(registry.assert_can_remove_account("other").is_ok());
    }
}
