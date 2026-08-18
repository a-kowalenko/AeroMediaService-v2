//! ATS↔AMS share handoff: `_ams_manifest.v1.json` parse + integrity gate + status outbox.
//! Spec: `docs/HANDOFF.md` (Phase 13 / P1 + P1b).

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::model::marker::{MARKER_FERTIG, MARKER_PROCESSING};
use crate::upload::checkpoint::CHECKPOINT_FILENAME;

pub const MANIFEST_FILENAME: &str = "_ams_manifest.v1.json";
pub const HANDOFF_DIRNAME: &str = ".ams-handoff";
pub const PROTOCOL_NAME: &str = "ams-handoff";
pub const SCHEMA_V1: u32 = 1;
pub const INTEGRITY_ALGO_SIZE: &str = "size";

pub const CODE_MANIFEST_MISSING_LEGACY: &str = "manifest_missing_legacy";
pub const CODE_MANIFEST_INVALID_JSON: &str = "manifest_invalid_json";
pub const CODE_MANIFEST_UNSUPPORTED_SCHEMA: &str = "manifest_unsupported_schema";
pub const CODE_FILE_MISSING: &str = "file_missing";
pub const CODE_SIZE_MISMATCH: &str = "size_mismatch";
pub const CODE_MANIFEST_REQUIRED: &str = "manifest_required";
pub const CODE_MARKER_INVALID: &str = "marker_invalid";
pub const CODE_CUSTOMER_LOOKUP_FAILED: &str = "customer_lookup_failed";
pub const CODE_CANCELLED: &str = "cancelled";
pub const CODE_APPEND_PARENT_MISSING: &str = "append_parent_missing";
pub const CODE_APPEND_PARENT_NOT_READY: &str = "append_parent_not_ready";
pub const CODE_HANDOFF_TIMEOUT: &str = "handoff_timeout";
pub const CODE_HANDOFF_NO_FERTIG: &str = "handoff_no_fertig";
pub const CODE_HANDOFF_NO_MEDIA: &str = "handoff_no_media";
pub const CODE_HANDOFF_FOLDER_MISSING: &str = "handoff_folder_missing";

pub const EXT_KIND: &str = "kind";
pub const KIND_APPEND: &str = "append";
pub const EXT_PARENT_CORRELATION_ID: &str = "parent_correlation_id";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    /// No manifest → legacy Stability + Claim.
    Legacy,
    /// Manifest present and integrity OK → Claim; Stability may be skipped.
    Ready { correlation_id: String },
    /// Manifest present but invalid / incomplete, or required but missing.
    Rejected {
        code: &'static str,
        message: String,
        correlation_id: Option<String>,
    },
}

/// Status-Outbox states written under `aktuell/.ams-handoff/<correlation_id>.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxState {
    Accepted,
    Rejected,
    Queued,
    Uploading,
    Completed,
    Failed,
}

impl OutboxState {
    pub fn as_str(self) -> &'static str {
        match self {
            OutboxState::Accepted => "accepted",
            OutboxState::Rejected => "rejected",
            OutboxState::Queued => "queued",
            OutboxState::Uploading => "uploading",
            OutboxState::Completed => "completed",
            OutboxState::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct OutboxError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct OutboxAmsMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_id: Option<String>,
    /// `erfolg` | `fehler` | `abgebrochen` | null
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatusOutboxV1 {
    pub schema: u32,
    pub correlation_id: String,
    pub updated_at: String,
    pub state: OutboxState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<OutboxError>,
    #[serde(default)]
    pub ams: OutboxAmsMeta,
    #[serde(default)]
    pub extensions: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HandoffManifestV1 {
    pub schema: u32,
    pub protocol: String,
    pub correlation_id: String,
    pub producer: ProducerInfo,
    #[serde(default)]
    pub producer_ref: ProducerRef,
    pub created_at: String,
    pub folder_name: String,
    pub integrity: IntegrityBlock,
    #[serde(default)]
    pub marker_hint: MarkerHint,
    #[serde(default)]
    pub extensions: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProducerInfo {
    pub app: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProducerRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vorgang_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrityBlock {
    pub algo: String,
    pub files: Vec<ManifestFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestFileEntry {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MarkerHint {
    #[serde(default)]
    pub format: String,
    #[serde(default, rename = "type")]
    pub marker_type: String,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("Manifest nicht parsebar: {0}")]
    InvalidJson(String),
    #[error("{0}")]
    UnsupportedSchema(String),
    #[error("{0}")]
    Invalid(String),
    #[error("file_missing: '{0}' fehlt im Job-Ordner.")]
    FileMissing(String),
    #[error("size_mismatch: '{path}' erwartet {expected} Bytes, gefunden {actual}.")]
    SizeMismatch {
        path: String,
        expected: u64,
        actual: u64,
    },
}

impl ManifestError {
    pub fn code(&self) -> &'static str {
        match self {
            ManifestError::Io(_) | ManifestError::InvalidJson(_) | ManifestError::Invalid(_) => {
                CODE_MANIFEST_INVALID_JSON
            }
            ManifestError::UnsupportedSchema(_) => CODE_MANIFEST_UNSUPPORTED_SCHEMA,
            ManifestError::FileMissing(_) => CODE_FILE_MISSING,
            ManifestError::SizeMismatch { .. } => CODE_SIZE_MISMATCH,
        }
    }
}

pub fn manifest_path(job_folder: &Path) -> PathBuf {
    job_folder.join(MANIFEST_FILENAME)
}

/// Share root (`aktuell`) that contains job folders and `.ams-handoff/`.
pub fn share_root_from_job(job_folder: &Path) -> Option<PathBuf> {
    job_folder.parent().map(|p| p.to_path_buf())
}

pub fn outbox_dir(share_root: &Path) -> PathBuf {
    share_root.join(HANDOFF_DIRNAME)
}

pub fn outbox_path(share_root: &Path, correlation_id: &str) -> PathBuf {
    outbox_dir(share_root).join(format!("{correlation_id}.json"))
}

pub fn is_handoff_scan_dir(name: &str) -> bool {
    name == HANDOFF_DIRNAME
}

/// Names ignored for fingerprint / uploadable-file scan / manifest inventory.
pub fn is_ignored_handoff_name(name: &str) -> bool {
    name == MARKER_FERTIG
        || name == MARKER_PROCESSING
        || name == MANIFEST_FILENAME
        || name == CHECKPOINT_FILENAME
        || name == HANDOFF_DIRNAME
        || name == "Thumbs.db"
        || name == ".DS_Store"
        || name.starts_with(".aero_ck_")
}

/// Best-effort read of `correlation_id` from a job manifest (even if invalid).
pub fn peek_correlation_id(folder: &Path) -> Option<String> {
    let raw = fs::read_to_string(manifest_path(folder)).ok()?;
    let value: Value = serde_json::from_str(raw.trim()).ok()?;
    value
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Evaluate the P1 gate for a job folder that already has `_fertig.txt`.
pub fn evaluate_manifest_gate(folder: &Path, manifest_required: bool) -> GateDecision {
    let path = manifest_path(folder);
    if !path.is_file() {
        if manifest_required {
            return GateDecision::Rejected {
                code: CODE_MANIFEST_REQUIRED,
                message: format!(
                    "Manifest '{MANIFEST_FILENAME}' fehlt, aber manifest_required ist aktiv."
                ),
                correlation_id: None,
            };
        }
        return GateDecision::Legacy;
    }

    match load_and_validate_manifest(folder) {
        Ok(m) => {
            if is_append_manifest(&m) && parent_correlation_id(&m).is_none() {
                return GateDecision::Rejected {
                    code: CODE_APPEND_PARENT_MISSING,
                    message: "Nachreichung ohne parent_correlation_id.".into(),
                    correlation_id: Some(m.correlation_id),
                };
            }
            GateDecision::Ready {
                correlation_id: m.correlation_id,
            }
        }
        Err(e) => GateDecision::Rejected {
            code: e.code(),
            message: e.to_string(),
            correlation_id: peek_correlation_id(folder),
        },
    }
}

/// Atomically write status outbox JSON under `share_root/.ams-handoff/<correlation_id>.json`.
pub fn write_status_outbox(
    share_root: &Path,
    correlation_id: &str,
    state: OutboxState,
    error: Option<OutboxError>,
    ams: OutboxAmsMeta,
) -> io::Result<PathBuf> {
    let cid = correlation_id.trim();
    if cid.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "correlation_id is empty",
        ));
    }
    let dir = outbox_dir(share_root);
    fs::create_dir_all(&dir)?;
    let dest = outbox_path(share_root, cid);
    let doc = StatusOutboxV1 {
        schema: SCHEMA_V1,
        correlation_id: cid.to_string(),
        updated_at: Local::now().to_rfc3339(),
        state,
        error,
        ams,
        extensions: Value::Object(Default::default()),
    };
    let bytes = serde_json::to_vec_pretty(&doc)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    atomic_write(&dest, &bytes)?;
    Ok(dest)
}

/// Convenience: write outbox for a job folder (share root = parent). No-op if cid empty / no parent.
pub fn write_job_outbox(
    job_folder: &Path,
    correlation_id: Option<&str>,
    state: OutboxState,
    error: Option<OutboxError>,
    archive: Option<&str>,
) {
    let Some(cid) = correlation_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    let Some(share_root) = share_root_from_job(job_folder) else {
        return;
    };
    let ams = OutboxAmsMeta {
        history_id: None,
        archive: archive.map(|s| s.to_string()),
    };
    if let Err(e) = write_status_outbox(&share_root, cid, state, error, ams) {
        crate::storage::logging::log_warn(&format!(
            "Handoff-Outbox schreiben fehlgeschlagen (correlation_id={cid}, state={}): {e}",
            state.as_str()
        ));
    }
}

pub fn read_status_outbox(
    share_root: &Path,
    correlation_id: &str,
) -> Result<StatusOutboxV1, ManifestError> {
    let path = outbox_path(share_root, correlation_id.trim());
    let raw = fs::read_to_string(&path)?;
    serde_json::from_str(raw.trim()).map_err(|e| ManifestError::InvalidJson(e.to_string()))
}

fn atomic_write(dest: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_name = format!(
        ".ams_outbox_tmp_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let tmp = dest
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(tmp_name);
    let result = (|| {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        if dest.exists() {
            fs::remove_file(dest)?;
        }
        fs::rename(&tmp, dest)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn load_and_validate_manifest(folder: &Path) -> Result<HandoffManifestV1, ManifestError> {
    let path = manifest_path(folder);
    let raw = fs::read_to_string(&path)?;
    let manifest = parse_manifest_json(&raw)?;
    validate_integrity(folder, &manifest)?;
    Ok(manifest)
}

pub fn parse_manifest_json(raw: &str) -> Result<HandoffManifestV1, ManifestError> {
    let value: Value =
        serde_json::from_str(raw.trim()).map_err(|e| ManifestError::InvalidJson(e.to_string()))?;

    let schema = value
        .get("schema")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            ManifestError::InvalidJson("Feld 'schema' fehlt oder ist ungültig.".into())
        })?;
    if schema != SCHEMA_V1 as u64 {
        return Err(ManifestError::UnsupportedSchema(format!(
            "Unsupported Manifest-Schema {schema} (erwartet {SCHEMA_V1})."
        )));
    }

    let protocol = value
        .get("protocol")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if protocol != PROTOCOL_NAME {
        return Err(ManifestError::Invalid(format!(
            "Ungültiges protocol '{protocol}' (erwartet '{PROTOCOL_NAME}')."
        )));
    }

    let manifest: HandoffManifestV1 = serde_json::from_value(value)
        .map_err(|e| ManifestError::InvalidJson(format!("Manifest-Schema ungültig: {e}")))?;

    if manifest.correlation_id.trim().is_empty() {
        return Err(ManifestError::Invalid(
            "Feld 'correlation_id' fehlt oder ist leer.".into(),
        ));
    }
    if manifest.integrity.algo.trim() != INTEGRITY_ALGO_SIZE {
        return Err(ManifestError::UnsupportedSchema(format!(
            "Unsupported integrity.algo '{}' (v1 unterstützt nur '{INTEGRITY_ALGO_SIZE}').",
            manifest.integrity.algo
        )));
    }

    Ok(manifest)
}

pub fn manifest_kind(manifest: &HandoffManifestV1) -> Option<&str> {
    manifest
        .extensions
        .get(EXT_KIND)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub fn is_append_manifest(manifest: &HandoffManifestV1) -> bool {
    manifest_kind(manifest)
        .map(|k| k.eq_ignore_ascii_case(KIND_APPEND))
        .unwrap_or(false)
}

pub fn parent_correlation_id(manifest: &HandoffManifestV1) -> Option<String> {
    manifest
        .extensions
        .get(EXT_PARENT_CORRELATION_ID)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Best-effort append parent id from a job folder.
pub fn peek_parent_correlation_id(folder: &Path) -> Option<String> {
    let raw = fs::read_to_string(manifest_path(folder)).ok()?;
    let value: Value = serde_json::from_str(raw.trim()).ok()?;
    value
        .get("extensions")
        .and_then(|ext| ext.get(EXT_PARENT_CORRELATION_ID))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn validate_integrity(
    folder: &Path,
    manifest: &HandoffManifestV1,
) -> Result<(), ManifestError> {
    for entry in &manifest.integrity.files {
        let rel = normalize_rel_path(&entry.path);
        if rel.is_empty() || Path::new(&rel).is_absolute() || has_parent_component(&rel) {
            return Err(ManifestError::Invalid(format!(
                "Ungültiger Manifest-Pfad '{}'.",
                entry.path
            )));
        }
        let full = folder.join(Path::new(&rel));
        let meta = match fs::metadata(&full) {
            Ok(m) if m.is_file() => m,
            _ => return Err(ManifestError::FileMissing(entry.path.clone())),
        };
        let actual = meta.len();
        if actual != entry.size {
            return Err(ManifestError::SizeMismatch {
                path: entry.path.clone(),
                expected: entry.size,
                actual,
            });
        }
    }
    Ok(())
}

fn normalize_rel_path(raw: &str) -> String {
    let replaced = raw.replace('\\', "/");
    let trimmed = replaced.trim().trim_start_matches('/');
    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop();
            continue;
        }
        parts.push(part);
    }
    parts.join("/")
}

fn has_parent_component(rel: &str) -> bool {
    Path::new(rel)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn sample_manifest(files: Vec<ManifestFileEntry>) -> HandoffManifestV1 {
        HandoffManifestV1 {
            schema: 1,
            protocol: PROTOCOL_NAME.into(),
            correlation_id: "11111111-2222-3333-4444-555555555555".into(),
            producer: ProducerInfo {
                app: "AeroTandemStudio".into(),
                version: "0.2.16".into(),
            },
            producer_ref: ProducerRef {
                vorgang_id: Some(42),
            },
            created_at: "2026-08-15T00:00:00+02:00".into(),
            folder_name: "job".into(),
            integrity: IntegrityBlock {
                algo: INTEGRITY_ALGO_SIZE.into(),
                files,
            },
            marker_hint: MarkerHint {
                format: "pure_contact".into(),
                marker_type: "Handcam".into(),
            },
            extensions: json!({}),
        }
    }

    fn write_manifest(dir: &Path, m: &HandoffManifestV1) {
        fs::write(manifest_path(dir), serde_json::to_string_pretty(m).unwrap()).unwrap();
    }

    #[test]
    fn parse_valid_manifest() {
        let m = sample_manifest(vec![ManifestFileEntry {
            path: "Handcam_Video/a.mp4".into(),
            size: 10,
        }]);
        let raw = serde_json::to_string(&m).unwrap();
        let parsed = parse_manifest_json(&raw).unwrap();
        assert_eq!(parsed.correlation_id, m.correlation_id);
        assert_eq!(parsed.integrity.files.len(), 1);
    }

    #[test]
    fn reject_invalid_json() {
        let err = parse_manifest_json("{nope").unwrap_err();
        assert_eq!(err.code(), CODE_MANIFEST_INVALID_JSON);
    }

    #[test]
    fn reject_unsupported_schema() {
        let raw = r#"{"schema":99,"protocol":"ams-handoff","correlation_id":"x","producer":{"app":"a","version":"1"},"created_at":"t","folder_name":"f","integrity":{"algo":"size","files":[]}}"#;
        let err = parse_manifest_json(raw).unwrap_err();
        assert_eq!(err.code(), CODE_MANIFEST_UNSUPPORTED_SCHEMA);
    }

    #[test]
    fn gate_legacy_without_manifest() {
        let dir = tempdir().unwrap();
        assert_eq!(
            evaluate_manifest_gate(dir.path(), false),
            GateDecision::Legacy
        );
    }

    #[test]
    fn gate_required_without_manifest_rejects() {
        let dir = tempdir().unwrap();
        match evaluate_manifest_gate(dir.path(), true) {
            GateDecision::Rejected {
                code,
                correlation_id,
                ..
            } => {
                assert_eq!(code, CODE_MANIFEST_REQUIRED);
                assert!(correlation_id.is_none());
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn gate_file_missing() {
        let dir = tempdir().unwrap();
        write_manifest(
            dir.path(),
            &sample_manifest(vec![ManifestFileEntry {
                path: "Handcam_Video/missing.mp4".into(),
                size: 1,
            }]),
        );
        match evaluate_manifest_gate(dir.path(), false) {
            GateDecision::Rejected {
                code,
                message,
                correlation_id,
            } => {
                assert_eq!(code, CODE_FILE_MISSING);
                assert!(message.contains("missing.mp4"));
                assert_eq!(
                    correlation_id.as_deref(),
                    Some("11111111-2222-3333-4444-555555555555")
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn gate_size_mismatch() {
        let dir = tempdir().unwrap();
        let media = dir.path().join("Handcam_Video");
        fs::create_dir_all(&media).unwrap();
        fs::write(media.join("a.mp4"), b"abcd").unwrap();
        write_manifest(
            dir.path(),
            &sample_manifest(vec![ManifestFileEntry {
                path: "Handcam_Video/a.mp4".into(),
                size: 99,
            }]),
        );
        match evaluate_manifest_gate(dir.path(), false) {
            GateDecision::Rejected { code, .. } => assert_eq!(code, CODE_SIZE_MISMATCH),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn outbox_atomic_write_and_read_roundtrip() {
        let root = tempdir().unwrap();
        let job = root.path().join("job");
        fs::create_dir(&job).unwrap();
        let cid = "cccccccc-dddd-eeee-ffff-000000000001";
        let path = write_status_outbox(
            root.path(),
            cid,
            OutboxState::Rejected,
            Some(OutboxError {
                code: CODE_SIZE_MISMATCH.into(),
                message: "too small".into(),
            }),
            OutboxAmsMeta::default(),
        )
        .unwrap();
        assert_eq!(path, outbox_path(root.path(), cid));
        assert!(path.is_file());
        assert!(!is_handoff_scan_dir("job"));
        assert!(is_handoff_scan_dir(HANDOFF_DIRNAME));

        let read = read_status_outbox(root.path(), cid).unwrap();
        assert_eq!(read.schema, 1);
        assert_eq!(read.correlation_id, cid);
        assert_eq!(read.state, OutboxState::Rejected);
        assert_eq!(read.error.as_ref().unwrap().code, CODE_SIZE_MISMATCH);

        write_job_outbox(
            &job,
            Some(cid),
            OutboxState::Completed,
            None,
            Some("erfolg"),
        );
        let done = read_status_outbox(root.path(), cid).unwrap();
        assert_eq!(done.state, OutboxState::Completed);
        assert_eq!(done.ams.archive.as_deref(), Some("erfolg"));
        // Outbox file remains after "final" write (archive move is separate).
        assert!(outbox_path(root.path(), cid).is_file());
    }

    #[test]
    fn gate_ready_when_sizes_match() {
        let dir = tempdir().unwrap();
        let media = dir.path().join("Handcam_Video");
        fs::create_dir_all(&media).unwrap();
        fs::write(media.join("a.mp4"), b"abcd").unwrap();
        write_manifest(
            dir.path(),
            &sample_manifest(vec![ManifestFileEntry {
                path: "Handcam_Video/a.mp4".into(),
                size: 4,
            }]),
        );
        match evaluate_manifest_gate(dir.path(), false) {
            GateDecision::Ready { correlation_id } => {
                assert_eq!(correlation_id, "11111111-2222-3333-4444-555555555555");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn gate_rejects_append_without_parent_id() {
        let dir = tempdir().unwrap();
        let media = dir.path().join("Handcam_Foto");
        fs::create_dir_all(&media).unwrap();
        fs::write(media.join("a.jpg"), b"jpeg").unwrap();
        let mut m = sample_manifest(vec![ManifestFileEntry {
            path: "Handcam_Foto/a.jpg".into(),
            size: 4,
        }]);
        m.extensions = json!({ "kind": "append" });
        write_manifest(dir.path(), &m);
        match evaluate_manifest_gate(dir.path(), false) {
            GateDecision::Rejected { code, correlation_id, .. } => {
                assert_eq!(code, CODE_APPEND_PARENT_MISSING);
                assert_eq!(
                    correlation_id.as_deref(),
                    Some("11111111-2222-3333-4444-555555555555")
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn append_extensions_parse() {
        let mut m = sample_manifest(vec![]);
        m.extensions = json!({
            "kind": "append",
            "parent_correlation_id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
        });
        assert!(is_append_manifest(&m));
        assert_eq!(
            parent_correlation_id(&m).as_deref(),
            Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
        );
    }

    #[test]
    fn ignore_list_covers_handoff_artifacts() {
        assert!(is_ignored_handoff_name(MANIFEST_FILENAME));
        assert!(is_ignored_handoff_name(HANDOFF_DIRNAME));
        assert!(is_ignored_handoff_name("Thumbs.db"));
        assert!(is_ignored_handoff_name(".DS_Store"));
        assert!(is_handoff_scan_dir(HANDOFF_DIRNAME));
        assert!(!is_ignored_handoff_name("clip.mp4"));
    }
}
