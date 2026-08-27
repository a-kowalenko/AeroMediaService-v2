//! Marker files (`_fertig.txt` / `_in_verarbeitung.txt`) and payload parsing.
//!
//! Port of legacy `core/upload_markers.py` plus the marker/Kunde helpers from
//! `core/monitor.py`. HTTP customer lookup stays in a later phase.
#![allow(dead_code)]

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::model::kunde::{normalize_phone, Kunde};
use crate::storage::logging;

pub const MARKER_FERTIG: &str = "_fertig.txt";
pub const MARKER_PROCESSING: &str = "_in_verarbeitung.txt";

const PURE_CONTACT_MARKER_KEYS: [&str; 4] = ["vorname", "nachname", "email", "telefon"];
pub const MEDIA_FLAG_KEYS: [&str; 8] = [
    "handcam_foto",
    "handcam_video",
    "outside_foto",
    "outside_video",
    "ist_bezahlt_handcam_foto",
    "ist_bezahlt_handcam_video",
    "ist_bezahlt_outside_foto",
    "ist_bezahlt_outside_video",
];

#[derive(Debug, Error)]
pub enum MarkerError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("Marker-Datei ist leer.")]
    Empty,
    #[error("Marker-Datei ist kein gültiges JSON: {0}")]
    InvalidJson(String),
    #[error("Marker-JSON muss ein Objekt sein.")]
    NotAnObject,
    #[error("Pflichtfeld '{0}' fehlt oder ist leer.")]
    MissingField(&'static str),
    #[error("Pflichtfeld 'type' fehlt.")]
    MissingType,
    #[error(
        "Ungültiges Marker-Format. Erwartet entweder \
         'kunden_id_hash' + 'booking_id_hash' oder 'kunden_id' + 'booking_id'."
    )]
    InvalidApiFormat,
    #[error(
        "Ungültiges Marker-Format. Erwartet entweder \
         'kunden_id_hash' + 'booking_id_hash', 'kunden_id' + 'booking_id' \
         oder 'vorname' + 'nachname' + 'email'."
    )]
    InvalidFormat,
    #[error("API-Lookup-Marker — Customer-Fetch folgt in einer späteren Phase")]
    ApiLookupRequired,
    #[error("Marker-Inhalt ist leer — Datei kann nicht geschrieben werden.")]
    EmptyWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupMode {
    Hash,
    Id,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiMarkerQuery {
    pub customer_id: String,
    pub booking_id: String,
    #[serde(rename = "type")]
    pub marker_type: String,
}

pub fn marker_paths(folder_path: &Path) -> (PathBuf, PathBuf) {
    (
        folder_path.join(MARKER_FERTIG),
        folder_path.join(MARKER_PROCESSING),
    )
}

/// Reads a marker file: UTF-8 (with optional BOM) first, CP1252 on decode error.
pub fn read_marker_file(path: &Path) -> Result<String, MarkerError> {
    let bytes = fs::read(path)?;
    Ok(decode_marker_bytes(&bytes, path))
}

fn decode_marker_bytes(bytes: &[u8], path: &Path) -> String {
    let without_bom = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if let Ok(text) = std::str::from_utf8(without_bom) {
        return text.trim().to_string();
    }

    let (cow, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
    if logging::log_path().is_some() {
        logging::log_warn(&format!(
            "Marker {} nicht als UTF-8 lesbar, mit CP1252 gelesen.",
            path.display()
        ));
    }
    cow.trim().to_string()
}

/// Reads `_in_verarbeitung.txt` first, then `_fertig.txt`.
pub fn read_marker_raw(folder_path: &Path) -> Option<String> {
    let (fertig_path, processing_path) = marker_paths(folder_path);
    for path in [processing_path, fertig_path] {
        if !path.is_file() {
            continue;
        }
        if let Ok(content) = read_marker_file(&path) {
            return Some(content);
        }
    }
    None
}

pub fn write_marker_file(path: &Path, content: &str) -> Result<(), MarkerError> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(MarkerError::EmptyWrite);
    }
    fs::write(path, trimmed.as_bytes())?;
    Ok(())
}

pub fn write_fertig_marker(folder_path: &Path, content: &str) -> Result<PathBuf, MarkerError> {
    let (fertig, _) = marker_paths(folder_path);
    write_marker_file(&fertig, content)?;
    Ok(fertig)
}

pub fn write_processing_marker(folder_path: &Path, content: &str) -> Result<PathBuf, MarkerError> {
    let (_, processing) = marker_paths(folder_path);
    write_marker_file(&processing, content)?;
    Ok(processing)
}

/// Claim: `_fertig.txt` → `_in_verarbeitung.txt`.
pub fn claim_fertig_marker(folder_path: &Path) -> Result<(), MarkerError> {
    let (fertig, processing) = marker_paths(folder_path);
    fs::rename(fertig, processing)?;
    Ok(())
}

pub fn remove_upload_markers(folder_path: &Path) {
    for name in [MARKER_FERTIG, MARKER_PROCESSING] {
        let path = folder_path.join(name);
        if !path.is_file() {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => {
                if logging::log_path().is_some() {
                    logging::log_debug(&format!("Marker entfernt: {}", path.display()));
                }
            }
            Err(exc) => {
                if logging::log_path().is_some() {
                    logging::log_warn(&format!(
                        "Marker {name} konnte nicht entfernt werden: {exc}"
                    ));
                }
            }
        }
    }
}

/// Removes a leftover `_fertig.txt` in a folder that is already claimed.
pub fn discard_stale_fertig_marker(folder_path: &Path) -> bool {
    let (fertig_path, _) = marker_paths(folder_path);
    if !fertig_path.is_file() {
        return false;
    }
    match fs::remove_file(&fertig_path) {
        Ok(()) => {
            if logging::log_path().is_some() {
                let name = folder_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                logging::log_warn(&format!(
                    "Veraltetes _fertig.txt in bereits übernommenem Ordner '{name}' entfernt."
                ));
            }
            true
        }
        Err(exc) => {
            if logging::log_path().is_some() {
                logging::log_error(&format!(
                    "Veraltetes _fertig.txt konnte nicht entfernt werden ({}): {exc}",
                    folder_path.display()
                ));
            }
            false
        }
    }
}

/// `Handcam` → `Handycam` (exact match after trim). Other values are unchanged.
pub fn normalize_marker_type(raw_type: Option<&str>) -> String {
    let value = raw_type.unwrap_or("").trim();
    if value == "Handcam" {
        "Handycam".into()
    } else {
        value.to_string()
    }
}

pub fn load_marker_data(marker_content: &str) -> Result<Value, MarkerError> {
    if marker_content.trim().is_empty() {
        return Err(MarkerError::Empty);
    }
    let data: Value = serde_json::from_str(marker_content)
        .map_err(|exc| MarkerError::InvalidJson(exc.to_string()))?;
    if !data.is_object() {
        return Err(MarkerError::NotAnObject);
    }
    Ok(data)
}

fn as_object(data: &Value) -> Result<&Map<String, Value>, MarkerError> {
    data.as_object().ok_or(MarkerError::NotAnObject)
}

pub fn has_api_lookup_fields(data: &Value) -> bool {
    let Some(obj) = data.as_object() else {
        return false;
    };
    (obj.contains_key("kunden_id_hash") && obj.contains_key("booking_id_hash"))
        || (obj.contains_key("kunden_id") && obj.contains_key("booking_id"))
}

pub fn has_direct_contact_fields(data: &Value) -> bool {
    let Some(obj) = data.as_object() else {
        return false;
    };
    obj.contains_key("vorname") && obj.contains_key("nachname") && obj.contains_key("email")
}

/// True when the marker only has vorname, nachname, email, and optional telefon.
pub fn is_pure_contact_marker(data: &Value) -> bool {
    if !has_direct_contact_fields(data) {
        return false;
    }
    let Some(obj) = data.as_object() else {
        return false;
    };
    let allowed: HashSet<&str> = PURE_CONTACT_MARKER_KEYS.into_iter().collect();
    if !obj.keys().all(|k| allowed.contains(k.as_str())) {
        return false;
    }
    for field in ["vorname", "nachname", "email"] {
        if json_field_str(obj, field).trim().is_empty() {
            return false;
        }
    }
    true
}

/// With Custom API selected, pure-contact markers still upload via Dropbox.
pub fn should_use_dropbox_client_for_marker(
    selected_cloud_service: &str,
    marker_content: &str,
) -> Result<bool, MarkerError> {
    if selected_cloud_service != "custom_api" {
        return Ok(false);
    }
    let data = load_marker_data(marker_content)?;
    Ok(is_pure_contact_marker(&data))
}

pub fn parse_api_marker_data(data: &Value) -> Result<(ApiMarkerQuery, LookupMode), MarkerError> {
    let obj = as_object(data)?;
    let raw_type = json_field_str(obj, "type");
    let marker_type = normalize_marker_type(Some(&raw_type));
    if marker_type.is_empty() {
        return Err(MarkerError::MissingType);
    }

    if obj.contains_key("kunden_id_hash") && obj.contains_key("booking_id_hash") {
        return Ok((
            ApiMarkerQuery {
                customer_id: json_field_str(obj, "kunden_id_hash").trim().to_string(),
                booking_id: json_field_str(obj, "booking_id_hash").trim().to_string(),
                marker_type,
            },
            LookupMode::Hash,
        ));
    }

    if obj.contains_key("kunden_id") && obj.contains_key("booking_id") {
        return Ok((
            ApiMarkerQuery {
                customer_id: json_field_str(obj, "kunden_id").trim().to_string(),
                booking_id: json_field_str(obj, "booking_id").trim().to_string(),
                marker_type,
            },
            LookupMode::Id,
        ));
    }

    Err(MarkerError::InvalidApiFormat)
}

pub fn parse_marker_payload(
    marker_content: &str,
) -> Result<(ApiMarkerQuery, LookupMode), MarkerError> {
    parse_api_marker_data(&load_marker_data(marker_content)?)
}

fn parse_marker_bool(data: &Map<String, Value>, key: &str, default: bool) -> bool {
    match data.get(key) {
        None => default,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => {
            matches!(
                s.trim().to_ascii_lowercase().as_str(),
                "true" | "1" | "yes" | "ja"
            )
        }
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else if let Some(f) = n.as_f64() {
                f != 0.0
            } else {
                default
            }
        }
        _ => default,
    }
}

fn media_flags(data: &Map<String, Value>) -> [bool; 8] {
    let mut flags = [false; 8];
    for (i, key) in MEDIA_FLAG_KEYS.iter().enumerate() {
        flags[i] = parse_marker_bool(data, key, false);
    }
    flags
}

fn apply_media_flags(kunde: &mut Kunde, flags: [bool; 8]) {
    kunde.handcam_foto = flags[0];
    kunde.handcam_video = flags[1];
    kunde.outside_foto = flags[2];
    kunde.outside_video = flags[3];
    kunde.ist_bezahlt_handcam_foto = flags[4];
    kunde.ist_bezahlt_handcam_video = flags[5];
    kunde.ist_bezahlt_outside_foto = flags[6];
    kunde.ist_bezahlt_outside_video = flags[7];
}

const MEDIA_FLAG_ALIASES: [&[&str]; 8] = [
    &["handcam_foto", "handcamFoto", "handcam_photo", "handcamPhoto"],
    &["handcam_video", "handcamVideo"],
    &["outside_foto", "outsideFoto", "outside_photo", "outsidePhoto"],
    &["outside_video", "outsideVideo"],
    &[
        "ist_bezahlt_handcam_foto",
        "istBezahltHandcamFoto",
        "paid_handcam_foto",
        "paidHandcamFoto",
    ],
    &[
        "ist_bezahlt_handcam_video",
        "istBezahltHandcamVideo",
        "paid_handcam_video",
        "paidHandcamVideo",
    ],
    &[
        "ist_bezahlt_outside_foto",
        "istBezahltOutsideFoto",
        "paid_outside_foto",
        "paidOutsideFoto",
    ],
    &[
        "ist_bezahlt_outside_video",
        "istBezahltOutsideVideo",
        "paid_outside_video",
        "paidOutsideVideo",
    ],
];

/// QR-/API-`media`-Codes (ATS `media_flags_from_code`).
fn media_flags_from_code(code: &str) -> Option<(bool, bool, bool, bool)> {
    match code.trim().to_ascii_lowercase().as_str() {
        "none" | "" => Some((false, false, false, false)),
        "hc_f" => Some((true, false, false, false)),
        "hc_v" => Some((false, true, false, false)),
        "hc_fv" | "hc_vf" => Some((true, true, false, false)),
        "ou_f" => Some((false, false, true, false)),
        "ou_v" => Some((false, false, false, true)),
        "ou_fv" | "ou_vf" => Some((false, false, true, true)),
        _ => None,
    }
}

fn map_get_ci<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    if let Some(value) = obj.get(key) {
        return Some(value);
    }
    obj.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

fn parse_bool_value(value: &Value, default: bool) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::String(s) => matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes" | "ja"
        ),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else if let Some(f) = n.as_f64() {
                f != 0.0
            } else {
                default
            }
        }
        _ => default,
    }
}

/// `media.bezahlt` is often a payment method (`Bar`, `Online`), not a boolean.
fn parse_paid_value(value: &Value) -> Option<bool> {
    match value {
        Value::Null => None,
        Value::Bool(b) => Some(*b),
        Value::Number(n) => Some(
            n.as_i64()
                .map(|i| i != 0)
                .or_else(|| n.as_u64().map(|u| u != 0))
                .or_else(|| n.as_f64().map(|f| f != 0.0))
                .unwrap_or(false),
        ),
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Some(false);
            }
            match trimmed.to_ascii_lowercase().as_str() {
                "false" | "0" | "no" | "nein" | "none" | "null" | "offen" | "unpaid"
                | "unbezahlt" => Some(false),
                "true" | "1" | "yes" | "ja" | "paid" | "bezahlt" => Some(true),
                _ => Some(true),
            }
        }
        Value::Object(obj) => map_get_ci(obj, "bezahlt")
            .or_else(|| map_get_ci(obj, "paid"))
            .or_else(|| map_get_ci(obj, "ist_bezahlt"))
            .and_then(parse_paid_value),
        _ => None,
    }
}

fn parse_booked_product(value: Option<&Value>) -> bool {
    let Some(value) = value else {
        return false;
    };
    match value {
        Value::Null => false,
        Value::Bool(_) | Value::Number(_) | Value::String(_) => parse_bool_value(value, false),
        Value::Object(obj) => {
            if let Some(booked) = map_get_ci(obj, "gebucht")
                .or_else(|| map_get_ci(obj, "booked"))
                .or_else(|| map_get_ci(obj, "aktiv"))
            {
                return parse_bool_value(booked, true);
            }
            true
        }
        _ => false,
    }
}

struct FotoVideoPaid {
    foto: bool,
    video: bool,
    paid_foto: Option<bool>,
    paid_video: Option<bool>,
}

fn art_to_foto_video(art: &str) -> Option<(bool, bool)> {
    match art.trim().to_ascii_lowercase().as_str() {
        "foto" | "photo" | "f" => Some((true, false)),
        "video" | "v" => Some((false, true)),
        "foto_video" | "video_foto" | "foto+video" | "foto-video" | "foto video" | "fv" | "vf"
        | "both" => Some((true, true)),
        other => media_flags_from_code(other).map(|(hf, hv, of, ov)| (hf || of, hv || ov)),
    }
}

fn extract_foto_video_paid(data: &Value) -> FotoVideoPaid {
    let mut result = FotoVideoPaid {
        foto: false,
        video: false,
        paid_foto: None,
        paid_video: None,
    };
    let Some(obj) = data.as_object() else {
        return result;
    };
    let mut global_paid = None;

    if let Some(art) = ["art", "media_option", "mediaOption", "media_code", "mediaCode", "code"]
        .into_iter()
        .find_map(|key| map_get_ci(obj, key).and_then(value_as_option_code))
    {
        if let Some((foto, video)) = art_to_foto_video(&art) {
            result.foto |= foto;
            result.video |= video;
        }
    }

    result.foto |= parse_booked_product(map_get_ci(obj, "foto").or_else(|| map_get_ci(obj, "photo")));
    result.video |= parse_booked_product(map_get_ci(obj, "video"));

    if let Some(foto_obj) = map_get_ci(obj, "foto")
        .or_else(|| map_get_ci(obj, "photo"))
        .filter(|v| v.is_object())
    {
        result.paid_foto = parse_paid_value(foto_obj);
    }
    if let Some(video_obj) = map_get_ci(obj, "video").filter(|v| v.is_object()) {
        result.paid_video = parse_paid_value(video_obj);
    }

    if let Some(bezahlt) = map_get_ci(obj, "bezahlt") {
        if let Some(paid_obj) = bezahlt.as_object() {
            if let Some(foto_val) =
                map_get_ci(paid_obj, "foto").or_else(|| map_get_ci(paid_obj, "photo"))
            {
                result.foto |= parse_booked_product(Some(foto_val))
                    || parse_paid_value(foto_val) == Some(true);
                result.paid_foto = parse_paid_value(foto_val).or(result.paid_foto);
            }
            if let Some(video_val) = map_get_ci(paid_obj, "video") {
                result.video |= parse_booked_product(Some(video_val))
                    || parse_paid_value(video_val) == Some(true);
                result.paid_video = parse_paid_value(video_val).or(result.paid_video);
            }
        } else {
            global_paid = parse_paid_value(bezahlt);
        }
    }
    if global_paid.is_none() {
        global_paid = map_get_ci(obj, "paid")
            .filter(|v| !v.is_object())
            .and_then(parse_paid_value);
    }

    if let Some(foto_paid) = ["foto_paid", "fotoPaid", "bezahlt_foto", "bezahltFoto", "photo_paid"]
        .into_iter()
        .find_map(|key| map_get_ci(obj, key).and_then(parse_paid_value))
    {
        result.paid_foto = Some(foto_paid);
        if foto_paid {
            result.foto = true;
        }
    }
    if let Some(video_paid) = ["video_paid", "videoPaid", "bezahlt_video", "bezahltVideo"]
        .into_iter()
        .find_map(|key| map_get_ci(obj, key).and_then(parse_paid_value))
    {
        result.paid_video = Some(video_paid);
        if video_paid {
            result.video = true;
        }
    }
    if result.foto {
        result.paid_foto = result.paid_foto.or(global_paid);
    }
    if result.video {
        result.paid_video = result.paid_video.or(global_paid);
    }
    result
}

fn value_as_option_code(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Number(n) => Some(n.to_string()),
        Value::Object(obj) => map_get_ci(obj, "key")
            .or_else(|| map_get_ci(obj, "code"))
            .or_else(|| map_get_ci(obj, "art"))
            .or_else(|| map_get_ci(obj, "media_option"))
            .and_then(value_as_option_code),
        _ => None,
    }
}

fn family_from_typ(typ: &str) -> Option<&'static str> {
    let normalized = normalize_marker_type(Some(typ)).to_ascii_lowercase();
    if normalized == "outside" {
        Some("outside")
    } else if normalized == "handycam" || normalized == "handcam" {
        Some("handcam")
    } else {
        None
    }
}

fn family_from_media_toggles(data: &Value) -> Option<&'static str> {
    let obj = data.as_object()?;
    let as_toggle = |key: &str| {
        map_get_ci(obj, key)
            .filter(|v| !v.is_object() && !v.is_array())
            .map(|v| parse_bool_value(v, false))
    };
    let outside = as_toggle("outside");
    let handcam = as_toggle("handycam").or_else(|| as_toggle("handcam"));
    match (outside, handcam) {
        (Some(true), Some(false)) | (Some(true), None) => Some("outside"),
        (Some(false), Some(true)) | (None, Some(true)) => Some("handcam"),
        _ => None,
    }
}

fn apply_family_flags(flags: &mut [bool; 8], family: &str, fv: &FotoVideoPaid) {
    let (foto_i, video_i, paid_foto_i, paid_video_i) = match family {
        "handcam" => (0, 1, 4, 5),
        "outside" => (2, 3, 6, 7),
        _ => return,
    };
    if fv.foto {
        flags[foto_i] = true;
        if let Some(paid) = fv.paid_foto {
            flags[paid_foto_i] = paid;
        }
    }
    if fv.video {
        flags[video_i] = true;
        if let Some(paid) = fv.paid_video {
            flags[paid_video_i] = paid;
        }
    }
}

fn api_media_sources(customer: &Value) -> Vec<&Value> {
    let mut sources = vec![customer];
    if let Some(obj) = customer.as_object() {
        for key in [
            "media",
            "media_option",
            "mediaOption",
            "booking",
            "handycam",
            "handcam",
            "outside",
            "produkte",
            "products",
            "extras",
            "optionen",
            "options",
            "flags",
        ] {
            if let Some(nested) = map_get_ci(obj, key).filter(|v| v.is_object()) {
                sources.push(nested);
            }
        }
    }
    sources
}

fn apply_code_flags(flags: &mut [bool; 8], code: &str, paid: Option<bool>) -> bool {
    let Some((hf, hv, of, ov)) = media_flags_from_code(code) else {
        return false;
    };
    flags[0] |= hf;
    flags[1] |= hv;
    flags[2] |= of;
    flags[3] |= ov;
    let paid = paid.unwrap_or(true);
    flags[4] |= hf && paid;
    flags[5] |= hv && paid;
    flags[6] |= of && paid;
    flags[7] |= ov && paid;
    true
}

/// Live `/aero-media-customer` payload: nested `media` + `typ`, QR-Codes, or eight booleans.
fn apply_api_customer_media_flags(kunde: &mut Kunde, customer: &Value) {
    let mut flags = [false; 8];
    let sources = api_media_sources(customer);

    for source in &sources {
        if let Some(code) = source.as_str() {
            apply_code_flags(&mut flags, code, None);
            continue;
        }
        let Some(obj) = source.as_object() else {
            continue;
        };
        if let Some(code) = [
            "media",
            "media_option",
            "mediaOption",
            "code",
            "art",
            "media_code",
            "mediaCode",
        ]
        .into_iter()
        .find_map(|key| {
            map_get_ci(obj, key)
                .and_then(value_as_option_code)
                .filter(|s| media_flags_from_code(s).is_some())
        })
        {
            let paid = map_get_ci(obj, "bezahlt")
                .or_else(|| map_get_ci(obj, "paid"))
                .and_then(parse_paid_value);
            apply_code_flags(&mut flags, &code, paid);
        }
    }

    let typ = customer
        .as_object()
        .and_then(|obj| {
            map_get_ci(obj, "typ")
                .or_else(|| map_get_ci(obj, "type"))
                .map(json_to_python_str)
        })
        .unwrap_or_default();
    let family = family_from_typ(&typ).or_else(|| {
        sources
            .iter()
            .find_map(|source| family_from_media_toggles(source))
    });

    for source in &sources {
        let fv = extract_foto_video_paid(source);
        if !(fv.foto || fv.video) {
            continue;
        }
        if let Some(family) = family {
            apply_family_flags(&mut flags, family, &fv);
        }
    }

    if let Some(obj) = customer.as_object() {
        for (key, family) in [
            ("handycam", "handcam"),
            ("handcam", "handcam"),
            ("outside", "outside"),
        ] {
            if let Some(nested) = map_get_ci(obj, key) {
                apply_family_flags(&mut flags, family, &extract_foto_video_paid(nested));
            }
            if let Some(media) = map_get_ci(obj, "media").and_then(Value::as_object) {
                if let Some(nested) = map_get_ci(media, key) {
                    apply_family_flags(&mut flags, family, &extract_foto_video_paid(nested));
                }
            }
        }
    }

    for source in &sources {
        let Some(obj) = source.as_object() else {
            continue;
        };
        for (i, aliases) in MEDIA_FLAG_ALIASES.iter().enumerate() {
            for alias in *aliases {
                if let Some(value) = map_get_ci(obj, alias) {
                    flags[i] = if i >= 4 {
                        parse_paid_value(value).unwrap_or(false)
                    } else {
                        parse_bool_value(value, false)
                    };
                    break;
                }
            }
        }
    }

    apply_media_flags(kunde, flags);
}

/// Compact shape of the customer payload for logs (no names/emails).
pub fn describe_customer_media_shape(customer: &Value) -> String {
    let Some(obj) = customer.as_object() else {
        return "customer=kein Objekt".into();
    };
    let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    let typ = map_get_ci(obj, "typ")
        .or_else(|| map_get_ci(obj, "type"))
        .map(json_to_python_str)
        .unwrap_or_default();
    let media = match obj.get("media") {
        None => "media=fehlt".into(),
        Some(Value::String(s)) => format!("media=\"{s}\""),
        Some(Value::Object(m)) => {
            let nested: Vec<&str> = m.keys().map(String::as_str).collect();
            format!("media.keys=[{}]", nested.join(","))
        }
        Some(Value::Bool(b)) => format!("media={b}"),
        Some(Value::Number(n)) => format!("media={n}"),
        Some(Value::Array(a)) => format!("media=array({})", a.len()),
        Some(Value::Null) => "media=null".into(),
    };
    let media_option = map_get_ci(obj, "media_option")
        .or_else(|| map_get_ci(obj, "mediaOption"))
        .and_then(value_as_option_code)
        .unwrap_or_default();
    let foto_paid = map_get_ci(obj, "foto_paid").map(json_to_python_str);
    let video_paid = map_get_ci(obj, "video_paid").map(json_to_python_str);
    let paid = map_get_ci(obj, "paid").map(json_to_python_str);
    format!(
        "typ={typ:?} media_option={media_option:?} foto_paid={foto_paid:?} video_paid={video_paid:?} paid={paid:?} keys=[{}] {media}",
        keys.join(",")
    )
}

/// True when at least one booked/paid flag key is present (even if false).
pub fn history_has_media_flags(data: &Value) -> bool {
    let Some(obj) = data.as_object() else {
        return false;
    };
    MEDIA_FLAG_KEYS.iter().any(|key| obj.contains_key(*key))
}

/// True when at least one of the four products is booked.
pub fn history_has_booked_option(data: &Value) -> bool {
    let Some(obj) = data.as_object() else {
        return false;
    };
    ["handcam_foto", "handcam_video", "outside_foto", "outside_video"]
        .iter()
        .any(|key| parse_marker_bool(obj, key, false))
}

pub fn apply_media_flags_from_json(kunde: &mut Kunde, data: &Value) {
    if let Some(obj) = data.as_object() {
        apply_media_flags(kunde, media_flags(obj));
    }
}

/// Overlay marker/API flags only when the payload actually contains those keys.
pub fn apply_media_flags_if_present(kunde: &mut Kunde, data: &Value) {
    if history_has_media_flags(data) {
        apply_media_flags_from_json(kunde, data);
    }
}

pub fn merge_kunde_media_flags(target: &mut Value, kunde: &Kunde) {
    let Value::Object(map) = target else {
        return;
    };
    map.insert("handcam_foto".into(), Value::Bool(kunde.handcam_foto));
    map.insert("handcam_video".into(), Value::Bool(kunde.handcam_video));
    map.insert("outside_foto".into(), Value::Bool(kunde.outside_foto));
    map.insert("outside_video".into(), Value::Bool(kunde.outside_video));
    map.insert(
        "ist_bezahlt_handcam_foto".into(),
        Value::Bool(kunde.ist_bezahlt_handcam_foto),
    );
    map.insert(
        "ist_bezahlt_handcam_video".into(),
        Value::Bool(kunde.ist_bezahlt_handcam_video),
    );
    map.insert(
        "ist_bezahlt_outside_foto".into(),
        Value::Bool(kunde.ist_bezahlt_outside_foto),
    );
    map.insert(
        "ist_bezahlt_outside_video".into(),
        Value::Bool(kunde.ist_bezahlt_outside_video),
    );
}

fn json_to_python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn json_field_str(obj: &Map<String, Value>, key: &str) -> String {
    match obj.get(key) {
        None => String::new(),
        Some(v) => json_to_python_str(v),
    }
}

fn json_phone(obj: &Map<String, Value>, key: &str) -> Option<String> {
    match obj.get(key) {
        None | Some(Value::Null) => None,
        Some(v) => normalize_phone(Some(&json_to_python_str(v))),
    }
}

fn nonempty_or_none(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Maps a direct-contact marker (`vorname` / `nachname` / `email`) onto `Kunde`.
pub fn build_kunde_from_marker(data: &Value) -> Result<Kunde, MarkerError> {
    let obj = as_object(data)?;
    for field in ["vorname", "nachname", "email"] {
        if json_field_str(obj, field).trim().is_empty() {
            return Err(MarkerError::MissingField(field));
        }
    }

    let raw_type = json_field_str(obj, "type");
    let marker_type = normalize_marker_type(Some(&raw_type));
    let mut kunde = Kunde {
        first_name: Some(json_field_str(obj, "vorname").trim().to_string()),
        last_name: Some(json_field_str(obj, "nachname").trim().to_string()),
        email: Some(json_field_str(obj, "email").trim().to_string()),
        phone: json_phone(obj, "telefon"),
        customer_number: None,
        booking_number: None,
        customer_type: nonempty_or_none(marker_type),
        ..Kunde::default()
    };
    apply_media_flags(&mut kunde, media_flags(obj));
    Ok(kunde)
}

/// Maps a customer-API payload onto `Kunde` (no HTTP).
pub fn build_kunde_from_customer(customer: &Value) -> Result<Kunde, MarkerError> {
    let obj = as_object(customer)?;
    let mut kunde = Kunde {
        customer_number: nonempty_or_none(json_field_str(obj, "customer_id")),
        booking_number: nonempty_or_none(json_field_str(obj, "booking_id")),
        email: nonempty_or_none(json_field_str(obj, "email")),
        first_name: nonempty_or_none(json_field_str(obj, "vorname")),
        last_name: nonempty_or_none(json_field_str(obj, "nachname")),
        phone: json_phone(obj, "telefon"),
        customer_type: nonempty_or_none(json_field_str(obj, "typ")),
        ..Kunde::default()
    };
    apply_api_customer_media_flags(&mut kunde, customer);
    Ok(kunde)
}

/// Resolves a marker to `Kunde`. API-hash/id markers require an HTTP fetch (see custom_api::orders).
pub fn resolve_kunde_from_marker(marker_content: &str) -> Result<Kunde, MarkerError> {
    let data = load_marker_data(marker_content)?;
    if has_api_lookup_fields(&data) {
        let _ = parse_api_marker_data(&data)?;
        return Err(MarkerError::ApiLookupRequired);
    }
    if has_direct_contact_fields(&data) {
        return build_kunde_from_marker(&data);
    }
    Err(MarkerError::InvalidFormat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn sample_extended() -> Value {
        json!({
            "vorname": "Andy",
            "nachname": "Kowa",
            "email": "gsr.andy@hotmail.de",
            "type": "Outside",
            "handcam_foto": false,
            "handcam_video": false,
            "outside_foto": true,
            "outside_video": true,
            "ist_bezahlt_handcam_foto": false,
            "ist_bezahlt_handcam_video": false,
            "ist_bezahlt_outside_foto": true,
            "ist_bezahlt_outside_video": false,
            "telefon": "016099501966",
        })
    }

    fn sample_pure() -> Value {
        json!({
            "vorname": "Andreas",
            "nachname": "Kowalenko",
            "email": "andreas@kowalenko.de",
        })
    }

    fn sample_pure_phone() -> Value {
        json!({
            "vorname": "Andreas",
            "nachname": "Kowalenko",
            "email": "gsr.andy@hotmail.de",
            "telefon": "016099501966",
        })
    }

    fn encoding_payload() -> Value {
        json!({
            "vorname": "Max",
            "nachname": "Möller",
            "email": "max@example.de",
        })
    }

    #[test]
    fn utf8_and_cp1252_marker_roundtrip() {
        let dir = tempdir().unwrap();
        let utf8_path = dir.path().join(MARKER_FERTIG);
        let cp1252_path = dir.path().join(MARKER_PROCESSING);
        let payload = encoding_payload();
        let json_text = serde_json::to_string(&payload).unwrap();

        fs::write(&utf8_path, json_text.as_bytes()).unwrap();
        let (encoded, _, _) = encoding_rs::WINDOWS_1252.encode(&json_text);
        fs::write(&cp1252_path, encoded.as_ref()).unwrap();

        let utf8_raw = read_marker_file(&utf8_path).unwrap();
        let cp1252_raw = read_marker_file(&cp1252_path).unwrap();
        let folder_raw = read_marker_raw(dir.path()).unwrap();

        assert_eq!(
            serde_json::from_str::<Value>(&utf8_raw).unwrap()["nachname"],
            "Möller"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&cp1252_raw).unwrap()["nachname"],
            "Möller"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&folder_raw).unwrap()["nachname"],
            "Möller"
        );

        let kunde = resolve_kunde_from_marker(&cp1252_raw).unwrap();
        assert_eq!(kunde.last_name.as_deref(), Some("Möller"));
    }

    #[test]
    fn utf8_bom_is_stripped() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(MARKER_FERTIG);
        let json_text = serde_json::to_string(&encoding_payload()).unwrap();
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(json_text.as_bytes());
        fs::write(&path, bytes).unwrap();

        let raw = read_marker_file(&path).unwrap();
        assert!(!raw.starts_with('\u{feff}'));
        assert_eq!(
            serde_json::from_str::<Value>(&raw).unwrap()["nachname"],
            "Möller"
        );
    }

    #[test]
    fn processing_marker_is_preferred_over_fertig() {
        let dir = tempdir().unwrap();
        write_fertig_marker(
            dir.path(),
            r#"{"vorname":"A","nachname":"Fertig","email":"a@b.de"}"#,
        )
        .unwrap();
        write_processing_marker(
            dir.path(),
            r#"{"vorname":"A","nachname":"Processing","email":"a@b.de"}"#,
        )
        .unwrap();
        let raw = read_marker_raw(dir.path()).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&raw).unwrap()["nachname"],
            "Processing"
        );
    }

    #[test]
    fn write_read_delete_and_claim() {
        let dir = tempdir().unwrap();
        let content = r#"{"vorname":"A","nachname":"B","email":"a@b.de"}"#;
        write_fertig_marker(dir.path(), content).unwrap();
        assert!(dir.path().join(MARKER_FERTIG).is_file());

        claim_fertig_marker(dir.path()).unwrap();
        assert!(!dir.path().join(MARKER_FERTIG).is_file());
        assert!(dir.path().join(MARKER_PROCESSING).is_file());

        write_fertig_marker(dir.path(), content).unwrap();
        assert!(discard_stale_fertig_marker(dir.path()));
        assert!(!dir.path().join(MARKER_FERTIG).is_file());
        assert!(!discard_stale_fertig_marker(dir.path()));

        remove_upload_markers(dir.path());
        assert!(!dir.path().join(MARKER_PROCESSING).is_file());
        assert!(read_marker_raw(dir.path()).is_none());
    }

    #[test]
    fn empty_write_is_rejected() {
        let dir = tempdir().unwrap();
        let err = write_fertig_marker(dir.path(), "   ").unwrap_err();
        assert!(matches!(err, MarkerError::EmptyWrite));
    }

    #[test]
    fn build_kunde_from_extended_marker() {
        let k = build_kunde_from_marker(&sample_extended()).unwrap();
        assert_eq!(k.first_name.as_deref(), Some("Andy"));
        assert_eq!(k.customer_type.as_deref(), Some("Outside"));
        assert!(k.outside_foto);
        assert!(k.outside_video);
        assert!(k.ist_bezahlt_outside_foto);
        assert!(!k.ist_bezahlt_outside_video);
        assert_eq!(k.phone.as_deref(), Some("016099501966"));
    }

    #[test]
    fn phone_placeholders_become_none() {
        let mut pure = sample_pure();
        pure["telefon"] = Value::Null;
        assert_eq!(build_kunde_from_marker(&pure).unwrap().phone, None);

        pure["telefon"] = json!("None");
        assert_eq!(build_kunde_from_marker(&pure).unwrap().phone, None);

        let customer = json!({
            "telefon": Value::Null,
            "customer_id": "1",
            "booking_id": "2",
            "email": "a@b.de",
            "vorname": "A",
            "nachname": "B",
            "typ": "",
        });
        let k = build_kunde_from_customer(&customer).unwrap();
        assert_eq!(k.phone, None);
        assert_eq!(k.customer_number.as_deref(), Some("1"));
        assert_eq!(k.booking_number.as_deref(), Some("2"));
        assert_eq!(k.customer_type, None);
    }

    #[test]
    fn pure_contact_detection() {
        assert!(!is_pure_contact_marker(&sample_extended()));
        assert!(is_pure_contact_marker(&sample_pure()));
        assert!(is_pure_contact_marker(&sample_pure_phone()));
        let empty_name = json!({"vorname":"","nachname":"X","email":"a@b.de"});
        assert!(!is_pure_contact_marker(&empty_name));
    }

    #[test]
    fn dropbox_override_only_for_custom_api_pure_contact() {
        let pure = serde_json::to_string(&sample_pure()).unwrap();
        let pure_phone = serde_json::to_string(&sample_pure_phone()).unwrap();
        let extended = serde_json::to_string(&sample_extended()).unwrap();

        assert!(should_use_dropbox_client_for_marker("custom_api", &pure).unwrap());
        assert!(should_use_dropbox_client_for_marker("custom_api", &pure_phone).unwrap());
        assert!(!should_use_dropbox_client_for_marker("custom_api", &extended).unwrap());
        assert!(!should_use_dropbox_client_for_marker("dropbox", &pure).unwrap());
    }

    #[test]
    fn resolve_direct_marker_and_reject_api_lookup() {
        let k =
            resolve_kunde_from_marker(&serde_json::to_string(&sample_extended()).unwrap()).unwrap();
        assert!(k.outside_foto);

        let api = json!({
            "type": "Handcam",
            "kunden_id_hash": "abc",
            "booking_id_hash": "def",
        });
        let err = resolve_kunde_from_marker(&serde_json::to_string(&api).unwrap()).unwrap_err();
        assert!(matches!(err, MarkerError::ApiLookupRequired));
    }

    #[test]
    fn handcam_normalizes_to_handycam() {
        assert_eq!(normalize_marker_type(Some("Handcam")), "Handycam");
        assert_eq!(normalize_marker_type(Some(" Handcam ")), "Handycam");
        assert_eq!(normalize_marker_type(Some("Handycam")), "Handycam");
        assert_eq!(normalize_marker_type(Some("handcam")), "handcam");
        assert_eq!(normalize_marker_type(Some("Outside")), "Outside");
        assert_eq!(normalize_marker_type(None), "");

        let marker = json!({
            "vorname": "A",
            "nachname": "B",
            "email": "a@b.de",
            "type": "Handcam",
        });
        let k = build_kunde_from_marker(&marker).unwrap();
        assert_eq!(k.customer_type.as_deref(), Some("Handycam"));

        let (query, mode) = parse_api_marker_data(&json!({
            "type": "Handcam",
            "kunden_id": "1",
            "booking_id": "2",
        }))
        .unwrap();
        assert_eq!(query.marker_type, "Handycam");
        assert_eq!(mode, LookupMode::Id);
    }

    #[test]
    fn parse_api_hash_takes_precedence() {
        let (query, mode) = parse_marker_payload(
            r#"{"type":"Outside","kunden_id_hash":"h1","booking_id_hash":"h2","kunden_id":"1","booking_id":"2"}"#,
        )
        .unwrap();
        assert_eq!(mode, LookupMode::Hash);
        assert_eq!(query.customer_id, "h1");
        assert_eq!(query.booking_id, "h2");
        assert_eq!(query.marker_type, "Outside");
    }

    #[test]
    fn parse_api_requires_type() {
        let err = parse_api_marker_data(&json!({
            "kunden_id": "1",
            "booking_id": "2",
        }))
        .unwrap_err();
        assert!(matches!(err, MarkerError::MissingType));
    }

    #[test]
    fn bool_flags_accept_string_and_number() {
        let data = json!({
            "vorname": "A",
            "nachname": "B",
            "email": "a@b.de",
            "handcam_foto": "true",
            "handcam_video": "JA",
            "outside_foto": 1,
            "outside_video": 0,
            "ist_bezahlt_handcam_foto": "yes",
            "ist_bezahlt_handcam_video": "false",
        });
        let k = build_kunde_from_marker(&data).unwrap();
        assert!(k.handcam_foto);
        assert!(k.handcam_video);
        assert!(k.outside_foto);
        assert!(!k.outside_video);
        assert!(k.ist_bezahlt_handcam_foto);
        assert!(!k.ist_bezahlt_handcam_video);
    }

    #[test]
    fn history_media_flags_detect_presence_and_apply() {
        assert!(!history_has_media_flags(&json!({
            "first_name": "Ada",
            "marker_raw": "{}",
        })));
        let data = json!({
            "handcam_video": true,
            "ist_bezahlt_handcam_video": false,
        });
        assert!(history_has_media_flags(&data));
        let mut k = Kunde::default();
        apply_media_flags_from_json(&mut k, &data);
        assert!(k.handcam_video);
        assert!(!k.ist_bezahlt_handcam_video);

        let mut payload = json!({ "dir_name": "Flug" });
        k.outside_foto = true;
        k.ist_bezahlt_outside_foto = true;
        merge_kunde_media_flags(&mut payload, &k);
        assert_eq!(payload["outside_foto"], true);
        assert_eq!(payload["ist_bezahlt_outside_foto"], true);
        assert_eq!(payload["handcam_video"], true);
    }

    #[test]
    fn history_has_booked_option_ignores_all_false_keys() {
        assert!(!history_has_booked_option(&json!({
            "handcam_foto": false,
            "handcam_video": false,
        })));
        assert!(history_has_booked_option(&json!({
            "outside_foto": true,
            "ist_bezahlt_outside_foto": false,
        })));
    }

    #[test]
    fn customer_api_nested_media_outside_paid() {
        let customer = json!({
            "customer_id": "1",
            "booking_id": "2",
            "vorname": "Anna",
            "nachname": "Muster",
            "email": "a@b.de",
            "typ": "Outside",
            "media": {
                "foto": true,
                "video": true,
                "bezahlt": "Bar",
                "url": "https://example.com/order"
            }
        });
        let k = build_kunde_from_customer(&customer).unwrap();
        assert!(!k.handcam_foto && !k.handcam_video);
        assert!(k.outside_foto && k.outside_video);
        assert!(k.ist_bezahlt_outside_foto && k.ist_bezahlt_outside_video);
    }

    #[test]
    fn customer_api_media_code_ou_fv() {
        let customer = json!({
            "customer_id": "1",
            "booking_id": "2",
            "vorname": "Anna",
            "nachname": "Muster",
            "email": "a@b.de",
            "typ": "Outside",
            "media": "ou_fv"
        });
        let k = build_kunde_from_customer(&customer).unwrap();
        assert!(k.outside_foto && k.outside_video);
        assert!(k.ist_bezahlt_outside_foto && k.ist_bezahlt_outside_video);
        assert!(!k.handcam_foto && !k.handcam_video);
    }

    #[test]
    fn customer_api_nested_foto_objects_unpaid_video() {
        let customer = json!({
            "customer_id": "1",
            "booking_id": "2",
            "vorname": "Anna",
            "nachname": "Muster",
            "email": "a@b.de",
            "typ": "Handycam",
            "media": {
                "foto": { "bezahlt": true },
                "video": { "bezahlt": false }
            }
        });
        let k = build_kunde_from_customer(&customer).unwrap();
        assert!(k.handcam_foto && k.handcam_video);
        assert!(k.ist_bezahlt_handcam_foto);
        assert!(!k.ist_bezahlt_handcam_video);
        assert!(!k.outside_foto && !k.outside_video);
    }

    #[test]
    fn customer_api_explicit_booleans_still_win() {
        let customer = json!({
            "customer_id": "1",
            "booking_id": "2",
            "vorname": "Anna",
            "nachname": "Muster",
            "email": "a@b.de",
            "typ": "Outside",
            "media": { "foto": true, "video": true, "bezahlt": true },
            "outside_foto": true,
            "outside_video": true,
            "ist_bezahlt_outside_foto": true,
            "ist_bezahlt_outside_video": false,
        });
        let k = build_kunde_from_customer(&customer).unwrap();
        assert!(k.outside_foto && k.outside_video);
        assert!(k.ist_bezahlt_outside_foto);
        assert!(!k.ist_bezahlt_outside_video);
    }

    #[test]
    fn customer_api_media_toggles_without_typ() {
        let customer = json!({
            "customer_id": "1",
            "booking_id": "2",
            "vorname": "Anna",
            "nachname": "Muster",
            "email": "a@b.de",
            "media": {
                "outside": true,
                "handycam": false,
                "foto": true,
                "video": true,
                "bezahlt": true
            }
        });
        let k = build_kunde_from_customer(&customer).unwrap();
        assert!(k.outside_foto && k.outside_video);
        assert!(k.ist_bezahlt_outside_foto && k.ist_bezahlt_outside_video);
        assert!(!k.handcam_foto && !k.handcam_video);
    }

    #[test]
    fn customer_api_foto_paid_video_paid_outside() {
        let customer = json!({
            "Created_at": "2026-08-18",
            "booking_id": "2",
            "customer_id": "1",
            "email": "a@b.de",
            "foto_paid": true,
            "media_option": "ou_fv",
            "nachname": "Muster",
            "paid": false,
            "telefon": "0123",
            "typ": "outside",
            "video_paid": true,
            "vorname": "Anna",
        });
        let k = build_kunde_from_customer(&customer).unwrap();
        assert!(k.outside_foto && k.outside_video);
        assert!(k.ist_bezahlt_outside_foto && k.ist_bezahlt_outside_video);
        assert!(!k.handcam_foto && !k.handcam_video);
        assert!(!k.ist_bezahlt_handcam_foto && !k.ist_bezahlt_handcam_video);
    }

    #[test]
    fn customer_api_paid_flags_without_media_option() {
        let customer = json!({
            "booking_id": "2",
            "customer_id": "1",
            "email": "a@b.de",
            "foto_paid": true,
            "nachname": "Muster",
            "paid": false,
            "typ": "outside",
            "video_paid": true,
            "vorname": "Anna",
        });
        let k = build_kunde_from_customer(&customer).unwrap();
        assert!(k.outside_foto && k.outside_video);
        assert!(k.ist_bezahlt_outside_foto && k.ist_bezahlt_outside_video);
    }

    #[test]
    fn customer_api_media_option_unpaid_foto() {
        let customer = json!({
            "booking_id": "2",
            "customer_id": "1",
            "email": "a@b.de",
            "foto_paid": false,
            "media_option": "ou_fv",
            "nachname": "Muster",
            "typ": "outside",
            "video_paid": true,
            "vorname": "Anna",
        });
        let k = build_kunde_from_customer(&customer).unwrap();
        assert!(k.outside_foto && k.outside_video);
        assert!(!k.ist_bezahlt_outside_foto);
        assert!(k.ist_bezahlt_outside_video);
    }

    #[test]
    fn customer_api_media_option_object_with_key() {
        let customer = json!({
            "booking_id": "2",
            "customer_id": "1",
            "email": "a@b.de",
            "foto_paid": true,
            "media_option": {
                "key": "ou_fv",
                "created_at": "2026-08-16T06:17:24.370799+00:00",
                "foto": false,
                "video": true
            },
            "nachname": "Muster",
            "typ": "outside",
            "video_paid": true,
            "vorname": "Anna",
        });
        let k = build_kunde_from_customer(&customer).unwrap();
        assert!(k.outside_foto && k.outside_video);
        assert!(k.ist_bezahlt_outside_foto && k.ist_bezahlt_outside_video);
    }

    #[test]
    fn missing_contact_fields_and_invalid_json() {
        assert!(matches!(load_marker_data("   "), Err(MarkerError::Empty)));
        assert!(matches!(
            load_marker_data("[1,2]"),
            Err(MarkerError::NotAnObject)
        ));
        assert!(matches!(
            load_marker_data("{not json"),
            Err(MarkerError::InvalidJson(_))
        ));
        let err = build_kunde_from_marker(&json!({"vorname":"A","nachname":"B","email":"  "}))
            .unwrap_err();
        assert!(matches!(err, MarkerError::MissingField("email")));
        let err = resolve_kunde_from_marker(r#"{"foo":1}"#).unwrap_err();
        assert!(matches!(err, MarkerError::InvalidFormat));
    }
}
