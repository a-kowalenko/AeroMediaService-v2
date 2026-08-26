//! Append extra media into an already uploaded Dropbox / Cloud order folder.
//! Does not create a new monitor directory, share link, or customer notification.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::cloud::binding::{
    client_for_binding, merge_binding_into_history, resolve_binding_for_history, CustomDropboxPin,
};
use crate::cloud::dropbox::DropboxClient;
use crate::cloud::manifest::STANDARD_CATEGORIES;
use crate::cloud::traits::CloudClient;
use crate::cloud::{CloudState, CustomApiClient, DropboxPool};
use crate::constants::is_direct_dropbox_upload_mode;
use crate::events;
use crate::model::handoff::{
    is_append_manifest, load_and_validate_manifest, parent_correlation_id,
    CODE_APPEND_PARENT_MISSING, CODE_APPEND_PARENT_NOT_READY,
};
use crate::model::marker::merge_kunde_media_flags;
use crate::model::kunde::Kunde;
use crate::monitor::stability::has_uploadable_files;
use crate::storage::dropbox_accounts::DropboxAccountStore;
use crate::notify::resend::remote_path_for_entry;
use crate::storage::config::runtime_setting;
use crate::storage::history::{HistoryEntry, HistoryStore};
use crate::storage::logging;
use crate::upload::preview_watermark::write_preview_media;
use crate::upload::registry::AppendTarget;
use crate::upload::retry::resolve_kunde_from_history_entry;
use crate::upload::UploadControl;
use crate::upload::UploadQueueRegistry;

pub const APPENDABLE_STATUS: &str = "Erfolgreich";

/// ATS append folder suffix (`{parent}_nachreichung_01`).
pub const APPEND_FOLDER_SUFFIX: &str = "_nachreichung_";
pub const APPEND_EVENT_QUEUED: &str = "Wartet";
pub const APPEND_EVENT_UPLOADING: &str = "In Bearbeitung";
pub const APPEND_EVENT_COMPLETED: &str = "Erfolgreich";
pub const APPEND_EVENT_FAILED: &str = "Fehler";
pub const APPEND_EVENT_CANCELLED: &str = "Abgebrochen";

const VIDEO_EXTS: &[&str] = &[
    ".mp4", ".mov", ".mkv", ".avi", ".m4v", ".webm", ".mts", ".m2ts",
];
const PHOTO_EXTS: &[&str] = &[
    ".jpg", ".jpeg", ".png", ".bmp", ".tiff", ".tif", ".webp", ".heic", ".dng",
];

#[derive(Debug, Clone, Deserialize)]
pub struct AppendFileItem {
    pub path: String,
    pub category: String,
    #[serde(default)]
    pub preview: bool,
}

struct StagedDir(PathBuf);

impl Drop for StagedDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn json_str<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry.get(key).and_then(Value::as_str).unwrap_or("")
}

fn json_int(entry: &Value, key: &str) -> i64 {
    match entry.get(key) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

pub fn can_append_media(status: &str) -> bool {
    status.trim() == APPENDABLE_STATUS
}

fn now_timestamp() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

pub fn is_append_dir_name(dir_name: &str) -> bool {
    let raw = dir_name.trim();
    let Some((_, suffix)) = raw.rsplit_once(APPEND_FOLDER_SUFFIX) else {
        return false;
    };
    !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
}

pub fn existing_order_id(entry: &Value) -> Option<String> {
    let value = json_str(entry, "order_id").trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
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

/// Map UI category (+ optional Preview) to a Dropbox standard folder.
pub fn dest_subdir(category: &str, preview: bool) -> Result<&'static str, String> {
    let raw = category.trim();
    if let Some(std) = STANDARD_CATEGORIES
        .iter()
        .copied()
        .find(|n| n.eq_ignore_ascii_case(raw))
    {
        if preview && !std.starts_with("Preview_") {
            return Ok(if std.ends_with("_Video") {
                "Preview_Video"
            } else {
                "Preview_Foto"
            });
        }
        return Ok(std);
    }
    let key = raw.to_ascii_lowercase().replace('-', "_");
    let (is_video, folder) = match key.as_str() {
        "handcam_video" | "hv" => (true, "Handcam_Video"),
        "handcam_foto" | "hf" => (false, "Handcam_Foto"),
        "outside_video" | "ov" => (true, "Outside_Video"),
        "outside_foto" | "of" => (false, "Outside_Foto"),
        "preview_video" | "pv" => (true, "Preview_Video"),
        "preview_foto" | "pf" => (false, "Preview_Foto"),
        _ => return Err(format!("Unbekannte Kategorie '{category}'.")),
    };
    if preview && !folder.starts_with("Preview_") {
        Ok(if is_video {
            "Preview_Video"
        } else {
            "Preview_Foto"
        })
    } else {
        Ok(folder)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppendCategory {
    HandcamVideo,
    HandcamFoto,
    OutsideVideo,
    OutsideFoto,
}

impl AppendCategory {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "handcam_video" | "hv" => Ok(Self::HandcamVideo),
            "handcam_foto" | "hf" => Ok(Self::HandcamFoto),
            "outside_video" | "ov" => Ok(Self::OutsideVideo),
            "outside_foto" | "of" => Ok(Self::OutsideFoto),
            other => Err(format!("Unbekannte Kategorie '{other}'.")),
        }
    }

    fn is_video(self) -> bool {
        matches!(self, Self::HandcamVideo | Self::OutsideVideo)
    }

    fn is_booked(self, k: &Kunde) -> bool {
        match self {
            Self::HandcamVideo => k.handcam_video,
            Self::HandcamFoto => k.handcam_foto,
            Self::OutsideVideo => k.outside_video,
            Self::OutsideFoto => k.outside_foto,
        }
    }

    fn is_unpaid(self, k: &Kunde) -> bool {
        self.is_booked(k)
            && match self {
                Self::HandcamVideo => !k.ist_bezahlt_handcam_video,
                Self::HandcamFoto => !k.ist_bezahlt_handcam_foto,
                Self::OutsideVideo => !k.ist_bezahlt_outside_video,
                Self::OutsideFoto => !k.ist_bezahlt_outside_foto,
            }
    }
}

fn validate_unpaid_preview_rules(items: &[(AppendCategory, bool, &Path)], k: &Kunde) -> Result<(), String> {
    let has_unpaid_photos = items
        .iter()
        .any(|(cat, _, _)| cat.is_unpaid(k) && !cat.is_video());
    let has_unpaid_photo_preview = items.iter().any(|(cat, preview, _)| {
        cat.is_unpaid(k) && !cat.is_video() && *preview
    });
    if has_unpaid_photos && !has_unpaid_photo_preview {
        return Err(
            "Foto-Produkt ist nicht bezahlt — bitte mindestens ein Foto für das Wasserzeichen auswählen."
                .into(),
        );
    }

    let has_unpaid_videos = items
        .iter()
        .any(|(cat, _, _)| cat.is_unpaid(k) && cat.is_video());
    let has_unpaid_video_preview = items.iter().any(|(cat, preview, _)| {
        cat.is_unpaid(k) && cat.is_video() && *preview
    });
    if has_unpaid_videos && !has_unpaid_video_preview {
        return Err(
            "Video-Produkt ist nicht bezahlt — bitte mindestens ein Video für die Preview auswählen."
                .into(),
        );
    }
    Ok(())
}

fn copy_file_to_subdir(
    root: &Path,
    subdir: &str,
    src: &Path,
    used_by_dir: &mut std::collections::HashMap<String, HashSet<String>>,
) -> Result<(), String> {
    let dest_dir = root.join(subdir);
    fs::create_dir_all(&dest_dir).map_err(|e| format!("Unterordner '{subdir}' anlegen: {e}"))?;
    let original = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "media.bin".into());
    let used = used_by_dir.entry(subdir.to_string()).or_default();
    let out_name = unique_filename(&dest_dir, &original, used);
    let dest = dest_dir.join(&out_name);
    fs::copy(src, &dest).map_err(|e| {
        format!(
            "Kopieren '{}' → '{}': {e}",
            src.display(),
            dest.display()
        )
    })?;
    Ok(())
}

fn write_watermarked_preview(
    root: &Path,
    cat: AppendCategory,
    src: &Path,
    used_by_dir: &mut std::collections::HashMap<String, HashSet<String>>,
) -> Result<(), String> {
    let subdir = dest_subdir(
        match cat {
            AppendCategory::HandcamVideo => "handcam_video",
            AppendCategory::HandcamFoto => "handcam_foto",
            AppendCategory::OutsideVideo => "outside_video",
            AppendCategory::OutsideFoto => "outside_foto",
        },
        true,
    )?;
    let dest_dir = root.join(subdir);
    fs::create_dir_all(&dest_dir).map_err(|e| format!("Unterordner '{subdir}' anlegen: {e}"))?;
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "media".into());
    let out_name = if cat.is_video() {
        format!("{stem}_preview.mp4")
    } else {
        format!("{stem}_preview.jpg")
    };
    let used = used_by_dir.entry(subdir.to_string()).or_default();
    let final_name = unique_filename(&dest_dir, &out_name, used);
    let dest = dest_dir.join(&final_name);
    write_preview_media(src, &dest, cat.is_video())
}

fn unique_filename(dir: &Path, original: &str, used: &mut HashSet<String>) -> String {
    let claimed = |name: &str, used: &mut HashSet<String>| {
        if used.contains(name) || dir.join(name).exists() {
            false
        } else {
            used.insert(name.to_string());
            true
        }
    };
    if claimed(original, used) {
        return original.to_string();
    }
    let (stem, ext) = match original.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (original.to_string(), String::new()),
    };
    for n in 1..=9999 {
        let candidate = format!("{stem}_{n:03}{ext}");
        if claimed(&candidate, used) {
            return candidate;
        }
    }
    format!("{stem}_{}{ext}", Uuid::new_v4().simple())
}

pub fn expand_append_media_paths(paths: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in paths {
        let path = PathBuf::from(raw.trim());
        if raw.trim().is_empty() {
            continue;
        }
        collect_media_files(&path, &mut out, &mut seen)?;
    }
    Ok(out)
}

fn collect_media_files(
    path: &Path,
    out: &mut Vec<String>,
    seen: &mut HashSet<String>,
) -> Result<(), String> {
    let meta = fs::metadata(path).map_err(|e| format!("Pfad lesen '{}': {e}", path.display()))?;
    if meta.is_file() {
        let ext = ext_of(path);
        if is_video_ext(&ext) || is_photo_ext(&ext) {
            let key = path.to_string_lossy().to_string();
            if seen.insert(key.clone()) {
                out.push(key);
            }
        }
        return Ok(());
    }
    if meta.is_dir() {
        let entries = fs::read_dir(path)
            .map_err(|e| format!("Ordner lesen '{}': {e}", path.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("Ordner lesen '{}': {e}", path.display()))?;
            collect_media_files(&entry.path(), out, seen)?;
        }
    }
    Ok(())
}

pub fn stage_append_files(items: &[AppendFileItem], kunde: &Kunde) -> Result<PathBuf, String> {
    if items.is_empty() {
        return Err("Bitte mindestens eine Datei zum Nachreichen wählen.".into());
    }
    let parsed: Vec<(AppendCategory, bool, PathBuf)> = items
        .iter()
        .map(|item| {
            let cat = AppendCategory::parse(&item.category).map_err(|e| {
                format!("{} ({})", e, item.path)
            })?;
            let src = PathBuf::from(item.path.trim());
            if !src.is_file() {
                return Err(format!("Datei fehlt: {}", src.display()));
            }
            let ext = ext_of(&src);
            if cat.is_video() && !is_video_ext(&ext) {
                return Err(format!(
                    "'{}' ist kein Video.",
                    src.file_name().and_then(|n| n.to_str()).unwrap_or("Datei")
                ));
            }
            if !cat.is_video() && !is_photo_ext(&ext) {
                return Err(format!(
                    "'{}' ist kein Foto.",
                    src.file_name().and_then(|n| n.to_str()).unwrap_or("Datei")
                ));
            }
            Ok((cat, item.preview, src))
        })
        .collect::<Result<Vec<_>, String>>()?;

    let parsed_refs: Vec<(AppendCategory, bool, &Path)> = parsed
        .iter()
        .map(|(c, p, s)| (*c, *p, s.as_path()))
        .collect();
    validate_unpaid_preview_rules(&parsed_refs, kunde)?;

    let root = std::env::temp_dir().join(format!("ams-append-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).map_err(|e| format!("Staging-Ordner anlegen: {e}"))?;

    let mut used_by_dir: std::collections::HashMap<String, HashSet<String>> =
        std::collections::HashMap::new();

    for (cat, preview, src) in parsed {
        let category_key = match cat {
            AppendCategory::HandcamVideo => "handcam_video",
            AppendCategory::HandcamFoto => "handcam_foto",
            AppendCategory::OutsideVideo => "outside_video",
            AppendCategory::OutsideFoto => "outside_foto",
        };
        let booked = cat.is_booked(kunde);
        let unpaid = cat.is_unpaid(kunde);

        let fail = |e: String| {
            let _ = fs::remove_dir_all(&root);
            e
        };

        if !booked {
            if preview {
                write_watermarked_preview(&root, cat, &src, &mut used_by_dir).map_err(fail)?;
            } else {
                let sub = dest_subdir(category_key, false).map_err(fail)?;
                copy_file_to_subdir(&root, sub, &src, &mut used_by_dir).map_err(fail)?;
            }
            continue;
        }

        // Gebucht: Original immer ins Produkt-Verzeichnis (wie ATS).
        let full_sub = dest_subdir(category_key, false).map_err(fail)?;
        copy_file_to_subdir(&root, full_sub, &src, &mut used_by_dir).map_err(fail)?;

        if unpaid && preview {
            write_watermarked_preview(&root, cat, &src, &mut used_by_dir).map_err(fail)?;
        }
    }
    Ok(root)
}

pub fn append_target_from_parent_entry(entry: &HistoryEntry) -> Result<AppendTarget, String> {
    if !can_append_media(&entry.status) {
        return Err(format!(
            "Status „{}“ unterstützt kein Nachreichen (nur Erfolgreich).",
            entry.status
        ));
    }
    let json = entry.to_json();
    let remote_path = remote_path_for_entry(&json);
    if remote_path.trim().is_empty() {
        return Err("Historieneintrag ohne Dropbox-Pfad (remote_path / dir_name).".into());
    }
    let share = json_str(&json, "share_link").trim();
    let ams = json_str(&json, "dropbox_account_ams_id").trim();
    let pool = json_str(&json, "dropbox_account_pool").trim();
    let dbid = json_str(&json, "dropbox_account_id").trim();
    let email = json_str(&json, "dropbox_account_email").trim();
    Ok(AppendTarget {
        parent_dir_name: entry.dir_name.clone(),
        remote_path,
        order_id: existing_order_id(&json),
        share_link: if share.is_empty() {
            None
        } else {
            Some(share.to_string())
        },
        dropbox_account_ams_id: if ams.is_empty() {
            None
        } else {
            Some(ams.to_string())
        },
        dropbox_account_pool: if pool.is_empty() {
            None
        } else {
            Some(pool.to_string())
        },
        dropbox_account_id: if dbid.is_empty() {
            None
        } else {
            Some(dbid.to_string())
        },
        dropbox_account_email: if email.is_empty() {
            None
        } else {
            Some(email.to_string())
        },
    })
}

/// Derive parent job folder name from an append staging folder (`…_nachreichung_01`).
pub fn parent_dir_name_from_append_folder(folder: &Path) -> Option<String> {
    let name = folder.file_name()?.to_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let (parent, suffix) = name.rsplit_once(APPEND_FOLDER_SUFFIX)?;
    if suffix.is_empty() || !suffix.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let parent = parent.trim();
    if parent.is_empty() {
        return None;
    }
    Some(parent.to_string())
}

fn resolve_parent_history_entry(
    store: &HistoryStore,
    parent_cid: &str,
    append_folder: &Path,
) -> Result<HistoryEntry, (String, String)> {
    let db_err = |e: &str| {
        (
            CODE_APPEND_PARENT_NOT_READY.into(),
            format!("Parent-Auftrag nicht lesbar: {e}"),
        )
    };
    // Ordnername zuerst: ATS benennt `{parent}_nachreichung_NN` zuverlässig; correlation_id kann
    // in älteren Historieneinträgen fehlen oder von ATS/AMS abweichen.
    if let Some(parent_name) = parent_dir_name_from_append_folder(append_folder) {
        if let Some(entry) = store.find_by_dir_name(&parent_name).map_err(|e| db_err(&e.to_string()))? {
            logging::log_info(&format!(
                "Nachreichung: Parent '{parent_name}' per Ordnername aufgelöst."
            ));
            return Ok(entry);
        }
    }
    if let Some(entry) = store
        .find_by_correlation_id(parent_cid)
        .map_err(|e| db_err(&e.to_string()))?
    {
        return Ok(entry);
    }
    Err((
        CODE_APPEND_PARENT_NOT_READY.into(),
        format!("Kein AMS-Auftrag für parent_correlation_id={parent_cid}."),
    ))
}

fn event_identity_matches(event: &Value, source_dir_name: &str, correlation_id: Option<&str>) -> bool {
    let source = event
        .get("source_dir_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !source.is_empty() && source == source_dir_name.trim() {
        return true;
    }
    let Some(cid) = correlation_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    event.get("correlation_id")
        .and_then(Value::as_str)
        .map(str::trim)
        == Some(cid)
}

fn append_events_from_parent(parent: &HistoryEntry) -> Vec<Value> {
    parent
        .extra
        .get("append_events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn append_count_from_parent(parent: &HistoryEntry) -> i64 {
    match parent.to_json().get("append_count") {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

pub fn build_append_parent_history_update(
    append: &AppendTarget,
    source_dir_name: &str,
    state: &str,
    correlation_id: Option<&str>,
    kunde: Option<&Kunde>,
    marker_raw: Option<&str>,
    archived_path: Option<&Path>,
    error_message: Option<&str>,
    share_link: Option<&str>,
    order_id: Option<&str>,
) -> Value {
    let now = now_timestamp();
    let mut history = json!({
        "dir_name": append.parent_dir_name,
        "remote_path": append.remote_path,
    });
    let mut append_count = 0i64;
    let mut events = Vec::new();
    let mut had_completed_state = false;

    if let Ok(store) = HistoryStore::open_default() {
        if let Ok(Some(parent)) = store.find_by_dir_name(&append.parent_dir_name) {
            append_count = append_count_from_parent(&parent);
            events = append_events_from_parent(&parent);
            if let Some(existing) = events
                .iter()
                .find(|event| event_identity_matches(event, source_dir_name, correlation_id))
            {
                had_completed_state = existing
                    .get("state")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    == Some(APPEND_EVENT_COMPLETED);
            }
        }
    }

    let idx = events
        .iter()
        .position(|event| event_identity_matches(event, source_dir_name, correlation_id));
    let mut event = idx
        .and_then(|i| events.get(i).cloned())
        .unwrap_or_else(|| {
            json!({
                "source_dir_name": source_dir_name,
                "created_at": now,
            })
        });
    event["kind"] = Value::String("append_handoff".into());
    event["source_dir_name"] = Value::String(source_dir_name.to_string());
    event["state"] = Value::String(state.to_string());
    event["updated_at"] = Value::String(now.clone());
    event["parent_dir_name"] = Value::String(append.parent_dir_name.clone());
    event["remote_path"] = Value::String(append.remote_path.clone());
    if let Some(cid) = correlation_id.map(str::trim).filter(|s| !s.is_empty()) {
        event["correlation_id"] = Value::String(cid.to_string());
    }
    if let Some(raw) = marker_raw.map(str::trim).filter(|s| !s.is_empty()) {
        event["marker_raw"] = Value::String(raw.to_string());
    }
    if let Some(path) = archived_path {
        event["archived_path"] = Value::String(path.to_string_lossy().into_owned());
    }
    if let Some(msg) = error_message.map(str::trim).filter(|s| !s.is_empty()) {
        event["error_msg"] = Value::String(msg.to_string());
    } else if event.get("error_msg").is_some() {
        event["error_msg"] = Value::String(String::new());
    }
    let link = share_link
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(append.share_link.as_deref());
    if let Some(link) = link {
        event["share_link"] = Value::String(link.to_string());
        history["share_link"] = Value::String(link.to_string());
    }
    let oid = order_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| append.order_id.clone());
    if let Some(oid) = oid {
        event["order_id"] = Value::String(oid.clone());
        history["order_id"] = Value::String(oid);
    }
    if state.trim() == APPEND_EVENT_COMPLETED {
        event["completed_at"] = Value::String(now.clone());
        if !had_completed_state {
            append_count += 1;
        }
        history["status"] = Value::String(APPENDABLE_STATUS.into());
        history["append_count"] = Value::from(append_count);
        history["last_append_at"] = Value::String(now.clone());
    }
    if let Some(index) = idx {
        events[index] = event;
    } else {
        events.push(event);
    }
    events.sort_by(|a, b| {
        let a_ts = a
            .get("updated_at")
            .and_then(Value::as_str)
            .or_else(|| a.get("created_at").and_then(Value::as_str))
            .unwrap_or("");
        let b_ts = b
            .get("updated_at")
            .and_then(Value::as_str)
            .or_else(|| b.get("created_at").and_then(Value::as_str))
            .unwrap_or("");
        b_ts.cmp(a_ts)
    });
    history["append_events"] = Value::Array(events);
    if let Some(kunde) = kunde {
        if let Some(v) = kunde.first_name.clone().filter(|s| !s.trim().is_empty()) {
            history["first_name"] = Value::String(v);
        }
        if let Some(v) = kunde.last_name.clone().filter(|s| !s.trim().is_empty()) {
            history["last_name"] = Value::String(v);
        }
        if let Some(v) = kunde.email.clone().filter(|s| !s.trim().is_empty()) {
            history["email"] = Value::String(v);
        }
        if let Some(v) = kunde.phone.clone().filter(|s| !s.trim().is_empty()) {
            history["phone"] = Value::String(v);
        }
        merge_kunde_media_flags(&mut history, kunde);
    }
    crate::storage::history::touch_last_updated(&mut history);
    history
}

/// If the folder is an ATS append handoff, resolve the parent order. `Ok(None)` = normal job.
pub fn resolve_claimed_append_target(
    folder: &Path,
) -> Result<Option<AppendTarget>, (String, String)> {
    let manifest = match load_and_validate_manifest(folder) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    if !is_append_manifest(&manifest) {
        return Ok(None);
    }
    let Some(parent_cid) = parent_correlation_id(&manifest) else {
        return Err((
            CODE_APPEND_PARENT_MISSING.into(),
            "Nachreichung ohne parent_correlation_id.".into(),
        ));
    };
    let store = HistoryStore::open_default().map_err(|e| {
        (
            CODE_APPEND_PARENT_NOT_READY.into(),
            format!("Historie nicht lesbar: {e}"),
        )
    })?;
    let parent = resolve_parent_history_entry(&store, &parent_cid, folder)?;
    append_target_from_parent_entry(&parent).map(Some).map_err(|message| {
        (CODE_APPEND_PARENT_NOT_READY.into(), message)
    })
}

/// Upload files from `local_dir` into the history entry's existing remote folder.
pub async fn append_media_from_history(
    history_entry: &Value,
    local_dir: &Path,
    selected_cloud: &str,
    cloud: &CloudState,
    control: &UploadControl,
    registry: &UploadQueueRegistry,
) -> Result<Value, String> {
    let status = json_str(history_entry, "status").trim();
    if !can_append_media(status) {
        return Err(format!(
            "Status „{status}“ unterstützt kein Nachreichen (nur Erfolgreich)."
        ));
    }

    if !registry.snapshot_dicts().is_empty() {
        return Err(
            "Ein Upload läuft bereits. Bitte warten, bis die Warteschlange leer ist.".into(),
        );
    }

    if !local_dir.is_dir() {
        return Err(format!(
            "Ordner existiert nicht: {}",
            local_dir.display()
        ));
    }
    if !has_uploadable_files(local_dir) {
        return Err("Der gewählte Ordner enthält keine Medien-Dateien.".into());
    }

    let remote_path = remote_path_for_entry(history_entry);
    if remote_path.trim().is_empty() {
        return Err("Historieneintrag ohne Dropbox-Pfad (remote_path / dir_name).".into());
    }

    let dir_name = json_str(history_entry, "dir_name").trim();
    let kunde = resolve_kunde_from_history_entry(history_entry).await?;
    let order_id = existing_order_id(history_entry);
    let use_custom = selected_cloud.trim() == "custom_api";
    let pool = if use_custom {
        DropboxPool::CustomApi
    } else {
        DropboxPool::Native
    };
    let accounts = DropboxAccountStore::open_default().map_err(|e| e.to_string())?;
    let binding = if accounts.list(pool).map_err(|e| e.to_string())?.is_empty() {
        None
    } else {
        Some(resolve_binding_for_history(history_entry, pool, &accounts)?)
    };

    if use_custom && !is_direct_dropbox_upload_mode(&runtime_setting("custom_api_upload_mode")) {
        return Err(
            "Nachreichen über Skydive Media ist nur im Modus „Dropbox + Manifest“ möglich."
                .into(),
        );
    }

    logging::log_info(&format!(
        "Nachreichen in {remote_path} aus {} (dir_name={dir_name})",
        local_dir.display()
    ));
    events::emit_status(format!("Nachreichen: {remote_path}"));

    control.reset_for_new_job();

    let _pin = if use_custom {
        binding
            .as_ref()
            .map(|b| CustomDropboxPin::pin(cloud, &b.ams_id))
    } else {
        None
    };
    let ok = if use_custom {
        append_via_custom_api(
            &cloud.custom_api,
            local_dir,
            &remote_path,
            control,
            &kunde,
            order_id.as_deref(),
        )
        .await?
    } else {
        let dropbox = match &binding {
            Some(b) => client_for_binding(cloud, b),
            None => cloud.dropbox(),
        };
        append_via_dropbox(&dropbox, local_dir, &remote_path, control, &kunde).await?
    };

    if !ok {
        return Err("Nachreichen fehlgeschlagen (siehe Log).".into());
    }

    let now = Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let append_count = json_int(history_entry, "append_count") + 1;
    let mut updates = json!({
        "dir_name": dir_name,
        "status": "Erfolgreich",
        "remote_path": remote_path,
        "last_append_at": now,
        "append_count": append_count,
    });
    merge_binding_into_history(&mut updates, binding.as_ref());
    let stored_link = json_str(history_entry, "share_link").trim();
    if !stored_link.is_empty() {
        updates["share_link"] = json!(stored_link);
    }
    let resolved_order = if use_custom {
        CloudClient::last_order_id(cloud.custom_api.as_ref()).or(order_id)
    } else {
        order_id
    };
    if let Some(oid) = resolved_order.filter(|s| !s.is_empty()) {
        updates["order_id"] = json!(oid);
    }

    crate::storage::history::touch_last_updated(&mut updates);
    events::emit(events::UPLOAD_HISTORY_UPDATE, &updates);
    events::emit_status(format!("Nachgereicht: {remote_path}"));
    Ok(updates)
}

/// Copy selected files into standard category folders, then append into the existing order.
pub async fn append_media_from_files(
    history_entry: &Value,
    items: &[AppendFileItem],
    selected_cloud: &str,
    cloud: &CloudState,
    control: &UploadControl,
    registry: &UploadQueueRegistry,
) -> Result<Value, String> {
    let kunde = resolve_kunde_from_history_entry(history_entry).await?;
    let staged = StagedDir(stage_append_files(items, &kunde)?);
    let result = append_media_from_history(
        history_entry,
        &staged.0,
        selected_cloud,
        cloud,
        control,
        registry,
    )
    .await;
    result
}

async fn append_via_dropbox(
    client: &DropboxClient,
    local_dir: &Path,
    remote_path: &str,
    control: &UploadControl,
    kunde: &crate::model::kunde::Kunde,
) -> Result<bool, String> {
    client.set_append_upload(true);
    let result = client
        .upload_directory(local_dir, remote_path, control, kunde)
        .await;
    client.set_append_upload(false);
    result.map_err(|e| e.to_string())
}

async fn append_via_custom_api(
    client: &CustomApiClient,
    local_dir: &Path,
    remote_path: &str,
    control: &UploadControl,
    kunde: &crate::model::kunde::Kunde,
    order_id: Option<&str>,
) -> Result<bool, String> {
    client.set_append_order_id(order_id.map(str::to_string));
    client.set_append_upload(true);
    let result = client
        .upload_directory(local_dir, remote_path, control, kunde)
        .await;
    client.set_append_upload(false);
    client.set_append_order_id(None);
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_successful_jobs_can_append() {
        assert!(can_append_media("Erfolgreich"));
        assert!(!can_append_media("Fehler"));
        assert!(!can_append_media("Gestartet"));
    }

    #[test]
    fn order_id_from_history() {
        assert_eq!(
            existing_order_id(&json!({"order_id": "order_abc"})).as_deref(),
            Some("order_abc")
        );
        assert_eq!(existing_order_id(&json!({"order_id": "  "})), None);
        assert_eq!(existing_order_id(&json!({})), None);
    }

    #[test]
    fn append_target_requires_successful_parent() {
        let mut entry = HistoryEntry {
            dir_name: "Flug_001".into(),
            status: "Erfolgreich".into(),
            remote_path: "/Flug_001".into(),
            ..HistoryEntry::default()
        };
        entry.extra.insert("order_id".into(), json!("ord-1"));
        entry.extra.insert("dropbox_account_ams_id".into(), json!("ams-parent"));
        entry.extra.insert("dropbox_account_pool".into(), json!("native"));
        entry.extra.insert("dropbox_account_email".into(), json!("p@x.de"));
        let target = append_target_from_parent_entry(&entry).unwrap();
        assert_eq!(target.parent_dir_name, "Flug_001");
        assert_eq!(target.remote_path, "/Flug_001");
        assert_eq!(target.order_id.as_deref(), Some("ord-1"));
        assert_eq!(target.dropbox_account_ams_id.as_deref(), Some("ams-parent"));
        assert_eq!(target.dropbox_account_pool.as_deref(), Some("native"));
        assert_eq!(target.dropbox_account_email.as_deref(), Some("p@x.de"));

        entry.status = "Fehler".into();
        assert!(append_target_from_parent_entry(&entry).is_err());
    }

    #[test]
    fn dest_subdir_maps_options_and_preview() {
        assert_eq!(dest_subdir("handcam_video", false).unwrap(), "Handcam_Video");
        assert_eq!(dest_subdir("HV", false).unwrap(), "Handcam_Video");
        assert_eq!(dest_subdir("outside_foto", false).unwrap(), "Outside_Foto");
        assert_eq!(dest_subdir("Handcam_Foto", false).unwrap(), "Handcam_Foto");
        assert_eq!(dest_subdir("outside_video", true).unwrap(), "Preview_Video");
        assert_eq!(dest_subdir("hf", true).unwrap(), "Preview_Foto");
        assert!(dest_subdir("nope", false).is_err());
    }

    #[test]
    fn stage_append_files_uses_category_folders() {
        let src_dir = tempfile::tempdir().unwrap();
        let photo = src_dir.path().join("a.jpg");
        let video = src_dir.path().join("b.mp4");
        fs::write(&photo, b"jpeg").unwrap();
        fs::write(&video, b"mp4!").unwrap();
        let kunde = Kunde {
            handcam_video: true,
            ist_bezahlt_handcam_video: true,
            ..Kunde::default()
        };
        let staged = stage_append_files(
            &[
                AppendFileItem {
                    path: photo.to_string_lossy().into(),
                    category: "outside_foto".into(),
                    preview: false,
                },
                AppendFileItem {
                    path: video.to_string_lossy().into(),
                    category: "handcam_video".into(),
                    preview: true,
                },
            ],
            &kunde,
        )
        .unwrap();
        assert!(staged.join("Outside_Foto").join("a.jpg").is_file());
        assert!(staged.join("Handcam_Video").join("b.mp4").is_file());
        let _ = fs::remove_dir_all(&staged);
    }

    #[test]
    fn stage_unpaid_requires_at_least_one_preview() {
        let src_dir = tempfile::tempdir().unwrap();
        let photo = src_dir.path().join("a.jpg");
        fs::write(&photo, b"jpeg").unwrap();
        let kunde = Kunde {
            outside_foto: true,
            ist_bezahlt_outside_foto: false,
            ..Kunde::default()
        };
        let err = stage_append_files(
            &[AppendFileItem {
                path: photo.to_string_lossy().into(),
                category: "outside_foto".into(),
                preview: false,
            }],
            &kunde,
        )
        .unwrap_err();
        assert!(err.contains("Wasserzeichen"), "{err}");
    }

    #[test]
    fn stage_rejects_video_in_foto_category() {
        let src_dir = tempfile::tempdir().unwrap();
        let video = src_dir.path().join("clip.mp4");
        fs::write(&video, b"mp4").unwrap();
        let err = stage_append_files(
            &[AppendFileItem {
                path: video.to_string_lossy().into(),
                category: "handcam_foto".into(),
                preview: false,
            }],
            &Kunde::default(),
        )
        .unwrap_err();
        assert!(err.contains("kein Foto"), "{err}");
    }

    #[test]
    fn parent_dir_from_append_folder_name() {
        let folder = Path::new(r"C:\share\20260817_Test_TA_TM_nachreichung_01");
        assert_eq!(
            parent_dir_name_from_append_folder(folder).as_deref(),
            Some("20260817_Test_TA_TM")
        );
        assert!(parent_dir_name_from_append_folder(Path::new("JobA")).is_none());
    }

    #[test]
    fn append_dir_name_requires_numeric_suffix() {
        assert!(is_append_dir_name("JobA_nachreichung_01"));
        assert!(!is_append_dir_name("JobA_nachreichung_xx"));
        assert!(!is_append_dir_name("JobA_nachreichung_"));
        assert!(!is_append_dir_name("JobA"));
    }

    #[test]
    fn resolve_parent_falls_back_to_dir_name_without_cid() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::open_at(dir.path().join("h.db")).unwrap();
        store
            .add_or_update(&json!({
                "dir_name": "JobA",
                "status": "Erfolgreich",
                "remote_path": "/JobA",
            }))
            .unwrap()
            .unwrap();
        let append = Path::new("/monitor/JobA_nachreichung_01");
        let parent =
            resolve_parent_history_entry(&store, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", append)
                .unwrap();
        assert_eq!(parent.dir_name, "JobA");
        assert_eq!(parent.status, "Erfolgreich");
    }
}
