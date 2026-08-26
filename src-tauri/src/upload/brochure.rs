//! Phase 17 — Infobroschüre PDF (Erst-Upload only).
//!
//! App-managed source copy under app-data; inject into the upload file list only
//! (never into the local monitor/job folder). Append never injects.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cloud::dropbox::{join_dropbox_path, UploadFile};
use crate::storage::config::{app_config_dir, runtime_setting};
use crate::storage::logging;

pub const BROCHURE_ENABLED_KEY: &str = "brochure_enabled";
pub const BROCHURE_EXPORT_NAME_KEY: &str = "brochure_export_name";
pub const BROCHURE_SUBDIR_KEY: &str = "brochure_subdir";

pub const DEFAULT_EXPORT_NAME: &str = "Infobroschuere.pdf";
pub const MAX_BROCHURE_BYTES: u64 = 5 * 1024 * 1024;
const SOURCE_DIR: &str = "brochure";
const SOURCE_FILE: &str = "source.pdf";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrochureSettings {
    pub enabled: bool,
    pub export_name: String,
    pub subdir: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrochureSourceInfo {
    pub present: bool,
    pub path: String,
    pub size_bytes: u64,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrochureUploadEntry {
    pub local_path: PathBuf,
    pub rel_norm: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrochureSkipReason {
    Disabled,
    Append,
    MissingSource,
    RemoteExists,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrochurePlan {
    Skip(BrochureSkipReason),
    Add(BrochureUploadEntry),
    /// Same relative path already in the media list — overwrite with brochure source.
    Replace {
        index: usize,
        entry: BrochureUploadEntry,
    },
}

pub fn brochure_settings_from_runtime() -> BrochureSettings {
    BrochureSettings {
        enabled: is_truthy(&runtime_setting(BROCHURE_ENABLED_KEY)),
        export_name: normalize_export_name(&runtime_setting(BROCHURE_EXPORT_NAME_KEY)),
        subdir: normalize_subdir(&runtime_setting(BROCHURE_SUBDIR_KEY)),
    }
}

pub fn brochure_source_path() -> Result<PathBuf, String> {
    Ok(app_config_dir()
        .map_err(|e| e.to_string())?
        .join(SOURCE_DIR)
        .join(SOURCE_FILE))
}

pub fn brochure_source_info() -> Result<BrochureSourceInfo, String> {
    let path = brochure_source_path()?;
    if !path.is_file() {
        return Ok(BrochureSourceInfo {
            present: false,
            path: path.display().to_string(),
            size_bytes: 0,
            display_name: String::new(),
        });
    }
    let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Ok(BrochureSourceInfo {
        present: true,
        path: path.display().to_string(),
        size_bytes: size,
        display_name: DEFAULT_EXPORT_NAME.to_string(),
    })
}

/// Validate and copy a PDF into the app-managed brochure location (immediate commit).
pub fn import_brochure_pdf(source: &Path) -> Result<BrochureSourceInfo, String> {
    validate_pdf_candidate(source)?;
    let dest = brochure_source_path()?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::copy(source, &dest).map_err(|e| format!("Broschüre konnte nicht gespeichert werden: {e}"))?;
    logging::log_info(&format!(
        "Infobroschüre gesetzt: {}",
        dest.display()
    ));
    brochure_source_info()
}

pub fn remove_brochure_pdf() -> Result<(), String> {
    let path = brochure_source_path()?;
    if path.is_file() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
        logging::log_info("Infobroschüre entfernt.");
    }
    Ok(())
}

pub fn validate_pdf_candidate(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("Datei nicht gefunden.".into());
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !name.ends_with(".pdf") {
        return Err("Nur PDF-Dateien sind erlaubt.".into());
    }
    let size = fs::metadata(path)
        .map_err(|e| e.to_string())?
        .len();
    if size == 0 {
        return Err("Die PDF-Datei ist leer.".into());
    }
    if size > MAX_BROCHURE_BYTES {
        return Err(format!(
            "PDF zu groß (max. {} MB).",
            MAX_BROCHURE_BYTES / (1024 * 1024)
        ));
    }
    Ok(())
}

/// Basename only; always ends with `.pdf` (ASCII-normalized extension).
pub fn normalize_export_name(raw: &str) -> String {
    let trimmed = raw.trim().replace('\\', "/");
    let base = trimmed
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(|c: char| c == '.' || c.is_whitespace());
    let stem = if base.is_empty() {
        String::new()
    } else {
        let lower = base.to_ascii_lowercase();
        if lower.ends_with(".pdf") && base.len() > 4 {
            base[..base.len() - 4]
                .trim()
                .trim_end_matches('.')
                .to_string()
        } else if lower == "pdf" {
            // Bare ".pdf" / "pdf" after stripping → default
            String::new()
        } else {
            base.to_string()
        }
    };
    if stem.is_empty() {
        DEFAULT_EXPORT_NAME.to_string()
    } else {
        format!("{stem}.pdf")
    }
}

/// Optional relative subdir under the job root; empty = job root.
pub fn normalize_subdir(raw: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for part in raw.replace('\\', "/").split('/') {
        let p = part.trim();
        if p.is_empty() || p == "." {
            continue;
        }
        if p == ".." {
            let _ = parts.pop();
            continue;
        }
        parts.push(p.to_string());
    }
    parts.join("/")
}

pub fn brochure_rel_norm(subdir: &str, export_name: &str) -> String {
    let name = normalize_export_name(export_name);
    let sub = normalize_subdir(subdir);
    if sub.is_empty() {
        name
    } else {
        format!("{sub}/{name}")
    }
}

pub fn plan_brochure(
    settings: &BrochureSettings,
    source_path: Option<&Path>,
    source_size: u64,
    is_append: bool,
    remote_exists: bool,
    existing_rels: &[String],
) -> BrochurePlan {
    if !settings.enabled {
        return BrochurePlan::Skip(BrochureSkipReason::Disabled);
    }
    if is_append {
        return BrochurePlan::Skip(BrochureSkipReason::Append);
    }
    let Some(path) = source_path.filter(|p| p.is_file()) else {
        return BrochurePlan::Skip(BrochureSkipReason::MissingSource);
    };
    if source_size == 0 {
        return BrochurePlan::Skip(BrochureSkipReason::MissingSource);
    }
    if remote_exists {
        return BrochurePlan::Skip(BrochureSkipReason::RemoteExists);
    }

    let rel_norm = brochure_rel_norm(&settings.subdir, &settings.export_name);
    let entry = BrochureUploadEntry {
        local_path: path.to_path_buf(),
        rel_norm: rel_norm.clone(),
        size: source_size,
    };

    if let Some(index) = existing_rels.iter().position(|r| r == &rel_norm) {
        BrochurePlan::Replace { index, entry }
    } else {
        BrochurePlan::Add(entry)
    }
}

/// Apply a plan to a Dropbox upload list; returns whether an entry was added/replaced.
pub fn apply_brochure_plan(
    files: &mut Vec<UploadFile>,
    remote_base: &str,
    plan: BrochurePlan,
) -> bool {
    match plan {
        BrochurePlan::Skip(reason) => {
            log_skip(reason);
            false
        }
        BrochurePlan::Add(entry) => {
            let dropbox_path = join_dropbox_path(remote_base, &entry.rel_norm);
            files.push(UploadFile {
                local_path: entry.local_path,
                dropbox_path,
                size: entry.size,
                rel_norm: entry.rel_norm.clone(),
            });
            files.sort_by(|a, b| a.rel_norm.cmp(&b.rel_norm));
            logging::log_info(&format!(
                "Infobroschüre wird hochgeladen: {}",
                entry.rel_norm
            ));
            true
        }
        BrochurePlan::Replace { index, entry } => {
            logging::log_warn(&format!(
                "Infobroschüre überschreibt vorhandenen Medien-Eintrag '{}'.",
                entry.rel_norm
            ));
            let dropbox_path = join_dropbox_path(remote_base, &entry.rel_norm);
            if let Some(slot) = files.get_mut(index) {
                slot.local_path = entry.local_path;
                slot.dropbox_path = dropbox_path;
                slot.size = entry.size;
                slot.rel_norm = entry.rel_norm.clone();
            }
            logging::log_info(&format!(
                "Infobroschüre wird hochgeladen: {}",
                entry.rel_norm
            ));
            true
        }
    }
}

/// Resolve settings + source and plan injection for an Erst-Upload file list.
pub fn inject_brochure_for_upload(
    files: &mut Vec<UploadFile>,
    remote_base: &str,
    is_append: bool,
    remote_exists: bool,
) -> bool {
    let settings = brochure_settings_from_runtime();
    let source = brochure_source_path().ok();
    let source_size = source
        .as_ref()
        .and_then(|p| fs::metadata(p).ok())
        .map(|m| m.len())
        .unwrap_or(0);
    let existing: Vec<String> = files.iter().map(|f| f.rel_norm.clone()).collect();
    let plan = plan_brochure(
        &settings,
        source.as_deref(),
        source_size,
        is_append,
        remote_exists,
        &existing,
    );
    apply_brochure_plan(files, remote_base, plan)
}

/// Proxied-session variant: returns `(rel_norm, local_path, size)` to push, or None.
pub fn resolve_brochure_proxied_entry(
    is_append: bool,
    existing_names: &[String],
) -> Option<(String, PathBuf, u64)> {
    let settings = brochure_settings_from_runtime();
    let source = brochure_source_path().ok();
    let source_size = source
        .as_ref()
        .and_then(|p| fs::metadata(p).ok())
        .map(|m| m.len())
        .unwrap_or(0);
    // Proxied has no Dropbox remote-exists check; resume uses checkpoint.
    let plan = plan_brochure(
        &settings,
        source.as_deref(),
        source_size,
        is_append,
        false,
        existing_names,
    );
    match plan {
        BrochurePlan::Skip(reason) => {
            log_skip(reason);
            None
        }
        BrochurePlan::Add(entry) => {
            logging::log_info(&format!(
                "Infobroschüre wird hochgeladen: {}",
                entry.rel_norm
            ));
            Some((entry.rel_norm, entry.local_path, entry.size))
        }
        BrochurePlan::Replace { entry, .. } => {
            logging::log_warn(&format!(
                "Infobroschüre überschreibt vorhandenen Medien-Eintrag '{}'.",
                entry.rel_norm
            ));
            logging::log_info(&format!(
                "Infobroschüre wird hochgeladen: {}",
                entry.rel_norm
            ));
            Some((entry.rel_norm, entry.local_path, entry.size))
        }
    }
}

fn log_skip(reason: BrochureSkipReason) {
    match reason {
        BrochureSkipReason::Disabled => {}
        BrochureSkipReason::Append => {
            // Silent by design — Append must never inject.
        }
        BrochureSkipReason::MissingSource => {
            logging::log_warn(
                "Infobroschüre aktiv, aber keine PDF hinterlegt — Upload ohne Broschüre.",
            );
        }
        BrochureSkipReason::RemoteExists => {
            logging::log_info(
                "Infobroschüre remote bereits vorhanden — Injektion übersprungen (Idempotenz).",
            );
        }
    }
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Defaults used by `setting_default` / docs.
#[allow(dead_code)]
pub fn brochure_setting_default(key: &str) -> Option<&'static str> {
    match key {
        BROCHURE_ENABLED_KEY => Some("false"),
        BROCHURE_EXPORT_NAME_KEY => Some(DEFAULT_EXPORT_NAME),
        BROCHURE_SUBDIR_KEY => Some(""),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::setting_default;
    use tempfile::tempdir;

    fn settings(enabled: bool, export: &str, subdir: &str) -> BrochureSettings {
        BrochureSettings {
            enabled,
            export_name: normalize_export_name(export),
            subdir: normalize_subdir(subdir),
        }
    }

    fn write_pdf(dir: &Path, name: &str, bytes: usize) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, vec![b'%'; bytes.max(1)]).unwrap();
        path
    }

    #[test]
    fn defaults_match_plan() {
        assert_eq!(
            brochure_setting_default(BROCHURE_ENABLED_KEY),
            Some("false")
        );
        assert_eq!(
            brochure_setting_default(BROCHURE_EXPORT_NAME_KEY),
            Some(DEFAULT_EXPORT_NAME)
        );
        assert_eq!(brochure_setting_default(BROCHURE_SUBDIR_KEY), Some(""));
        assert_eq!(setting_default(BROCHURE_ENABLED_KEY), Some("false"));
    }

    #[test]
    fn disabled_skips() {
        let plan = plan_brochure(
            &settings(false, DEFAULT_EXPORT_NAME, ""),
            Some(Path::new("x.pdf")),
            100,
            false,
            false,
            &[],
        );
        assert_eq!(plan, BrochurePlan::Skip(BrochureSkipReason::Disabled));
    }

    #[test]
    fn append_never_injects() {
        let dir = tempdir().unwrap();
        let pdf = write_pdf(dir.path(), "a.pdf", 64);
        let plan = plan_brochure(
            &settings(true, DEFAULT_EXPORT_NAME, ""),
            Some(&pdf),
            64,
            true,
            false,
            &[],
        );
        assert_eq!(plan, BrochurePlan::Skip(BrochureSkipReason::Append));
    }

    #[test]
    fn missing_source_warns_via_skip() {
        let plan = plan_brochure(
            &settings(true, DEFAULT_EXPORT_NAME, ""),
            None,
            0,
            false,
            false,
            &[],
        );
        assert_eq!(plan, BrochurePlan::Skip(BrochureSkipReason::MissingSource));
    }

    #[test]
    fn retry_idempotent_when_remote_exists() {
        let dir = tempdir().unwrap();
        let pdf = write_pdf(dir.path(), "a.pdf", 64);
        let plan = plan_brochure(
            &settings(true, DEFAULT_EXPORT_NAME, ""),
            Some(&pdf),
            64,
            false,
            true,
            &[],
        );
        assert_eq!(plan, BrochurePlan::Skip(BrochureSkipReason::RemoteExists));
    }

    #[test]
    fn export_name_hardening() {
        assert_eq!(normalize_export_name(""), DEFAULT_EXPORT_NAME);
        assert_eq!(normalize_export_name("  "), DEFAULT_EXPORT_NAME);
        assert_eq!(normalize_export_name("Info"), "Info.pdf");
        assert_eq!(normalize_export_name("Info.PDF"), "Info.pdf");
        assert_eq!(
            normalize_export_name(r"C:\evil\..\Broschuere.pdf"),
            "Broschuere.pdf"
        );
        assert_eq!(normalize_export_name("/abs/x.pdf"), "x.pdf");
        assert_eq!(normalize_export_name(".pdf"), DEFAULT_EXPORT_NAME);
    }

    #[test]
    fn subdir_empty_vs_set() {
        assert_eq!(normalize_subdir(""), "");
        assert_eq!(normalize_subdir("  /  "), "");
        assert_eq!(normalize_subdir(r"Docs\Info"), "Docs/Info");
        assert_eq!(normalize_subdir("../secret/../Info"), "Info");
        assert_eq!(
            brochure_rel_norm("", "Infobroschuere.pdf"),
            "Infobroschuere.pdf"
        );
        assert_eq!(
            brochure_rel_norm("Info", "Infobroschuere.pdf"),
            "Info/Infobroschuere.pdf"
        );
    }

    #[test]
    fn five_mb_limit_rejects_import() {
        let dir = tempdir().unwrap();
        let big = dir.path().join("big.pdf");
        let mut data = vec![0u8; (MAX_BROCHURE_BYTES as usize) + 1];
        data[0] = b'%';
        fs::write(&big, &data).unwrap();
        let err = validate_pdf_candidate(&big).unwrap_err();
        assert!(err.contains("groß") || err.contains("MB"));
        let ok = write_pdf(dir.path(), "ok.pdf", 128);
        assert!(validate_pdf_candidate(&ok).is_ok());
        let txt = dir.path().join("x.txt");
        fs::write(&txt, b"nope").unwrap();
        assert!(validate_pdf_candidate(&txt).is_err());
    }

    #[test]
    fn collision_replaces_media_entry() {
        let dir = tempdir().unwrap();
        let pdf = write_pdf(dir.path(), "brochure.pdf", 32);
        let existing = vec!["Infobroschuere.pdf".to_string()];
        let plan = plan_brochure(
            &settings(true, "Infobroschuere.pdf", ""),
            Some(&pdf),
            32,
            false,
            false,
            &existing,
        );
        match plan {
            BrochurePlan::Replace { index, entry } => {
                assert_eq!(index, 0);
                assert_eq!(entry.rel_norm, "Infobroschuere.pdf");
            }
            other => panic!("expected Replace, got {other:?}"),
        }
    }

    #[test]
    fn apply_add_and_replace() {
        let dir = tempdir().unwrap();
        let pdf = write_pdf(dir.path(), "b.pdf", 16);
        let mut files = vec![UploadFile {
            local_path: dir.path().join("photo.jpg"),
            dropbox_path: "/Job/photo.jpg".into(),
            size: 10,
            rel_norm: "photo.jpg".into(),
        }];
        let added = apply_brochure_plan(
            &mut files,
            "/Job",
            BrochurePlan::Add(BrochureUploadEntry {
                local_path: pdf.clone(),
                rel_norm: "Infobroschuere.pdf".into(),
                size: 16,
            }),
        );
        assert!(added);
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.rel_norm == "Infobroschuere.pdf"));

        let brochure_idx = files
            .iter()
            .position(|f| f.rel_norm == "Infobroschuere.pdf")
            .unwrap();
        let replaced = apply_brochure_plan(
            &mut files,
            "/Job",
            BrochurePlan::Replace {
                index: brochure_idx,
                entry: BrochureUploadEntry {
                    local_path: pdf,
                    rel_norm: "Infobroschuere.pdf".into(),
                    size: 16,
                },
            },
        );
        assert!(replaced);
    }
}
