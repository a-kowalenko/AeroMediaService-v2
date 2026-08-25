//! Move processed folders into archive subfolders
//! (`1 Erfolgreich` / `2 Abgebrochen` / `3 Fehler`).
//! Port of legacy `core/archive.py`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::marker::{read_marker_raw, remove_upload_markers, MarkerError};
use crate::storage::logging;

pub const ARCHIVE_SUCCESS: &str = "1 Erfolgreich";
pub const ARCHIVE_CANCELLED: &str = "2 Abgebrochen";
pub const ARCHIVE_ERROR: &str = "3 Fehler";

/// Current + legacy names when recovering jobs for retry.
pub const ARCHIVE_RETRY_SUBFOLDERS: &[&str] = &[
    ARCHIVE_ERROR,
    ARCHIVE_CANCELLED,
    "fehler",
    "abgebrochen",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveMove {
    pub dir_name: String,
    pub archived_path: PathBuf,
    pub archive_subfolder: String,
}

/// Locate an archived folder by name (exact or `{dir_name}_*` timestamp suffix).
#[allow(dead_code)]
pub fn find_archived_folder(
    archive_base: &str,
    dir_name: &str,
    subfolders: &[&str],
    archived_path_hint: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(hint) = archived_path_hint {
        if hint.is_dir() {
            return Some(hint.to_path_buf());
        }
    }
    if archive_base.trim().is_empty() || dir_name.is_empty() {
        return None;
    }

    for subfolder in subfolders {
        let sub_dir = Path::new(archive_base).join(subfolder);
        if !sub_dir.is_dir() {
            continue;
        }
        let exact = sub_dir.join(dir_name);
        if exact.is_dir() {
            return Some(exact);
        }
        let Ok(names) = fs::read_dir(&sub_dir) else {
            continue;
        };
        let prefix = format!("{dir_name}_");
        for entry in names.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.as_ref() != dir_name && !name.starts_with(&prefix) {
                continue;
            }
            let candidate = entry.path();
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Move `local_dir_path` under `archive_base/<subfolder_name>/`.
pub fn archive_directory(
    archive_base: &str,
    local_dir_path: &Path,
    subfolder_name: &str,
) -> Option<ArchiveMove> {
    if archive_base.trim().is_empty() {
        logging::log_warn(&format!(
            "Kein Archiv-Pfad konfiguriert. {} wird nicht verschoben.",
            local_dir_path.display()
        ));
        return None;
    }

    let target_dir = Path::new(archive_base).join(subfolder_name);
    if let Err(e) = fs::create_dir_all(&target_dir) {
        logging::log_error(&format!(
            "Konnte {subfolder_name}-Ordner nicht erstellen: {e}"
        ));
        return None;
    }

    let dir_name = local_dir_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let mut destination_path = target_dir.join(&dir_name);
    if destination_path.exists() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        destination_path = PathBuf::from(format!("{}_{stamp}", destination_path.to_string_lossy()));
        logging::log_warn(&format!(
            "Zielpfad existiert, benenne um zu: {}",
            destination_path.display()
        ));
    }

    match move_directory(local_dir_path, &destination_path) {
        Ok(()) => {
            remove_upload_markers(&destination_path);
            logging::log_info(&format!(
                "Verzeichnis verschoben nach: {}",
                destination_path.display()
            ));
            Some(ArchiveMove {
                dir_name,
                archived_path: destination_path,
                archive_subfolder: subfolder_name.to_string(),
            })
        }
        Err(e) => {
            logging::log_error(&format!(
                "Konnte Verzeichnis nicht nach {} verschieben: {e}",
                destination_path.display()
            ));
            None
        }
    }
}

#[allow(dead_code)]
pub fn is_customer_lookup_failure(message: &str) -> bool {
    message.contains("Customer-Lookup fehlgeschlagen")
}

pub fn is_marker_format_failure(err: &MarkerError) -> bool {
    matches!(
        err,
        MarkerError::Empty
            | MarkerError::InvalidJson(_)
            | MarkerError::NotAnObject
            | MarkerError::MissingField(_)
            | MarkerError::MissingType
            | MarkerError::InvalidApiFormat
            | MarkerError::InvalidFormat
            | MarkerError::EmptyWrite
    )
}

/// Archive a folder after a failed customer API lookup into `3 Fehler`.
pub fn handle_customer_lookup_failure(
    archive_base: &str,
    local_dir_path: &Path,
    error_msg: &str,
    marker_raw: Option<&str>,
) -> Option<ArchiveMove> {
    handle_marker_failure(archive_base, local_dir_path, error_msg, marker_raw)
}
pub fn handle_marker_failure(
    archive_base: &str,
    local_dir_path: &Path,
    error_msg: &str,
    marker_raw: Option<&str>,
) -> Option<ArchiveMove> {
    let dir_name = local_dir_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    crate::events::emit_status(format!("Fehler: {dir_name}"));
    let raw = marker_raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| read_marker_raw(local_dir_path));
    let mut payload = serde_json::json!({
        "dir_name": dir_name,
        "status": "Fehler",
        "error_msg": error_msg,
    });
    if let Some(raw) = raw {
        payload["marker_raw"] = serde_json::Value::String(raw);
    }
    crate::events::emit(crate::events::UPLOAD_HISTORY_UPDATE, payload);
    archive_directory(archive_base, local_dir_path, ARCHIVE_ERROR)
}

pub(crate) fn move_directory(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_dir_all(src, dst)?;
            fs::remove_dir_all(src)
        }
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::marker::{write_fertig_marker, MARKER_FERTIG};
    use tempfile::tempdir;

    #[test]
    fn archive_moves_into_subfolder_and_strips_markers() {
        let root = tempdir().unwrap();
        let src = root.path().join("job1");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("photo.jpg"), b"img").unwrap();
        write_fertig_marker(&src, r#"{"vorname":"A","nachname":"B","email":"a@b.de"}"#).unwrap();
        let archive = root.path().join("archive");

        let moved = archive_directory(archive.to_str().unwrap(), &src, ARCHIVE_SUCCESS).unwrap();
        assert!(!src.exists());
        assert!(moved.archived_path.join("photo.jpg").is_file());
        assert!(!moved.archived_path.join(MARKER_FERTIG).exists());
        assert_eq!(moved.archive_subfolder, ARCHIVE_SUCCESS);
        assert_eq!(moved.dir_name, "job1");
    }

    #[test]
    fn archive_renames_when_destination_exists() {
        let root = tempdir().unwrap();
        let archive = root.path().join("archive");
        fs::create_dir_all(archive.join(ARCHIVE_ERROR).join("job1")).unwrap();
        let src = root.path().join("job1");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("a.bin"), b"x").unwrap();

        let moved = archive_directory(archive.to_str().unwrap(), &src, ARCHIVE_ERROR).unwrap();
        assert_ne!(moved.archived_path.file_name().unwrap(), "job1");
        assert!(moved
            .archived_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("job1_"));
    }

    #[test]
    fn find_archived_folder_matches_prefix() {
        let root = tempdir().unwrap();
        let dest = root.path().join(ARCHIVE_ERROR).join("clip_111");
        fs::create_dir_all(&dest).unwrap();
        let found = find_archived_folder(
            root.path().to_str().unwrap(),
            "clip",
            &[ARCHIVE_ERROR, ARCHIVE_CANCELLED],
            None,
        )
        .unwrap();
        assert_eq!(found, dest);
    }

    #[test]
    fn find_archived_folder_still_finds_legacy_names() {
        let root = tempdir().unwrap();
        let dest = root.path().join("fehler").join("clip_legacy");
        fs::create_dir_all(&dest).unwrap();
        let found = find_archived_folder(
            root.path().to_str().unwrap(),
            "clip_legacy",
            ARCHIVE_RETRY_SUBFOLDERS,
            None,
        )
        .unwrap();
        assert_eq!(found, dest);
    }

    #[test]
    fn missing_archive_path_does_not_move() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.bin"), b"x").unwrap();
        assert!(archive_directory("", dir.path(), ARCHIVE_SUCCESS).is_none());
        assert!(dir.path().is_dir());
    }

    #[test]
    fn marker_format_failure_matches_legacy_value_error() {
        assert!(is_marker_format_failure(&MarkerError::InvalidFormat));
        assert!(is_marker_format_failure(&MarkerError::Empty));
        assert!(!is_marker_format_failure(&MarkerError::ApiLookupRequired));
        assert!(is_customer_lookup_failure(
            "Customer-Lookup fehlgeschlagen: timeout"
        ));
        assert!(!is_customer_lookup_failure("Upload fehlgeschlagen"));
    }
}
