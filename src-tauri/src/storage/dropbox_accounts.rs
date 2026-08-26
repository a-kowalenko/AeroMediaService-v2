//! Dropbox account profiles (Phase 16a) — metadata in SQLite, secrets in keyring.

use std::sync::{Arc, Mutex};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

use crate::cloud::dropbox::{DropboxPool, DropboxSecretKeys};
use crate::constants::CONFIG_DB_FILE;
use crate::storage::config::{app_config_dir, ConfigError, ConfigStore};
use crate::storage::secrets;

const MIGRATE_FLAG: &str = "dropbox_multi_account_migrated";

#[derive(Debug, Error)]
pub enum DropboxAccountError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("secret error: {0}")]
    Secret(#[from] secrets::SecretError),
    #[error("{0}")]
    Message(String),
}

impl From<ConfigError> for DropboxAccountError {
    fn from(value: ConfigError) -> Self {
        DropboxAccountError::Message(value.to_string())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DropboxAccountRow {
    pub id: String,
    pub pool: String,
    pub label: String,
    pub dropbox_account_id: String,
    pub email: String,
    pub display_name: String,
    pub app_key_hint: String,
    /// Manual or auto-discovered Dropbox app folder name under `/Apps/…`.
    pub app_folder_name: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DropboxAccountMigrateReport {
    pub skipped: bool,
    pub native_created: bool,
    pub custom_created: bool,
    pub message: String,
}

pub struct DropboxAccountStore {
    db_path: std::path::PathBuf,
}

impl DropboxAccountStore {
    pub fn open_default() -> Result<Self, DropboxAccountError> {
        let path = app_config_dir()?.join(CONFIG_DB_FILE);
        Self::open_at(path)
    }

    pub fn open_at(db_path: std::path::PathBuf) -> Result<Self, DropboxAccountError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self { db_path };
        store.ensure_schema()?;
        Ok(store)
    }

    fn connect(&self) -> Result<Connection, DropboxAccountError> {
        Ok(Connection::open(&self.db_path)?)
    }

    fn ensure_schema(&self) -> Result<(), DropboxAccountError> {
        let conn = self.connect()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS dropbox_accounts (
                id TEXT PRIMARY KEY,
                pool TEXT NOT NULL,
                label TEXT NOT NULL DEFAULT '',
                dropbox_account_id TEXT NOT NULL DEFAULT '',
                email TEXT NOT NULL DEFAULT '',
                display_name TEXT NOT NULL DEFAULT '',
                app_key_hint TEXT NOT NULL DEFAULT '',
                app_folder_name TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_dropbox_accounts_pool_dbxid
                ON dropbox_accounts(pool, dropbox_account_id)
                WHERE dropbox_account_id != '';",
        )?;
        ensure_column(&conn, "dropbox_accounts", "app_folder_name", "TEXT NOT NULL DEFAULT ''")?;
        Ok(())
    }

    fn row_from_query(row: &rusqlite::Row<'_>) -> rusqlite::Result<DropboxAccountRow> {
        Ok(DropboxAccountRow {
            id: row.get(0)?,
            pool: row.get(1)?,
            label: row.get(2)?,
            dropbox_account_id: row.get(3)?,
            email: row.get(4)?,
            display_name: row.get(5)?,
            app_key_hint: row.get(6)?,
            app_folder_name: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    }

    pub fn list(&self, pool: DropboxPool) -> Result<Vec<DropboxAccountRow>, DropboxAccountError> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT id, pool, label, dropbox_account_id, email, display_name, app_key_hint,
                    app_folder_name, created_at, updated_at
             FROM dropbox_accounts
             WHERE pool = ?1
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![pool.as_str()], Self::row_from_query)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get(&self, ams_id: &str) -> Result<Option<DropboxAccountRow>, DropboxAccountError> {
        let conn = self.connect()?;
        let row = conn
            .query_row(
                "SELECT id, pool, label, dropbox_account_id, email, display_name, app_key_hint,
                        app_folder_name, created_at, updated_at
                 FROM dropbox_accounts WHERE id = ?1",
                params![ams_id.trim()],
                Self::row_from_query,
            )
            .optional()?;
        Ok(row)
    }

    pub fn create(
        &self,
        pool: DropboxPool,
        label: &str,
    ) -> Result<DropboxAccountRow, DropboxAccountError> {
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let label = label.trim();
        let label = if label.is_empty() {
            default_label(pool)
        } else {
            label.to_string()
        };
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO dropbox_accounts
                (id, pool, label, dropbox_account_id, email, display_name, app_key_hint,
                 app_folder_name, created_at, updated_at)
             VALUES (?1, ?2, ?3, '', '', '', '', '', ?4, ?4)",
            params![id, pool.as_str(), label, now],
        )?;
        self.get(&id)?
            .ok_or_else(|| DropboxAccountError::Message("Profil konnte nicht gelesen werden.".into()))
    }

    pub fn update_identity(
        &self,
        ams_id: &str,
        dropbox_account_id: &str,
        email: &str,
        display_name: &str,
        app_key_hint: &str,
    ) -> Result<DropboxAccountRow, DropboxAccountError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connect()?;
        let changed = conn.execute(
            "UPDATE dropbox_accounts
             SET dropbox_account_id = ?2,
                 email = ?3,
                 display_name = ?4,
                 app_key_hint = ?5,
                 updated_at = ?6
             WHERE id = ?1",
            params![
                ams_id.trim(),
                dropbox_account_id.trim(),
                email.trim(),
                display_name.trim(),
                app_key_hint.trim(),
                now
            ],
        )?;
        if changed == 0 {
            return Err(DropboxAccountError::Message(format!(
                "Dropbox-Profil nicht gefunden: {ams_id}"
            )));
        }
        self.get(ams_id)?
            .ok_or_else(|| DropboxAccountError::Message("Profil konnte nicht gelesen werden.".into()))
    }

    pub fn set_app_folder_name(
        &self,
        ams_id: &str,
        app_folder_name: &str,
    ) -> Result<DropboxAccountRow, DropboxAccountError> {
        let now = Utc::now().to_rfc3339();
        let name = app_folder_name.trim();
        let conn = self.connect()?;
        let changed = conn.execute(
            "UPDATE dropbox_accounts SET app_folder_name = ?2, updated_at = ?3 WHERE id = ?1",
            params![ams_id.trim(), name, now],
        )?;
        if changed == 0 {
            return Err(DropboxAccountError::Message(format!(
                "Dropbox-Profil nicht gefunden: {ams_id}"
            )));
        }
        self.get(ams_id)?
            .ok_or_else(|| DropboxAccountError::Message("Profil konnte nicht gelesen werden.".into()))
    }

    /// Fill app folder name from OAuth/API when the profile has none yet.
    pub fn maybe_set_app_folder_name_from_discovery(
        &self,
        ams_id: &str,
        discovered: &str,
    ) -> Result<(), DropboxAccountError> {
        let discovered = discovered.trim();
        if discovered.is_empty() {
            return Ok(());
        }
        let Some(row) = self.get(ams_id)? else {
            return Ok(());
        };
        if !row.app_folder_name.trim().is_empty() {
            return Ok(());
        }
        let _ = self.set_app_folder_name(ams_id, discovered)?;
        Ok(())
    }

    pub fn rename(&self, ams_id: &str, label: &str) -> Result<DropboxAccountRow, DropboxAccountError> {
        let now = Utc::now().to_rfc3339();
        let label = label.trim();
        if label.is_empty() {
            return Err(DropboxAccountError::Message("Label darf nicht leer sein.".into()));
        }
        let conn = self.connect()?;
        let changed = conn.execute(
            "UPDATE dropbox_accounts SET label = ?2, updated_at = ?3 WHERE id = ?1",
            params![ams_id.trim(), label, now],
        )?;
        if changed == 0 {
            return Err(DropboxAccountError::Message(format!(
                "Dropbox-Profil nicht gefunden: {ams_id}"
            )));
        }
        self.get(ams_id)?
            .ok_or_else(|| DropboxAccountError::Message("Profil konnte nicht gelesen werden.".into()))
    }

    pub fn delete(&self, ams_id: &str) -> Result<(), DropboxAccountError> {
        let Some(row) = self.get(ams_id)? else {
            return Ok(());
        };
        let pool = DropboxPool::parse(&row.pool)
            .map_err(|e| DropboxAccountError::Message(e.to_string()))?;
        let keys = DropboxSecretKeys::for_account(pool, &row.id);
        let _ = secrets::delete_secret(&keys.app_key);
        let _ = secrets::delete_secret(&keys.app_secret);
        let _ = secrets::delete_secret(&keys.refresh_token);
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM dropbox_accounts WHERE id = ?1",
            params![ams_id.trim()],
        )?;
        Ok(())
    }

    pub fn find_by_dropbox_account_id(
        &self,
        pool: DropboxPool,
        dropbox_account_id: &str,
    ) -> Result<Option<DropboxAccountRow>, DropboxAccountError> {
        let id = dropbox_account_id.trim();
        if id.is_empty() {
            return Ok(None);
        }
        let conn = self.connect()?;
        let row = conn
            .query_row(
                "SELECT id, pool, label, dropbox_account_id, email, display_name, app_key_hint,
                        app_folder_name, created_at, updated_at
                 FROM dropbox_accounts
                 WHERE pool = ?1 AND dropbox_account_id = ?2",
                params![pool.as_str(), id],
                Self::row_from_query,
            )
            .optional()?;
        Ok(row)
    }
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), DropboxAccountError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(());
        }
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )?;
    Ok(())
}

fn default_label(pool: DropboxPool) -> String {
    match pool {
        DropboxPool::Native => "Dropbox".into(),
        DropboxPool::CustomApi => "Skydive Media Dropbox".into(),
    }
}

fn legacy_has_any_secret(keys: &DropboxSecretKeys) -> bool {
    [&keys.app_key, &keys.app_secret, &keys.refresh_token]
        .into_iter()
        .any(|k| {
            secrets::get_secret(k)
                .ok()
                .flatten()
                .filter(|s| !s.trim().is_empty())
                .is_some()
        })
}

fn copy_secret(from: &str, to: &str) -> Result<(), DropboxAccountError> {
    if let Some(value) = secrets::get_secret(from)?.filter(|s| !s.is_empty()) {
        let existing = secrets::get_secret(to)?.filter(|s| !s.is_empty());
        if existing.is_none() {
            secrets::save_secret(to, &value)?;
        }
    }
    Ok(())
}

fn migrate_pool(
    accounts: &DropboxAccountStore,
    config: &mut ConfigStore,
    pool: DropboxPool,
) -> Result<bool, DropboxAccountError> {
    let existing = accounts.list(pool)?;
    if !existing.is_empty() {
        let active_key = pool.active_setting_key();
        if config.get(active_key, Some("")).trim().is_empty() {
            config.save(active_key, &existing[0].id)?;
        }
        return Ok(false);
    }

    let legacy = pool.legacy_keys();
    if !legacy_has_any_secret(&legacy) {
        return Ok(false);
    }

    let row = accounts.create(pool, &default_label(pool))?;
    let namespaced = DropboxSecretKeys::for_account(pool, &row.id);
    copy_secret(&legacy.app_key, &namespaced.app_key)?;
    copy_secret(&legacy.app_secret, &namespaced.app_secret)?;
    copy_secret(&legacy.refresh_token, &namespaced.refresh_token)?;

    let hint = secrets::get_secret(&namespaced.app_key)?
        .filter(|s| !s.is_empty())
        .map(|s| crate::cloud::dropbox::app_key_hint(&s))
        .unwrap_or_default();
    if !hint.is_empty() {
        let _ = accounts.update_identity(&row.id, "", "", "", &hint);
    }

    config.save(pool.active_setting_key(), &row.id)?;
    Ok(true)
}

/// One-shot: legacy `db_*` / `custom_db_*` → one profile per pool + active IDs.
pub fn ensure_migrated(
    accounts: &DropboxAccountStore,
    config: &mut ConfigStore,
) -> Result<DropboxAccountMigrateReport, DropboxAccountError> {
    if config.get(MIGRATE_FLAG, Some("false")) == "true" {
        return Ok(DropboxAccountMigrateReport {
            skipped: true,
            native_created: false,
            custom_created: false,
            message: "Dropbox-Multi-Account-Migration bereits erledigt.".into(),
        });
    }

    let native_created = migrate_pool(accounts, config, DropboxPool::Native)?;
    let custom_created = migrate_pool(accounts, config, DropboxPool::CustomApi)?;
    config.save(MIGRATE_FLAG, "true")?;

    let message = match (native_created, custom_created) {
        (false, false) => "Keine Legacy-Dropbox-Credentials — keine Profile angelegt.".into(),
        (true, false) => "Native Dropbox-Profil aus Legacy-Keys angelegt.".into(),
        (false, true) => "Skydive-Media-Dropbox-Profil aus Legacy-Keys angelegt.".into(),
        (true, true) => "Native- und Skydive-Media-Dropbox-Profile aus Legacy-Keys angelegt.".into(),
    };

    Ok(DropboxAccountMigrateReport {
        skipped: false,
        native_created,
        custom_created,
        message,
    })
}

/// Mirror legacy pool keys ↔ active profile namespaced keys (Settings still use legacy names).
pub fn sync_active_secrets_with_legacy(
    config: &ConfigStore,
    pool: DropboxPool,
) -> Result<(), DropboxAccountError> {
    let active = config.get(pool.active_setting_key(), Some(""));
    let active = active.trim();
    if active.is_empty() {
        return Ok(());
    }
    let legacy = pool.legacy_keys();
    let namespaced = DropboxSecretKeys::for_account(pool, active);
    for (legacy_key, ns_key) in [
        (legacy.app_key.as_str(), namespaced.app_key.as_str()),
        (legacy.app_secret.as_str(), namespaced.app_secret.as_str()),
        (legacy.refresh_token.as_str(), namespaced.refresh_token.as_str()),
    ] {
        let legacy_val = secrets::get_secret(legacy_key)?.filter(|s| !s.is_empty());
        let ns_val = secrets::get_secret(ns_key)?.filter(|s| !s.is_empty());
        match (legacy_val, ns_val) {
            (Some(l), Some(n)) if l != n => {
                // Prefer namespaced (OAuth/profile) as source of truth; keep legacy in sync for UI.
                secrets::save_secret(legacy_key, &n)?;
            }
            (Some(l), None) => secrets::save_secret(ns_key, &l)?,
            (None, Some(n)) => secrets::save_secret(legacy_key, &n)?,
            _ => {}
        }
    }
    Ok(())
}

#[derive(Clone)]
pub struct DropboxAccountState {
    store: Arc<Mutex<DropboxAccountStore>>,
}

impl DropboxAccountState {
    pub fn new() -> Result<Self, String> {
        let store = DropboxAccountStore::open_default().map_err(|e| e.to_string())?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
        })
    }

    #[cfg(test)]
    pub fn from_store(store: DropboxAccountStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }

    pub fn with_store<T>(
        &self,
        f: impl FnOnce(&DropboxAccountStore) -> Result<T, DropboxAccountError>,
    ) -> Result<T, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        f(&store).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::secrets::{clear_test_secrets, save_secret};
    use std::sync::Mutex;
    use tempfile::tempdir;

    static SECRET_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn open_pair() -> (tempfile::TempDir, DropboxAccountStore, ConfigStore) {
        let dir = tempdir().unwrap();
        let db = dir.path().join(CONFIG_DB_FILE);
        let accounts = DropboxAccountStore::open_at(db.clone()).unwrap();
        let config = ConfigStore::open_at(db).unwrap();
        (dir, accounts, config)
    }

    #[test]
    fn for_account_keys_isolated_between_pools() {
        let a = DropboxSecretKeys::for_account(DropboxPool::Native, "x");
        let b = DropboxSecretKeys::for_account(DropboxPool::CustomApi, "x");
        assert!(!a.app_key.starts_with("custom_"));
        assert!(b.app_key.starts_with("custom_"));
        assert_ne!(a.refresh_token, b.refresh_token);
    }

    #[test]
    fn migrate_creates_native_and_custom_profiles() {
        let _guard = SECRET_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_test_secrets();
        let (_dir, accounts, mut config) = open_pair();
        save_secret("db_app_key", "nk").unwrap();
        save_secret("db_app_secret", "ns").unwrap();
        save_secret("db_refresh_token", "nr").unwrap();
        save_secret("custom_db_app_key", "ck").unwrap();
        save_secret("custom_db_app_secret", "cs").unwrap();
        save_secret("custom_db_refresh_token", "cr").unwrap();

        let report = ensure_migrated(&accounts, &mut config).unwrap();
        assert!(!report.skipped);
        assert!(report.native_created);
        assert!(report.custom_created);

        let native = accounts.list(DropboxPool::Native).unwrap();
        let custom = accounts.list(DropboxPool::CustomApi).unwrap();
        assert_eq!(native.len(), 1);
        assert_eq!(custom.len(), 1);
        assert_eq!(
            config.get("active_dropbox_account_id", None),
            native[0].id
        );
        assert_eq!(
            config.get("active_custom_dropbox_account_id", None),
            custom[0].id
        );

        let nk = DropboxSecretKeys::for_account(DropboxPool::Native, &native[0].id);
        let ck = DropboxSecretKeys::for_account(DropboxPool::CustomApi, &custom[0].id);
        assert_eq!(secrets::get_secret(&nk.refresh_token).unwrap().as_deref(), Some("nr"));
        assert_eq!(secrets::get_secret(&ck.refresh_token).unwrap().as_deref(), Some("cr"));
        assert_ne!(nk.refresh_token, ck.refresh_token);

        let again = ensure_migrated(&accounts, &mut config).unwrap();
        assert!(again.skipped);
        assert_eq!(accounts.list(DropboxPool::Native).unwrap().len(), 1);
        clear_test_secrets();
    }

    #[test]
    fn migrate_noop_without_legacy_secrets() {
        let _guard = SECRET_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_test_secrets();
        let (_dir, accounts, mut config) = open_pair();
        let report = ensure_migrated(&accounts, &mut config).unwrap();
        assert!(!report.skipped);
        assert!(!report.native_created);
        assert!(!report.custom_created);
        assert!(accounts.list(DropboxPool::Native).unwrap().is_empty());
        assert!(accounts.list(DropboxPool::CustomApi).unwrap().is_empty());
        clear_test_secrets();
    }

    #[test]
    fn create_list_rename_delete_roundtrip() {
        let _guard = SECRET_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_test_secrets();
        let (_dir, accounts, _config) = open_pair();
        let a = accounts.create(DropboxPool::Native, "Alpha").unwrap();
        let b = accounts.create(DropboxPool::Native, "Beta").unwrap();
        assert_eq!(accounts.list(DropboxPool::Native).unwrap().len(), 2);
        assert!(accounts.list(DropboxPool::CustomApi).unwrap().is_empty());

        let renamed = accounts.rename(&a.id, "Alpha 2").unwrap();
        assert_eq!(renamed.label, "Alpha 2");

        save_secret(
            &DropboxSecretKeys::for_account(DropboxPool::Native, &b.id).refresh_token,
            "tok",
        )
        .unwrap();
        accounts.delete(&b.id).unwrap();
        assert_eq!(accounts.list(DropboxPool::Native).unwrap().len(), 1);
        assert!(secrets::get_secret(
            &DropboxSecretKeys::for_account(DropboxPool::Native, &b.id).refresh_token
        )
        .unwrap()
        .is_none());
        clear_test_secrets();
    }
}
