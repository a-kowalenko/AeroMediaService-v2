//! Persistent upload checkpoints in the source folder (`_aero_upload_checkpoint.json`).
//! Port of legacy `utils/upload_checkpoint.py`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::storage::logging;

pub const CHECKPOINT_FILENAME: &str = "_aero_upload_checkpoint.json";
pub const CHECKPOINT_VERSION: u32 = 1;

pub fn checkpoint_path(local_dir: &Path) -> PathBuf {
    local_dir.join(CHECKPOINT_FILENAME)
}

/// Stable fingerprint from sorted `name|size|type` lines (SHA-256 hex).
pub fn manifest_fingerprint(files_manifest: &[Value]) -> String {
    let mut items: Vec<&Value> = files_manifest.iter().collect();
    items.sort_by(|a, b| {
        let na = a.get("name").and_then(Value::as_str).unwrap_or("");
        let nb = b.get("name").and_then(Value::as_str).unwrap_or("");
        na.cmp(nb)
    });
    let mut lines = Vec::with_capacity(items.len());
    for item in items {
        let name = item.get("name").and_then(Value::as_str).unwrap_or("");
        let size = json_size(item);
        let file_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        lines.push(format!("{name}|{size}|{file_type}"));
    }
    let raw = lines.join("\n");
    let digest = Sha256::digest(raw.as_bytes());
    hex_encode(&digest)
}

fn json_size(item: &Value) -> String {
    if let Some(n) = item.get("size") {
        if let Some(i) = n.as_i64() {
            return i.to_string();
        }
        if let Some(u) = n.as_u64() {
            return u.to_string();
        }
        if let Some(f) = n.as_f64() {
            return (f as i64).to_string();
        }
        if let Some(s) = n.as_str() {
            return s.to_string();
        }
    }
    String::new()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn load_checkpoint(local_dir: &Path) -> Option<Value> {
    let path = checkpoint_path(local_dir);
    if !path.is_file() {
        return None;
    }
    match fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(data) if data.is_object() => {
                let version = data.get("version").and_then(Value::as_u64);
                if version == Some(CHECKPOINT_VERSION as u64) {
                    Some(data)
                } else {
                    None
                }
            }
            Ok(_) => None,
            Err(e) => {
                logging::log_warn(&format!(
                    "Checkpoint lesen fehlgeschlagen ({}): {e}",
                    path.display()
                ));
                None
            }
        },
        Err(e) => {
            logging::log_warn(&format!(
                "Checkpoint lesen fehlgeschlagen ({}): {e}",
                path.display()
            ));
            None
        }
    }
}

pub fn save_checkpoint(local_dir: &Path, data: &Value) -> io::Result<()> {
    let path = checkpoint_path(local_dir);
    let mut payload = match data {
        Value::Object(map) => Value::Object(map.clone()),
        other => json!({ "payload": other }),
    };
    payload["version"] = json!(CHECKPOINT_VERSION);
    fs::create_dir_all(local_dir)?;

    let tmp_name = format!(
        ".aero_ck_{}_{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let tmp = local_dir.join(tmp_name);
    let result = (|| {
        let encoded = serde_json::to_vec_pretty(&payload)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&encoded)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        replace_file(&tmp, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn replace_file(tmp: &Path, dest: &Path) -> io::Result<()> {
    if dest.exists() {
        fs::remove_file(dest)?;
    }
    fs::rename(tmp, dest)
}

pub fn clear_checkpoint(local_dir: &Path) {
    let path = checkpoint_path(local_dir);
    if !path.is_file() {
        return;
    }
    if let Err(e) = fs::remove_file(&path) {
        logging::log_debug(&format!("Checkpoint entfernen: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn fingerprint_is_order_independent_and_includes_type() {
        let a = json!([
            {"name": "b.jpg", "size": 2, "type": "image/jpeg"},
            {"name": "a.jpg", "size": 1, "type": "image/jpeg"},
        ]);
        let b = json!([
            {"name": "a.jpg", "size": 1, "type": "image/jpeg"},
            {"name": "b.jpg", "size": 2, "type": "image/jpeg"},
        ]);
        let fp_a = manifest_fingerprint(a.as_array().unwrap());
        let fp_b = manifest_fingerprint(b.as_array().unwrap());
        assert_eq!(fp_a, fp_b);
        assert_eq!(fp_a.len(), 64);

        let without_type = json!([{"name": "a.jpg", "size": 1}]);
        let with_type = json!([{"name": "a.jpg", "size": 1, "type": "image/jpeg"}]);
        assert_ne!(
            manifest_fingerprint(without_type.as_array().unwrap()),
            manifest_fingerprint(with_type.as_array().unwrap())
        );
    }

    #[test]
    fn load_save_clear_roundtrip() {
        let dir = tempdir().unwrap();
        assert!(load_checkpoint(dir.path()).is_none());
        save_checkpoint(
            dir.path(),
            &json!({
                "kind": "dropbox_native",
                "manifest_fp": "abc",
                "next_file_index": 1,
            }),
        )
        .unwrap();
        let loaded = load_checkpoint(dir.path()).unwrap();
        assert_eq!(loaded["version"], CHECKPOINT_VERSION);
        assert_eq!(loaded["kind"], "dropbox_native");
        assert_eq!(loaded["next_file_index"], 1);
        assert!(checkpoint_path(dir.path()).is_file());

        clear_checkpoint(dir.path());
        assert!(load_checkpoint(dir.path()).is_none());
        assert!(!checkpoint_path(dir.path()).is_file());
    }

    #[test]
    fn wrong_version_is_ignored() {
        let dir = tempdir().unwrap();
        fs::write(
            checkpoint_path(dir.path()),
            r#"{"version": 99, "kind": "dropbox_native"}"#,
        )
        .unwrap();
        assert!(load_checkpoint(dir.path()).is_none());
    }

    #[test]
    fn invalid_json_is_ignored() {
        let dir = tempdir().unwrap();
        fs::write(checkpoint_path(dir.path()), "not-json").unwrap();
        assert!(load_checkpoint(dir.path()).is_none());
    }
}
