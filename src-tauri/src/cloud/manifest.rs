//! Dropbox Manifest v1.1 (paths_only) builder for Cloud `/api/orders/create`.
//! Port of legacy `utils/dropbox_manifest.py`.

use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Map, Value};

use crate::model::kunde::Kunde;

pub const STANDARD_CATEGORIES: [&str; 6] = [
    "Outside_Foto",
    "Handcam_Foto",
    "Preview_Foto",
    "Outside_Video",
    "Handcam_Video",
    "Preview_Video",
];

pub fn is_standard_category(name: &str) -> bool {
    STANDARD_CATEGORIES.contains(&name)
}

pub fn normalize_customer_type(raw_type: Option<&str>) -> String {
    let value = raw_type.unwrap_or("").trim().to_ascii_lowercase();
    if value == "handycam" || value == "handcam" {
        return "handcam".into();
    }
    if value == "outside" {
        return "outside".into();
    }
    if value.is_empty() {
        "outside".into()
    } else {
        value
    }
}

fn client_hints(category_names: &HashSet<String>) -> Value {
    json!({
        "has_previews": category_names.iter().any(|n| n.contains("Preview_")),
        "has_videos": category_names.iter().any(|n| n.contains("_Video")),
        "has_photos": category_names.iter().any(|n| n.contains("_Foto")),
    })
}

fn kunde_str(value: Option<&str>) -> String {
    value.unwrap_or("").to_string()
}

/// Build Manifest v1.1 (paths_only) from local upload results.
///
/// Each entry in `uploaded_files` must have: `name`, `rel_path`, `size`, `mime`;
/// `dropbox_id` is optional.
pub fn build_manifest_v11(
    base_dir: &str,
    kunde: Option<&Kunde>,
    uploaded_files: &[Value],
    root_share_link: Option<&str>,
    uploader_version: &str,
) -> Value {
    let mut categories_map: BTreeMap<String, Vec<Value>> = BTreeMap::new();

    for file_row in uploaded_files {
        let rel_path = file_row
            .get("rel_path")
            .and_then(Value::as_str)
            .or_else(|| file_row.get("file_name").and_then(Value::as_str))
            .unwrap_or("")
            .replace('\\', "/");
        if rel_path.is_empty() {
            crate::storage::logging::log_warn(&format!(
                "Datei ohne rel_path übersprungen: {file_row}"
            ));
            continue;
        }

        let parts: Vec<&str> = rel_path.split('/').collect();
        if parts.len() < 2 {
            crate::storage::logging::log_warn(&format!(
                "Datei ohne Kategorie-Unterordner übersprungen: {rel_path}"
            ));
            continue;
        }

        let category_name = parts[0];
        if !is_standard_category(category_name) {
            crate::storage::logging::log_warn(&format!(
                "Unbekannte Kategorie '{category_name}' für {rel_path} — wird übersprungen."
            ));
            continue;
        }

        let name = file_row
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                rel_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&rel_path)
                    .to_string()
            });
        let size = file_row
            .get("size")
            .and_then(Value::as_u64)
            .or_else(|| file_row.get("size").and_then(Value::as_i64).map(|v| v as u64))
            .or_else(|| {
                file_row
                    .get("file_size")
                    .and_then(Value::as_u64)
            })
            .unwrap_or(0);
        let mime = file_row
            .get("mime")
            .and_then(Value::as_str)
            .or_else(|| file_row.get("type").and_then(Value::as_str))
            .unwrap_or("application/octet-stream");

        let mut entry = Map::new();
        entry.insert("name".into(), json!(name));
        entry.insert("rel_path".into(), json!(rel_path));
        entry.insert("size".into(), json!(size));
        entry.insert("mime".into(), json!(mime));
        if let Some(dropbox_id) = file_row.get("dropbox_id").and_then(Value::as_str) {
            if !dropbox_id.is_empty() {
                entry.insert("dropbox_id".into(), json!(dropbox_id));
            }
        }
        categories_map
            .entry(category_name.to_string())
            .or_default()
            .push(Value::Object(entry));
    }

    let mut categories = Vec::new();
    let mut files_count = 0u64;
    let mut bytes_total = 0u64;
    let category_names: HashSet<String> = categories_map.keys().cloned().collect();
    for (category_name, mut files) in categories_map {
        files.sort_by(|a, b| {
            let pa = a.get("rel_path").and_then(Value::as_str).unwrap_or("");
            let pb = b.get("rel_path").and_then(Value::as_str).unwrap_or("");
            pa.cmp(pb)
        });
        files_count += files.len() as u64;
        bytes_total += files
            .iter()
            .map(|f| f.get("size").and_then(Value::as_u64).unwrap_or(0))
            .sum::<u64>();
        categories.push(json!({
            "name": category_name,
            "folder_path": format!("/{base_dir}/{category_name}"),
            "files": files,
        }));
    }

    let customer = json!({
        "customer_number": kunde_str(kunde.and_then(|k| k.customer_number.as_deref())),
        "booking_number": kunde_str(kunde.and_then(|k| k.booking_number.as_deref())),
        "type": normalize_customer_type(kunde.and_then(|k| k.customer_type.as_deref())),
        "first_name": kunde_str(kunde.and_then(|k| k.first_name.as_deref())),
        "last_name": kunde_str(kunde.and_then(|k| k.last_name.as_deref())),
        "email": kunde_str(kunde.and_then(|k| k.email.as_deref())),
        "phone": kunde_str(kunde.and_then(|k| k.phone.as_deref())),
        "handcam_foto": kunde.map(|k| k.handcam_foto).unwrap_or(false),
        "handcam_video": kunde.map(|k| k.handcam_video).unwrap_or(false),
        "outside_foto": kunde.map(|k| k.outside_foto).unwrap_or(false),
        "outside_video": kunde.map(|k| k.outside_video).unwrap_or(false),
        "ist_bezahlt_handcam_foto": kunde.map(|k| k.ist_bezahlt_handcam_foto).unwrap_or(false),
        "ist_bezahlt_handcam_video": kunde.map(|k| k.ist_bezahlt_handcam_video).unwrap_or(false),
        "ist_bezahlt_outside_foto": kunde.map(|k| k.ist_bezahlt_outside_foto).unwrap_or(false),
        "ist_bezahlt_outside_video": kunde.map(|k| k.ist_bezahlt_outside_video).unwrap_or(false),
    });

    let mut root_folder = json!({ "path": format!("/{base_dir}") });
    if let Some(link) = root_share_link.filter(|s| !s.is_empty()) {
        root_folder["share_link"] = json!(link);
    }

    let created_at = chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        .replace("+00:00", "Z");

    json!({
        "meta": {
            "version": "1.1",
            "link_mode": "paths_only",
            "created_at": created_at,
            "uploader_version": uploader_version,
        },
        "customer": customer,
        "base_dir": base_dir,
        "root_folder": root_folder,
        "categories": categories,
        "totals": {
            "files_count": files_count,
            "bytes_total": bytes_total,
        },
        "client_hints": client_hints(&category_names),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_kunde() -> Kunde {
        Kunde {
            customer_number: Some("C1".into()),
            booking_number: Some("B2".into()),
            first_name: Some("Anna".into()),
            last_name: Some("Muster".into()),
            email: Some("anna@example.de".into()),
            phone: Some("0160".into()),
            customer_type: Some("Handycam".into()),
            outside_foto: true,
            ist_bezahlt_outside_foto: true,
            ..Kunde::default()
        }
    }

    #[test]
    fn normalize_customer_type_matches_legacy() {
        assert_eq!(normalize_customer_type(Some("Handycam")), "handcam");
        assert_eq!(normalize_customer_type(Some("handcam")), "handcam");
        assert_eq!(normalize_customer_type(Some("Outside")), "outside");
        assert_eq!(normalize_customer_type(None), "outside");
        assert_eq!(normalize_customer_type(Some("  ")), "outside");
        assert_eq!(normalize_customer_type(Some("Other")), "other");
    }

    #[test]
    fn build_manifest_groups_standard_categories_and_skips_unknown() {
        let files = vec![
            json!({
                "name": "b.jpg",
                "rel_path": "Outside_Foto/b.jpg",
                "size": 20,
                "mime": "image/jpeg",
                "dropbox_id": "id:b",
            }),
            json!({
                "name": "a.jpg",
                "rel_path": "Outside_Foto/a.jpg",
                "size": 10,
                "mime": "image/jpeg",
            }),
            json!({
                "name": "clip.mp4",
                "rel_path": "Outside_Video/clip.mp4",
                "size": 100,
                "type": "video/mp4",
            }),
            json!({
                "name": "skip.bin",
                "rel_path": "Other/skip.bin",
                "size": 5,
                "mime": "application/octet-stream",
            }),
            json!({
                "name": "root.jpg",
                "rel_path": "root.jpg",
                "size": 1,
                "mime": "image/jpeg",
            }),
        ];
        let manifest = build_manifest_v11(
            "Job-1",
            Some(&sample_kunde()),
            &files,
            Some("https://dropbox.com/s/x"),
            "0.1.0",
        );

        assert_eq!(manifest["meta"]["version"], "1.1");
        assert_eq!(manifest["meta"]["link_mode"], "paths_only");
        assert_eq!(manifest["meta"]["uploader_version"], "0.1.0");
        assert_eq!(manifest["base_dir"], "Job-1");
        assert_eq!(manifest["root_folder"]["path"], "/Job-1");
        assert_eq!(manifest["root_folder"]["share_link"], "https://dropbox.com/s/x");
        assert_eq!(manifest["customer"]["type"], "handcam");
        assert_eq!(manifest["customer"]["first_name"], "Anna");
        assert!(manifest["customer"]["outside_foto"].as_bool().unwrap());

        let cats = manifest["categories"].as_array().unwrap();
        assert_eq!(cats.len(), 2);
        assert_eq!(cats[0]["name"], "Outside_Foto");
        assert_eq!(cats[0]["folder_path"], "/Job-1/Outside_Foto");
        assert_eq!(cats[0]["files"][0]["rel_path"], "Outside_Foto/a.jpg");
        assert_eq!(cats[0]["files"][1]["dropbox_id"], "id:b");
        assert_eq!(cats[1]["name"], "Outside_Video");
        assert_eq!(manifest["totals"]["files_count"], 3);
        assert_eq!(manifest["totals"]["bytes_total"], 130);
        assert_eq!(manifest["client_hints"]["has_photos"], true);
        assert_eq!(manifest["client_hints"]["has_videos"], true);
        assert_eq!(manifest["client_hints"]["has_previews"], false);
    }
}
