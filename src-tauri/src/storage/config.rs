//! Persistent non-secret settings (port of legacy `core/config.py` QSettings half).
//!
//! Stored as a key/value SQLite table under the platform app-data directory.
//! Secret keys are rejected here — they belong in the OS keyring.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use directories::BaseDirs;
use once_cell::sync::OnceCell;
use rusqlite::{params, Connection};
use thiserror::Error;

use crate::constants::{setting_default, APP_DIR_NAME, CONFIG_DB_FILE};
use crate::storage::secrets::is_secret_key;

type SettingGetter = Arc<dyn Fn(&str) -> String + Send + Sync>;

static RUNTIME_GETTER: OnceCell<SettingGetter> = OnceCell::new();

/// Install the live settings reader used by cloud clients (shortener, upload mode).
pub fn install_runtime_getter(getter: SettingGetter) {
    let _ = RUNTIME_GETTER.set(getter);
}

/// Non-secret setting from the running ConfigStore, or the known default.
pub fn runtime_setting(key: &str) -> String {
    if let Some(getter) = RUNTIME_GETTER.get() {
        return getter(key);
    }
    setting_default(key).unwrap_or("").to_string()
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("secret key '{0}' must be stored in the OS keyring, not in settings")]
    SecretKey(String),
    #[error("empty setting key")]
    EmptyKey,
    #[error("{0}")]
    Message(String),
}

/// Resolve the platform app-data directory for Aero Media Service.
pub fn app_config_dir() -> Result<PathBuf, ConfigError> {
    let base = BaseDirs::new().ok_or_else(|| {
        ConfigError::Message("could not resolve user application data directory".into())
    })?;
    Ok(base.data_local_dir().join(APP_DIR_NAME))
}

#[allow(dead_code)]
pub fn config_db_path() -> Result<PathBuf, ConfigError> {
    Ok(app_config_dir()?.join(CONFIG_DB_FILE))
}

pub struct ConfigStore {
    db_path: PathBuf,
    cache: HashMap<String, String>,
}

impl ConfigStore {
    pub fn open_default() -> Result<Self, ConfigError> {
        let dir = app_config_dir()?;
        fs::create_dir_all(&dir)?;
        Self::open_at(dir.join(CONFIG_DB_FILE))
    }

    pub fn open_at(db_path: PathBuf) -> Result<Self, ConfigError> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut store = Self {
            db_path,
            cache: HashMap::new(),
        };
        store.reload()?;
        Ok(store)
    }

    fn connect(&self) -> Result<Connection, ConfigError> {
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;
        Ok(conn)
    }

    fn reload(&mut self) -> Result<(), ConfigError> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        self.cache.clear();
        for row in rows {
            let (key, value) = row?;
            self.cache.insert(key, value);
        }
        Ok(())
    }

    pub fn get(&self, key: &str, default: Option<&str>) -> String {
        if let Some(value) = self.cache.get(key) {
            return value.clone();
        }
        default
            .map(str::to_string)
            .or_else(|| setting_default(key).map(str::to_string))
            .unwrap_or_default()
    }

    pub fn save(&mut self, key: &str, value: &str) -> Result<(), ConfigError> {
        let key = key.trim();
        if key.is_empty() {
            return Err(ConfigError::EmptyKey);
        }
        if is_secret_key(key) {
            return Err(ConfigError::SecretKey(key.to_string()));
        }

        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        self.cache.insert(key.to_string(), value.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_survives_reopen() {
        let dir = tempdir().unwrap();
        let db = dir.path().join(CONFIG_DB_FILE);
        {
            let mut store = ConfigStore::open_at(db.clone()).unwrap();
            store.save("monitor_path", r"D:\Media\Inbox").unwrap();
            store.save("scan_interval", "15").unwrap();
        }
        let store = ConfigStore::open_at(db).unwrap();
        assert_eq!(store.get("monitor_path", None), r"D:\Media\Inbox");
        assert_eq!(store.get("scan_interval", None), "15");
    }

    #[test]
    fn missing_key_uses_explicit_then_known_default() {
        let dir = tempdir().unwrap();
        let store = ConfigStore::open_at(dir.path().join(CONFIG_DB_FILE)).unwrap();
        assert_eq!(store.get("scan_interval", None), "10");
        assert_eq!(store.get("scan_interval", Some("99")), "99");
        assert_eq!(store.get("unknown", Some("fallback")), "fallback");
        assert_eq!(store.get("unknown", None), "");
    }

    #[test]
    fn rejects_secret_keys() {
        let dir = tempdir().unwrap();
        let mut store = ConfigStore::open_at(dir.path().join(CONFIG_DB_FILE)).unwrap();
        let err = store
            .save("db_app_secret", "should-not-persist")
            .unwrap_err();
        assert!(matches!(err, ConfigError::SecretKey(_)));
        let raw = std::fs::read(dir.path().join(CONFIG_DB_FILE)).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("should-not-persist"));
    }

    #[test]
    fn rejects_empty_key() {
        let dir = tempdir().unwrap();
        let mut store = ConfigStore::open_at(dir.path().join(CONFIG_DB_FILE)).unwrap();
        assert!(matches!(
            store.save("  ", "x").unwrap_err(),
            ConfigError::EmptyKey
        ));
    }

    #[test]
    fn app_config_dir_ends_with_product_name() {
        let dir = app_config_dir().unwrap();
        assert_eq!(dir.file_name().unwrap(), APP_DIR_NAME);
    }

    #[test]
    fn platform_app_data_parent_matches_os_conventions() {
        let dir = app_config_dir().unwrap();
        let parent = dir
            .parent()
            .expect("app data dir has a parent")
            .to_string_lossy()
            .to_string();
        #[cfg(windows)]
        {
            let lower = parent.to_lowercase();
            assert!(
                lower.contains("appdata") || lower.contains("local"),
                "Windows app data parent should be under LocalAppData, got {parent}"
            );
        }
        #[cfg(target_os = "macos")]
        {
            assert!(
                parent.contains("Application Support"),
                "macOS app data parent should be Application Support, got {parent}"
            );
        }
        #[cfg(target_os = "linux")]
        {
            assert!(
                parent.contains(".local/share") || parent.contains("share"),
                "Linux app data parent should follow XDG local share, got {parent}"
            );
        }
    }
}
