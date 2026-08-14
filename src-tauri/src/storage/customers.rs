//! Customer intake queue + assignment history (Fertig-App replacement).
//!
//! Stores pending customers and writes Pure-Contact `_fertig.txt` markers into
//! media folders under the configured monitor path.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

use crate::constants::CUSTOMERS_DB_FILE;
use crate::model::marker::{
    marker_paths, write_fertig_marker, MARKER_FERTIG, MARKER_PROCESSING,
};
use crate::storage::app_config_dir;
use crate::storage::config::ConfigError;

const BUSY_WINDOW_MS: u128 = 3000;
const ASSIGNMENT_HISTORY_LIMIT: usize = 100;

#[derive(Debug, Error)]
pub enum CustomerError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Message(String),
}

impl From<ConfigError> for CustomerError {
    fn from(value: ConfigError) -> Self {
        CustomerError::Message(value.to_string())
    }
}

impl From<crate::model::marker::MarkerError> for CustomerError {
    fn from(value: crate::model::marker::MarkerError) -> Self {
        CustomerError::Message(value.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Customer {
    pub id: String,
    pub vorname: String,
    pub nachname: String,
    pub email: String,
    pub telefon: String,
    pub processed: bool,
    pub assigned_path: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentHistoryEntry {
    pub id: String,
    pub customer_id: String,
    pub vorname: String,
    pub nachname: String,
    pub email: String,
    pub telefon: String,
    pub file_path: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderState {
    Ready,
    Busy,
    Occupied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFolderInfo {
    pub name: String,
    pub path: String,
    pub is_ready: bool,
    pub block_reason: Option<String>,
    pub folder_state: FolderState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaDirectoryListing {
    pub path: String,
    pub parent: String,
    pub folders: Vec<MediaFolderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignResult {
    pub file_path: String,
}

#[derive(Clone)]
pub struct CustomerState {
    store: Arc<Mutex<CustomerStore>>,
}

impl CustomerState {
    pub fn new() -> Result<Self, String> {
        let store = CustomerStore::open_default().map_err(|e| e.to_string())?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
        })
    }

    pub fn list(
        &self,
        search: &str,
        filter: &str,
    ) -> Result<Vec<Customer>, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store
            .list(search, filter)
            .map_err(|e| e.to_string())
    }

    pub fn save(
        &self,
        vorname: &str,
        nachname: &str,
        email: &str,
        telefon: &str,
    ) -> Result<Customer, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store
            .save(vorname, nachname, email, telefon)
            .map_err(|e| e.to_string())
    }

    pub fn update(&self, customer: &Customer) -> Result<Customer, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store.update(customer).map_err(|e| e.to_string())
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store.delete(id).map_err(|e| e.to_string())
    }

    pub fn set_processed(&self, id: &str, processed: bool) -> Result<Customer, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store
            .set_processed(id, processed)
            .map_err(|e| e.to_string())
    }

    pub fn assign_to_folder(
        &self,
        id: &str,
        target_path: &Path,
    ) -> Result<AssignResult, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store
            .assign_to_folder(id, target_path)
            .map_err(|e| e.to_string())
    }

    pub fn assignment_history(&self) -> Result<Vec<AssignmentHistoryEntry>, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store.assignment_history().map_err(|e| e.to_string())
    }
}

pub struct CustomerStore {
    db_path: PathBuf,
}

impl CustomerStore {
    pub fn open_default() -> Result<Self, CustomerError> {
        let dir = app_config_dir()?;
        Self::open(&dir.join(CUSTOMERS_DB_FILE))
    }

    pub fn open(db_path: &Path) -> Result<Self, CustomerError> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let store = Self {
            db_path: db_path.to_path_buf(),
        };
        store.with_conn(|conn| {
            conn.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS customers (
                    id TEXT PRIMARY KEY NOT NULL,
                    vorname TEXT NOT NULL,
                    nachname TEXT NOT NULL,
                    email TEXT NOT NULL,
                    telefon TEXT NOT NULL DEFAULT '',
                    processed INTEGER NOT NULL DEFAULT 0,
                    assigned_path TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS assignment_history (
                    id TEXT PRIMARY KEY NOT NULL,
                    customer_id TEXT NOT NULL,
                    vorname TEXT NOT NULL,
                    nachname TEXT NOT NULL,
                    email TEXT NOT NULL,
                    telefon TEXT NOT NULL DEFAULT '',
                    file_path TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_customers_processed
                    ON customers(processed, created_at DESC);
                CREATE INDEX IF NOT EXISTS idx_assignment_history_created
                    ON assignment_history(created_at DESC);
                ",
            )?;
            Ok(())
        })?;
        Ok(store)
    }

    fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, CustomerError>,
    ) -> Result<T, CustomerError> {
        let conn = Connection::open(&self.db_path)?;
        f(&conn)
    }

    pub fn list(&self, search: &str, filter: &str) -> Result<Vec<Customer>, CustomerError> {
        self.with_conn(|conn| {
            let mut sql = String::from(
                "SELECT id, vorname, nachname, email, telefon, processed, assigned_path,
                        created_at, updated_at
                 FROM customers WHERE 1=1",
            );
            let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            match filter {
                "unprocessed" => sql.push_str(" AND processed = 0"),
                "processed" => sql.push_str(" AND processed = 1"),
                _ => {}
            }

            let term = search.trim().to_lowercase();
            if !term.is_empty() {
                sql.push_str(
                    " AND (
                        lower(vorname) LIKE ?1 OR lower(nachname) LIKE ?1
                        OR lower(email) LIKE ?1 OR lower(telefon) LIKE ?1
                        OR lower(vorname || ' ' || nachname) LIKE ?1
                    )",
                );
                binds.push(Box::new(format!("%{term}%")));
            }

            sql.push_str(" ORDER BY created_at DESC");

            let mut stmt = conn.prepare(&sql)?;
            let params_ref: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
            let rows = stmt.query_map(params_ref.as_slice(), map_customer)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    pub fn save(
        &self,
        vorname: &str,
        nachname: &str,
        email: &str,
        telefon: &str,
    ) -> Result<Customer, CustomerError> {
        let vorname = normalize_required(vorname, "Vorname")?;
        let nachname = normalize_required(nachname, "Nachname")?;
        let email = normalize_required(email, "E-Mail")?;
        let telefon = telefon.trim().to_string();
        let now = now_iso();
        let customer = Customer {
            id: Uuid::new_v4().to_string(),
            vorname,
            nachname,
            email,
            telefon,
            processed: false,
            assigned_path: String::new(),
            created_at: now.clone(),
            updated_at: now,
        };

        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO customers
                 (id, vorname, nachname, email, telefon, processed, assigned_path, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    customer.id,
                    customer.vorname,
                    customer.nachname,
                    customer.email,
                    customer.telefon,
                    0i32,
                    customer.assigned_path,
                    customer.created_at,
                    customer.updated_at,
                ],
            )?;
            Ok(())
        })?;
        Ok(customer)
    }

    pub fn update(&self, input: &Customer) -> Result<Customer, CustomerError> {
        let vorname = normalize_required(&input.vorname, "Vorname")?;
        let nachname = normalize_required(&input.nachname, "Nachname")?;
        let email = normalize_required(&input.email, "E-Mail")?;
        let telefon = input.telefon.trim().to_string();
        let updated_at = now_iso();

        self.with_conn(|conn| {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM customers WHERE id = ?1",
                    params![input.id],
                    |row| row.get(0),
                )
                .optional()?;
            if existing.is_none() {
                return Err(CustomerError::Message("Kunde nicht gefunden".into()));
            }
            conn.execute(
                "UPDATE customers
                 SET vorname = ?2, nachname = ?3, email = ?4, telefon = ?5, updated_at = ?6
                 WHERE id = ?1",
                params![input.id, vorname, nachname, email, telefon, updated_at],
            )?;
            Ok(())
        })?;

        self.get_by_id(&input.id)?
            .ok_or_else(|| CustomerError::Message("Kunde nicht gefunden".into()))
    }

    pub fn delete(&self, id: &str) -> Result<(), CustomerError> {
        self.with_conn(|conn| {
            let n = conn.execute("DELETE FROM customers WHERE id = ?1", params![id])?;
            if n == 0 {
                return Err(CustomerError::Message("Kunde nicht gefunden".into()));
            }
            Ok(())
        })
    }

    pub fn set_processed(&self, id: &str, processed: bool) -> Result<Customer, CustomerError> {
        let updated_at = now_iso();
        self.with_conn(|conn| {
            let n = conn.execute(
                "UPDATE customers SET processed = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, if processed { 1 } else { 0 }, updated_at],
            )?;
            if n == 0 {
                return Err(CustomerError::Message("Kunde nicht gefunden".into()));
            }
            Ok(())
        })?;
        self.get_by_id(id)?
            .ok_or_else(|| CustomerError::Message("Kunde nicht gefunden".into()))
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<Customer>, CustomerError> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, vorname, nachname, email, telefon, processed, assigned_path,
                        created_at, updated_at
                 FROM customers WHERE id = ?1",
                params![id],
                map_customer,
            )
            .optional()
            .map_err(CustomerError::from)
        })
    }

    pub fn assign_to_folder(
        &self,
        id: &str,
        target_path: &Path,
    ) -> Result<AssignResult, CustomerError> {
        if !target_path.is_dir() {
            return Err(CustomerError::Message(format!(
                "Zielordner existiert nicht: {}",
                target_path.display()
            )));
        }

        if let Some(reason) = folder_block_reason(target_path) {
            return Err(CustomerError::Message(format!(
                "Export nicht möglich: Im Ordner existiert bereits „{reason}“."
            )));
        }

        let customer = self
            .get_by_id(id)?
            .ok_or_else(|| CustomerError::Message("Kunde nicht gefunden".into()))?;

        let content = build_contact_marker_json(
            &customer.vorname,
            &customer.nachname,
            &customer.email,
            &customer.telefon,
        )?;
        let file_path = write_fertig_marker(target_path, &content)?;
        let file_path_str = file_path.to_string_lossy().to_string();
        let now = now_iso();
        let history_id = Uuid::new_v4().to_string();

        self.with_conn(|conn| {
            conn.execute(
                "UPDATE customers
                 SET processed = 1, assigned_path = ?2, updated_at = ?3
                 WHERE id = ?1",
                params![id, file_path_str, now],
            )?;
            conn.execute(
                "INSERT INTO assignment_history
                 (id, customer_id, vorname, nachname, email, telefon, file_path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    history_id,
                    customer.id,
                    customer.vorname,
                    customer.nachname,
                    customer.email,
                    customer.telefon,
                    file_path_str,
                    now,
                ],
            )?;
            // Keep only the newest N history rows.
            conn.execute(
                "DELETE FROM assignment_history
                 WHERE id NOT IN (
                     SELECT id FROM assignment_history
                     ORDER BY created_at DESC
                     LIMIT ?1
                 )",
                params![ASSIGNMENT_HISTORY_LIMIT as i64],
            )?;
            Ok(())
        })?;

        Ok(AssignResult {
            file_path: file_path_str,
        })
    }

    pub fn assignment_history(&self) -> Result<Vec<AssignmentHistoryEntry>, CustomerError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, customer_id, vorname, nachname, email, telefon, file_path, created_at
                 FROM assignment_history
                 ORDER BY created_at DESC
                 LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![ASSIGNMENT_HISTORY_LIMIT as i64], |row| {
                Ok(AssignmentHistoryEntry {
                    id: row.get(0)?,
                    customer_id: row.get(1)?,
                    vorname: row.get(2)?,
                    nachname: row.get(3)?,
                    email: row.get(4)?,
                    telefon: row.get(5)?,
                    file_path: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }
}

pub fn list_media_folders(dir_path: &Path) -> Result<MediaDirectoryListing, CustomerError> {
    if !dir_path.is_dir() {
        return Err(CustomerError::Message(format!(
            "Verzeichnis existiert nicht: {}",
            dir_path.display()
        )));
    }

    let mut folders = Vec::new();
    let entries = fs::read_dir(dir_path)?;
    for entry in entries {
        let entry = entry?;
        let meta = entry.file_type()?;
        if !meta.is_dir() {
            continue;
        }
        let full = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let info = inspect_media_folder(&full, &name)?;
        folders.push(info);
    }
    folders.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let parent = dir_path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| dir_path.to_string_lossy().to_string());

    Ok(MediaDirectoryListing {
        path: dir_path.to_string_lossy().to_string(),
        parent,
        folders,
    })
}

fn inspect_media_folder(path: &Path, name: &str) -> Result<MediaFolderInfo, CustomerError> {
    let block_reason = folder_block_reason(path);
    let mut is_ready = true;
    let now_ms = system_time_ms();

    match fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => {
                        is_ready = false;
                        break;
                    }
                };
                let ft = match entry.file_type() {
                    Ok(ft) => ft,
                    Err(_) => {
                        is_ready = false;
                        break;
                    }
                };
                if !ft.is_file() {
                    continue;
                }
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();
                if file_name == MARKER_FERTIG || file_name == MARKER_PROCESSING {
                    continue;
                }
                match fs::metadata(entry.path()) {
                    Ok(meta) => {
                        if let Ok(modified) = meta.modified() {
                            if let Ok(elapsed) = modified.duration_since(UNIX_EPOCH) {
                                if now_ms.saturating_sub(elapsed.as_millis()) < BUSY_WINDOW_MS {
                                    is_ready = false;
                                }
                            }
                        }
                    }
                    Err(_) => {
                        is_ready = false;
                        break;
                    }
                }
            }
        }
        Err(_) => {
            is_ready = false;
        }
    }

    let folder_state = if !is_ready {
        FolderState::Busy
    } else if block_reason.is_some() {
        FolderState::Occupied
    } else {
        FolderState::Ready
    };

    Ok(MediaFolderInfo {
        name: name.to_string(),
        path: path.to_string_lossy().to_string(),
        is_ready,
        block_reason,
        folder_state,
    })
}

pub fn folder_block_reason(folder_path: &Path) -> Option<String> {
    let (fertig, processing) = marker_paths(folder_path);
    if processing.is_file() {
        return Some(MARKER_PROCESSING.to_string());
    }
    if fertig.is_file() {
        return Some(MARKER_FERTIG.to_string());
    }
    None
}

pub fn build_contact_marker_json(
    vorname: &str,
    nachname: &str,
    email: &str,
    telefon: &str,
) -> Result<String, CustomerError> {
    let mut map = serde_json::Map::new();
    map.insert("vorname".into(), json!(vorname.trim()));
    map.insert("nachname".into(), json!(nachname.trim()));
    map.insert("email".into(), json!(email.trim()));
    let phone = telefon.trim();
    if !phone.is_empty() {
        map.insert("telefon".into(), json!(phone));
    }
    Ok(serde_json::to_string_pretty(&serde_json::Value::Object(map))?)
}

fn map_customer(row: &rusqlite::Row<'_>) -> rusqlite::Result<Customer> {
    let processed: i32 = row.get(5)?;
    Ok(Customer {
        id: row.get(0)?,
        vorname: row.get(1)?,
        nachname: row.get(2)?,
        email: row.get(3)?,
        telefon: row.get(4)?,
        processed: processed != 0,
        assigned_path: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn normalize_required(value: &str, label: &str) -> Result<String, CustomerError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(CustomerError::Message(format!(
            "{label} darf nicht leer sein."
        )));
    }
    Ok(trimmed.to_string())
}

fn now_iso() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string()
}

fn system_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_temp() -> (tempfile::TempDir, CustomerStore) {
        let dir = tempdir().unwrap();
        let store = CustomerStore::open(&dir.path().join("customers.db")).unwrap();
        (dir, store)
    }

    #[test]
    fn save_list_and_filter() {
        let (_dir, store) = open_temp();
        store
            .save("Anna", "Adler", "a@example.com", "+49111")
            .unwrap();
        store
            .save("Bernd", "Bauer", "b@example.com", "")
            .unwrap();

        let all = store.list("", "all").unwrap();
        assert_eq!(all.len(), 2);

        let search = store.list("adler", "all").unwrap();
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].nachname, "Adler");

        let unprocessed = store.list("", "unprocessed").unwrap();
        assert_eq!(unprocessed.len(), 2);
    }

    #[test]
    fn assign_writes_fertig_marker_and_marks_processed() {
        let (_dir, store) = open_temp();
        let customer = store
            .save("Max", "Mustermann", "max@example.com", "0123")
            .unwrap();

        let media = tempdir().unwrap();
        let job = media.path().join("Job-1");
        fs::create_dir_all(&job).unwrap();

        let result = store.assign_to_folder(&customer.id, &job).unwrap();
        assert!(Path::new(&result.file_path).is_file());

        let content = fs::read_to_string(&result.file_path).unwrap();
        assert!(content.contains("\"vorname\": \"Max\""));
        assert!(content.contains("\"telefon\": \"0123\""));

        let updated = store.get_by_id(&customer.id).unwrap().unwrap();
        assert!(updated.processed);
        assert_eq!(updated.assigned_path, result.file_path);

        let history = store.assignment_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].email, "max@example.com");
    }

    #[test]
    fn assign_blocked_when_processing_marker_exists() {
        let (_dir, store) = open_temp();
        let customer = store
            .save("Max", "Mustermann", "max@example.com", "")
            .unwrap();
        let media = tempdir().unwrap();
        let job = media.path().join("Job-1");
        fs::create_dir_all(&job).unwrap();
        fs::write(job.join(MARKER_PROCESSING), "{}").unwrap();

        let err = store.assign_to_folder(&customer.id, &job).unwrap_err();
        assert!(err.to_string().contains(MARKER_PROCESSING));
    }

    #[test]
    fn build_marker_omits_empty_phone() {
        let with_phone = build_contact_marker_json("A", "B", "a@b.de", "123").unwrap();
        assert!(with_phone.contains("telefon"));
        let without = build_contact_marker_json("A", "B", "a@b.de", "  ").unwrap();
        assert!(!without.contains("telefon"));
    }

    #[test]
    fn list_media_folders_reports_occupied() {
        let root = tempdir().unwrap();
        let ready = root.path().join("ready");
        let occupied = root.path().join("occupied");
        fs::create_dir_all(&ready).unwrap();
        fs::create_dir_all(&occupied).unwrap();
        fs::write(occupied.join(MARKER_FERTIG), r#"{"vorname":"A","nachname":"B","email":"a@b.de"}"#)
            .unwrap();

        let listing = list_media_folders(root.path()).unwrap();
        assert_eq!(listing.folders.len(), 2);
        let occ = listing
            .folders
            .iter()
            .find(|f| f.name == "occupied")
            .unwrap();
        assert!(matches!(occ.folder_state, FolderState::Occupied));
        assert_eq!(occ.block_reason.as_deref(), Some(MARKER_FERTIG));
        let ready_info = listing.folders.iter().find(|f| f.name == "ready").unwrap();
        assert!(matches!(ready_info.folder_state, FolderState::Ready));
    }
}
