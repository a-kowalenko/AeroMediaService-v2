//! Waits for unchanged folder content before a job is claimed.
//! Port of legacy `core/folder_stability.py`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::model::handoff::{is_ignored_handoff_name, HANDOFF_DIRNAME};
use crate::model::marker::MARKER_FERTIG;
use crate::storage::logging;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveResult {
    Waiting,
    Stable,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderFingerprint {
    pub total_bytes: u64,
    pub file_count: u64,
}

struct PendingState {
    fingerprint: FolderFingerprint,
    stable_since: Instant,
    logged_waiting: bool,
    logged_no_media: bool,
    dir_name: String,
    waiting_for_media: bool,
}

/// UI snapshot of a folder waiting before upload (stability wait or ATS handoff/ready).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StabilityPendingItem {
    pub dir_name: String,
    pub remaining_seconds: f64,
    pub required_seconds: f64,
    pub waiting_for_media: bool,
    /// `stability` (folder wait) or `handoff` (ATS ready signal, claim follows).
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub correlation_id: String,
    /// Handoff detail: `signaled`, `waiting_folder`, `waiting_fertig`, `waiting_media`, `rejected`.
    #[serde(default)]
    pub handoff_phase: String,
    #[serde(default)]
    pub handoff_error_code: String,
    #[serde(default)]
    pub handoff_error_message: String,
}

pub const PENDING_KIND_STABILITY: &str = "stability";
pub const PENDING_KIND_HANDOFF: &str = "handoff";

pub const HANDOFF_PHASE_SIGNALED: &str = "signaled";
pub const HANDOFF_PHASE_WAITING_FOLDER: &str = "waiting_folder";
pub const HANDOFF_PHASE_WAITING_FERTIG: &str = "waiting_fertig";
pub const HANDOFF_PHASE_WAITING_MEDIA: &str = "waiting_media";
pub const HANDOFF_PHASE_REJECTED: &str = "rejected";

/// Case-normalized absolute path key (legacy `os.path.normcase(os.path.abspath(...))`).
pub fn folder_key(dir_path: &Path) -> String {
    let absolute = if dir_path.is_absolute() {
        dir_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(dir_path))
            .unwrap_or_else(|_| dir_path.to_path_buf())
    };
    let normalized = normalize_lexically(&absolute);
    let key = normalized.to_string_lossy();
    #[cfg(windows)]
    {
        key.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        key.into_owned()
    }
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Sum of file sizes and count, ignoring handoff/marker/checkpoint noise files.
pub fn folder_content_fingerprint(dir_path: &Path) -> FolderFingerprint {
    let mut total_bytes = 0u64;
    let mut file_count = 0u64;
    walk_files(dir_path, &mut |path| {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return;
        };
        if is_ignored_handoff_name(name) {
            return;
        }
        match fs::metadata(path) {
            Ok(meta) => {
                total_bytes = total_bytes.saturating_add(meta.len());
                file_count = file_count.saturating_add(1);
            }
            Err(_) => {}
        }
    });
    FolderFingerprint {
        total_bytes,
        file_count,
    }
}

fn walk_files(dir: &Path, visit: &mut impl FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == HANDOFF_DIRNAME {
                continue;
            }
            walk_files(&path, visit);
        } else if path.is_file() {
            visit(&path);
        }
    }
}

/// True when at least one file besides marker/checkpoint is present.
pub fn has_uploadable_files(dir_path: &Path) -> bool {
    folder_content_fingerprint(dir_path).file_count > 0
}

/// Remembers folders with `_fertig.txt` until their content stays unchanged.
pub struct FolderStabilityTracker {
    required: Duration,
    pending: HashMap<String, PendingState>,
}

impl FolderStabilityTracker {
    pub fn new(required_stable_seconds: f64) -> Self {
        Self {
            required: duration_from_secs(required_stable_seconds),
            pending: HashMap::new(),
        }
    }

    pub fn set_required_seconds(&mut self, seconds: f64) {
        self.required = duration_from_secs(seconds);
    }

    pub fn observe(&mut self, dir_path: &Path) -> ObserveResult {
        self.observe_at(dir_path, Instant::now())
    }

    pub fn observe_at(&mut self, dir_path: &Path, now: Instant) -> ObserveResult {
        let key = folder_key(dir_path);
        let fertig_path = dir_path.join(MARKER_FERTIG);
        if !fertig_path.is_file() {
            self.pending.remove(&key);
            return ObserveResult::Removed;
        }

        let fingerprint = folder_content_fingerprint(dir_path);
        let dir_name = dir_path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if fingerprint.file_count == 0 {
            let state = self.pending.entry(key).or_insert_with(|| PendingState {
                fingerprint,
                stable_since: now,
                logged_waiting: false,
                logged_no_media: false,
                dir_name: dir_name.to_string(),
                waiting_for_media: true,
            });
            state.waiting_for_media = true;
            state.fingerprint = fingerprint;
            if !state.logged_no_media {
                logging::log_info(&format!(
                    "Ordner '{dir_name}': Marker gefunden, warte auf Medien-Dateien..."
                ));
                state.logged_no_media = true;
            }
            return ObserveResult::Waiting;
        }

        if self.required.is_zero() {
            self.pending.remove(&key);
            return ObserveResult::Stable;
        }

        let fingerprint_changed = match self.pending.get(&key) {
            Some(state) => state.fingerprint != fingerprint,
            None => true,
        };

        if fingerprint_changed {
            self.pending.insert(
                key.clone(),
                PendingState {
                    fingerprint,
                    stable_since: now,
                    logged_waiting: false,
                    logged_no_media: false,
                    dir_name: dir_name.to_string(),
                    waiting_for_media: false,
                },
            );
        }

        let Some(state) = self.pending.get_mut(&key) else {
            return ObserveResult::Waiting;
        };

        if !state.logged_waiting {
            logging::log_info(&format!(
                "Ordner '{dir_name}': Warte auf Datei-Stabilität ({:.0} s unverändert)...",
                self.required.as_secs_f64()
            ));
            state.logged_waiting = true;
        }

        let elapsed = now.saturating_duration_since(state.stable_since);
        if elapsed >= self.required {
            self.pending.remove(&key);
            logging::log_info(&format!(
                "Ordner '{dir_name}': Inhalt stabil — Upload wird vorbereitet."
            ));
            return ObserveResult::Stable;
        }

        ObserveResult::Waiting
    }

    pub fn discard(&mut self, dir_path: &Path) {
        self.pending.remove(&folder_key(dir_path));
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }

    /// Drop pending folders that were not observed as still waiting this scan.
    pub fn retain_keys(&mut self, keys: &HashSet<String>) {
        self.pending.retain(|key, _| keys.contains(key));
    }

    pub fn snapshot(&self) -> Vec<StabilityPendingItem> {
        self.snapshot_at(Instant::now())
    }

    pub fn snapshot_at(&self, now: Instant) -> Vec<StabilityPendingItem> {
        let required = self.required.as_secs_f64();
        let mut items: Vec<StabilityPendingItem> = self
            .pending
            .values()
            .map(|state| {
                let elapsed = now
                    .saturating_duration_since(state.stable_since)
                    .as_secs_f64();
                let remaining = if state.waiting_for_media {
                    0.0
                } else {
                    (required - elapsed).max(0.0)
                };
                StabilityPendingItem {
                    dir_name: state.dir_name.clone(),
                    remaining_seconds: remaining,
                    required_seconds: required,
                    waiting_for_media: state.waiting_for_media,
                    kind: PENDING_KIND_STABILITY.to_string(),
                    correlation_id: String::new(),
                    handoff_phase: String::new(),
                    handoff_error_code: String::new(),
                    handoff_error_message: String::new(),
                }
            })
            .collect();
        items.sort_by(|a, b| a.dir_name.cmp(&b.dir_name));
        items
    }

    #[cfg(test)]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

fn duration_from_secs(seconds: f64) -> Duration {
    let secs = if seconds.is_finite() {
        seconds.max(0.0)
    } else {
        0.0
    };
    Duration::from_secs_f64(secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::marker::MARKER_PROCESSING;
    use crate::upload::checkpoint::CHECKPOINT_FILENAME;
    use std::fs;
    use tempfile::tempdir;

    fn write_bytes(path: &Path, len: usize) {
        fs::write(path, vec![0u8; len]).unwrap();
    }

    #[test]
    fn fingerprint_ignores_marker_and_checkpoint() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join(MARKER_FERTIG), 12);
        write_bytes(&dir.path().join(MARKER_PROCESSING), 8);
        write_bytes(&dir.path().join(CHECKPOINT_FILENAME), 40);
        write_bytes(&dir.path().join("photo.jpg"), 10);
        let nested = dir.path().join("sub");
        fs::create_dir(&nested).unwrap();
        write_bytes(&nested.join("clip.mp4"), 25);
        write_bytes(&nested.join(MARKER_FERTIG), 3);

        let fp = folder_content_fingerprint(dir.path());
        assert_eq!(fp.file_count, 2);
        assert_eq!(fp.total_bytes, 35);
        assert!(has_uploadable_files(dir.path()));
    }

    #[test]
    fn fingerprint_empty_without_media() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join(MARKER_FERTIG), 4);
        let fp = folder_content_fingerprint(dir.path());
        assert_eq!(
            fp,
            FolderFingerprint {
                total_bytes: 0,
                file_count: 0
            }
        );
        assert!(!has_uploadable_files(dir.path()));
    }

    #[test]
    fn observe_removed_without_fertig_marker() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join("photo.jpg"), 4);
        let mut tracker = FolderStabilityTracker::new(15.0);
        assert_eq!(tracker.observe(dir.path()), ObserveResult::Removed);
        assert_eq!(tracker.pending_count(), 0);
    }

    #[test]
    fn observe_waits_when_no_media() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join(MARKER_FERTIG), 2);
        let mut tracker = FolderStabilityTracker::new(0.0);
        assert_eq!(tracker.observe(dir.path()), ObserveResult::Waiting);
        assert_eq!(tracker.pending_count(), 1);
    }

    #[test]
    fn observe_stable_immediately_when_required_is_zero() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join(MARKER_FERTIG), 2);
        write_bytes(&dir.path().join("a.bin"), 4);
        let mut tracker = FolderStabilityTracker::new(0.0);
        assert_eq!(tracker.observe(dir.path()), ObserveResult::Stable);
        assert_eq!(tracker.pending_count(), 0);
    }

    #[test]
    fn observe_waits_until_required_seconds() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join(MARKER_FERTIG), 2);
        write_bytes(&dir.path().join("a.bin"), 4);
        let mut tracker = FolderStabilityTracker::new(15.0);
        let t0 = Instant::now();

        assert_eq!(tracker.observe_at(dir.path(), t0), ObserveResult::Waiting);
        assert_eq!(
            tracker.observe_at(dir.path(), t0 + Duration::from_secs(14)),
            ObserveResult::Waiting
        );
        assert_eq!(
            tracker.observe_at(dir.path(), t0 + Duration::from_secs(15)),
            ObserveResult::Stable
        );
        assert_eq!(tracker.pending_count(), 0);
    }

    #[test]
    fn observe_resets_when_fingerprint_changes() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join(MARKER_FERTIG), 2);
        write_bytes(&dir.path().join("a.bin"), 4);
        let mut tracker = FolderStabilityTracker::new(10.0);
        let t0 = Instant::now();

        assert_eq!(tracker.observe_at(dir.path(), t0), ObserveResult::Waiting);
        write_bytes(&dir.path().join("b.bin"), 8);
        let t1 = t0 + Duration::from_secs(8);
        assert_eq!(tracker.observe_at(dir.path(), t1), ObserveResult::Waiting);
        assert_eq!(
            tracker.observe_at(dir.path(), t1 + Duration::from_secs(9)),
            ObserveResult::Waiting
        );
        assert_eq!(
            tracker.observe_at(dir.path(), t1 + Duration::from_secs(10)),
            ObserveResult::Stable
        );
    }

    #[test]
    fn discard_and_clear_drop_pending() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join(MARKER_FERTIG), 2);
        write_bytes(&dir.path().join("a.bin"), 4);
        let mut tracker = FolderStabilityTracker::new(30.0);
        assert_eq!(tracker.observe(dir.path()), ObserveResult::Waiting);
        tracker.discard(dir.path());
        assert_eq!(tracker.pending_count(), 0);

        assert_eq!(tracker.observe(dir.path()), ObserveResult::Waiting);
        tracker.clear();
        assert_eq!(tracker.pending_count(), 0);
    }

    #[test]
    fn observe_removed_clears_pending_when_marker_deleted() {
        let dir = tempdir().unwrap();
        let fertig = dir.path().join(MARKER_FERTIG);
        write_bytes(&fertig, 2);
        write_bytes(&dir.path().join("a.bin"), 4);
        let mut tracker = FolderStabilityTracker::new(30.0);
        assert_eq!(tracker.observe(dir.path()), ObserveResult::Waiting);
        fs::remove_file(&fertig).unwrap();
        assert_eq!(tracker.observe(dir.path()), ObserveResult::Removed);
        assert_eq!(tracker.pending_count(), 0);
    }

    #[test]
    fn folder_key_is_stable_for_same_path() {
        let dir = tempdir().unwrap();
        let a = folder_key(dir.path());
        let b = folder_key(dir.path());
        assert_eq!(a, b);
        assert!(!a.is_empty());
        #[cfg(windows)]
        {
            assert_eq!(a, a.to_lowercase());
        }
    }

    #[test]
    fn snapshot_reports_remaining_seconds() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join(MARKER_FERTIG), 2);
        write_bytes(&dir.path().join("a.bin"), 4);
        let mut tracker = FolderStabilityTracker::new(15.0);
        let t0 = Instant::now();
        assert_eq!(tracker.observe_at(dir.path(), t0), ObserveResult::Waiting);

        let snap = tracker.snapshot_at(t0);
        assert_eq!(snap.len(), 1);
        assert_eq!(
            snap[0].dir_name,
            dir.path().file_name().unwrap().to_string_lossy()
        );
        assert!(!snap[0].waiting_for_media);
        assert_eq!(snap[0].kind, PENDING_KIND_STABILITY);
        assert!((snap[0].required_seconds - 15.0).abs() < 0.01);
        assert!((snap[0].remaining_seconds - 15.0).abs() < 0.01);

        let later = tracker.snapshot_at(t0 + Duration::from_secs(5));
        assert!((later[0].remaining_seconds - 10.0).abs() < 0.01);
    }

    #[test]
    fn snapshot_marks_waiting_for_media() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join(MARKER_FERTIG), 2);
        let mut tracker = FolderStabilityTracker::new(15.0);
        assert_eq!(tracker.observe(dir.path()), ObserveResult::Waiting);
        let snap = tracker.snapshot();
        assert_eq!(snap.len(), 1);
        assert!(snap[0].waiting_for_media);
        assert_eq!(snap[0].remaining_seconds, 0.0);
    }

    #[test]
    fn retain_keys_drops_unseen_pending() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join(MARKER_FERTIG), 2);
        write_bytes(&dir.path().join("a.bin"), 4);
        let mut tracker = FolderStabilityTracker::new(30.0);
        assert_eq!(tracker.observe(dir.path()), ObserveResult::Waiting);
        tracker.retain_keys(&HashSet::new());
        assert_eq!(tracker.pending_count(), 0);
        assert!(tracker.snapshot().is_empty());
    }
}
