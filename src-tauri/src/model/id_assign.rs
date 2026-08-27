//! ID-assign pipeline (Phase 19c): media layout → rename → manifest → API-ID `_fertig.txt`.
//!
//! Pure-Contact assign stays in `storage/customers.rs`. Review-Dialog UI is 19d;
//! TM/VS/Dropzone overrides arrive as optional parameters.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::model::crew::CrewMember;
use crate::model::folder_rename::{predict_from_folder_name, PredictOptions};
use crate::model::handoff::{
    atomic_write_file, evaluate_manifest_gate, write_ams_assign_manifest, GateDecision,
    MANIFEST_FILENAME,
};
use crate::model::marker::{
    marker_paths, normalize_marker_type, MARKER_FERTIG, MARKER_PROCESSING, MEDIA_FLAG_KEYS,
};

pub const SUBDIR_HANDCAM_VIDEO: &str = "Handcam_Video";
pub const SUBDIR_OUTSIDE_VIDEO: &str = "Outside_Video";
pub const SUBDIR_HANDCAM_FOTO: &str = "Handcam_Foto";
pub const SUBDIR_OUTSIDE_FOTO: &str = "Outside_Foto";

const VIDEO_EXTS: &[&str] = &[
    ".mp4", ".mov", ".mkv", ".avi", ".m4v", ".webm", ".mts", ".m2ts",
];
const PHOTO_EXTS: &[&str] = &[
    ".jpg", ".jpeg", ".png", ".bmp", ".tiff", ".tif", ".webp", ".heic", ".dng",
];

const CANONICAL_MEDIA_SUBDIRS: &[&str] = &[
    SUBDIR_HANDCAM_VIDEO,
    SUBDIR_OUTSIDE_VIDEO,
    SUBDIR_HANDCAM_FOTO,
    SUBDIR_OUTSIDE_FOTO,
    "Preview_Video",
    "Preview_Foto",
];

/// Optional TM/VS/Dropzone from the Review dialog (19d) or tests.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IdAssignOverride {
    #[serde(default)]
    pub tandemmaster: Option<String>,
    #[serde(default)]
    pub videospringer: Option<String>,
    /// Dropzone letter without underscore, e.g. `"G"`.
    #[serde(default)]
    pub dropzone_suffix: Option<String>,
}

/// Non-mutating preview for the Assign Review dialog (Phase 19d).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdAssignPreview {
    pub customer_id: String,
    pub customer_label: String,
    pub folder_path: String,
    pub folder_name: String,
    pub preview_folder_name: String,
    pub needs_review: bool,
    pub review_reasons: Vec<String>,
    pub tandemmaster: Option<String>,
    pub videospringer: Option<String>,
    pub dropzone_suffix: Option<String>,
    pub tm_confidence: f32,
    pub vs_confidence: f32,
    pub outside_video: bool,
    /// Outside-Video → VS recommended in Review (optional; omitted from name if empty).
    pub vs_required: bool,
    /// Always true — TM/VS are optional and may be omitted from the folder name.
    pub can_confirm: bool,
    pub booking_date: String,
    pub crew: Vec<CrewMember>,
    /// Guest tokens skipped during prediction (Phase 19e transparency).
    #[serde(default)]
    pub skipped_guest_tokens: Vec<String>,
    /// Crew taken only from tokens after `TA`/`TD`.
    #[serde(default)]
    pub structured_crew_zone: bool,
}

/// Customer fields needed for the ID pipeline (avoids circular deps on CustomerStore).
#[derive(Debug, Clone)]
pub struct IdAssignCustomer<'a> {
    pub vorname: &'a str,
    pub nachname: &'a str,
    pub kunden_id: &'a str,
    pub booking_id: &'a str,
    pub booking_date: &'a str,
    pub typ: &'a str,
    pub handcam_foto: bool,
    pub handcam_video: bool,
    pub outside_foto: bool,
    pub outside_video: bool,
    pub ist_bezahlt_handcam_foto: bool,
    pub ist_bezahlt_handcam_video: bool,
    pub ist_bezahlt_outside_foto: bool,
    pub ist_bezahlt_outside_video: bool,
}

#[derive(Debug, Clone)]
pub struct IdAssignResult {
    pub folder_path: PathBuf,
    pub fertig_path: PathBuf,
    pub correlation_id: String,
    pub folder_name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum IdAssignError {
    #[error("{0}")]
    Message(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Manifest(#[from] crate::model::handoff::ManifestError),
}

impl IdAssignError {
    fn msg(s: impl AsRef<str>) -> Self {
        IdAssignError::Message(s.as_ref().to_string())
    }
}

/// Resolve TM/VS/Dropzone from predictor + optional Review overrides.
pub fn resolve_crew_fields(
    folder_name: &str,
    customer: &IdAssignCustomer<'_>,
    crew: &[CrewMember],
    override_opts: Option<&IdAssignOverride>,
) -> (crate::model::folder_rename::FolderRenamePrediction, Option<String>, Option<String>, Option<String>)
{
    let prediction = predict_from_folder_name(
        folder_name,
        crew,
        PredictOptions {
            outside_video: customer.outside_video,
            guest_vorname: Some(customer.vorname.to_string()),
            guest_nachname: Some(customer.nachname.to_string()),
        },
    );
    // When an override is provided (Review / confirm), its fields are authoritative:
    // missing/empty → omit from folder name (no fallback to prediction).
    let tm = if let Some(o) = override_opts {
        o.tandemmaster
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    } else {
        prediction.tandemmaster.clone()
    };
    let vs = if let Some(o) = override_opts {
        o.videospringer
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    } else {
        prediction.videospringer.clone()
    };
    let dropzone = if let Some(o) = override_opts {
        o.dropzone_suffix
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    } else {
        prediction.dropzone_suffix.clone()
    };
    (prediction, tm, vs, dropzone)
}

/// Preview target folder name + review flags without mutating the job folder.
pub fn preview_id_assign(
    folder: &Path,
    customer_id: &str,
    customer: &IdAssignCustomer<'_>,
    crew: &[CrewMember],
    override_opts: Option<&IdAssignOverride>,
) -> Result<IdAssignPreview, IdAssignError> {
    if !folder.is_dir() {
        return Err(IdAssignError::msg(format!(
            "Zielordner existiert nicht: {}",
            folder.display()
        )));
    }
    if customer.kunden_id.trim().is_empty() || customer.booking_id.trim().is_empty() {
        return Err(IdAssignError::msg(
            "ID-Pipeline erfordert kunden_id und booking_id.",
        ));
    }

    let folder_name = folder
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let (prediction, tm, vs, dropzone) =
        resolve_crew_fields(&folder_name, customer, crew, override_opts);

    let vs_required = customer.outside_video;
    // TM/VS are optional: without selection they are simply omitted from the folder name.
    let can_confirm = true;

    let preview_folder_name = build_job_folder_name(
        customer.vorname,
        customer.nachname,
        tm.as_deref().unwrap_or(""),
        vs.as_deref().unwrap_or(""),
        customer.booking_date,
        customer.outside_video,
        dropzone.as_deref().unwrap_or(""),
    );

    Ok(IdAssignPreview {
        customer_id: customer_id.to_string(),
        customer_label: guest_display_name(customer.vorname, customer.nachname),
        folder_path: folder.to_string_lossy().to_string(),
        folder_name,
        preview_folder_name,
        needs_review: prediction.needs_review,
        review_reasons: prediction.review_reasons,
        tandemmaster: tm,
        videospringer: vs,
        dropzone_suffix: dropzone,
        tm_confidence: prediction.tm_confidence,
        vs_confidence: prediction.vs_confidence,
        outside_video: customer.outside_video,
        vs_required,
        can_confirm,
        booking_date: customer.booking_date.to_string(),
        crew: crew.to_vec(),
        skipped_guest_tokens: prediction.skipped_guest_tokens,
        structured_crew_zone: prediction.structured_crew_zone,
    })
}

/// Full ID pipeline on an existing job folder. Strict order: layout → rename → manifest → marker.
pub fn run_id_assign_pipeline(
    folder: &Path,
    customer: &IdAssignCustomer<'_>,
    crew: &[CrewMember],
    override_opts: Option<&IdAssignOverride>,
) -> Result<IdAssignResult, IdAssignError> {
    if !folder.is_dir() {
        return Err(IdAssignError::msg(format!(
            "Zielordner existiert nicht: {}",
            folder.display()
        )));
    }
    if customer.kunden_id.trim().is_empty() || customer.booking_id.trim().is_empty() {
        return Err(IdAssignError::msg(
            "ID-Pipeline erfordert kunden_id und booking_id.",
        ));
    }

    // 1) Media layout
    rearrange_media_layout(folder, customer)?;

    // 2) Resolve crew names + build target name + rename
    let folder_name_before = folder
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let (_prediction, tm, vs, dropzone) =
        resolve_crew_fields(&folder_name_before, customer, crew, override_opts);

    let target_name = build_job_folder_name(
        customer.vorname,
        customer.nachname,
        tm.as_deref().unwrap_or(""),
        vs.as_deref().unwrap_or(""),
        customer.booking_date,
        customer.outside_video,
        dropzone.as_deref().unwrap_or(""),
    );
    let renamed = rename_job_folder(folder, &target_name)?;
    let folder_name = renamed
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(target_name.as_str())
        .to_string();

    // 3) Manifest (before marker)
    let marker_type = resolve_marker_type(customer);
    let (correlation_id, _) = write_ams_assign_manifest(&renamed, &folder_name, &marker_type)?;

    // 4) API-ID `_fertig.txt` last — only after manifest is ready
    let content = build_api_id_marker_json(customer, &marker_type)?;
    let fertig_path = write_fertig_marker_atomic(&renamed, &content)?;

    // Gate must be Ready for the ID path
    match evaluate_manifest_gate(&renamed, false) {
        GateDecision::Ready { .. } => {}
        other => {
            let _ = fs::remove_file(&fertig_path);
            return Err(IdAssignError::msg(format!(
                "Manifest-Gate nach Assign nicht ready: {other:?}"
            )));
        }
    }

    Ok(IdAssignResult {
        folder_path: renamed,
        fertig_path,
        correlation_id,
        folder_name,
    })
}

fn resolve_marker_type(customer: &IdAssignCustomer<'_>) -> String {
    let from_typ = normalize_marker_type(Some(customer.typ));
    if !from_typ.is_empty() {
        // Marker uses Handcam | Outside (not Handycam)
        if from_typ.eq_ignore_ascii_case("Handycam") || from_typ.eq_ignore_ascii_case("Handcam") {
            return "Handcam".into();
        }
        if from_typ.eq_ignore_ascii_case("Outside") {
            return "Outside".into();
        }
    }
    if customer.outside_video || customer.outside_foto {
        "Outside".into()
    } else {
        "Handcam".into()
    }
}

pub fn build_api_id_marker_json(
    customer: &IdAssignCustomer<'_>,
    marker_type: &str,
) -> Result<String, IdAssignError> {
    let mut map = serde_json::Map::new();
    map.insert("type".into(), json!(marker_type));
    map.insert("kunden_id".into(), json!(customer.kunden_id.trim()));
    map.insert("booking_id".into(), json!(customer.booking_id.trim()));
    map.insert("handcam_foto".into(), json!(customer.handcam_foto));
    map.insert("handcam_video".into(), json!(customer.handcam_video));
    map.insert("outside_foto".into(), json!(customer.outside_foto));
    map.insert("outside_video".into(), json!(customer.outside_video));
    map.insert(
        "ist_bezahlt_handcam_foto".into(),
        json!(customer.ist_bezahlt_handcam_foto),
    );
    map.insert(
        "ist_bezahlt_handcam_video".into(),
        json!(customer.ist_bezahlt_handcam_video),
    );
    map.insert(
        "ist_bezahlt_outside_foto".into(),
        json!(customer.ist_bezahlt_outside_foto),
    );
    map.insert(
        "ist_bezahlt_outside_video".into(),
        json!(customer.ist_bezahlt_outside_video),
    );
    // Ensure all eight media keys exist (MEDIA_FLAG_KEYS contract).
    for key in MEDIA_FLAG_KEYS {
        map.entry(key.to_string()).or_insert(json!(false));
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .map_err(|e| IdAssignError::msg(e.to_string()))
}

pub fn write_fertig_marker_atomic(
    folder_path: &Path,
    content: &str,
) -> Result<PathBuf, IdAssignError> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(IdAssignError::msg(
            "Marker-Inhalt ist leer — Datei kann nicht geschrieben werden.",
        ));
    }
    let (fertig, _) = marker_paths(folder_path);
    atomic_write_file(&fertig, trimmed.as_bytes())?;
    Ok(fertig)
}

/// Format booking date (DD.MM.YYYY / YYYY-MM-DD) or today as `YYYYMMDD`.
pub fn format_datum_yyyyymmdd(datum: &str) -> String {
    let trimmed = datum.trim();
    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() == 3 {
        if let (Ok(d), Ok(m), Ok(y)) = (
            parts[0].parse::<u32>(),
            parts[1].parse::<u32>(),
            parts[2].parse::<i32>(),
        ) {
            if let Some(date) = NaiveDate::from_ymd_opt(y, m, d) {
                return date.format("%Y%m%d").to_string();
            }
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        return date.format("%Y%m%d").to_string();
    }
    Local::now().format("%Y%m%d").to_string()
}

pub fn sanitize_filename(filename: &str) -> String {
    const INVALID: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    filename
        .chars()
        .filter(|c| !INVALID.contains(c))
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn normalize_whitespace_to_underscore(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_ws = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                out.push('_');
                prev_ws = true;
            }
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    out
}

fn guest_display_name(vorname: &str, nachname: &str) -> String {
    format!("{} {}", vorname.trim(), nachname.trim())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `{YYYYMMDD}_{Gast}[_TA_{TM}][_V_{VS}][_Dropzone]`.
/// Missing TM/VS are omitted (no empty `_TA_` / `_V_` stubs).
pub fn build_job_folder_name(
    vorname: &str,
    nachname: &str,
    tandemmaster: &str,
    videospringer: &str,
    booking_date: &str,
    outside_video: bool,
    dropzone_suffix: &str,
) -> String {
    let datum = format_datum_yyyyymmdd(booking_date);
    let gast = guest_display_name(vorname, nachname);
    let mut base = format!("{datum}_{gast}");
    let tm = tandemmaster.trim();
    if !tm.is_empty() {
        base.push_str(&format!("_TA_{tm}"));
    }
    if outside_video {
        let vs = videospringer.trim();
        if !vs.is_empty() {
            base.push_str(&format!("_V_{vs}"));
        }
    }
    let mut name = sanitize_filename(&normalize_whitespace_to_underscore(&base));
    let dz = dropzone_suffix.trim().trim_start_matches('_');
    if !dz.is_empty() {
        name.push('_');
        name.push_str(dz);
    }
    name
}

pub fn video_target_subdir(customer: &IdAssignCustomer<'_>) -> Option<&'static str> {
    match media_family(customer)? {
        "handcam" => Some(SUBDIR_HANDCAM_VIDEO),
        "outside" => Some(SUBDIR_OUTSIDE_VIDEO),
        _ => None,
    }
}

pub fn photo_target_subdir(customer: &IdAssignCustomer<'_>) -> Option<&'static str> {
    match media_family(customer)? {
        "handcam" => Some(SUBDIR_HANDCAM_FOTO),
        "outside" => Some(SUBDIR_OUTSIDE_FOTO),
        _ => None,
    }
}

/// Outside vs Handcam from booked flags, then `typ`.
pub fn media_family(customer: &IdAssignCustomer<'_>) -> Option<&'static str> {
    if customer.outside_video || customer.outside_foto {
        return Some("outside");
    }
    if customer.handcam_video || customer.handcam_foto {
        return Some("handcam");
    }
    let typ = customer.typ.trim().to_ascii_lowercase();
    if typ.contains("outside") {
        Some("outside")
    } else if typ.contains("handcam") || typ.contains("handycam") {
        Some("handcam")
    } else {
        None
    }
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_ascii_lowercase()))
        .unwrap_or_default()
}

fn is_video_ext(ext: &str) -> bool {
    VIDEO_EXTS.contains(&ext)
}

fn is_photo_ext(ext: &str) -> bool {
    PHOTO_EXTS.contains(&ext)
}

fn tree_has_ext_kind(
    job_root: &Path,
    dir: &Path,
    kind_video: bool,
) -> Result<bool, IdAssignError> {
    let entries: Vec<_> = fs::read_dir(dir)?.collect();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if is_ignored_name(&name) {
            continue;
        }
        if path.is_dir() {
            if tree_has_ext_kind(job_root, &path, kind_video)? {
                return Ok(true);
            }
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let ext = ext_of(&path);
        if kind_video && is_video_ext(&ext) {
            return Ok(true);
        }
        if !kind_video && is_photo_ext(&ext) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_ignored_name(name: &str) -> bool {
    name == MARKER_FERTIG
        || name == MARKER_PROCESSING
        || name == MANIFEST_FILENAME
        || name == "_aero_upload_checkpoint.json"
        || name == ".ams-handoff"
        || name == "Thumbs.db"
        || name == ".DS_Store"
        || name.starts_with('.')
}

fn unique_file_dest(dir: &Path, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    for n in 1..10_000 {
        let name = format!("{stem} ({n}){ext}");
        let cand = dir.join(&name);
        if !cand.exists() {
            return cand;
        }
    }
    dir.join(format!("{stem} ({})", uuid::Uuid::new_v4()))
}

/// Move media into canonical Outside/Handcam subdirs for the job family.
/// Booked kinds always get a target dir; unbooked kinds also if such files exist
/// (e.g. only Foto gebucht, aber Videos im Ordner → trotzdem `Outside_Video` / `Handcam_Video`).
pub fn rearrange_media_layout(
    job_root: &Path,
    customer: &IdAssignCustomer<'_>,
) -> Result<(), IdAssignError> {
    let Some(family) = media_family(customer) else {
        return Ok(());
    };
    let video_sub = match family {
        "handcam" => SUBDIR_HANDCAM_VIDEO,
        _ => SUBDIR_OUTSIDE_VIDEO,
    };
    let photo_sub = match family {
        "handcam" => SUBDIR_HANDCAM_FOTO,
        _ => SUBDIR_OUTSIDE_FOTO,
    };

    let video_booked = customer.outside_video || customer.handcam_video;
    let photo_booked = customer.outside_foto || customer.handcam_foto;
    let has_video = tree_has_ext_kind(job_root, job_root, true)?;
    let has_photo = tree_has_ext_kind(job_root, job_root, false)?;

    let video_dest = if video_booked || has_video {
        Some(video_sub)
    } else {
        None
    };
    let photo_dest = if photo_booked || has_photo {
        Some(photo_sub)
    } else {
        None
    };

    if video_dest.is_none() && photo_dest.is_none() {
        return Ok(());
    }

    if let Some(sub) = video_dest {
        fs::create_dir_all(job_root.join(sub))?;
    }
    if let Some(sub) = photo_dest {
        fs::create_dir_all(job_root.join(sub))?;
    }

    let mut to_move: Vec<(PathBuf, &'static str)> = Vec::new();
    collect_media_moves(job_root, job_root, video_dest, photo_dest, &mut to_move)?;

    for (src, sub) in to_move {
        let dest_dir = job_root.join(sub);
        let file_name = src
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("media")
            .to_string();
        let dest = unique_file_dest(&dest_dir, &file_name);
        if src == dest {
            continue;
        }
        // Same-directory no-op after unique resolve
        if let Some(parent) = src.parent() {
            if parent == dest_dir && !dest.exists() && src.file_name() == dest.file_name() {
                continue;
            }
        }
        fs::rename(&src, &dest).map_err(|e| {
            IdAssignError::msg(format!(
                "Medien verschieben fehlgeschlagen ({} → {}): {e}",
                src.display(),
                dest.display()
            ))
        })?;
    }

    prune_empty_dirs(job_root, job_root)?;
    Ok(())
}

fn collect_media_moves(
    job_root: &Path,
    dir: &Path,
    video_dest: Option<&'static str>,
    photo_dest: Option<&'static str>,
    out: &mut Vec<(PathBuf, &'static str)>,
) -> Result<(), IdAssignError> {
    let entries: Vec<_> = fs::read_dir(dir)?.collect();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if is_ignored_name(&name) {
            continue;
        }
        if path.is_dir() {
            collect_media_moves(job_root, &path, video_dest, photo_dest, out)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let ext = ext_of(&path);
        let rel = path.strip_prefix(job_root).unwrap_or(&path);
        let already_ok = |sub: &str| {
            rel.components()
                .next()
                .and_then(|c| c.as_os_str().to_str())
                .map(|s| s == sub)
                .unwrap_or(false)
        };

        if is_video_ext(&ext) {
            if let Some(sub) = video_dest {
                if !already_ok(sub) {
                    out.push((path, sub));
                }
            }
        } else if is_photo_ext(&ext) {
            if let Some(sub) = photo_dest {
                if !already_ok(sub) {
                    out.push((path, sub));
                }
            }
        }
    }
    Ok(())
}

fn prune_empty_dirs(job_root: &Path, dir: &Path) -> Result<(), IdAssignError> {
    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(e) => e.collect(),
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if is_ignored_name(&name) {
                continue;
            }
            prune_empty_dirs(job_root, &path)?;
            // Do not remove canonical media target dirs (even if empty) — they signal booked layout.
            let is_canonical = CANONICAL_MEDIA_SUBDIRS
                .iter()
                .any(|s| name.eq_ignore_ascii_case(s));
            if path != job_root && !is_canonical && dir_is_empty(&path) {
                let _ = fs::remove_dir(&path);
            }
        }
    }
    Ok(())
}

fn dir_is_empty(dir: &Path) -> bool {
    fs::read_dir(dir)
        .map(|mut it| it.next().is_none())
        .unwrap_or(false)
}

/// Rename job folder to `target_name`, appending ` (n)` on conflict.
pub fn rename_job_folder(folder: &Path, target_name: &str) -> Result<PathBuf, IdAssignError> {
    let parent = folder.parent().ok_or_else(|| {
        IdAssignError::msg(format!("Kein Elternordner für {}", folder.display()))
    })?;
    let current_name = folder
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if current_name == target_name {
        return Ok(folder.to_path_buf());
    }

    let dest = unique_folder_path(parent, target_name);
    if dest == folder {
        return Ok(folder.to_path_buf());
    }
    fs::rename(folder, &dest).map_err(|e| {
        IdAssignError::msg(format!(
            "Ordner umbenennen fehlgeschlagen ({} → {}): {e}",
            folder.display(),
            dest.display()
        ))
    })?;
    Ok(dest)
}

fn unique_folder_path(parent: &Path, base_name: &str) -> PathBuf {
    let candidate = parent.join(base_name);
    if !candidate.exists() {
        return candidate;
    }
    for n in 1..10_000 {
        let name = format!("{base_name} ({n})");
        let cand = parent.join(&name);
        if !cand.exists() {
            return cand;
        }
    }
    parent.join(format!("{base_name}_{}", uuid::Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::crew::default_crew_list;
    use crate::model::handoff::{load_and_validate_manifest, GateDecision};
    use tempfile::tempdir;

    fn customer_outside_video<'a>(
        vorname: &'a str,
        nachname: &'a str,
        kid: &'a str,
        bid: &'a str,
        date: &'a str,
    ) -> IdAssignCustomer<'a> {
        IdAssignCustomer {
            vorname,
            nachname,
            kunden_id: kid,
            booking_id: bid,
            booking_date: date,
            typ: "Outside",
            handcam_foto: false,
            handcam_video: false,
            outside_foto: false,
            outside_video: true,
            ist_bezahlt_handcam_foto: false,
            ist_bezahlt_handcam_video: false,
            ist_bezahlt_outside_foto: false,
            ist_bezahlt_outside_video: true,
        }
    }

    #[test]
    fn datum_formats_and_today_fallback() {
        assert_eq!(format_datum_yyyyymmdd("06.08.2026"), "20260806");
        assert_eq!(format_datum_yyyyymmdd("2026-08-06"), "20260806");
        let today = Local::now().format("%Y%m%d").to_string();
        assert_eq!(format_datum_yyyyymmdd(""), today);
        assert_eq!(format_datum_yyyyymmdd("not-a-date"), today);
    }

    #[test]
    fn folder_name_outside_with_vs_and_dropzone() {
        let name = build_job_folder_name(
            "Max",
            "Mustermann",
            "Stefan",
            "Robin",
            "2026-08-27",
            true,
            "G",
        );
        assert_eq!(name, "20260827_Max_Mustermann_TA_Stefan_V_Robin_G");
    }

    #[test]
    fn folder_name_handcam_omits_vs() {
        let name = build_job_folder_name("Anna", "Adler", "Futti", "Robin", "27.08.2026", false, "");
        assert_eq!(name, "20260827_Anna_Adler_TA_Futti");
    }

    #[test]
    fn layout_moves_videos_photos_with_collision_and_prunes_empty() {
        let dir = tempdir().unwrap();
        let job = dir.path().join("Roman_Stefan_Robin");
        fs::create_dir_all(job.join("nested")).unwrap();
        fs::write(job.join("clip.mp4"), b"video").unwrap();
        fs::write(job.join("nested").join("a.jpg"), b"img").unwrap();
        fs::write(job.join("nested").join("notes.txt"), b"keep").unwrap();
        // collision: same name already in target
        fs::create_dir_all(job.join(SUBDIR_OUTSIDE_VIDEO)).unwrap();
        fs::write(job.join(SUBDIR_OUTSIDE_VIDEO).join("clip.mp4"), b"old").unwrap();

        let c = IdAssignCustomer {
            vorname: "Roman",
            nachname: "Test",
            kunden_id: "1111",
            booking_id: "2222",
            booking_date: "2026-08-27",
            typ: "Outside",
            handcam_foto: false,
            handcam_video: false,
            outside_foto: true,
            outside_video: true,
            ist_bezahlt_handcam_foto: false,
            ist_bezahlt_handcam_video: false,
            ist_bezahlt_outside_foto: true,
            ist_bezahlt_outside_video: true,
        };
        rearrange_media_layout(&job, &c).unwrap();

        assert!(job.join(SUBDIR_OUTSIDE_VIDEO).join("clip (1).mp4").is_file());
        assert!(job.join(SUBDIR_OUTSIDE_FOTO).join("a.jpg").is_file());
        assert!(job.join("nested").join("notes.txt").is_file());
        // empty nested media-only dirs pruned; nested kept because of notes.txt
        assert!(job.join("nested").is_dir());
    }

    #[test]
    fn layout_moves_unbooked_videos_into_family_subdir() {
        let dir = tempdir().unwrap();
        let job = dir.path().join("FotoOnly_WithVideos");
        fs::create_dir_all(job.join("cam")).unwrap();
        fs::write(job.join("cam").join("shot.jpg"), b"img").unwrap();
        fs::write(job.join("cam").join("extra.mp4"), b"vid").unwrap();

        // Only Outside-Foto gebucht — Videos trotzdem nach Outside_Video.
        let c = IdAssignCustomer {
            vorname: "Ada",
            nachname: "Lovelace",
            kunden_id: "3971",
            booking_id: "2405",
            booking_date: "2026-08-27",
            typ: "Outside",
            handcam_foto: false,
            handcam_video: false,
            outside_foto: true,
            outside_video: false,
            ist_bezahlt_handcam_foto: false,
            ist_bezahlt_handcam_video: false,
            ist_bezahlt_outside_foto: true,
            ist_bezahlt_outside_video: false,
        };
        rearrange_media_layout(&job, &c).unwrap();

        assert!(job.join(SUBDIR_OUTSIDE_FOTO).join("shot.jpg").is_file());
        assert!(job.join(SUBDIR_OUTSIDE_VIDEO).join("extra.mp4").is_file());
    }

    #[test]
    fn api_id_marker_has_ids_flags_no_pii() {
        let c = customer_outside_video("Max", "M", "3971", "2405", "2026-08-27");
        let raw = build_api_id_marker_json(&c, "Outside").unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["type"], "Outside");
        assert_eq!(v["kunden_id"], "3971");
        assert_eq!(v["booking_id"], "2405");
        assert_eq!(v["outside_video"], true);
        assert!(v.get("vorname").is_none());
        assert!(v.get("email").is_none());
    }

    #[test]
    fn full_pipeline_manifest_gate_ready() {
        let dir = tempdir().unwrap();
        let job = dir.path().join("Roman_Stefan_Robin_Gera");
        fs::create_dir_all(&job).unwrap();
        fs::write(job.join("jump.mp4"), b"media-bytes").unwrap();
        fs::write(job.join("readme.txt"), b"extra").unwrap();

        let c = customer_outside_video("Roman", "Guest", "3971", "2405", "2026-08-27");
        let result = run_id_assign_pipeline(
            &job,
            &c,
            &default_crew_list(),
            Some(&IdAssignOverride {
                tandemmaster: Some("Stefan".into()),
                videospringer: Some("Robin".into()),
                dropzone_suffix: Some("G".into()),
            }),
        )
        .unwrap();

        assert!(result.fertig_path.is_file());
        assert!(result.folder_path.join(MANIFEST_FILENAME).is_file());
        assert!(result
            .folder_path
            .join(SUBDIR_OUTSIDE_VIDEO)
            .join("jump.mp4")
            .is_file());
        assert!(result.folder_path.join("readme.txt").is_file());
        assert!(result.folder_name.contains("Roman_Guest_TA_Stefan_V_Robin_G"));

        let m = load_and_validate_manifest(&result.folder_path).unwrap();
        assert_eq!(m.producer.app, "AeroMediaService");
        assert_eq!(m.marker_hint.format, "api_id");
        assert!(m.integrity.files.iter().any(|f| f.path == "readme.txt"));
        assert!(m
            .integrity
            .files
            .iter()
            .any(|f| f.path == "Outside_Video/jump.mp4"));

        match evaluate_manifest_gate(&result.folder_path, false) {
            GateDecision::Ready { correlation_id } => {
                assert_eq!(correlation_id, result.correlation_id);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn folder_name_omits_empty_tm_and_vs() {
        let name = build_job_folder_name("Nur", "Gast", "", "", "2026-08-27", true, "");
        assert_eq!(name, "20260827_Nur_Gast");
        let with_dz = build_job_folder_name("Nur", "Gast", "", "", "2026-08-27", true, "C");
        assert_eq!(with_dz, "20260827_Nur_Gast_C");
        let tm_only = build_job_folder_name("Nur", "Gast", "Futti", "", "2026-08-27", true, "");
        assert_eq!(tm_only, "20260827_Nur_Gast_TA_Futti");
    }

    #[test]
    fn pipeline_allows_missing_tm_and_omits_from_folder() {
        let dir = tempdir().unwrap();
        let job = dir.path().join("NurGast_Load2");
        fs::create_dir_all(&job).unwrap();
        fs::write(job.join("a.mp4"), b"v").unwrap();
        let c = customer_outside_video("Nur", "Gast", "1111", "2222", "2026-08-27");
        let result = run_id_assign_pipeline(&job, &c, &default_crew_list(), None).unwrap();
        assert!(result.folder_name.starts_with("20260827_Nur_Gast"));
        assert!(!result.folder_name.contains("_TA_"));
        assert!(!result.folder_name.contains("_V_"));
        assert!(result.fertig_path.exists());
    }

    #[test]
    fn preview_andreas_tacorni_g_includes_tm_and_dropzone() {
        let dir = tempdir().unwrap();
        let job = dir.path().join("20260827_Andreas_Kowalenko_TACorni_G");
        fs::create_dir_all(&job).unwrap();
        let c = customer_outside_video("Andreas", "Kowalenko", "3971", "2405", "2026-08-16");
        let preview =
            preview_id_assign(&job, "cust-1", &c, &default_crew_list(), None).unwrap();
        assert_eq!(preview.tandemmaster.as_deref(), Some("Cornelius"));
        assert_eq!(preview.dropzone_suffix.as_deref(), Some("G"));
        assert_eq!(
            preview.preview_folder_name,
            "20260816_Andreas_Kowalenko_TA_Cornelius_G"
        );

        let with_override = preview_id_assign(
            &job,
            "cust-1",
            &c,
            &default_crew_list(),
            Some(&IdAssignOverride {
                tandemmaster: Some("Cornelius".into()),
                videospringer: None,
                dropzone_suffix: Some("G".into()),
            }),
        )
        .unwrap();
        assert_eq!(
            with_override.preview_folder_name,
            "20260816_Andreas_Kowalenko_TA_Cornelius_G"
        );
    }

    #[test]
    fn preview_needs_review_for_outside_without_vs() {
        let dir = tempdir().unwrap();
        let job = dir.path().join("Niels_TACorni");
        fs::create_dir_all(&job).unwrap();
        let c = customer_outside_video("Niels", "Guest", "1111", "2222", "2026-08-27");
        let preview =
            preview_id_assign(&job, "cust-1", &c, &default_crew_list(), None).unwrap();
        assert!(preview.needs_review);
        assert!(preview.vs_required);
        assert!(preview.can_confirm);
        assert_eq!(preview.tandemmaster.as_deref(), Some("Cornelius"));
        assert!(preview.videospringer.is_none());
        assert!(preview.preview_folder_name.contains("_TA_Cornelius"));
        assert!(!preview.preview_folder_name.contains("_V_"));

        let with_vs = preview_id_assign(
            &job,
            "cust-1",
            &c,
            &default_crew_list(),
            Some(&IdAssignOverride {
                tandemmaster: Some("Cornelius".into()),
                videospringer: Some("Robin".into()),
                dropzone_suffix: None,
            }),
        )
        .unwrap();
        assert!(with_vs.can_confirm);
        assert!(with_vs.preview_folder_name.contains("_V_Robin"));
        assert!(with_vs.preview_folder_name.contains("_TA_Cornelius"));
    }

    #[test]
    fn rename_collision_appends_number() {
        let dir = tempdir().unwrap();
        let existing = dir.path().join("20260827_A_B_TA_Stefan");
        fs::create_dir_all(&existing).unwrap();
        let job = dir.path().join("src");
        fs::create_dir_all(&job).unwrap();
        let renamed = rename_job_folder(&job, "20260827_A_B_TA_Stefan").unwrap();
        assert_eq!(
            renamed.file_name().unwrap().to_str().unwrap(),
            "20260827_A_B_TA_Stefan (1)"
        );
    }
}
