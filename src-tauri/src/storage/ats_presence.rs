//! ATS bridge presence and activity persistence (Phase 13 / P5+).
//!
//! Bridge-only observability: hosts become "active" only through AMS bridge requests.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::constants::ATS_PRESENCE_DB_FILE;
use crate::storage::app_config_dir;
use crate::storage::config::ConfigError;

const DEFAULT_TTL_MINUTES: u32 = 60;
const MAX_TTL_MINUTES: u32 = 24 * 60;
const DEFAULT_DETAILS_LIMIT: u32 = 100;
const MAX_DETAILS_LIMIT: u32 = 500;
const DEFAULT_JOBS_PAGE_SIZE: u32 = 50;
const MAX_JOBS_PAGE_SIZE: u32 = 200;
const ACTIVITY_RETENTION_HOURS: i64 = 24 * 7;
const PRUNE_INTERVAL_SECONDS: i64 = 5 * 60;
const MAX_ACTIVITY_PAYLOAD_CHARS: usize = 8192;
/// ATS polls `/v1/health` every 45s; 120s = 2× poll interval + buffer.
const CONNECTED_TTL_SECONDS: i64 = 120;
/// Hosts seen within this window but not connected → „Nicht verbunden“.
const RECENT_WINDOW_DAYS: i64 = 30;

#[derive(Debug, Error)]
pub enum AtsPresenceError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

impl From<ConfigError> for AtsPresenceError {
    fn from(value: ConfigError) -> Self {
        AtsPresenceError::Io(std::io::Error::other(value.to_string()))
    }
}

#[derive(Debug, Clone)]
pub struct AtsIdentityInput {
    pub instance_id: String,
    pub hostname: String,
    pub ats_version: String,
    pub ats_app: String,
    pub degraded_identity: bool,
}

#[derive(Debug, Clone)]
pub struct AtsActivityInput {
    pub event_type: String,
    pub route: String,
    pub method: String,
    pub status_code_class: String,
    pub correlation_id: String,
    pub folder_name: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AtsPresenceCategory {
    Connected,
    Disconnected,
    InactiveLong,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AtsHostSummary {
    pub instance_id: String,
    pub hostname: String,
    pub display_label: String,
    pub ats_version: String,
    pub ats_app: String,
    pub last_event_type: String,
    pub last_event_at: String,
    pub last_seen_at: String,
    pub is_connected: bool,
    pub is_active: bool,
    pub presence_category: AtsPresenceCategory,
    pub activity_count_ttl: u32,
    pub jobs_count_ttl: u32,
    pub degraded_identity: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AtsActivityEntry {
    pub occurred_at: String,
    pub event_type: String,
    pub route: String,
    pub method: String,
    pub status_code_class: String,
    pub correlation_id: String,
    pub folder_name: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AtsJobOriginRecord {
    pub correlation_id: String,
    pub folder_name: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub source_event_type: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AtsHostDetails {
    pub instance_id: String,
    pub hostname: String,
    pub display_label: String,
    pub ats_version: String,
    pub ats_app: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub last_event_type: String,
    pub last_event_at: String,
    pub is_connected: bool,
    pub is_active: bool,
    pub presence_category: AtsPresenceCategory,
    pub degraded_identity: bool,
    pub activity_window_minutes: u32,
    pub recent_events: Vec<AtsActivityEntry>,
    pub recent_jobs: Vec<AtsJobOriginRecord>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AtsJobsPage {
    pub items: Vec<AtsJobOriginRecord>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Clone)]
pub struct AtsPresenceState {
    store: Arc<Mutex<AtsPresenceStore>>,
}

impl AtsPresenceState {
    pub fn new() -> Result<Self, String> {
        let store = AtsPresenceStore::open_default().map_err(|e| e.to_string())?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
        })
    }

    pub fn record_event(
        &self,
        identity: AtsIdentityInput,
        activity: AtsActivityInput,
    ) -> Result<(), String> {
        let mut store = self.store.lock().map_err(|e| e.to_string())?;
        store
            .record_event(&identity, &activity)
            .map_err(|e| e.to_string())
    }

    pub fn get_hosts_summary(&self, ttl_minutes: u32) -> Result<Vec<AtsHostSummary>, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store
            .get_hosts_summary(ttl_minutes)
            .map_err(|e| e.to_string())
    }

    pub fn get_host_details(
        &self,
        instance_id: &str,
        ttl_minutes: u32,
        limit: u32,
    ) -> Result<Option<AtsHostDetails>, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store
            .get_host_details(instance_id, ttl_minutes, limit)
            .map_err(|e| e.to_string())
    }

    pub fn get_jobs_by_host(
        &self,
        instance_id: &str,
        ttl_minutes: u32,
        page: u32,
        page_size: u32,
    ) -> Result<AtsJobsPage, String> {
        let store = self.store.lock().map_err(|e| e.to_string())?;
        store
            .get_jobs_by_host(instance_id, ttl_minutes, page, page_size)
            .map_err(|e| e.to_string())
    }

    #[cfg(test)]
    pub fn from_store(store: AtsPresenceStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }
}

pub struct AtsPresenceStore {
    db_path: PathBuf,
    last_prune_epoch: Option<i64>,
}

impl AtsPresenceStore {
    pub fn open_default() -> Result<Self, AtsPresenceError> {
        let dir = app_config_dir()?;
        fs::create_dir_all(&dir)?;
        Self::open_at(dir.join(ATS_PRESENCE_DB_FILE))
    }

    pub fn open_at(db_path: PathBuf) -> Result<Self, AtsPresenceError> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let store = Self {
            db_path,
            last_prune_epoch: None,
        };
        store.connect()?;
        Ok(store)
    }

    fn connect(&self) -> Result<Connection, AtsPresenceError> {
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS ats_hosts (
                instance_id TEXT PRIMARY KEY,
                display_hostname TEXT NOT NULL,
                ats_version TEXT NOT NULL DEFAULT '',
                ats_app TEXT NOT NULL DEFAULT '',
                first_seen_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                last_event_at TEXT NOT NULL,
                last_event_type TEXT NOT NULL,
                last_route TEXT NOT NULL DEFAULT '',
                degraded_identity INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS ats_activity (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                instance_id TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                event_type TEXT NOT NULL,
                route TEXT NOT NULL,
                method TEXT NOT NULL,
                status_code_class TEXT NOT NULL DEFAULT '',
                correlation_id TEXT NOT NULL DEFAULT '',
                folder_name TEXT NOT NULL DEFAULT '',
                hostname_snapshot TEXT NOT NULL DEFAULT '',
                ats_version_snapshot TEXT NOT NULL DEFAULT '',
                ats_app_snapshot TEXT NOT NULL DEFAULT '',
                degraded_identity INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY(instance_id) REFERENCES ats_hosts(instance_id)
             );
             CREATE INDEX IF NOT EXISTS idx_ats_activity_instance_time
                ON ats_activity(instance_id, occurred_at DESC);
             CREATE INDEX IF NOT EXISTS idx_ats_activity_time
                ON ats_activity(occurred_at DESC);
             CREATE INDEX IF NOT EXISTS idx_ats_activity_correlation
                ON ats_activity(correlation_id);
             CREATE TABLE IF NOT EXISTS ats_job_origin (
                correlation_id TEXT PRIMARY KEY,
                instance_id TEXT NOT NULL,
                folder_name TEXT NOT NULL DEFAULT '',
                first_seen_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                source_event_type TEXT NOT NULL,
                FOREIGN KEY(instance_id) REFERENCES ats_hosts(instance_id)
             );
             CREATE INDEX IF NOT EXISTS idx_ats_job_origin_instance_time
                ON ats_job_origin(instance_id, last_seen_at DESC);",
        )?;
        let _ = conn.execute(
            "ALTER TABLE ats_activity ADD COLUMN payload_json TEXT NOT NULL DEFAULT ''",
            [],
        );
        Ok(conn)
    }

    pub fn record_event(
        &mut self,
        identity: &AtsIdentityInput,
        activity: &AtsActivityInput,
    ) -> Result<(), AtsPresenceError> {
        let conn = self.connect()?;
        let now = Utc::now().to_rfc3339();
        let degraded = if identity.degraded_identity { 1 } else { 0 };
        conn.execute(
            "INSERT INTO ats_hosts (
                instance_id, display_hostname, ats_version, ats_app,
                first_seen_at, last_seen_at, last_event_at, last_event_type,
                last_route, degraded_identity, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?5, ?6, ?7, ?8, ?5, ?5)
             ON CONFLICT(instance_id) DO UPDATE SET
                display_hostname = excluded.display_hostname,
                ats_version = excluded.ats_version,
                ats_app = excluded.ats_app,
                last_seen_at = excluded.last_seen_at,
                last_event_at = excluded.last_event_at,
                last_event_type = excluded.last_event_type,
                last_route = excluded.last_route,
                degraded_identity = excluded.degraded_identity,
                updated_at = excluded.updated_at",
            params![
                &identity.instance_id,
                &identity.hostname,
                &identity.ats_version,
                &identity.ats_app,
                &now,
                &activity.event_type,
                &activity.route,
                degraded,
            ],
        )?;
        conn.execute(
            "INSERT INTO ats_activity (
                instance_id, occurred_at, event_type, route, method, status_code_class,
                correlation_id, folder_name, hostname_snapshot, ats_version_snapshot,
                ats_app_snapshot, degraded_identity, payload_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                &identity.instance_id,
                &now,
                &activity.event_type,
                &activity.route,
                &activity.method,
                &activity.status_code_class,
                &activity.correlation_id,
                &activity.folder_name,
                &identity.hostname,
                &identity.ats_version,
                &identity.ats_app,
                degraded,
                &activity.payload_json,
            ],
        )?;
        self.upsert_job_origin(&conn, &identity.instance_id, activity, &now)?;
        self.prune_if_due(&conn)?;
        Ok(())
    }

    fn upsert_job_origin(
        &self,
        conn: &Connection,
        instance_id: &str,
        activity: &AtsActivityInput,
        now: &str,
    ) -> Result<(), AtsPresenceError> {
        let cid = activity.correlation_id.trim();
        if cid.is_empty() {
            return Ok(());
        }
        let existing: Option<(String, String, String)> = conn
            .query_row(
                "SELECT instance_id, folder_name, source_event_type FROM ats_job_origin WHERE correlation_id = ?1",
                params![cid],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        match existing {
            None => {
                conn.execute(
                    "INSERT INTO ats_job_origin (
                        correlation_id, instance_id, folder_name, first_seen_at, last_seen_at, source_event_type
                     ) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
                    params![
                        cid,
                        instance_id,
                        activity.folder_name,
                        now,
                        activity.event_type,
                    ],
                )?;
            }
            Some((existing_instance, existing_folder, existing_source)) => {
                let keep_existing_source = existing_source == "handoff_ready" && activity.event_type != "handoff_ready";
                let new_source = if keep_existing_source {
                    existing_source
                } else {
                    activity.event_type.clone()
                };
                let new_instance = if keep_existing_source {
                    existing_instance
                } else {
                    instance_id.to_string()
                };
                let new_folder = if !activity.folder_name.trim().is_empty() {
                    activity.folder_name.clone()
                } else {
                    existing_folder
                };
                conn.execute(
                    "UPDATE ats_job_origin
                        SET instance_id = ?2,
                            folder_name = ?3,
                            last_seen_at = ?4,
                            source_event_type = ?5
                      WHERE correlation_id = ?1",
                    params![cid, new_instance, new_folder, now, new_source],
                )?;
            }
        }
        Ok(())
    }

    fn prune_if_due(&mut self, conn: &Connection) -> Result<(), AtsPresenceError> {
        let now_epoch = Utc::now().timestamp();
        if self
            .last_prune_epoch
            .map(|last| now_epoch - last < PRUNE_INTERVAL_SECONDS)
            .unwrap_or(false)
        {
            return Ok(());
        }
        let cutoff = (Utc::now() - Duration::hours(ACTIVITY_RETENTION_HOURS)).to_rfc3339();
        conn.execute(
            "DELETE FROM ats_activity WHERE occurred_at < ?1",
            params![cutoff],
        )?;
        self.last_prune_epoch = Some(now_epoch);
        Ok(())
    }

    pub fn get_hosts_summary(
        &self,
        ttl_minutes: u32,
    ) -> Result<Vec<AtsHostSummary>, AtsPresenceError> {
        let ttl = clamp_ttl(ttl_minutes);
        let cutoff = ttl_cutoff(ttl);
        let connected_cutoff = connected_cutoff();
        let recent_cutoff = recent_window_cutoff();
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT
                h.instance_id,
                h.display_hostname,
                h.ats_version,
                h.ats_app,
                h.last_event_type,
                h.last_event_at,
                h.last_seen_at,
                h.degraded_identity,
                COALESCE((
                    SELECT COUNT(*)
                    FROM ats_activity a
                    WHERE a.instance_id = h.instance_id
                      AND a.occurred_at >= ?1
                ), 0) AS activity_count,
                COALESCE((
                    SELECT COUNT(DISTINCT a2.correlation_id)
                    FROM ats_activity a2
                    WHERE a2.instance_id = h.instance_id
                      AND a2.occurred_at >= ?1
                      AND TRIM(a2.correlation_id) <> ''
                ), 0) AS jobs_count
             FROM ats_hosts h
             ORDER BY h.last_seen_at DESC, h.display_hostname COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map(params![cutoff], |row| {
            let hostname: String = row.get(1)?;
            let last_seen_at: String = row.get(6)?;
            let degraded: i64 = row.get(7)?;
            Ok(AtsHostSummary {
                instance_id: row.get(0)?,
                hostname: hostname.clone(),
                display_label: display_label(&hostname, degraded != 0),
                ats_version: row.get(2)?,
                ats_app: row.get(3)?,
                last_event_type: row.get(4)?,
                last_event_at: row.get(5)?,
                last_seen_at: last_seen_at.clone(),
                is_connected: last_seen_at >= connected_cutoff,
                is_active: last_seen_at >= cutoff,
                presence_category: presence_category(
                    &last_seen_at,
                    &connected_cutoff,
                    &recent_cutoff,
                ),
                activity_count_ttl: row.get::<_, i64>(8)?.max(0) as u32,
                jobs_count_ttl: row.get::<_, i64>(9)?.max(0) as u32,
                degraded_identity: degraded != 0,
            })
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    pub fn get_host_details(
        &self,
        instance_id: &str,
        ttl_minutes: u32,
        limit: u32,
    ) -> Result<Option<AtsHostDetails>, AtsPresenceError> {
        let id = instance_id.trim();
        if id.is_empty() {
            return Ok(None);
        }
        let ttl = clamp_ttl(ttl_minutes);
        let details_limit = clamp_details_limit(limit);
        let cutoff = ttl_cutoff(ttl);
        let connected_cutoff = connected_cutoff();
        let recent_cutoff = recent_window_cutoff();
        let conn = self.connect()?;
        let host: Option<(String, String, String, String, String, String, String, i64)> = conn
            .query_row(
                "SELECT display_hostname, ats_version, ats_app, first_seen_at, last_seen_at, last_event_type, last_event_at, degraded_identity
                 FROM ats_hosts WHERE instance_id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .optional()?;
        let Some((hostname, ats_version, ats_app, first_seen_at, last_seen_at, last_event_type, last_event_at, degraded)) = host else {
            return Ok(None);
        };

        let mut event_stmt = conn.prepare(
            "SELECT occurred_at, event_type, route, method, status_code_class, correlation_id, folder_name, payload_json
             FROM ats_activity
             WHERE instance_id = ?1 AND occurred_at >= ?2
             ORDER BY occurred_at DESC
             LIMIT ?3",
        )?;
        let event_rows = event_stmt.query_map(params![id, cutoff, details_limit], |row| {
            Ok(AtsActivityEntry {
                occurred_at: row.get(0)?,
                event_type: row.get(1)?,
                route: row.get(2)?,
                method: row.get(3)?,
                status_code_class: row.get(4)?,
                correlation_id: row.get(5)?,
                folder_name: row.get(6)?,
                payload_json: row.get(7)?,
            })
        })?;
        let mut recent_events = Vec::new();
        for row in event_rows {
            recent_events.push(row?);
        }

        let recent_jobs = self.load_jobs_for_host(&conn, id, &cutoff, 0, details_limit)?;
        Ok(Some(AtsHostDetails {
            instance_id: id.to_string(),
            hostname: hostname.clone(),
            display_label: display_label(&hostname, degraded != 0),
            ats_version,
            ats_app,
            first_seen_at,
            last_seen_at: last_seen_at.clone(),
            last_event_type,
            last_event_at,
            is_connected: last_seen_at >= connected_cutoff,
            is_active: last_seen_at >= cutoff,
            presence_category: presence_category(
                &last_seen_at,
                &connected_cutoff,
                &recent_cutoff,
            ),
            degraded_identity: degraded != 0,
            activity_window_minutes: ttl,
            recent_events,
            recent_jobs,
        }))
    }

    pub fn get_jobs_by_host(
        &self,
        instance_id: &str,
        ttl_minutes: u32,
        page: u32,
        page_size: u32,
    ) -> Result<AtsJobsPage, AtsPresenceError> {
        let id = instance_id.trim();
        if id.is_empty() {
            return Ok(AtsJobsPage {
                items: Vec::new(),
                total: 0,
                page: 0,
                page_size: clamp_jobs_page_size(page_size),
            });
        }
        let ttl = clamp_ttl(ttl_minutes);
        let cutoff = ttl_cutoff(ttl);
        let page_size = clamp_jobs_page_size(page_size);
        let conn = self.connect()?;
        let total = conn.query_row(
            "SELECT COUNT(*) FROM ats_job_origin WHERE instance_id = ?1 AND last_seen_at >= ?2",
            params![id, cutoff],
            |row| row.get::<_, i64>(0),
        )?;
        let total = total.max(0) as u32;
        let max_page = if total == 0 { 0 } else { (total - 1) / page_size };
        let page = page.min(max_page);
        let offset = page.saturating_mul(page_size);
        let items = self.load_jobs_for_host(&conn, id, &cutoff, offset, page_size)?;
        Ok(AtsJobsPage {
            items,
            total,
            page,
            page_size,
        })
    }

    fn load_jobs_for_host(
        &self,
        conn: &Connection,
        instance_id: &str,
        cutoff: &str,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<AtsJobOriginRecord>, AtsPresenceError> {
        let mut stmt = conn.prepare(
            "SELECT correlation_id, folder_name, first_seen_at, last_seen_at, source_event_type
             FROM ats_job_origin
             WHERE instance_id = ?1 AND last_seen_at >= ?2
             ORDER BY last_seen_at DESC
             LIMIT ?3 OFFSET ?4",
        )?;
        let rows = stmt.query_map(params![instance_id, cutoff, limit, offset], |row| {
            Ok(AtsJobOriginRecord {
                correlation_id: row.get(0)?,
                folder_name: row.get(1)?,
                first_seen_at: row.get(2)?,
                last_seen_at: row.get(3)?,
                source_event_type: row.get(4)?,
            })
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }
}

fn clamp_ttl(ttl_minutes: u32) -> u32 {
    ttl_minutes.clamp(1, MAX_TTL_MINUTES).max(DEFAULT_TTL_MINUTES.min(ttl_minutes.max(1)))
}

fn clamp_details_limit(limit: u32) -> u32 {
    if limit == 0 {
        DEFAULT_DETAILS_LIMIT
    } else {
        limit.clamp(1, MAX_DETAILS_LIMIT)
    }
}

fn clamp_jobs_page_size(page_size: u32) -> u32 {
    if page_size == 0 {
        DEFAULT_JOBS_PAGE_SIZE
    } else {
        page_size.clamp(1, MAX_JOBS_PAGE_SIZE)
    }
}

fn ttl_cutoff(ttl_minutes: u32) -> String {
    (Utc::now() - Duration::minutes(clamp_ttl(ttl_minutes) as i64)).to_rfc3339()
}

fn connected_cutoff() -> String {
    (Utc::now() - Duration::seconds(CONNECTED_TTL_SECONDS)).to_rfc3339()
}

fn recent_window_cutoff() -> String {
    (Utc::now() - Duration::days(RECENT_WINDOW_DAYS)).to_rfc3339()
}

fn presence_category(
    last_seen_at: &str,
    connected_cutoff: &str,
    recent_cutoff: &str,
) -> AtsPresenceCategory {
    if last_seen_at >= connected_cutoff {
        AtsPresenceCategory::Connected
    } else if last_seen_at >= recent_cutoff {
        AtsPresenceCategory::Disconnected
    } else {
        AtsPresenceCategory::InactiveLong
    }
}

fn display_label(hostname: &str, degraded_identity: bool) -> String {
    if degraded_identity {
        format!("{} (degradiert)", hostname.trim())
    } else {
        hostname.trim().to_string()
    }
}

pub fn clamp_activity_payload_json(value: serde_json::Value) -> String {
    let raw = value.to_string();
    if raw.chars().count() <= MAX_ACTIVITY_PAYLOAD_CHARS {
        return raw;
    }
    raw.chars().take(MAX_ACTIVITY_PAYLOAD_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_tmp() -> (tempfile::TempDir, AtsPresenceStore) {
        let dir = tempdir().unwrap();
        let store = AtsPresenceStore::open_at(dir.path().join(ATS_PRESENCE_DB_FILE)).unwrap();
        (dir, store)
    }

    fn host(id: &str, degraded: bool) -> AtsIdentityInput {
        AtsIdentityInput {
            instance_id: id.into(),
            hostname: if degraded {
                "Unbekannter ATS Host".into()
            } else {
                "ATS-Workstation".into()
            },
            ats_version: "2.3.4".into(),
            ats_app: "AeroTandemStudio".into(),
            degraded_identity: degraded,
        }
    }

    fn event(kind: &str, cid: &str, folder: &str) -> AtsActivityInput {
        AtsActivityInput {
            event_type: kind.into(),
            route: format!("/v1/{kind}"),
            method: "POST".into(),
            status_code_class: "2xx".into(),
            correlation_id: cid.into(),
            folder_name: folder.into(),
            payload_json: String::new(),
        }
    }

    #[test]
    fn records_summary_and_job_origin() {
        let (_dir, mut store) = open_tmp();
        store
            .record_event(
                &host("11111111-2222-3333-4444-555555555555", false),
                &event("handoff_ready", "cid-1", "Flug_01"),
            )
            .unwrap();
        let summary = store.get_hosts_summary(60).unwrap();
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0].activity_count_ttl, 1);
        assert_eq!(summary[0].jobs_count_ttl, 1);
        assert!(summary[0].is_active);
        assert!(summary[0].is_connected);
        assert_eq!(summary[0].presence_category, AtsPresenceCategory::Connected);
        let details = store
            .get_host_details("11111111-2222-3333-4444-555555555555", 60, 20)
            .unwrap()
            .unwrap();
        assert_eq!(details.recent_events.len(), 1);
        assert_eq!(details.recent_jobs.len(), 1);
        assert_eq!(details.recent_jobs[0].correlation_id, "cid-1");
        assert_eq!(details.recent_jobs[0].folder_name, "Flug_01");
    }

    #[test]
    fn stores_activity_payload_json() {
        let (_dir, mut store) = open_tmp();
        let mut activity = event("customer_lookup", "", "");
        activity.payload_json = r#"{"request":{"customer_id":"c1"}}"#.into();
        store
            .record_event(
                &host("11111111-2222-3333-4444-555555555555", false),
                &activity,
            )
            .unwrap();
        let details = store
            .get_host_details("11111111-2222-3333-4444-555555555555", 60, 20)
            .unwrap()
            .unwrap();
        assert_eq!(details.recent_events[0].payload_json, activity.payload_json);
    }

    #[test]
    fn degraded_identity_is_preserved() {
        let (_dir, mut store) = open_tmp();
        store
            .record_event(
                &host("unknown:missing-instance-id", true),
                &event("health", "", ""),
            )
            .unwrap();
        let summary = store.get_hosts_summary(60).unwrap();
        assert_eq!(summary[0].display_label, "Unbekannter ATS Host (degradiert)");
        assert!(summary[0].degraded_identity);
    }

    #[test]
    fn connected_requires_recent_bridge_activity() {
        let (_dir, store) = open_tmp();
        let conn = store.connect().unwrap();
        let now = Utc::now();
        let recent = now.to_rfc3339();
        let stale = (now - Duration::minutes(5)).to_rfc3339();
        conn.execute(
            "INSERT INTO ats_hosts (
                instance_id, display_hostname, ats_version, ats_app,
                first_seen_at, last_seen_at, last_event_at, last_event_type,
                last_route, degraded_identity, created_at, updated_at
             ) VALUES (?1, ?2, '', '', ?3, ?4, ?4, 'health', '/v1/health', 0, ?3, ?4)",
            params!["connected-host", "ATS-Online", &recent, &recent],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ats_hosts (
                instance_id, display_hostname, ats_version, ats_app,
                first_seen_at, last_seen_at, last_event_at, last_event_type,
                last_route, degraded_identity, created_at, updated_at
             ) VALUES (?1, ?2, '', '', ?3, ?4, ?4, 'health', '/v1/health', 0, ?3, ?4)",
            params!["recent-offline-host", "ATS-Offline", &stale, &stale],
        )
        .unwrap();
        let summary = store.get_hosts_summary(60).unwrap();
        assert_eq!(summary.len(), 2);
        let online = summary
            .iter()
            .find(|h| h.instance_id == "connected-host")
            .unwrap();
        let offline = summary
            .iter()
            .find(|h| h.instance_id == "recent-offline-host")
            .unwrap();
        assert!(online.is_connected);
        assert!(online.is_active);
        assert_eq!(online.presence_category, AtsPresenceCategory::Connected);
        assert!(!offline.is_connected);
        assert!(offline.is_active);
        assert_eq!(offline.presence_category, AtsPresenceCategory::Disconnected);
    }

    #[test]
    fn presence_categories_cover_recent_and_long_inactive() {
        let (_dir, store) = open_tmp();
        let conn = store.connect().unwrap();
        let recent = (Utc::now() - Duration::days(2)).to_rfc3339();
        let long_inactive = (Utc::now() - Duration::days(40)).to_rfc3339();
        conn.execute(
            "INSERT INTO ats_hosts (
                instance_id, display_hostname, ats_version, ats_app,
                first_seen_at, last_seen_at, last_event_at, last_event_type,
                last_route, degraded_identity, created_at, updated_at
             ) VALUES (?1, ?2, '', '', ?3, ?3, ?3, 'health', '/v1/health', 0, ?3, ?3)",
            params!["recent-host", "ATS-Recent", &recent],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ats_hosts (
                instance_id, display_hostname, ats_version, ats_app,
                first_seen_at, last_seen_at, last_event_at, last_event_type,
                last_route, degraded_identity, created_at, updated_at
             ) VALUES (?1, ?2, '', '', ?3, ?3, ?3, 'health', '/v1/health', 0, ?3, ?3)",
            params!["old-host", "ATS-Old", &long_inactive],
        )
        .unwrap();
        let summary = store.get_hosts_summary(60).unwrap();
        assert_eq!(summary.len(), 2);
        let recent_host = summary.iter().find(|h| h.instance_id == "recent-host").unwrap();
        let old_host = summary.iter().find(|h| h.instance_id == "old-host").unwrap();
        assert_eq!(recent_host.presence_category, AtsPresenceCategory::Disconnected);
        assert!(!recent_host.is_active);
        assert_eq!(old_host.presence_category, AtsPresenceCategory::InactiveLong);
        assert!(!old_host.is_active);
    }

    #[test]
    fn handoff_ready_keeps_job_origin_over_job_status() {
        let (_dir, mut store) = open_tmp();
        let identity_a = host("inst-a", false);
        let identity_b = AtsIdentityInput {
            instance_id: "inst-b".into(),
            hostname: "ATS-B".into(),
            ats_version: "2.3.4".into(),
            ats_app: "AeroTandemStudio".into(),
            degraded_identity: false,
        };
        store
            .record_event(&identity_a, &event("handoff_ready", "cid-2", "Flug_02"))
            .unwrap();
        store
            .record_event(&identity_b, &event("job_status", "cid-2", ""))
            .unwrap();
        let jobs = store.get_jobs_by_host("inst-a", 60, 0, 50).unwrap();
        assert_eq!(jobs.total, 1);
        assert_eq!(jobs.items[0].source_event_type, "handoff_ready");
        assert!(store.get_jobs_by_host("inst-b", 60, 0, 50).unwrap().items.is_empty());
    }
}
