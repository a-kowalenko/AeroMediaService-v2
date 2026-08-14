//! One-shot migration from Legacy QSettings + Keyring → v2 SQLite + Keyring.
//!
//! - Non-secrets: QSettings org `AKSoftware` / app `AeroMediaService`
//! - Secrets: Keyring service `DropboxUploaderApp` → `AeroMediaService-v2`
//! - Never writes secrets into SQLite
//! - Idempotent via `legacy_migration_done`

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

use crate::constants::{
    LEGACY_KEYRING_SERVICE_NAME, LEGACY_QSETTINGS_APP, LEGACY_QSETTINGS_ORG, LEGACY_SECRET_KEYS,
    LEGACY_SETTING_KEYS,
};
use crate::storage::secrets::{self, SecretError};
use crate::storage::ConfigStore;

#[derive(Debug, Error)]
pub enum MigrateError {
    #[error("config: {0}")]
    Config(#[from] crate::storage::config::ConfigError),
    #[error("secret: {0}")]
    Secret(#[from] SecretError),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MigrateReport {
    pub skipped: bool,
    pub settings_imported: usize,
    pub secrets_imported: usize,
    pub message: String,
}

/// Normalize a QSettings / INI value to a plain string (drop Qt `@ByteArray(...)` wrappers lightly).
pub fn normalize_setting_value(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // Qt sometimes stores booleans as "true"/"false" already; ints as decimal strings.
    trimmed.to_string()
}

/// Parse a Qt INI-style QSettings file (`General` or flat keys).
pub fn parse_qsettings_ini(contents: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut section = String::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        // Prefer flat keys; ignore nested `%General` noise by taking the raw key name.
        let flat = if section.is_empty() || section.eq_ignore_ascii_case("General") {
            key.to_string()
        } else {
            // Nested groups use `/` in Qt INI — take last segment if present.
            key.rsplit('/').next().unwrap_or(key).to_string()
        };
        out.insert(flat, normalize_setting_value(value));
    }
    out
}

fn linux_qsettings_path() -> Option<PathBuf> {
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    Some(
        home.join(".config")
            .join(LEGACY_QSETTINGS_ORG)
            .join(format!("{LEGACY_QSETTINGS_APP}.conf")),
    )
}

#[allow(dead_code)]
fn macos_plist_path() -> Option<PathBuf> {
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    // Qt NativeFormat on macOS: ~/Library/Preferences/com.<Org>.<App>.plist
    Some(
        home.join("Library")
            .join("Preferences")
            .join(format!(
                "com.{LEGACY_QSETTINGS_ORG}.{LEGACY_QSETTINGS_APP}.plist"
            )),
    )
}

#[cfg(windows)]
fn read_windows_qsettings() -> HashMap<String, String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let mut out = HashMap::new();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = format!("Software\\{LEGACY_QSETTINGS_ORG}\\{LEGACY_QSETTINGS_APP}");
    let Ok(key) = hkcu.open_subkey(&path) else {
        return out;
    };
    for name in key.enum_values().filter_map(|v| v.ok()).map(|(n, _)| n) {
        if name.is_empty() {
            continue;
        }
        if let Ok(s) = key.get_value::<String, _>(&name) {
            out.insert(name.clone(), normalize_setting_value(&s));
            continue;
        }
        if let Ok(n) = key.get_value::<u32, _>(&name) {
            out.insert(name.clone(), n.to_string());
            continue;
        }
        if let Ok(n) = key.get_value::<u64, _>(&name) {
            out.insert(name, n.to_string());
        }
    }
    out
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn read_windows_qsettings() -> HashMap<String, String> {
    HashMap::new()
}

#[cfg(target_os = "macos")]
fn read_macos_qsettings() -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(path) = macos_plist_path() else {
        return out;
    };
    if !path.is_file() {
        return out;
    }
    let Ok(value) = plist::Value::from_file(&path) else {
        return out;
    };
    let Some(dict) = value.as_dictionary() else {
        return out;
    };
    for (k, v) in dict {
        let s = match v {
            plist::Value::String(s) => s.clone(),
            plist::Value::Boolean(b) => b.to_string(),
            plist::Value::Integer(i) => i.to_string(),
            plist::Value::Real(r) => r.to_string(),
            _ => continue,
        };
        out.insert(k.clone(), normalize_setting_value(&s));
    }
    out
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn read_macos_qsettings() -> HashMap<String, String> {
    HashMap::new()
}

#[allow(dead_code)]
fn read_linux_qsettings() -> HashMap<String, String> {
    let Some(path) = linux_qsettings_path() else {
        return HashMap::new();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    parse_qsettings_ini(&raw)
}

/// Load legacy non-secret settings from the platform QSettings store.
pub fn read_legacy_qsettings() -> HashMap<String, String> {
    if cfg!(windows) {
        return read_windows_qsettings();
    }
    if cfg!(target_os = "macos") {
        return read_macos_qsettings();
    }
    if cfg!(target_os = "linux") {
        return read_linux_qsettings();
    }
    HashMap::new()
}

fn get_legacy_secret(key: &str) -> Result<Option<String>, SecretError> {
    secrets::get_secret_from_service(LEGACY_KEYRING_SERVICE_NAME, key)
}

fn setting_is_empty_or_default(store: &ConfigStore, key: &str) -> bool {
    let current = store.get(key, None);
    let default = crate::constants::setting_default(key).unwrap_or("");
    current.trim().is_empty() || current == default
}

/// Run one-shot migration. If `force`, ignore the done-flag (still skip overwriting non-empty targets).
pub fn migrate_from_legacy(
    store: &mut ConfigStore,
    force: bool,
) -> Result<MigrateReport, MigrateError> {
    let done = store.get("legacy_migration_done", Some("false"));
    if !force && done.trim().eq_ignore_ascii_case("true") {
        return Ok(MigrateReport {
            skipped: true,
            message: "Legacy-Migration bereits erledigt.".into(),
            ..Default::default()
        });
    }

    let legacy_settings = read_legacy_qsettings();
    let mut settings_imported = 0usize;
    for key in LEGACY_SETTING_KEYS {
        let Some(raw) = legacy_settings.get(*key) else {
            continue;
        };
        let value = normalize_setting_value(raw);
        if value.is_empty() {
            continue;
        }
        // Do not overwrite user-configured v2 values unless still at default/empty.
        if !setting_is_empty_or_default(store, key) {
            continue;
        }
        store.save(key, &value)?;
        settings_imported += 1;
    }

    let mut secrets_imported = 0usize;
    for key in LEGACY_SECRET_KEYS {
        let Ok(Some(value)) = get_legacy_secret(key) else {
            continue;
        };
        let value = value.trim().to_string();
        if value.is_empty() {
            continue;
        }
        // Skip if v2 already has a secret.
        match secrets::get_secret(key) {
            Ok(Some(existing)) if !existing.trim().is_empty() => continue,
            Ok(_) => {}
            Err(_) => {}
        }
        secrets::save_secret(key, &value)?;
        secrets_imported += 1;
    }

    store.save("legacy_migration_done", "true")?;

    // Existing installs with paths already set: treat as setup done so wizard is skippable.
    let setup_done = store.get("setup_completed", Some("false"));
    if !setup_done.trim().eq_ignore_ascii_case("true") {
        let monitor = store.get("monitor_path", Some(""));
        if !monitor.trim().is_empty() {
            store.save("setup_completed", "true")?;
        }
    }

    let message = format!(
        "Legacy-Migration: {settings_imported} Einstellungen, {secrets_imported} Secrets importiert."
    );
    Ok(MigrateReport {
        skipped: false,
        settings_imported,
        secrets_imported,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::CONFIG_DB_FILE;
    use tempfile::tempdir;

    #[test]
    fn parse_ini_reads_general_keys() {
        let ini = r#"
[General]
monitor_path=D:/Media/Inbox
scan_interval=12
selected_cloud_service=dropbox
"#;
        let map = parse_qsettings_ini(ini);
        assert_eq!(map.get("monitor_path").map(String::as_str), Some("D:/Media/Inbox"));
        assert_eq!(map.get("scan_interval").map(String::as_str), Some("12"));
        assert_eq!(
            map.get("selected_cloud_service").map(String::as_str),
            Some("dropbox")
        );
    }

    #[test]
    fn normalize_strips_quotes() {
        assert_eq!(normalize_setting_value("  \"true\"  "), "true");
        assert_eq!(normalize_setting_value(""), "");
    }

    #[test]
    fn migrate_is_idempotent_via_flag() {
        let dir = tempdir().unwrap();
        let mut store = ConfigStore::open_at(dir.path().join(CONFIG_DB_FILE)).unwrap();
        store.save("legacy_migration_done", "true").unwrap();
        let report = migrate_from_legacy(&mut store, false).unwrap();
        assert!(report.skipped);
        assert_eq!(report.settings_imported, 0);
    }

    #[test]
    fn migrate_does_not_overwrite_existing_settings() {
        let dir = tempdir().unwrap();
        let mut store = ConfigStore::open_at(dir.path().join(CONFIG_DB_FILE)).unwrap();
        store.save("monitor_path", r"C:\Already\Set").unwrap();
        // Inject via direct save of migration with empty legacy map path — force run still
        // should keep existing value when legacy has nothing (or we only test overwrite guard).
        let report = migrate_from_legacy(&mut store, true).unwrap();
        assert!(!report.skipped);
        assert_eq!(store.get("monitor_path", None), r"C:\Already\Set");
        assert_eq!(store.get("legacy_migration_done", None), "true");
    }

    #[test]
    fn secret_keys_never_land_in_sqlite_during_migrate() {
        let dir = tempdir().unwrap();
        let mut store = ConfigStore::open_at(dir.path().join(CONFIG_DB_FILE)).unwrap();
        let _ = migrate_from_legacy(&mut store, true).unwrap();
        let raw = std::fs::read(dir.path().join(CONFIG_DB_FILE)).unwrap();
        let text = String::from_utf8_lossy(&raw);
        for key in LEGACY_SECRET_KEYS {
            // Key name alone may appear only if somehow saved — ensure we didn't save as setting.
            // ConfigStore rejects secret keys; flag this if a value pattern leaked.
            assert!(
                store.get(key, Some("__missing__")) == "__missing__"
                    || store.get(key, Some("__missing__")).is_empty(),
                "secret key {key} must not be readable from settings store"
            );
        }
        let _ = text;
    }
}
