//! OS-dependent default folders for archive and logs.
//!
//! Layout under the user Videos/Movies directory:
//! `AeroMediaService/{Archiv,Logs}`
//! Under `Archiv`: `1 Erfolgreich` / `2 Abgebrochen` / `3 Fehler`.

use std::fs;
use std::path::{Path, PathBuf};

use directories::UserDirs;
use serde::{Deserialize, Serialize};

use crate::constants::APP_DIR_NAME;
use crate::util::archive::{ARCHIVE_CANCELLED, ARCHIVE_ERROR, ARCHIVE_SUCCESS};

const ARCHIVE_DIR: &str = "Archiv";
const LOGS_DIR: &str = "Logs";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultDirKind {
    Archive,
    Logs,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DefaultDirsProposal {
    pub root: String,
    pub archive_path: String,
    pub log_path: String,
    /// `true` when the AeroMediaService root already exists as a directory.
    pub root_exists: bool,
    /// `true` when `…/Archiv` already exists as a directory.
    pub archive_exists: bool,
    /// `true` when `…/Logs` already exists as a directory.
    pub log_exists: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EnsureDefaultDirResult {
    pub kind: DefaultDirKind,
    pub root: String,
    pub path: String,
    pub created: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EnsureDefaultAppRootResult {
    pub root: String,
    pub archive_path: String,
    pub log_path: String,
    /// `true` when at least one of root / Archiv / Logs was newly created.
    pub created: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DefaultDirsError {
    #[error("{0}")]
    Message(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Resolve `…/Videos|Movies/AeroMediaService` (or override root).
pub fn media_root_from_override(override_root: Option<&Path>) -> Result<PathBuf, DefaultDirsError> {
    if let Some(root) = override_root {
        if root.as_os_str().is_empty() {
            return Err(DefaultDirsError::Message(
                "override root path is empty".into(),
            ));
        }
        return Ok(root.to_path_buf());
    }
    Ok(default_media_root()?)
}

pub fn default_media_root() -> Result<PathBuf, DefaultDirsError> {
    let base = user_videos_or_movies_dir()?;
    Ok(base.join(APP_DIR_NAME))
}

pub fn paths_under_root(root: &Path) -> (PathBuf, PathBuf) {
    (root.join(ARCHIVE_DIR), root.join(LOGS_DIR))
}

pub fn path_for_kind(root: &Path, kind: DefaultDirKind) -> PathBuf {
    match kind {
        DefaultDirKind::Archive => root.join(ARCHIVE_DIR),
        DefaultDirKind::Logs => root.join(LOGS_DIR),
    }
}

pub fn propose_default_dirs() -> Result<DefaultDirsProposal, DefaultDirsError> {
    let root = default_media_root()?;
    let (archive, logs) = paths_under_root(&root);
    let warnings = collect_path_warnings(&root);

    Ok(DefaultDirsProposal {
        root: path_to_string(&root),
        archive_path: path_to_string(&archive),
        log_path: path_to_string(&logs),
        root_exists: root.is_dir(),
        archive_exists: archive.is_dir(),
        log_exists: logs.is_dir(),
        warnings,
    })
}

/// Create exactly one default folder (`Archiv` or `Logs`) under `root`.
pub fn ensure_default_dir(
    kind: DefaultDirKind,
    override_root: Option<&Path>,
) -> Result<EnsureDefaultDirResult, DefaultDirsError> {
    let root = media_root_from_override(override_root)?;
    let path = path_for_kind(&root, kind);

    let mut created = ensure_dir(&path)?;
    if kind == DefaultDirKind::Archive {
        created = ensure_archive_status_dirs(&path)? || created;
    }
    probe_writable(&path)?;

    let warnings = collect_path_warnings(&root);

    Ok(EnsureDefaultDirResult {
        kind,
        root: path_to_string(&root),
        path: path_to_string(&path),
        created,
        warnings,
    })
}

/// Create `AeroMediaService` root plus `Archiv` (with status subfolders) and `Logs`.
pub fn ensure_default_app_root(
    override_root: Option<&Path>,
) -> Result<EnsureDefaultAppRootResult, DefaultDirsError> {
    let root = media_root_from_override(override_root)?;
    let (archive, logs) = paths_under_root(&root);

    let mut created = ensure_dir(&root)?;
    created = ensure_dir(&archive)? || created;
    created = ensure_archive_status_dirs(&archive)? || created;
    created = ensure_dir(&logs)? || created;

    probe_writable(&archive)?;
    probe_writable(&logs)?;

    let warnings = collect_path_warnings(&root);

    Ok(EnsureDefaultAppRootResult {
        root: path_to_string(&root),
        archive_path: path_to_string(&archive),
        log_path: path_to_string(&logs),
        created,
        warnings,
    })
}

fn ensure_archive_status_dirs(archive: &Path) -> Result<bool, DefaultDirsError> {
    let mut created_any = false;
    for name in [ARCHIVE_SUCCESS, ARCHIVE_CANCELLED, ARCHIVE_ERROR] {
        created_any = ensure_dir(&archive.join(name))? || created_any;
    }
    Ok(created_any)
}

fn ensure_dir(path: &Path) -> Result<bool, DefaultDirsError> {
    let existed = path.is_dir();
    fs::create_dir_all(path)?;
    if !path.is_dir() {
        return Err(DefaultDirsError::Message(format!(
            "Konnte Ordner nicht anlegen: {}",
            path.display()
        )));
    }
    Ok(!existed)
}

fn probe_writable(dir: &Path) -> Result<(), DefaultDirsError> {
    let probe = dir.join(".aero_write_probe");
    match fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            Ok(())
        }
        Err(e) => Err(DefaultDirsError::Message(format!(
            "Ordner nicht beschreibbar ({}): {e}",
            dir.display()
        ))),
    }
}

fn user_videos_or_movies_dir() -> Result<PathBuf, DefaultDirsError> {
    let user_dirs = UserDirs::new().ok_or_else(|| {
        DefaultDirsError::Message("Benutzerordner konnte nicht ermittelt werden.".into())
    })?;
    if let Some(videos) = user_dirs.video_dir() {
        return Ok(videos.to_path_buf());
    }
    let home = user_dirs.home_dir();
    #[cfg(target_os = "macos")]
    {
        Ok(home.join("Movies"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(home.join("Videos"))
    }
}

fn collect_path_warnings(root: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let path_str = path_to_string(root);
    if looks_like_cloud_path(&path_str) {
        warnings.push(
            "Der vorgeschlagene Pfad liegt unter einem Cloud-Sync-Ordner (OneDrive/iCloud/Dropbox). Große Dateien können Probleme verursachen."
                .into(),
        );
    }
    warnings
}

/// Path-segment heuristics for common sync roots (case-insensitive).
pub fn looks_like_cloud_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "onedrive",
        "icloud drive",
        "icloud~",
        "mobile documents",
        "com.apple.clouddocs",
        "dropbox",
        "google drive",
        "googledrive",
    ];
    NEEDLES.iter().any(|n| lower.contains(n))
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn paths_under_root_layout() {
        let root = PathBuf::from("/tmp/AeroMediaService");
        let (a, l) = paths_under_root(&root);
        assert!(a.ends_with("Archiv"));
        assert!(l.ends_with("Logs"));
        assert_eq!(a.parent(), l.parent());
    }

    #[test]
    fn cloud_path_detection() {
        assert!(looks_like_cloud_path(
            r"C:\Users\x\OneDrive\Videos\AeroMediaService"
        ));
        assert!(looks_like_cloud_path(
            "/Users/x/Library/Mobile Documents/com~apple~CloudDocs"
        ));
        assert!(looks_like_cloud_path("/home/x/Dropbox/Videos/AeroMediaService"));
        assert!(!looks_like_cloud_path(r"D:\AeroMediaService"));
        assert!(!looks_like_cloud_path("/home/x/Videos/AeroMediaService"));
    }

    #[test]
    fn ensure_creates_one_dir_per_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(APP_DIR_NAME);
        let archive = ensure_default_dir(DefaultDirKind::Archive, Some(&root)).unwrap();
        assert!(PathBuf::from(&archive.path).is_dir());
        assert!(archive.created);
        assert!(archive.path.contains("Archiv"));
        assert!(root.join(ARCHIVE_DIR).join(ARCHIVE_SUCCESS).is_dir());
        assert!(!root.join(LOGS_DIR).exists());

        let logs = ensure_default_dir(DefaultDirKind::Logs, Some(&root)).unwrap();
        assert!(PathBuf::from(&logs.path).is_dir());
        assert!(logs.created);
        assert!(logs.path.contains("Logs"));

        let again = ensure_default_dir(DefaultDirKind::Archive, Some(&root)).unwrap();
        assert!(!again.created);
        assert_eq!(again.path, archive.path);
    }

    #[test]
    fn ensure_app_root_creates_archiv_and_logs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(APP_DIR_NAME);
        let ensured = ensure_default_app_root(Some(&root)).unwrap();
        assert!(ensured.created);
        assert!(PathBuf::from(&ensured.root).is_dir());
        assert!(PathBuf::from(&ensured.archive_path).is_dir());
        assert!(PathBuf::from(&ensured.log_path).is_dir());
        assert!(PathBuf::from(&ensured.archive_path)
            .join(ARCHIVE_CANCELLED)
            .is_dir());
        assert!(PathBuf::from(&ensured.archive_path)
            .join(ARCHIVE_ERROR)
            .is_dir());

        let again = ensure_default_app_root(Some(&root)).unwrap();
        assert!(!again.created);
    }

    #[test]
    fn media_root_override_rejects_empty() {
        let err = media_root_from_override(Some(Path::new(""))).unwrap_err();
        assert!(matches!(err, DefaultDirsError::Message(_)));
    }

    #[test]
    fn propose_returns_consistent_siblings() {
        let p = propose_default_dirs().unwrap();
        assert!(p.root.contains(APP_DIR_NAME));
        assert!(p.archive_path.contains(ARCHIVE_DIR));
        assert!(p.log_path.contains(LOGS_DIR));
        let archive_parent = PathBuf::from(&p.archive_path).parent().map(PathBuf::from);
        let log_parent = PathBuf::from(&p.log_path).parent().map(PathBuf::from);
        assert_eq!(archive_parent, log_parent);
        assert_eq!(
            archive_parent.map(|x| path_to_string(&x)),
            Some(p.root.clone())
        );
        assert_eq!(p.root_exists, PathBuf::from(&p.root).is_dir());
        assert_eq!(p.archive_exists, PathBuf::from(&p.archive_path).is_dir());
        assert_eq!(p.log_exists, PathBuf::from(&p.log_path).is_dir());
    }
}
