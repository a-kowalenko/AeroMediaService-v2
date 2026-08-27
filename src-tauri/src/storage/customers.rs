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
use crate::model::crew::CrewMember;
use crate::model::customer_intake::normalize_id_pair;
use crate::model::id_assign::{
    preview_id_assign, run_id_assign_pipeline, IdAssignCustomer, IdAssignError, IdAssignOverride,
    IdAssignPreview,
};
use crate::model::marker::{marker_paths, write_fertig_marker, MARKER_FERTIG, MARKER_PROCESSING};
use crate::storage::app_config_dir;
use crate::storage::config::ConfigError;
use crate::storage::folder_match;

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

impl From<IdAssignError> for CustomerError {
    fn from(value: IdAssignError) -> Self {
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
    #[serde(default)]
    pub kunden_id: String,
    #[serde(default)]
    pub booking_id: String,
    #[serde(default)]
    pub booking_date: String,
    #[serde(default)]
    pub typ: String,
    #[serde(default)]
    pub handcam_foto: bool,
    #[serde(default)]
    pub handcam_video: bool,
    #[serde(default)]
    pub outside_foto: bool,
    #[serde(default)]
    pub outside_video: bool,
    #[serde(default)]
    pub ist_bezahlt_handcam_foto: bool,
    #[serde(default)]
    pub ist_bezahlt_handcam_video: bool,
    #[serde(default)]
    pub ist_bezahlt_outside_foto: bool,
    #[serde(default)]
    pub ist_bezahlt_outside_video: bool,
    #[serde(default)]
    pub media_option: String,
    pub processed: bool,
    pub assigned_path: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Customer {
    pub fn has_api_ids(&self) -> bool {
        !self.kunden_id.trim().is_empty() && !self.booking_id.trim().is_empty()
    }
}

fn id_assign_ctx(customer: &Customer) -> IdAssignCustomer<'_> {
    IdAssignCustomer {
        vorname: &customer.vorname,
        nachname: &customer.nachname,
        kunden_id: &customer.kunden_id,
        booking_id: &customer.booking_id,
        booking_date: &customer.booking_date,
        typ: &customer.typ,
        handcam_foto: customer.handcam_foto,
        handcam_video: customer.handcam_video,
        outside_foto: customer.outside_foto,
        outside_video: customer.outside_video,
        // Gebucht = paid; ungebucht = nicht paid (Marker/Upload).
        ist_bezahlt_handcam_foto: customer.handcam_foto,
        ist_bezahlt_handcam_video: customer.handcam_video,
        ist_bezahlt_outside_foto: customer.outside_foto,
        ist_bezahlt_outside_video: customer.outside_video,
    }
}

/// Intake / save payload (Phase 19b).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CustomerDraft {
    pub vorname: String,
    pub nachname: String,
    pub email: String,
    #[serde(default)]
    pub telefon: String,
    #[serde(default)]
    pub kunden_id: String,
    #[serde(default)]
    pub booking_id: String,
    #[serde(default)]
    pub booking_date: String,
    #[serde(default)]
    pub typ: String,
    #[serde(default)]
    pub handcam_foto: bool,
    #[serde(default)]
    pub handcam_video: bool,
    #[serde(default)]
    pub outside_foto: bool,
    #[serde(default)]
    pub outside_video: bool,
    #[serde(default)]
    pub ist_bezahlt_handcam_foto: bool,
    #[serde(default)]
    pub ist_bezahlt_handcam_video: bool,
    #[serde(default)]
    pub ist_bezahlt_outside_foto: bool,
    #[serde(default)]
    pub ist_bezahlt_outside_video: bool,
    #[serde(default)]
    pub media_option: String,
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
    #[serde(default)]
    pub match_score: u32,
    #[serde(default)]
    pub recommended: bool,
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
    /// Final job folder after ID-rename (same as input for Pure Contact).
    #[serde(default)]
    pub folder_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCustomerProposal {
    pub customer: Customer,
    pub suggested_path: Option<String>,
    pub suggested_name: Option<String>,
    pub match_score: u32,
    pub included: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAssignmentProposal {
    pub rows: Vec<BatchCustomerProposal>,
    pub folders: Vec<MediaFolderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAssignItem {
    pub id: String,
    pub path: String,
    /// Optional TM/VS/Dropzone override for ID-assign (Phase 19c/19d).
    #[serde(default)]
    pub id_override: Option<IdAssignOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAssignOk {
    pub id: String,
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAssignError {
    pub id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAssignOutcome {
    pub assigned: Vec<BatchAssignOk>,
    pub errors: Vec<BatchAssignError>,
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

    pub fn list(&self, search: &str, filter: &str) -> Result<Vec<Customer>, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store.list(search, filter).map_err(|e| e.to_string())
    }

    pub fn save(&self, draft: &CustomerDraft) -> Result<Customer, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store.save(draft).map_err(|e| e.to_string())
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
        crew: &[CrewMember],
        id_override: Option<&IdAssignOverride>,
    ) -> Result<AssignResult, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store
            .assign_to_folder(id, target_path, crew, id_override)
            .map_err(|e| e.to_string())
    }

    pub fn preview_id_assign(
        &self,
        id: &str,
        target_path: &Path,
        crew: &[CrewMember],
        id_override: Option<&IdAssignOverride>,
    ) -> Result<IdAssignPreview, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store
            .preview_id_assign(id, target_path, crew, id_override)
            .map_err(|e| e.to_string())
    }

    pub fn assignment_history(&self) -> Result<Vec<AssignmentHistoryEntry>, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store.assignment_history().map_err(|e| e.to_string())
    }

    pub fn assign_many(
        &self,
        items: &[BatchAssignItem],
        crew: &[CrewMember],
    ) -> Result<BatchAssignOutcome, String> {
        let mut assigned = Vec::new();
        let mut errors = Vec::new();
        for item in items {
            let path = PathBuf::from(item.path.trim());
            match self.assign_to_folder(
                &item.id,
                &path,
                crew,
                item.id_override.as_ref(),
            ) {
                Ok(result) => assigned.push(BatchAssignOk {
                    id: item.id.clone(),
                    file_path: result.file_path,
                }),
                Err(message) => errors.push(BatchAssignError {
                    id: item.id.clone(),
                    message,
                }),
            }
        }
        Ok(BatchAssignOutcome { assigned, errors })
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
            migrate_customer_columns(conn)?;
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
                        created_at, updated_at,
                        kunden_id, booking_id, booking_date, typ,
                        handcam_foto, handcam_video, outside_foto, outside_video,
                        ist_bezahlt_handcam_foto, ist_bezahlt_handcam_video,
                        ist_bezahlt_outside_foto, ist_bezahlt_outside_video,
                        media_option
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
                        OR lower(kunden_id) LIKE ?1 OR lower(booking_id) LIKE ?1
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

    pub fn save(&self, draft: &CustomerDraft) -> Result<Customer, CustomerError> {
        let vorname = normalize_required(&draft.vorname, "Vorname")?;
        let nachname = normalize_required(&draft.nachname, "Nachname")?;
        let email = normalize_required(&draft.email, "E-Mail")?;
        let telefon = draft.telefon.trim().to_string();
        let (kunden_id, booking_id) = normalize_id_pair(&draft.kunden_id, &draft.booking_id)
            .map_err(CustomerError::Message)?;
        let now = now_iso();
        let customer = Customer {
            id: Uuid::new_v4().to_string(),
            vorname,
            nachname,
            email,
            telefon,
            kunden_id,
            booking_id,
            booking_date: draft.booking_date.trim().to_string(),
            typ: draft.typ.trim().to_string(),
            handcam_foto: draft.handcam_foto,
            handcam_video: draft.handcam_video,
            outside_foto: draft.outside_foto,
            outside_video: draft.outside_video,
            ist_bezahlt_handcam_foto: draft.ist_bezahlt_handcam_foto,
            ist_bezahlt_handcam_video: draft.ist_bezahlt_handcam_video,
            ist_bezahlt_outside_foto: draft.ist_bezahlt_outside_foto,
            ist_bezahlt_outside_video: draft.ist_bezahlt_outside_video,
            media_option: draft.media_option.trim().to_string(),
            processed: false,
            assigned_path: String::new(),
            created_at: now.clone(),
            updated_at: now,
        };

        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO customers
                 (id, vorname, nachname, email, telefon, processed, assigned_path, created_at, updated_at,
                  kunden_id, booking_id, booking_date, typ,
                  handcam_foto, handcam_video, outside_foto, outside_video,
                  ist_bezahlt_handcam_foto, ist_bezahlt_handcam_video,
                  ist_bezahlt_outside_foto, ist_bezahlt_outside_video, media_option)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                         ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
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
                    customer.kunden_id,
                    customer.booking_id,
                    customer.booking_date,
                    customer.typ,
                    bool_i32(customer.handcam_foto),
                    bool_i32(customer.handcam_video),
                    bool_i32(customer.outside_foto),
                    bool_i32(customer.outside_video),
                    bool_i32(customer.ist_bezahlt_handcam_foto),
                    bool_i32(customer.ist_bezahlt_handcam_video),
                    bool_i32(customer.ist_bezahlt_outside_foto),
                    bool_i32(customer.ist_bezahlt_outside_video),
                    customer.media_option,
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
        let (kunden_id, booking_id) = normalize_id_pair(&input.kunden_id, &input.booking_id)
            .map_err(CustomerError::Message)?;
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
                 SET vorname = ?2, nachname = ?3, email = ?4, telefon = ?5, updated_at = ?6,
                     kunden_id = ?7, booking_id = ?8, booking_date = ?9, typ = ?10,
                     handcam_foto = ?11, handcam_video = ?12, outside_foto = ?13, outside_video = ?14,
                     ist_bezahlt_handcam_foto = ?15, ist_bezahlt_handcam_video = ?16,
                     ist_bezahlt_outside_foto = ?17, ist_bezahlt_outside_video = ?18,
                     media_option = ?19
                 WHERE id = ?1",
                params![
                    input.id,
                    vorname,
                    nachname,
                    email,
                    telefon,
                    updated_at,
                    kunden_id,
                    booking_id,
                    input.booking_date.trim(),
                    input.typ.trim(),
                    bool_i32(input.handcam_foto),
                    bool_i32(input.handcam_video),
                    bool_i32(input.outside_foto),
                    bool_i32(input.outside_video),
                    bool_i32(input.ist_bezahlt_handcam_foto),
                    bool_i32(input.ist_bezahlt_handcam_video),
                    bool_i32(input.ist_bezahlt_outside_foto),
                    bool_i32(input.ist_bezahlt_outside_video),
                    input.media_option.trim(),
                ],
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
                        created_at, updated_at,
                        kunden_id, booking_id, booking_date, typ,
                        handcam_foto, handcam_video, outside_foto, outside_video,
                        ist_bezahlt_handcam_foto, ist_bezahlt_handcam_video,
                        ist_bezahlt_outside_foto, ist_bezahlt_outside_video,
                        media_option
                 FROM customers WHERE id = ?1",
                params![id],
                map_customer,
            )
            .optional()
            .map_err(CustomerError::from)
        })
    }

    pub fn preview_id_assign(
        &self,
        id: &str,
        target_path: &Path,
        crew: &[CrewMember],
        id_override: Option<&IdAssignOverride>,
    ) -> Result<IdAssignPreview, CustomerError> {
        if !target_path.is_dir() {
            return Err(CustomerError::Message(format!(
                "Zielordner existiert nicht: {}",
                target_path.display()
            )));
        }
        let customer = self
            .get_by_id(id)?
            .ok_or_else(|| CustomerError::Message("Kunde nicht gefunden".into()))?;
        if !customer.has_api_ids() {
            return Err(CustomerError::Message(
                "Review-Vorschau nur für Kunden mit Kunden- und Buchungs-ID.".into(),
            ));
        }
        let ctx = id_assign_ctx(&customer);
        Ok(preview_id_assign(
            target_path,
            &customer.id,
            &ctx,
            crew,
            id_override,
        )?)
    }

    pub fn assign_to_folder(
        &self,
        id: &str,
        target_path: &Path,
        crew: &[CrewMember],
        id_override: Option<&IdAssignOverride>,
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

        if customer.has_api_ids() {
            if is_folder_busy(target_path) {
                return Err(CustomerError::Message(
                    "Ordner ist gerade belegt (Dateien werden noch geschrieben). Bitte kurz warten."
                        .into(),
                ));
            }
            return self.assign_id_pipeline(&customer, target_path, crew, id_override);
        }

        let content = build_contact_marker_json(
            &customer.vorname,
            &customer.nachname,
            &customer.email,
            &customer.telefon,
        )?;
        let file_path = write_fertig_marker(target_path, &content)?;
        let file_path_str = file_path.to_string_lossy().to_string();
        let folder_path_str = target_path.to_string_lossy().to_string();
        self.mark_assigned(&customer, &file_path_str)?;

        Ok(AssignResult {
            file_path: file_path_str,
            folder_path: folder_path_str,
        })
    }

    fn assign_id_pipeline(
        &self,
        customer: &Customer,
        target_path: &Path,
        crew: &[CrewMember],
        id_override: Option<&IdAssignOverride>,
    ) -> Result<AssignResult, CustomerError> {
        let ctx = id_assign_ctx(customer);
        let result = run_id_assign_pipeline(target_path, &ctx, crew, id_override)?;
        let file_path_str = result.fertig_path.to_string_lossy().to_string();
        let folder_path_str = result.folder_path.to_string_lossy().to_string();
        self.mark_assigned(customer, &file_path_str)?;
        Ok(AssignResult {
            file_path: file_path_str,
            folder_path: folder_path_str,
        })
    }

    fn mark_assigned(
        &self,
        customer: &Customer,
        file_path_str: &str,
    ) -> Result<(), CustomerError> {
        let now = now_iso();
        let history_id = Uuid::new_v4().to_string();
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE customers
                 SET processed = 1, assigned_path = ?2, updated_at = ?3
                 WHERE id = ?1",
                params![customer.id, file_path_str, now],
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
        if is_hidden_dir_name(&name) {
            continue;
        }
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

/// Rank folders for a customer: recommended first, then by match score.
pub fn rank_folders_for_customer(
    folders: &mut Vec<MediaFolderInfo>,
    vorname: &str,
    nachname: &str,
) {
    if vorname.trim().is_empty() && nachname.trim().is_empty() {
        return;
    }
    let today = Local::now().format("%Y%m%d").to_string();
    for folder in folders.iter_mut() {
        let assignable = matches!(folder.folder_state, FolderState::Ready);
        folder.match_score =
            folder_match::score_folder_name(&folder.name, vorname, nachname, assignable, &today);
        folder.recommended = false;
    }

    let best = folders.iter().map(|f| f.match_score).max().unwrap_or(0);
    let last_name_hits = folders
        .iter()
        .filter(|f| f.match_score >= folder_match::SCORE_NACHNAME)
        .count();

    for folder in folders.iter_mut() {
        let assignable = matches!(folder.folder_state, FolderState::Ready);
        folder.recommended =
            folder_match::is_recommended(folder.match_score, assignable, best, last_name_hits);
    }

    folders.sort_by(|a, b| {
        folder_match::cmp_rank(
            a.recommended,
            a.match_score,
            &a.name,
            b.recommended,
            b.match_score,
            &b.name,
        )
    });
}

pub fn propose_batch_assignments(
    customers: &[Customer],
    folders: &[MediaFolderInfo],
) -> Vec<BatchCustomerProposal> {
    let today = Local::now().format("%Y%m%d").to_string();
    let ready: Vec<&MediaFolderInfo> = folders
        .iter()
        .filter(|f| matches!(f.folder_state, FolderState::Ready))
        .collect();
    let names: Vec<(&str, &str)> = customers
        .iter()
        .map(|c| (c.vorname.as_str(), c.nachname.as_str()))
        .collect();
    let folder_names: Vec<&str> = ready.iter().map(|f| f.name.as_str()).collect();
    let hits = folder_match::propose_unique_assignments(&names, &folder_names, &today);

    let mut by_customer: Vec<Option<(usize, u32)>> = vec![None; customers.len()];
    for hit in hits {
        if hit.customer_index < by_customer.len() {
            by_customer[hit.customer_index] = Some((hit.folder_index, hit.score));
        }
    }

    customers
        .iter()
        .enumerate()
        .map(|(idx, customer)| {
            if let Some((folder_idx, score)) = by_customer[idx] {
                let folder = ready[folder_idx];
                BatchCustomerProposal {
                    customer: customer.clone(),
                    suggested_path: Some(folder.path.clone()),
                    suggested_name: Some(folder.name.clone()),
                    match_score: score,
                    included: true,
                }
            } else {
                BatchCustomerProposal {
                    customer: customer.clone(),
                    suggested_path: None,
                    suggested_name: None,
                    match_score: 0,
                    included: false,
                }
            }
        })
        .collect()
}

fn inspect_media_folder(path: &Path, name: &str) -> Result<MediaFolderInfo, CustomerError> {
    let block_reason = folder_block_reason(path);
    let is_ready = !is_folder_busy(path);

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
        match_score: 0,
        recommended: false,
    })
}

fn is_hidden_dir_name(name: &str) -> bool {
    name.starts_with('.')
}

/// True when any non-marker file was modified within the busy window (copy in progress).
pub fn is_folder_busy(folder_path: &Path) -> bool {
    let now_ms = system_time_ms();
    let Ok(entries) = fs::read_dir(folder_path) else {
        return true;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            return true;
        };
        let Ok(ft) = entry.file_type() else {
            return true;
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
                            return true;
                        }
                    }
                }
            }
            Err(_) => return true,
        }
    }
    false
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
    Ok(serde_json::to_string_pretty(&serde_json::Value::Object(
        map,
    ))?)
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
        kunden_id: row.get(9)?,
        booking_id: row.get(10)?,
        booking_date: row.get(11)?,
        typ: row.get(12)?,
        handcam_foto: i32_bool(row.get(13)?),
        handcam_video: i32_bool(row.get(14)?),
        outside_foto: i32_bool(row.get(15)?),
        outside_video: i32_bool(row.get(16)?),
        ist_bezahlt_handcam_foto: i32_bool(row.get(17)?),
        ist_bezahlt_handcam_video: i32_bool(row.get(18)?),
        ist_bezahlt_outside_foto: i32_bool(row.get(19)?),
        ist_bezahlt_outside_video: i32_bool(row.get(20)?),
        media_option: row.get(21)?,
    })
}

fn bool_i32(value: bool) -> i32 {
    if value { 1 } else { 0 }
}

fn i32_bool(value: i32) -> bool {
    value != 0
}

fn migrate_customer_columns(conn: &Connection) -> Result<(), CustomerError> {
    let columns = [
        ("kunden_id", "TEXT NOT NULL DEFAULT ''"),
        ("booking_id", "TEXT NOT NULL DEFAULT ''"),
        ("booking_date", "TEXT NOT NULL DEFAULT ''"),
        ("typ", "TEXT NOT NULL DEFAULT ''"),
        ("handcam_foto", "INTEGER NOT NULL DEFAULT 0"),
        ("handcam_video", "INTEGER NOT NULL DEFAULT 0"),
        ("outside_foto", "INTEGER NOT NULL DEFAULT 0"),
        ("outside_video", "INTEGER NOT NULL DEFAULT 0"),
        ("ist_bezahlt_handcam_foto", "INTEGER NOT NULL DEFAULT 0"),
        ("ist_bezahlt_handcam_video", "INTEGER NOT NULL DEFAULT 0"),
        ("ist_bezahlt_outside_foto", "INTEGER NOT NULL DEFAULT 0"),
        ("ist_bezahlt_outside_video", "INTEGER NOT NULL DEFAULT 0"),
        ("media_option", "TEXT NOT NULL DEFAULT ''"),
    ];
    for (name, definition) in columns {
        ensure_column(conn, "customers", name, definition)?;
    }
    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), CustomerError> {
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

    fn draft(
        vorname: &str,
        nachname: &str,
        email: &str,
        telefon: &str,
    ) -> CustomerDraft {
        CustomerDraft {
            vorname: vorname.into(),
            nachname: nachname.into(),
            email: email.into(),
            telefon: telefon.into(),
            ..CustomerDraft::default()
        }
    }

    #[test]
    fn save_list_and_filter() {
        let (_dir, store) = open_temp();
        store
            .save(&draft("Anna", "Adler", "a@example.com", "+49111"))
            .unwrap();
        store
            .save(&draft("Bernd", "Bauer", "b@example.com", ""))
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
    fn save_with_api_ids_and_flags_persists() {
        let (_dir, store) = open_temp();
        let mut input = draft("Anna", "Adler", "a@example.com", "");
        input.kunden_id = "3971".into();
        input.booking_id = "2405".into();
        input.booking_date = "2026-08-27".into();
        input.typ = "Outside".into();
        input.outside_video = true;
        input.ist_bezahlt_outside_video = true;
        input.media_option = "ou_v".into();
        let saved = store.save(&input).unwrap();
        assert!(saved.has_api_ids());
        assert_eq!(saved.kunden_id, "3971");
        assert_eq!(saved.booking_id, "2405");
        assert!(saved.outside_video);
        assert_eq!(saved.media_option, "ou_v");

        let by_id = store.list("3971", "all").unwrap();
        assert_eq!(by_id.len(), 1);

        let err = store
            .save(&CustomerDraft {
                vorname: "X".into(),
                nachname: "Y".into(),
                email: "x@y.de".into(),
                kunden_id: "1111".into(),
                ..CustomerDraft::default()
            })
            .unwrap_err();
        assert!(err.to_string().contains("beide"));
    }

    #[test]
    fn assign_writes_fertig_marker_and_marks_processed() {
        let (_dir, store) = open_temp();
        let customer = store
            .save(&draft("Max", "Mustermann", "max@example.com", "0123"))
            .unwrap();

        let media = tempdir().unwrap();
        let job = media.path().join("Job-1");
        fs::create_dir_all(&job).unwrap();

        let result = store
            .assign_to_folder(&customer.id, &job, &[], None)
            .unwrap();
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
            .save(&draft("Max", "Mustermann", "max@example.com", ""))
            .unwrap();
        let media = tempdir().unwrap();
        let job = media.path().join("Job-1");
        fs::create_dir_all(&job).unwrap();
        fs::write(job.join(MARKER_PROCESSING), "{}").unwrap();

        let err = store
            .assign_to_folder(&customer.id, &job, &[], None)
            .unwrap_err();
        assert!(err.to_string().contains(MARKER_PROCESSING));
    }

    #[test]
    fn assign_id_customer_runs_pipeline() {
        use crate::model::crew::default_crew_list;
        use crate::model::handoff::{evaluate_manifest_gate, GateDecision, MANIFEST_FILENAME};
        use crate::model::id_assign::{IdAssignOverride, SUBDIR_OUTSIDE_VIDEO};

        let (_dir, store) = open_temp();
        let mut input = draft("Roman", "Guest", "r@example.com", "");
        input.kunden_id = "3971".into();
        input.booking_id = "2405".into();
        input.booking_date = "2026-08-27".into();
        input.typ = "Outside".into();
        input.outside_video = true;
        input.ist_bezahlt_outside_video = true;
        let customer = store.save(&input).unwrap();

        let media = tempdir().unwrap();
        let job = media.path().join("Roman_Stefan_Robin");
        fs::create_dir_all(&job).unwrap();
        let media_file = job.join("jump.mp4");
        fs::write(&media_file, b"media").unwrap();
        // Busy-Fenster (3s): mtime zurückdatieren, sonst blockiert frischer Write.
        let f = fs::File::options().write(true).open(&media_file).unwrap();
        f.set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000))
            .unwrap();
        drop(f);

        let result = store
            .assign_to_folder(
                &customer.id,
                &job,
                &default_crew_list(),
                Some(&IdAssignOverride {
                    tandemmaster: Some("Stefan".into()),
                    videospringer: Some("Robin".into()),
                    dropzone_suffix: None,
                }),
            )
            .unwrap();

        assert!(Path::new(&result.file_path).is_file());
        assert!(Path::new(&result.folder_path)
            .join(MANIFEST_FILENAME)
            .is_file());
        assert!(Path::new(&result.folder_path)
            .join(SUBDIR_OUTSIDE_VIDEO)
            .join("jump.mp4")
            .is_file());
        let content = fs::read_to_string(&result.file_path).unwrap();
        assert!(content.contains("\"kunden_id\""));
        assert!(!content.contains("vorname"));
        match evaluate_manifest_gate(Path::new(&result.folder_path), false) {
            GateDecision::Ready { .. } => {}
            other => panic!("expected Ready, got {other:?}"),
        }
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
        fs::write(
            occupied.join(MARKER_FERTIG),
            r#"{"vorname":"A","nachname":"B","email":"a@b.de"}"#,
        )
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

    #[test]
    fn list_media_folders_skips_dot_directories() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("Job-1")).unwrap();
        fs::create_dir_all(root.path().join(".ams-handoff")).unwrap();
        fs::create_dir_all(root.path().join(".hidden")).unwrap();

        let listing = list_media_folders(root.path()).unwrap();
        assert_eq!(listing.folders.len(), 1);
        assert_eq!(listing.folders[0].name, "Job-1");
    }

    #[test]
    fn rank_folders_pins_matching_ready_job() {
        let mut folders = vec![
            MediaFolderInfo {
                name: "20260815_Bernd_Bauer_TA_X".into(),
                path: "/b".into(),
                is_ready: true,
                block_reason: None,
                folder_state: FolderState::Ready,
                match_score: 0,
                recommended: false,
            },
            MediaFolderInfo {
                name: "20260815_Max_Mustermann_TA_Schmidt".into(),
                path: "/m".into(),
                is_ready: true,
                block_reason: None,
                folder_state: FolderState::Ready,
                match_score: 0,
                recommended: false,
            },
            MediaFolderInfo {
                name: "zzz-other".into(),
                path: "/z".into(),
                is_ready: true,
                block_reason: None,
                folder_state: FolderState::Ready,
                match_score: 0,
                recommended: false,
            },
        ];
        rank_folders_for_customer(&mut folders, "Max", "Mustermann");
        assert_eq!(folders[0].name, "20260815_Max_Mustermann_TA_Schmidt");
        assert!(folders[0].recommended);
        assert!(folders[0].match_score >= folder_match::SCORE_NACHNAME);
        assert!(!folders[1].recommended);
    }

    fn sample_customer(id: &str, vorname: &str, nachname: &str) -> Customer {
        Customer {
            id: id.into(),
            vorname: vorname.into(),
            nachname: nachname.into(),
            email: format!("{vorname}@ex.de"),
            telefon: String::new(),
            kunden_id: String::new(),
            booking_id: String::new(),
            booking_date: String::new(),
            typ: String::new(),
            handcam_foto: false,
            handcam_video: false,
            outside_foto: false,
            outside_video: false,
            ist_bezahlt_handcam_foto: false,
            ist_bezahlt_handcam_video: false,
            ist_bezahlt_outside_foto: false,
            ist_bezahlt_outside_video: false,
            media_option: String::new(),
            processed: false,
            assigned_path: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn propose_batch_includes_unmatched_unchecked() {
        let customers = vec![
            sample_customer("1", "Max", "Mustermann"),
            sample_customer("2", "Ohne", "Treffer"),
        ];
        let folders = vec![MediaFolderInfo {
            name: "20260815_Max_Mustermann_TA_X".into(),
            path: "/m".into(),
            is_ready: true,
            block_reason: None,
            folder_state: FolderState::Ready,
            match_score: 0,
            recommended: false,
        }];
        let rows = propose_batch_assignments(&customers, &folders);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].included);
        assert_eq!(rows[0].suggested_path.as_deref(), Some("/m"));
        assert!(!rows[1].included);
        assert!(rows[1].suggested_path.is_none());
    }
}
