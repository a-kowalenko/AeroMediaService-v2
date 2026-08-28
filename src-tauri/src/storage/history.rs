//! Persistent upload history (SQLite). Port of legacy `utils/history_manager.py`.
//!
//! Matching is by `dir_name`. JSON is not the primary store; an optional one-shot
//! import reads legacy `upload_history.json` if present.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::Local;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use uuid::Uuid;

use crate::constants::{HISTORY_DB_FILE, LEGACY_HISTORY_JSON};
use crate::model::history_status::{build_combined_error_text, build_overall_status};
use crate::storage::app_config_dir;
use crate::storage::config::ConfigError;
use crate::upload::append::is_append_dir_name;

const KNOWN_KEYS: &[&str] = &[
    "id",
    "dir_name",
    "status",
    "email_status",
    "sms_status",
    "error_msg",
    "first_name",
    "last_name",
    "email",
    "phone",
    "customer_number",
    "booking_number",
    "type",
    "customer_type",
    "marker_raw",
    "remote_path",
    "share_link",
    "sms_id",
    "archived_path",
    "archive_subfolder",
    "last_sms_resent_at",
    "sms_status_locked",
    "created_at",
    "last_updated",
    "overall_status",
    "combined_error",
    "display_name",
];

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Message(String),
}

impl From<ConfigError> for HistoryError {
    fn from(value: ConfigError) -> Self {
        HistoryError::Message(value.to_string())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub dir_name: String,
    pub status: String,
    pub email_status: String,
    pub sms_status: String,
    pub error_msg: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone: String,
    pub customer_number: String,
    pub booking_number: String,
    #[serde(rename = "type")]
    pub customer_type: String,
    pub marker_raw: String,
    pub remote_path: String,
    pub share_link: String,
    pub sms_id: String,
    pub archived_path: String,
    pub archive_subfolder: String,
    pub last_sms_resent_at: String,
    pub sms_status_locked: bool,
    pub created_at: String,
    pub last_updated: String,
    pub overall_status: String,
    pub combined_error: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl HistoryEntry {
    fn to_status_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("status".into(), Value::String(self.status.clone()));
        map.insert(
            "email_status".into(),
            Value::String(self.email_status.clone()),
        );
        map.insert("sms_status".into(), Value::String(self.sms_status.clone()));
        map.insert("error_msg".into(), Value::String(self.error_msg.clone()));
        map.insert("email".into(), Value::String(self.email.clone()));
        map.insert("phone".into(), Value::String(self.phone.clone()));
        map.insert(
            "last_sms_resent_at".into(),
            Value::String(self.last_sms_resent_at.clone()),
        );
        map.insert(
            "last_updated".into(),
            Value::String(self.last_updated.clone()),
        );
        map.insert("created_at".into(), Value::String(self.created_at.clone()));
        map.insert(
            "sms_status_locked".into(),
            Value::Bool(self.sms_status_locked),
        );
        for (key, value) in &self.extra {
            map.insert(key.clone(), value.clone());
        }
        Value::Object(map)
    }

    fn refresh_computed(&mut self) {
        let json = self.to_status_json();
        self.overall_status = build_overall_status(&json);
        self.combined_error = build_combined_error_text(&json);
        self.display_name = self.compute_display_name();
    }

    /// Flattens columns + `extra` into a legacy-style JSON object.
    pub fn to_json(&self) -> Value {
        let mut map = self.extra.clone();
        let mut put = |key: &str, value: String| {
            map.insert(key.to_string(), Value::String(value));
        };
        put("id", self.id.clone());
        put("dir_name", self.dir_name.clone());
        put("status", self.status.clone());
        put("email_status", self.email_status.clone());
        put("sms_status", self.sms_status.clone());
        put("error_msg", self.error_msg.clone());
        put("first_name", self.first_name.clone());
        put("last_name", self.last_name.clone());
        put("email", self.email.clone());
        put("phone", self.phone.clone());
        put("customer_number", self.customer_number.clone());
        put("booking_number", self.booking_number.clone());
        put("type", self.customer_type.clone());
        put("marker_raw", self.marker_raw.clone());
        put("remote_path", self.remote_path.clone());
        put("share_link", self.share_link.clone());
        put("sms_id", self.sms_id.clone());
        put("archived_path", self.archived_path.clone());
        put("archive_subfolder", self.archive_subfolder.clone());
        put("last_sms_resent_at", self.last_sms_resent_at.clone());
        put("created_at", self.created_at.clone());
        put("last_updated", self.last_updated.clone());
        put("overall_status", self.overall_status.clone());
        put("combined_error", self.combined_error.clone());
        put("display_name", self.display_name.clone());
        map.insert(
            "sms_status_locked".into(),
            Value::Bool(self.sms_status_locked),
        );
        Value::Object(map)
    }

    fn compute_display_name(&self) -> String {
        let name = format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string();
        if name.is_empty() {
            if self.dir_name.is_empty() {
                "Unbekannt".into()
            } else {
                self.dir_name.clone()
            }
        } else {
            name
        }
    }

    fn search_blob(&self) -> String {
        let extra = serde_json::to_string(&self.extra).unwrap_or_default();
        [
            self.id.as_str(),
            self.dir_name.as_str(),
            self.status.as_str(),
            self.email_status.as_str(),
            self.sms_status.as_str(),
            self.error_msg.as_str(),
            self.first_name.as_str(),
            self.last_name.as_str(),
            self.email.as_str(),
            self.phone.as_str(),
            self.customer_number.as_str(),
            self.booking_number.as_str(),
            self.customer_type.as_str(),
            self.marker_raw.as_str(),
            self.remote_path.as_str(),
            self.share_link.as_str(),
            self.sms_id.as_str(),
            self.archived_path.as_str(),
            self.archive_subfolder.as_str(),
            self.last_sms_resent_at.as_str(),
            self.created_at.as_str(),
            self.last_updated.as_str(),
            self.overall_status.as_str(),
            extra.as_str(),
        ]
        .join("\n")
        .to_lowercase()
    }
}

fn is_hidden_append_shadow(entry: &HistoryEntry) -> bool {
    is_append_dir_name(&entry.dir_name)
        && entry.status.trim() == "Gestartet"
        && entry.remote_path.trim().is_empty()
        && entry.share_link.trim().is_empty()
        && entry.archived_path.trim().is_empty()
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryPage {
    pub items: Vec<HistoryEntry>,
    pub total: usize,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Clone)]
pub struct HistoryState {
    store: Arc<Mutex<HistoryStore>>,
}

impl HistoryState {
    pub fn new() -> Result<Self, String> {
        let store = HistoryStore::open_default().map_err(|e| e.to_string())?;
        Ok(Self::from_store(store))
    }

    pub fn from_store(store: HistoryStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }

    pub fn import_legacy_json_if_needed(&self) -> Result<usize, String> {
        let mut store = self.store.lock().map_err(|e| e.to_string())?;
        store
            .import_legacy_json_if_needed()
            .map_err(|e| e.to_string())
    }

    pub fn add_or_update_from_value(
        &self,
        payload: &Value,
    ) -> Result<Option<HistoryEntry>, String> {
        let mut store = self.store.lock().map_err(|e| e.to_string())?;
        store.add_or_update(payload).map_err(|e| e.to_string())
    }

    pub fn get_filtered(
        &self,
        search: &str,
        page: u32,
        page_size: u32,
    ) -> Result<HistoryPage, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store
            .get_filtered_page(search, page, page_size)
            .map_err(|e| e.to_string())
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<HistoryEntry>, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store.get_by_id(id).map_err(|e| e.to_string())
    }

    #[allow(dead_code)]
    pub fn find_by_correlation_id(&self, cid: &str) -> Result<Option<HistoryEntry>, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store
            .find_by_correlation_id(cid)
            .map_err(|e| e.to_string())
    }

    pub fn find_by_dir_name(&self, dir_name: &str) -> Result<Option<HistoryEntry>, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store.find_by_dir_name(dir_name).map_err(|e| e.to_string())
    }

    pub fn all_entries(&self) -> Result<Vec<HistoryEntry>, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store.get_filtered_history("").map_err(|e| e.to_string())
    }

    pub fn delete_items(&self, ids: &[String]) -> Result<usize, String> {
        let mut store = self.store.lock().map_err(|e| e.to_string())?;
        store.delete_items(ids).map_err(|e| e.to_string())
    }

    pub fn clear_all(&self) -> Result<(), String> {
        let mut store = self.store.lock().map_err(|e| e.to_string())?;
        store.clear_all().map_err(|e| e.to_string())
    }
}

pub struct HistoryStore {
    db_path: PathBuf,
}

impl HistoryStore {
    pub fn open_default() -> Result<Self, HistoryError> {
        let dir = app_config_dir()?;
        fs::create_dir_all(&dir)?;
        Self::open_at(dir.join(HISTORY_DB_FILE))
    }

    pub fn open_at(db_path: PathBuf) -> Result<Self, HistoryError> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let store = Self { db_path };
        store.connect()?;
        Ok(store)
    }

    fn connect(&self) -> Result<Connection, HistoryError> {
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS history (
                id TEXT PRIMARY KEY,
                dir_name TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL DEFAULT '',
                email_status TEXT NOT NULL DEFAULT '',
                sms_status TEXT NOT NULL DEFAULT '',
                error_msg TEXT NOT NULL DEFAULT '',
                first_name TEXT NOT NULL DEFAULT '',
                last_name TEXT NOT NULL DEFAULT '',
                email TEXT NOT NULL DEFAULT '',
                phone TEXT NOT NULL DEFAULT '',
                customer_number TEXT NOT NULL DEFAULT '',
                booking_number TEXT NOT NULL DEFAULT '',
                customer_type TEXT NOT NULL DEFAULT '',
                marker_raw TEXT NOT NULL DEFAULT '',
                remote_path TEXT NOT NULL DEFAULT '',
                share_link TEXT NOT NULL DEFAULT '',
                sms_id TEXT NOT NULL DEFAULT '',
                archived_path TEXT NOT NULL DEFAULT '',
                archive_subfolder TEXT NOT NULL DEFAULT '',
                last_sms_resent_at TEXT NOT NULL DEFAULT '',
                sms_status_locked INTEGER NOT NULL DEFAULT 0,
                extra_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                last_updated TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_history_last_updated
                ON history(last_updated DESC);
             CREATE INDEX IF NOT EXISTS idx_history_created_at
                ON history(created_at DESC);
             CREATE TABLE IF NOT EXISTS history_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );",
        )?;
        Ok(conn)
    }

    fn meta_get(&self, conn: &Connection, key: &str) -> Result<Option<String>, HistoryError> {
        Ok(conn
            .query_row(
                "SELECT value FROM history_meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn meta_set(&self, conn: &Connection, key: &str, value: &str) -> Result<(), HistoryError> {
        conn.execute(
            "INSERT INTO history_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Optional one-shot import of legacy `upload_history.json`.
    pub fn import_legacy_json_if_needed(&mut self) -> Result<usize, HistoryError> {
        let conn = self.connect()?;
        if self.meta_get(&conn, "legacy_json_imported")?.as_deref() == Some("1") {
            return Ok(0);
        }
        drop(conn);

        let Some(path) = find_legacy_json() else {
            return Ok(0);
        };
        let count = self.import_from_json_file(&path)?;
        let conn = self.connect()?;
        self.meta_set(&conn, "legacy_json_imported", "1")?;
        let imported_name = format!("{LEGACY_HISTORY_JSON}.imported");
        if let Some(parent) = path.parent() {
            let _ = fs::rename(&path, parent.join(imported_name));
        }
        Ok(count)
    }

    pub fn import_from_json_file(&mut self, path: &Path) -> Result<usize, HistoryError> {
        let raw = fs::read_to_string(path)?;
        let value: Value = serde_json::from_str(&raw)?;
        let items = match value {
            Value::Array(items) => items,
            _ => return Ok(0),
        };
        let mut count = 0;
        for item in items {
            if self.add_or_update(&item)?.is_some() {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Analogous to legacy `HistoryManager.add_or_update`. Returns `None` without `dir_name`.
    pub fn add_or_update(&mut self, data: &Value) -> Result<Option<HistoryEntry>, HistoryError> {
        let dir_name = json_str_if_present(data, "dir_name")
            .unwrap_or_default()
            .trim()
            .to_string();
        if dir_name.is_empty() {
            return Ok(None);
        }

        let conn = self.connect()?;
        let existing = conn
            .query_row(
                "SELECT * FROM history WHERE dir_name = ?1",
                params![dir_name],
                row_to_entry,
            )
            .optional()?;

        let now = history_timestamp_now();
        let mut entry = existing.unwrap_or_else(|| HistoryEntry {
            id: json_str_if_present(data, "id").unwrap_or_else(|| Uuid::new_v4().to_string()),
            dir_name: dir_name.clone(),
            status: json_str_if_present(data, "status").unwrap_or_else(|| "Gestartet".into()),
            created_at: json_str_if_present(data, "created_at").unwrap_or_else(|| now.clone()),
            last_updated: json_str_if_present(data, "last_updated").unwrap_or_else(|| now.clone()),
            ..HistoryEntry::default()
        });

        merge_patch(&mut entry, data);
        // `last_updated` only changes when the patch sets it explicitly (or on insert above).
        // Metadata patches (booking flags, contact, archive path, SMS DLR) must not touch it.
        entry.dir_name = dir_name;
        entry.refresh_computed();
        upsert_entry(&conn, &entry)?;
        Ok(Some(entry))
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<HistoryEntry>, HistoryError> {
        let conn = self.connect()?;
        let mut entry = conn
            .query_row(
                "SELECT * FROM history WHERE id = ?1",
                params![id],
                row_to_entry,
            )
            .optional()?;
        if let Some(entry) = entry.as_mut() {
            entry.refresh_computed();
        }
        Ok(entry)
    }

    pub fn find_by_correlation_id(
        &self,
        correlation_id: &str,
    ) -> Result<Option<HistoryEntry>, HistoryError> {
        let cid = correlation_id.trim();
        if cid.is_empty() {
            return Ok(None);
        }
        for mut entry in self.get_filtered_history("")? {
            let stored = entry
                .extra
                .get("correlation_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if stored == cid {
                entry.refresh_computed();
                return Ok(Some(entry));
            }
        }
        Ok(None)
    }

    pub fn find_by_dir_name(&self, dir_name: &str) -> Result<Option<HistoryEntry>, HistoryError> {
        let name = dir_name.trim();
        if name.is_empty() {
            return Ok(None);
        }
        let conn = self.connect()?;
        let mut entry = conn
            .query_row(
                "SELECT * FROM history WHERE dir_name = ?1",
                params![name],
                row_to_entry,
            )
            .optional()?;
        if let Some(entry) = entry.as_mut() {
            entry.refresh_computed();
        }
        Ok(entry)
    }

    pub fn get_filtered_page(
        &self,
        search: &str,
        page: u32,
        page_size: u32,
    ) -> Result<HistoryPage, HistoryError> {
        let page_size = page_size.clamp(1, 200);
        let filtered = self.get_filtered_history(search)?;
        let total = filtered.len();
        let max_page = if total == 0 {
            0
        } else {
            ((total - 1) / page_size as usize) as u32
        };
        let page = page.min(max_page);
        let start = (page as usize) * (page_size as usize);
        let items = filtered
            .into_iter()
            .skip(start)
            .take(page_size as usize)
            .collect();
        Ok(HistoryPage {
            items,
            total,
            page,
            page_size,
        })
    }

    pub fn get_filtered_history(
        &self,
        search_text: &str,
    ) -> Result<Vec<HistoryEntry>, HistoryError> {
        let conn = self.connect()?;
        let mut stmt =
            conn.prepare("SELECT * FROM history ORDER BY created_at DESC, last_updated DESC")?;
        let rows = stmt.query_map([], row_to_entry)?;
        let needle = search_text.trim().to_lowercase();
        let mut items = Vec::new();
        for row in rows {
            let mut entry = row?;
            entry.refresh_computed();
            if is_hidden_append_shadow(&entry) {
                continue;
            }
            if needle.is_empty() || entry.search_blob().contains(&needle) {
                items.push(entry);
            }
        }
        Ok(items)
    }

    pub fn delete_items(&mut self, ids: &[String]) -> Result<usize, HistoryError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let conn = self.connect()?;
        let mut deleted = 0usize;
        for id in ids {
            deleted += conn.execute("DELETE FROM history WHERE id = ?1", params![id])?;
        }
        Ok(deleted)
    }

    pub fn clear_all(&mut self) -> Result<(), HistoryError> {
        let conn = self.connect()?;
        conn.execute("DELETE FROM history", [])?;
        Ok(())
    }
}

fn find_legacy_json() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(dir) = app_config_dir() {
        candidates.push(dir.join(LEGACY_HISTORY_JSON));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(LEGACY_HISTORY_JSON));
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn json_str_if_present(data: &Value, key: &str) -> Option<String> {
    let value = data.get(key)?;
    Some(match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    })
}

fn json_bool_if_present(data: &Value, key: &str) -> Option<bool> {
    let value = data.get(key)?;
    match value {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => Some(n.as_i64().unwrap_or(0) != 0),
        Value::String(s) => {
            let lower = s.trim().to_lowercase();
            Some(matches!(lower.as_str(), "1" | "true" | "yes"))
        }
        _ => None,
    }
}

/// Local timestamp for history `created_at` / `last_updated`.
pub fn history_timestamp_now() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S%.f").to_string()
}

/// Stamp `last_updated` on an activity payload so `add_or_update` refreshes it.
pub fn touch_last_updated(payload: &mut Value) {
    if let Some(obj) = payload.as_object_mut() {
        obj.insert(
            "last_updated".into(),
            Value::String(history_timestamp_now()),
        );
    }
}

fn merge_patch(entry: &mut HistoryEntry, data: &Value) {
    if let Some(v) = json_str_if_present(data, "status") {
        entry.status = v;
    }
    if let Some(v) = json_str_if_present(data, "email_status") {
        entry.email_status = v;
    }
    if let Some(v) = json_str_if_present(data, "sms_status") {
        entry.sms_status = v;
    }
    if let Some(v) = json_str_if_present(data, "error_msg") {
        entry.error_msg = v;
    }
    if let Some(v) = json_str_if_present(data, "first_name") {
        entry.first_name = v;
    }
    if let Some(v) = json_str_if_present(data, "last_name") {
        entry.last_name = v;
    }
    if let Some(v) = json_str_if_present(data, "email") {
        entry.email = v;
    }
    if let Some(v) = json_str_if_present(data, "phone") {
        entry.phone = v;
    }
    if let Some(v) = json_str_if_present(data, "customer_number") {
        entry.customer_number = v;
    }
    if let Some(v) = json_str_if_present(data, "booking_number") {
        entry.booking_number = v;
    }
    if let Some(v) =
        json_str_if_present(data, "type").or_else(|| json_str_if_present(data, "customer_type"))
    {
        entry.customer_type = v;
    }
    if let Some(v) = json_str_if_present(data, "marker_raw") {
        entry.marker_raw = v;
    }
    if let Some(v) = json_str_if_present(data, "remote_path") {
        entry.remote_path = v;
    }
    if let Some(v) = json_str_if_present(data, "share_link") {
        entry.share_link = v;
    }
    if let Some(v) = json_str_if_present(data, "sms_id") {
        entry.sms_id = v;
    }
    if let Some(v) = json_str_if_present(data, "archived_path") {
        entry.archived_path = v;
    }
    if let Some(v) = json_str_if_present(data, "archive_subfolder") {
        entry.archive_subfolder = v;
    }
    if let Some(v) = json_str_if_present(data, "last_sms_resent_at") {
        entry.last_sms_resent_at = v;
    }
    if let Some(v) = json_bool_if_present(data, "sms_status_locked") {
        entry.sms_status_locked = v;
    }
    if let Some(v) = json_str_if_present(data, "created_at") {
        if entry.created_at.is_empty() {
            entry.created_at = v;
        }
    }
    if let Some(v) = json_str_if_present(data, "last_updated") {
        entry.last_updated = v;
    }
    if let Some(obj) = data.as_object() {
        for (key, value) in obj {
            if KNOWN_KEYS.contains(&key.as_str()) {
                continue;
            }
            entry.extra.insert(key.clone(), value.clone());
        }
    }
}

fn upsert_entry(conn: &Connection, entry: &HistoryEntry) -> Result<(), HistoryError> {
    let extra = serde_json::to_string(&entry.extra)?;
    conn.execute(
        "INSERT INTO history (
            id, dir_name, status, email_status, sms_status, error_msg,
            first_name, last_name, email, phone, customer_number, booking_number,
            customer_type, marker_raw, remote_path, share_link, sms_id,
            archived_path, archive_subfolder, last_sms_resent_at, sms_status_locked,
            extra_json, created_at, last_updated
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
            ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24
         )
         ON CONFLICT(dir_name) DO UPDATE SET
            status = excluded.status,
            email_status = excluded.email_status,
            sms_status = excluded.sms_status,
            error_msg = excluded.error_msg,
            first_name = excluded.first_name,
            last_name = excluded.last_name,
            email = excluded.email,
            phone = excluded.phone,
            customer_number = excluded.customer_number,
            booking_number = excluded.booking_number,
            customer_type = excluded.customer_type,
            marker_raw = excluded.marker_raw,
            remote_path = excluded.remote_path,
            share_link = excluded.share_link,
            sms_id = excluded.sms_id,
            archived_path = excluded.archived_path,
            archive_subfolder = excluded.archive_subfolder,
            last_sms_resent_at = excluded.last_sms_resent_at,
            sms_status_locked = excluded.sms_status_locked,
            extra_json = excluded.extra_json,
            last_updated = excluded.last_updated",
        params![
            entry.id,
            entry.dir_name,
            entry.status,
            entry.email_status,
            entry.sms_status,
            entry.error_msg,
            entry.first_name,
            entry.last_name,
            entry.email,
            entry.phone,
            entry.customer_number,
            entry.booking_number,
            entry.customer_type,
            entry.marker_raw,
            entry.remote_path,
            entry.share_link,
            entry.sms_id,
            entry.archived_path,
            entry.archive_subfolder,
            entry.last_sms_resent_at,
            if entry.sms_status_locked { 1 } else { 0 },
            extra,
            entry.created_at,
            entry.last_updated,
        ],
    )?;
    Ok(())
}

fn row_to_entry(row: &Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let extra_json: String = row.get("extra_json")?;
    let extra: Map<String, Value> = serde_json::from_str(&extra_json).unwrap_or_default();
    let locked: i64 = row.get("sms_status_locked")?;
    Ok(HistoryEntry {
        id: row.get("id")?,
        dir_name: row.get("dir_name")?,
        status: row.get("status")?,
        email_status: row.get("email_status")?,
        sms_status: row.get("sms_status")?,
        error_msg: row.get("error_msg")?,
        first_name: row.get("first_name")?,
        last_name: row.get("last_name")?,
        email: row.get("email")?,
        phone: row.get("phone")?,
        customer_number: row.get("customer_number")?,
        booking_number: row.get("booking_number")?,
        customer_type: row.get("customer_type")?,
        marker_raw: row.get("marker_raw")?,
        remote_path: row.get("remote_path")?,
        share_link: row.get("share_link")?,
        sms_id: row.get("sms_id")?,
        archived_path: row.get("archived_path")?,
        archive_subfolder: row.get("archive_subfolder")?,
        last_sms_resent_at: row.get("last_sms_resent_at")?,
        sms_status_locked: locked != 0,
        extra,
        created_at: row.get("created_at")?,
        last_updated: row.get("last_updated")?,
        overall_status: String::new(),
        combined_error: String::new(),
        display_name: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn open_tmp() -> (tempfile::TempDir, HistoryStore) {
        let dir = tempdir().unwrap();
        let store = HistoryStore::open_at(dir.path().join(HISTORY_DB_FILE)).unwrap();
        (dir, store)
    }

    #[test]
    fn add_or_update_matches_dir_name_and_merges() {
        let (_dir, mut store) = open_tmp();
        let created = store
            .add_or_update(&json!({
                "dir_name": "Flug_001",
                "status": "Gestartet",
                "first_name": "Ada",
                "last_name": "Lovelace",
                "email": "ada@example.de",
            }))
            .unwrap()
            .unwrap();
        assert!(!created.id.is_empty());
        assert_eq!(created.status, "Gestartet");

        let updated = store
            .add_or_update(&json!({
                "dir_name": "Flug_001",
                "status": "Erfolgreich",
                "email_status": "Gesendet",
                "share_link": "https://example/share",
            }))
            .unwrap()
            .unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.first_name, "Ada");
        assert_eq!(updated.status, "Erfolgreich");
        assert_eq!(updated.email_status, "Gesendet");
        assert_eq!(updated.share_link, "https://example/share");
        assert_eq!(updated.overall_status, "Komplett");
        assert!(updated.combined_error.is_empty());
    }

    #[test]
    fn find_by_correlation_id_reads_extra() {
        let (_dir, mut store) = open_tmp();
        store
            .add_or_update(&json!({
                "dir_name": "Flug_cid",
                "status": "Erfolgreich",
                "remote_path": "/Flug_cid",
                "correlation_id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            }))
            .unwrap();
        let found = store
            .find_by_correlation_id("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
            .unwrap()
            .unwrap();
        assert_eq!(found.dir_name, "Flug_cid");
        assert!(store.find_by_correlation_id("missing").unwrap().is_none());
    }

    #[test]
    fn find_by_dir_name_reads_entry() {
        let (_dir, mut store) = open_tmp();
        store
            .add_or_update(&json!({
                "dir_name": "Flug_dir",
                "status": "Erfolgreich",
                "remote_path": "/Flug_dir",
            }))
            .unwrap();
        let found = store.find_by_dir_name("Flug_dir").unwrap().unwrap();
        assert_eq!(found.dir_name, "Flug_dir");
        assert!(store.find_by_dir_name("missing").unwrap().is_none());
    }

    #[test]
    fn successful_retry_does_not_keep_upload_error_as_current() {
        let (_dir, mut store) = open_tmp();
        store
            .add_or_update(&json!({
                "dir_name": "Flug_002",
                "status": "Fehler",
                "error_msg": "Dropbox 429",
            }))
            .unwrap();
        let updated = store
            .add_or_update(&json!({
                "dir_name": "Flug_002",
                "status": "Erfolgreich",
                "email_status": "Gesendet",
            }))
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, "Erfolgreich");
        assert_eq!(updated.error_msg, "Dropbox 429");
        assert!(updated.combined_error.is_empty());
        assert_eq!(updated.overall_status, "Komplett");
    }

    #[test]
    fn skips_payload_without_dir_name() {
        let (_dir, mut store) = open_tmp();
        assert!(store
            .add_or_update(&json!({"status": "Fehler"}))
            .unwrap()
            .is_none());
        assert!(store.get_filtered_history("").unwrap().is_empty());
    }

    #[test]
    fn filter_search_and_delete() {
        let (_dir, mut store) = open_tmp();
        store
            .add_or_update(&json!({"dir_name": "Alpha", "first_name": "Ann"}))
            .unwrap();
        store
            .add_or_update(&json!({"dir_name": "Beta", "first_name": "Bob", "phone": "0160"}))
            .unwrap();
        let filtered = store.get_filtered_history("bob").unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].dir_name, "Beta");

        let id = filtered[0].id.clone();
        assert_eq!(store.delete_items(&[id]).unwrap(), 1);
        assert_eq!(store.get_filtered_history("").unwrap().len(), 1);
        store.clear_all().unwrap();
        assert!(store.get_filtered_history("").unwrap().is_empty());
    }

    #[test]
    fn hidden_append_shadow_rows_are_excluded_from_history() {
        let (_dir, mut store) = open_tmp();
        store
            .add_or_update(&json!({
                "dir_name": "20260818_Test_TA_TM_nachreichung_02",
                "status": "Gestartet",
                "first_name": "Shadow",
            }))
            .unwrap();
        store
            .add_or_update(&json!({
                "dir_name": "20260818_Test_TA_TM",
                "status": "Erfolgreich",
                "remote_path": "/20260818_Test_TA_TM",
            }))
            .unwrap();

        let items = store.get_filtered_history("").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].dir_name, "20260818_Test_TA_TM");
    }

    #[test]
    fn import_legacy_json_preserves_id_and_timestamps() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join(LEGACY_HISTORY_JSON);
        fs::write(
            &json_path,
            serde_json::to_string_pretty(&json!([
                {
                    "id": "legacy-1",
                    "dir_name": "OldJob",
                    "status": "Erfolgreich",
                    "email_status": "Gesendet",
                    "email": "a@b.de",
                    "created_at": "2024-01-01T10:00:00",
                    "last_updated": "2024-01-02T12:00:00",
                    "sms_price": "0.0750"
                }
            ]))
            .unwrap(),
        )
        .unwrap();

        let mut store = HistoryStore::open_at(dir.path().join(HISTORY_DB_FILE)).unwrap();
        let count = store.import_from_json_file(&json_path).unwrap();
        assert_eq!(count, 1);
        let items = store.get_filtered_history("oldjob").unwrap();
        assert_eq!(items[0].id, "legacy-1");
        assert_eq!(items[0].created_at, "2024-01-01T10:00:00");
        assert_eq!(items[0].last_updated, "2024-01-02T12:00:00");
        assert_eq!(items[0].overall_status, "Komplett");
        assert_eq!(
            items[0].extra.get("sms_price").and_then(Value::as_str),
            Some("0.0750")
        );
    }

    #[test]
    fn pagination_newest_created_first() {
        let (_dir, mut store) = open_tmp();
        store
            .add_or_update(&json!({
                "dir_name": "A",
                "created_at": "2024-01-01T00:00:00",
                "last_updated": "2024-12-01T00:00:00"
            }))
            .unwrap();
        store
            .add_or_update(&json!({
                "dir_name": "B",
                "created_at": "2024-06-01T00:00:00",
                "last_updated": "2024-06-02T00:00:00"
            }))
            .unwrap();
        store
            .add_or_update(&json!({
                "dir_name": "C",
                "created_at": "2024-03-01T00:00:00",
                "last_updated": "2024-11-01T00:00:00"
            }))
            .unwrap();
        let page = store.get_filtered_page("", 0, 2).unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].dir_name, "B");
        assert_eq!(page.items[1].dir_name, "C");
    }

    #[test]
    fn metadata_patch_does_not_bump_last_updated() {
        let (_dir, mut store) = open_tmp();
        let created = store
            .add_or_update(&json!({
                "dir_name": "Flug_meta",
                "status": "Erfolgreich",
                "created_at": "2024-01-01T10:00:00",
                "last_updated": "2024-01-01T10:00:00",
            }))
            .unwrap()
            .unwrap();
        let updated = store
            .add_or_update(&json!({
                "dir_name": "Flug_meta",
                "email": "new@example.de",
                "handcam_video": true,
            }))
            .unwrap()
            .unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.email, "new@example.de");
        assert_eq!(updated.last_updated, "2024-01-01T10:00:00");
        assert_eq!(updated.created_at, "2024-01-01T10:00:00");
    }

    #[test]
    fn explicit_last_updated_is_applied() {
        let (_dir, mut store) = open_tmp();
        store
            .add_or_update(&json!({
                "dir_name": "Flug_touch",
                "status": "Gestartet",
                "created_at": "2024-01-01T10:00:00",
                "last_updated": "2024-01-01T10:00:00",
            }))
            .unwrap();
        let updated = store
            .add_or_update(&json!({
                "dir_name": "Flug_touch",
                "status": "Erfolgreich",
                "last_updated": "2024-02-01T12:00:00",
            }))
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, "Erfolgreich");
        assert_eq!(updated.last_updated, "2024-02-01T12:00:00");
        assert_eq!(updated.created_at, "2024-01-01T10:00:00");
    }

    #[test]
    fn display_name_falls_back_to_dir() {
        let entry = HistoryEntry {
            dir_name: "FolderX".into(),
            ..HistoryEntry::default()
        };
        assert_eq!(entry.compute_display_name(), "FolderX");
        let named = HistoryEntry {
            first_name: "Ada".into(),
            last_name: "Lovelace".into(),
            dir_name: "FolderX".into(),
            ..HistoryEntry::default()
        };
        assert_eq!(named.compute_display_name(), "Ada Lovelace");
    }
}
