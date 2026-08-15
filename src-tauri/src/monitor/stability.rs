//! Waits for unchanged folder content before a job is claimed.
//! Port of legacy `core/folder_stability.py`.

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

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
}

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
        let dir_name = dir_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if fingerprint.file_count == 0 {
            let state = self.pending.entry(key).or_insert_with(|| PendingState {
                fingerprint,
                stable_since: now,
                logged_waiting: false,
                logged_no_media: false,
            });
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
        assert_eq!(fp, FolderFingerprint { total_bytes: 0, file_count: 0 });
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
}
