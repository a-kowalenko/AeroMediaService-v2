//! SMB server session diagnostics + Safe-Close (Phase 20).
//!
//! Windows: list/close server sessions via Win32 `NetSessionEnum` /
//! `NetSessionDel` (same fields as CIM `MSFT_SmbSession`; no PowerShell spawn).
//! 20c: focus-share via `NetShareEnum`/`NetConnectionEnum`, ATS presence hint,
//! optional Auto-Close (default off).
//! Other OS: unsupported stub.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::storage::logging::{log_info, log_warn};
use crate::util::smb_export::{local_paths_match, ShareExport};

pub const DEFAULT_WARN_THRESHOLD: u32 = 8;
pub const DEFAULT_POLL_SECONDS: u32 = 30;
pub const DEFAULT_IDLE_MIN_SECONDS: u64 = 600;
pub const DEFAULT_AUTO_CLOSE_ENABLED: bool = false;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SmbSessionQueryStatus {
    Ok,
    Unsupported,
    PermissionDenied,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SmbSessionCloseOutcome {
    Closed,
    SkippedUnsafe,
    SkippedOffFocus,
    NotFound,
    PermissionDenied,
    Unsupported,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmbSessionCloseMode {
    Manual,
    Auto,
    /// Close executed inside elevated `ams-smb-helper` (Phase 20d).
    ElevatedHelper,
}

impl SmbSessionCloseMode {
    fn as_audit_label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
            Self::ElevatedHelper => "elevated-helper",
        }
    }
}

/// Lightweight ATS presence identity for SMB client correlation (hostname / label).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtsHostRef {
    pub hostname: String,
    pub display_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmbSessionRow {
    /// Composite key `{client}|{user}|{idx}` (JS-safe; NetAPI has no CIM SessionId).
    pub session_id: String,
    pub client_computer_name: String,
    pub client_user_name: String,
    pub seconds_idle: u64,
    pub seconds_exists: u64,
    pub num_opens: u32,
    /// Connected to monitor / `aktuell` focus share (when share filter known).
    pub on_focus_share: bool,
    /// Matched ATS presence host label, if any (diagnose only — never auto-kill).
    pub ats_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmbSessionSnapshot {
    pub status: SmbSessionQueryStatus,
    pub message: String,
    pub session_count: usize,
    pub warn_threshold: u32,
    pub poll_seconds: u32,
    pub idle_min_seconds: u64,
    pub auto_close_enabled: bool,
    /// Resolved local share name for `monitor_path` / `aktuell`, when known.
    pub focus_share_name: Option<String>,
    pub focus_session_count: usize,
    /// Chip warning: SMB session overload only (`ok` + count ≥ threshold).
    pub warn: bool,
    pub sessions: Vec<SmbSessionRow>,
    pub platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmbSessionCloseDetail {
    pub session_id: String,
    pub client_computer_name: String,
    pub client_user_name: String,
    pub seconds_idle: u64,
    pub num_opens: u32,
    pub outcome: SmbSessionCloseOutcome,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmbSessionCloseReport {
    pub closed: usize,
    pub skipped: usize,
    pub failed: usize,
    pub permission_denied: bool,
    pub message: String,
    pub details: Vec<SmbSessionCloseDetail>,
}

/// Parse warn threshold from settings (default 8; minimum 1).
pub fn parse_warn_threshold(raw: &str) -> u32 {
    raw.trim()
        .parse::<u32>()
        .unwrap_or(DEFAULT_WARN_THRESHOLD)
        .max(1)
}

/// Parse poll interval from settings (default 30; clamp 5..=300).
pub fn parse_poll_seconds(raw: &str) -> u32 {
    raw.trim()
        .parse::<u32>()
        .unwrap_or(DEFAULT_POLL_SECONDS)
        .clamp(5, 300)
}

/// Parse idle minimum for Safe-Close (default 600; clamp 60..=86400).
pub fn parse_idle_min_seconds(raw: &str) -> u64 {
    raw.trim()
        .parse::<u64>()
        .unwrap_or(DEFAULT_IDLE_MIN_SECONDS)
        .clamp(60, 86_400)
}

/// Parse Auto-Close toggle (default false).
pub fn parse_auto_close_enabled(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return DEFAULT_AUTO_CLOSE_ENABLED;
    }
    matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Normalize SMB client computer token (`\\HOST`, `HOST.`, IP) for matching.
pub fn normalize_smb_client_token(raw: &str) -> String {
    let mut s = raw.trim().trim_matches(|c| c == '\\' || c == '/').to_string();
    while s.ends_with('.') && s.len() > 1 {
        s.pop();
    }
    s.to_ascii_lowercase()
}

/// Pick focus share: path match to `monitor_path`, else share named `aktuell`.
pub fn pick_focus_share_name(exports: &[ShareExport], monitor_path: &str) -> Option<String> {
    let monitor = monitor_path.trim();
    if !monitor.is_empty() {
        if let Some(export) = exports
            .iter()
            .find(|e| local_paths_match(&e.local_path, monitor))
        {
            let name = export.share_name.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    exports.iter().find_map(|e| {
        let name = e.share_name.trim();
        if name.eq_ignore_ascii_case("aktuell") {
            Some(name.to_string())
        } else {
            None
        }
    })
}

/// Correlate SMB client name with known ATS hosts (hostname / display label).
/// Diagnose only — never a kill gate.
pub fn match_ats_host_hint(client_computer_name: &str, hosts: &[AtsHostRef]) -> Option<String> {
    let client = normalize_smb_client_token(client_computer_name);
    if client.is_empty() {
        return None;
    }
    for host in hosts {
        let hostname = normalize_smb_client_token(&host.hostname);
        let label = normalize_smb_client_token(&host.display_label);
        if (!hostname.is_empty() && hostname == client)
            || (!label.is_empty() && label == client)
        {
            let hint = host.display_label.trim();
            if !hint.is_empty() {
                return Some(hint.to_string());
            }
            let host_hint = host.hostname.trim();
            if !host_hint.is_empty() {
                return Some(host_hint.to_string());
            }
            return Some("ATS?".into());
        }
    }
    None
}

/// Annotate rows with focus-share + ATS hints; sort focus sessions first.
pub fn enrich_sessions(
    mut sessions: Vec<SmbSessionRow>,
    focus_clients: &HashSet<String>,
    focus_share_known: bool,
    ats_hosts: &[AtsHostRef],
) -> Vec<SmbSessionRow> {
    for row in &mut sessions {
        let client_key = normalize_smb_client_token(&row.client_computer_name);
        row.on_focus_share = if focus_share_known {
            !client_key.is_empty() && focus_clients.contains(&client_key)
        } else {
            false
        };
        row.ats_hint = match_ats_host_hint(&row.client_computer_name, ats_hosts);
    }
    sessions.sort_by(|a, b| {
        b.on_focus_share
            .cmp(&a.on_focus_share)
            .then_with(|| b.seconds_idle.cmp(&a.seconds_idle))
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    sessions
}

/// Safe-Close predicate: idle long enough and no open file handles.
pub fn is_safe_to_close(row: &SmbSessionRow, idle_min_seconds: u64) -> bool {
    row.num_opens == 0 && row.seconds_idle >= idle_min_seconds
}

/// Bulk/Auto close: Safe-Close + focus-share when the share filter is known.
pub fn is_bulk_close_candidate(
    row: &SmbSessionRow,
    idle_min_seconds: u64,
    focus_share_known: bool,
) -> bool {
    if !is_safe_to_close(row, idle_min_seconds) {
        return false;
    }
    if focus_share_known {
        row.on_focus_share
    } else {
        true
    }
}

/// Sessions that pass the Safe-Close (+ optional focus) filter.
pub fn filter_safe_close_candidates(
    sessions: &[SmbSessionRow],
    idle_min_seconds: u64,
    focus_share_known: bool,
) -> Vec<&SmbSessionRow> {
    sessions
        .iter()
        .filter(|row| is_bulk_close_candidate(row, idle_min_seconds, focus_share_known))
        .collect()
}

/// Whether the Clients chip should show a warning tone (overload only).
///
/// `permission_denied` / `error` / `unsupported` are surfaced in the dialog
/// (message + Elevate CTA), not as a permanent header warning.
pub fn should_warn(
    status: &SmbSessionQueryStatus,
    session_count: usize,
    warn_threshold: u32,
) -> bool {
    match status {
        SmbSessionQueryStatus::Ok => session_count >= warn_threshold as usize,
        SmbSessionQueryStatus::Unsupported
        | SmbSessionQueryStatus::PermissionDenied
        | SmbSessionQueryStatus::Error => false,
    }
}

pub fn current_platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

/// Build a snapshot from live OS query + config values.
pub fn collect_snapshot(
    warn_threshold: u32,
    poll_seconds: u32,
    idle_min_seconds: u64,
    auto_close_enabled: bool,
    monitor_path: &str,
    ats_hosts: &[AtsHostRef],
) -> SmbSessionSnapshot {
    let warn_threshold = warn_threshold.max(1);
    let poll_seconds = poll_seconds.clamp(5, 300);
    let idle_min_seconds = idle_min_seconds.clamp(60, 86_400);
    let platform = current_platform().to_string();
    let focus_share_name = resolve_focus_share_name(monitor_path);
    let focus_share_known = focus_share_name.is_some();
    let focus_clients = focus_share_name
        .as_deref()
        .map(list_focus_share_clients)
        .unwrap_or_default();

    match query_sessions() {
        Ok(raw) => {
            let sessions = enrich_sessions(raw, &focus_clients, focus_share_known, ats_hosts);
            let session_count = sessions.len();
            let focus_session_count = sessions.iter().filter(|s| s.on_focus_share).count();
            let status = SmbSessionQueryStatus::Ok;
            let warn = should_warn(&status, session_count, warn_threshold);
            let safe_count =
                filter_safe_close_candidates(&sessions, idle_min_seconds, focus_share_known).len();
            let message = build_ok_message(
                session_count,
                focus_session_count,
                safe_count,
                focus_share_name.as_deref(),
            );
            SmbSessionSnapshot {
                status,
                message,
                session_count,
                warn_threshold,
                poll_seconds,
                idle_min_seconds,
                auto_close_enabled,
                focus_share_name,
                focus_session_count,
                warn,
                sessions,
                platform,
            }
        }
        Err(QueryError::Unsupported) => empty_snapshot(
            SmbSessionQueryStatus::Unsupported,
            "SMB-Session-Diagnose ist nur unter Windows verfügbar.".into(),
            warn_threshold,
            poll_seconds,
            idle_min_seconds,
            auto_close_enabled,
            focus_share_name,
            false,
            platform,
        ),
        Err(QueryError::PermissionDenied(detail)) => {
            let status = SmbSessionQueryStatus::PermissionDenied;
            empty_snapshot(
                status.clone(),
                format!(
                    "SMB-Sessions konnten nicht gelesen werden (Admin-Rechte nötig). {detail}"
                ),
                warn_threshold,
                poll_seconds,
                idle_min_seconds,
                auto_close_enabled,
                focus_share_name,
                should_warn(&status, 0, warn_threshold),
                platform,
            )
        }
        Err(QueryError::Other(detail)) => {
            let status = SmbSessionQueryStatus::Error;
            empty_snapshot(
                status.clone(),
                format!("SMB-Session-Abfrage fehlgeschlagen: {detail}"),
                warn_threshold,
                poll_seconds,
                idle_min_seconds,
                auto_close_enabled,
                focus_share_name,
                should_warn(&status, 0, warn_threshold),
                platform,
            )
        }
    }
}

fn build_ok_message(
    session_count: usize,
    focus_session_count: usize,
    safe_count: usize,
    focus_share: Option<&str>,
) -> String {
    if session_count == 0 {
        return "Keine SMB-Server-Sessions.".to_string();
    }
    match focus_share {
        Some(share) => format!(
            "{session_count} SMB-Server-Session(s), {focus_session_count} auf Freigabe „{share}“, davon {safe_count} Safe-Close-Kandidat(en)."
        ),
        None => format!(
            "{session_count} SMB-Server-Session(s), davon {safe_count} Safe-Close-Kandidat(en) (Share-Filter nicht auflösbar)."
        ),
    }
}

fn empty_snapshot(
    status: SmbSessionQueryStatus,
    message: String,
    warn_threshold: u32,
    poll_seconds: u32,
    idle_min_seconds: u64,
    auto_close_enabled: bool,
    focus_share_name: Option<String>,
    warn: bool,
    platform: String,
) -> SmbSessionSnapshot {
    SmbSessionSnapshot {
        status,
        message,
        session_count: 0,
        warn_threshold,
        poll_seconds,
        idle_min_seconds,
        auto_close_enabled,
        focus_share_name,
        focus_session_count: 0,
        warn,
        sessions: vec![],
        platform,
    }
}

/// Close one session by id — only if it still matches Safe-Close rules.
/// Manual single close is not blocked by share focus (operator intent).
pub fn close_session_by_id(
    session_id: &str,
    idle_min_seconds: u64,
    mode: SmbSessionCloseMode,
) -> SmbSessionCloseReport {
    let idle_min_seconds = idle_min_seconds.clamp(60, 86_400);
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return SmbSessionCloseReport {
            closed: 0,
            skipped: 0,
            failed: 1,
            permission_denied: false,
            message: "Keine Session-ID angegeben.".into(),
            details: vec![],
        };
    }

    match query_sessions() {
        Ok(sessions) => {
            let Some(row) = find_session_row(&sessions, session_id) else {
                let detail = SmbSessionCloseDetail {
                    session_id: session_id.to_string(),
                    client_computer_name: String::new(),
                    client_user_name: String::new(),
                    seconds_idle: 0,
                    num_opens: 0,
                    outcome: SmbSessionCloseOutcome::NotFound,
                    message: "Session nicht mehr gefunden (bereits getrennt?).".into(),
                };
                audit_close(&detail, mode, idle_min_seconds);
                return summarize_details(vec![detail], idle_min_seconds, None);
            };
            let detail = close_one_checked(row, idle_min_seconds, mode, false);
            summarize_details(vec![detail], idle_min_seconds, None)
        }
        Err(QueryError::Unsupported) => unsupported_close_report(),
        Err(QueryError::PermissionDenied(detail)) => {
            permission_denied_close_report(&detail, session_id, mode)
        }
        Err(QueryError::Other(detail)) => query_error_close_report(&detail),
    }
}

/// Close every session that currently passes Safe-Close (+ focus when known).
pub fn close_all_safe_idle(
    idle_min_seconds: u64,
    mode: SmbSessionCloseMode,
    monitor_path: &str,
) -> SmbSessionCloseReport {
    let idle_min_seconds = idle_min_seconds.clamp(60, 86_400);
    let focus_share_name = resolve_focus_share_name(monitor_path);
    let focus_share_known = focus_share_name.is_some();
    let focus_clients = focus_share_name
        .as_deref()
        .map(list_focus_share_clients)
        .unwrap_or_default();

    match query_sessions() {
        Ok(raw) => {
            let sessions = enrich_sessions(raw, &focus_clients, focus_share_known, &[]);
            let candidates: Vec<SmbSessionRow> =
                filter_safe_close_candidates(&sessions, idle_min_seconds, focus_share_known)
                    .into_iter()
                    .cloned()
                    .collect();
            if candidates.is_empty() {
                let share_note = focus_share_name
                    .as_deref()
                    .map(|s| format!(" auf Freigabe „{s}“"))
                    .unwrap_or_default();
                return SmbSessionCloseReport {
                    closed: 0,
                    skipped: 0,
                    failed: 0,
                    permission_denied: false,
                    message: format!(
                        "Keine Safe-Close-Kandidaten{share_note} (Idle ≥ {idle_min_seconds}s und Opens = 0)."
                    ),
                    details: vec![],
                };
            }
            let details: Vec<_> = candidates
                .iter()
                .map(|row| close_one_checked(row, idle_min_seconds, mode, focus_share_known))
                .collect();
            summarize_details(details, idle_min_seconds, focus_share_name.as_deref())
        }
        Err(QueryError::Unsupported) => unsupported_close_report(),
        Err(QueryError::PermissionDenied(detail)) => {
            permission_denied_close_report(&detail, "", mode)
        }
        Err(QueryError::Other(detail)) => query_error_close_report(&detail),
    }
}

/// Auto-Close tick: no-op unless enabled; only Safe-Close + focus-share candidates.
///
/// Phase 20d: never elevates. If NetAPI returns permission denied, returns that
/// report (UI/chip: Admin nötig) — no silent UAC spam.
pub fn run_auto_close_if_enabled(
    enabled: bool,
    idle_min_seconds: u64,
    monitor_path: &str,
) -> Option<SmbSessionCloseReport> {
    if !enabled {
        return None;
    }
    Some(close_all_safe_idle(
        idle_min_seconds,
        SmbSessionCloseMode::Auto,
        monitor_path,
    ))
}

fn close_one_checked(
    row: &SmbSessionRow,
    idle_min_seconds: u64,
    mode: SmbSessionCloseMode,
    enforce_focus: bool,
) -> SmbSessionCloseDetail {
    if enforce_focus && !row.on_focus_share {
        let detail = SmbSessionCloseDetail {
            session_id: row.session_id.clone(),
            client_computer_name: row.client_computer_name.clone(),
            client_user_name: row.client_user_name.clone(),
            seconds_idle: row.seconds_idle,
            num_opens: row.num_opens,
            outcome: SmbSessionCloseOutcome::SkippedOffFocus,
            message: "Nicht geschlossen: Session liegt nicht auf der Monitor-/aktuell-Freigabe."
                .into(),
        };
        audit_close(&detail, mode, idle_min_seconds);
        return detail;
    }

    if !is_safe_to_close(row, idle_min_seconds) {
        let detail = SmbSessionCloseDetail {
            session_id: row.session_id.clone(),
            client_computer_name: row.client_computer_name.clone(),
            client_user_name: row.client_user_name.clone(),
            seconds_idle: row.seconds_idle,
            num_opens: row.num_opens,
            outcome: SmbSessionCloseOutcome::SkippedUnsafe,
            message: format!(
                "Nicht geschlossen: Idle {}s / Opens {} (braucht Idle ≥ {idle_min_seconds}s und Opens = 0).",
                row.seconds_idle, row.num_opens
            ),
        };
        audit_close(&detail, mode, idle_min_seconds);
        return detail;
    }

    let detail = match delete_session(&row.client_computer_name, &row.client_user_name) {
        Ok(()) => SmbSessionCloseDetail {
            session_id: row.session_id.clone(),
            client_computer_name: row.client_computer_name.clone(),
            client_user_name: row.client_user_name.clone(),
            seconds_idle: row.seconds_idle,
            num_opens: row.num_opens,
            outcome: SmbSessionCloseOutcome::Closed,
            message: "Session geschlossen.".into(),
        },
        Err(DeleteError::AlreadyGone) => SmbSessionCloseDetail {
            session_id: row.session_id.clone(),
            client_computer_name: row.client_computer_name.clone(),
            client_user_name: row.client_user_name.clone(),
            seconds_idle: row.seconds_idle,
            num_opens: row.num_opens,
            outcome: SmbSessionCloseOutcome::Closed,
            message: "Session war bereits getrennt.".into(),
        },
        Err(DeleteError::PermissionDenied(msg)) => SmbSessionCloseDetail {
            session_id: row.session_id.clone(),
            client_computer_name: row.client_computer_name.clone(),
            client_user_name: row.client_user_name.clone(),
            seconds_idle: row.seconds_idle,
            num_opens: row.num_opens,
            outcome: SmbSessionCloseOutcome::PermissionDenied,
            message: format!("Admin-Rechte nötig zum Schließen. {msg}"),
        },
        Err(DeleteError::Unsupported) => SmbSessionCloseDetail {
            session_id: row.session_id.clone(),
            client_computer_name: row.client_computer_name.clone(),
            client_user_name: row.client_user_name.clone(),
            seconds_idle: row.seconds_idle,
            num_opens: row.num_opens,
            outcome: SmbSessionCloseOutcome::Unsupported,
            message: "SMB-Session-Close ist nur unter Windows verfügbar.".into(),
        },
        Err(DeleteError::Other(msg)) => SmbSessionCloseDetail {
            session_id: row.session_id.clone(),
            client_computer_name: row.client_computer_name.clone(),
            client_user_name: row.client_user_name.clone(),
            seconds_idle: row.seconds_idle,
            num_opens: row.num_opens,
            outcome: SmbSessionCloseOutcome::Error,
            message: msg,
        },
    };
    audit_close(&detail, mode, idle_min_seconds);
    detail
}

fn audit_close(detail: &SmbSessionCloseDetail, mode: SmbSessionCloseMode, idle_min_seconds: u64) {
    let line = format!(
        "SMB-Session-Close [{}] id={} client={} user={} idle={}s opens={} idle_min={}s → {:?}: {}",
        mode.as_audit_label(),
        detail.session_id,
        detail.client_computer_name,
        detail.client_user_name,
        detail.seconds_idle,
        detail.num_opens,
        idle_min_seconds,
        detail.outcome,
        detail.message
    );
    match detail.outcome {
        SmbSessionCloseOutcome::Closed => log_info(&line),
        SmbSessionCloseOutcome::SkippedUnsafe
        | SmbSessionCloseOutcome::SkippedOffFocus
        | SmbSessionCloseOutcome::NotFound => log_info(&line),
        SmbSessionCloseOutcome::PermissionDenied
        | SmbSessionCloseOutcome::Unsupported
        | SmbSessionCloseOutcome::Error => log_warn(&line),
    }
}

fn summarize_details(
    details: Vec<SmbSessionCloseDetail>,
    idle_min_seconds: u64,
    focus_share: Option<&str>,
) -> SmbSessionCloseReport {
    let mut closed = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut permission_denied = false;
    for d in &details {
        match d.outcome {
            SmbSessionCloseOutcome::Closed => closed += 1,
            SmbSessionCloseOutcome::SkippedUnsafe
            | SmbSessionCloseOutcome::SkippedOffFocus
            | SmbSessionCloseOutcome::NotFound => skipped += 1,
            SmbSessionCloseOutcome::PermissionDenied => {
                failed += 1;
                permission_denied = true;
            }
            SmbSessionCloseOutcome::Unsupported | SmbSessionCloseOutcome::Error => failed += 1,
        }
    }
    let share_note = focus_share
        .map(|s| format!(" (Fokus: „{s}“)"))
        .unwrap_or_default();
    let message = if permission_denied {
        "Schließen fehlgeschlagen: Administratorrechte erforderlich. AMS muss nicht dauerhaft als Admin laufen — Close ggf. mit erhöhten Rechten erneut versuchen.".into()
    } else if closed > 0 && failed == 0 {
        format!(
            "{closed} Idle-Session(s) geschlossen (Idle ≥ {idle_min_seconds}s, Opens = 0){share_note}."
        )
    } else if closed == 0 && failed == 0 && skipped > 0 {
        format!(
            "Keine Session geschlossen ({skipped} übersprungen; Kriterien: Idle ≥ {idle_min_seconds}s und Opens = 0){share_note}."
        )
    } else if details.is_empty() {
        format!(
            "Keine Safe-Close-Kandidaten (Idle ≥ {idle_min_seconds}s und Opens = 0){share_note}."
        )
    } else {
        format!("{closed} geschlossen, {skipped} übersprungen, {failed} fehlgeschlagen{share_note}.")
    };
    SmbSessionCloseReport {
        closed,
        skipped,
        failed,
        permission_denied,
        message,
        details,
    }
}

fn unsupported_close_report() -> SmbSessionCloseReport {
    SmbSessionCloseReport {
        closed: 0,
        skipped: 0,
        failed: 1,
        permission_denied: false,
        message: "SMB-Session-Close ist nur unter Windows verfügbar.".into(),
        details: vec![SmbSessionCloseDetail {
            session_id: String::new(),
            client_computer_name: String::new(),
            client_user_name: String::new(),
            seconds_idle: 0,
            num_opens: 0,
            outcome: SmbSessionCloseOutcome::Unsupported,
            message: "Nicht unterstützt auf dieser Plattform.".into(),
        }],
    }
}

fn permission_denied_close_report(
    detail: &str,
    session_id: &str,
    mode: SmbSessionCloseMode,
) -> SmbSessionCloseReport {
    let msg = format!(
        "Admin-Rechte nötig. SMB-Sessions konnten nicht gelesen/geschlossen werden. {detail}"
    );
    log_warn(&format!(
        "SMB-Session-Close [{}] permission_denied session_id={session_id}: {msg}",
        mode.as_audit_label()
    ));
    SmbSessionCloseReport {
        closed: 0,
        skipped: 0,
        failed: 1,
        permission_denied: true,
        message: "Schließen fehlgeschlagen: Administratorrechte erforderlich. AMS muss nicht dauerhaft als Admin laufen — Close ggf. mit erhöhten Rechten erneut versuchen.".into(),
        details: vec![SmbSessionCloseDetail {
            session_id: session_id.to_string(),
            client_computer_name: String::new(),
            client_user_name: String::new(),
            seconds_idle: 0,
            num_opens: 0,
            outcome: SmbSessionCloseOutcome::PermissionDenied,
            message: msg,
        }],
    }
}

fn query_error_close_report(detail: &str) -> SmbSessionCloseReport {
    let msg = format!("SMB-Session-Abfrage vor Close fehlgeschlagen: {detail}");
    log_warn(&msg);
    SmbSessionCloseReport {
        closed: 0,
        skipped: 0,
        failed: 1,
        permission_denied: false,
        message: msg.clone(),
        details: vec![SmbSessionCloseDetail {
            session_id: String::new(),
            client_computer_name: String::new(),
            client_user_name: String::new(),
            seconds_idle: 0,
            num_opens: 0,
            outcome: SmbSessionCloseOutcome::Error,
            message: msg,
        }],
    }
}

/// Prefer exact id; fall back to client|user when the index suffix drifted.
fn find_session_row<'a>(sessions: &'a [SmbSessionRow], session_id: &str) -> Option<&'a SmbSessionRow> {
    if let Some(row) = sessions.iter().find(|s| s.session_id == session_id) {
        return Some(row);
    }
    let (client, user) = parse_composite_client_user(session_id)?;
    sessions.iter().find(|s| {
        s.client_computer_name == client && s.client_user_name == user
    })
}

fn parse_composite_client_user(session_id: &str) -> Option<(String, String)> {
    let mut parts = session_id.rsplitn(3, '|');
    let _idx = parts.next()?;
    let user = parts.next()?.to_string();
    let client = parts.next()?.to_string();
    Some((client, user))
}

fn resolve_focus_share_name(monitor_path: &str) -> Option<String> {
    let exports = list_local_share_exports();
    pick_focus_share_name(&exports, monitor_path)
}

fn list_focus_share_clients(share_name: &str) -> HashSet<String> {
    #[cfg(windows)]
    {
        list_focus_share_clients_windows(share_name)
    }
    #[cfg(not(windows))]
    {
        let _ = share_name;
        HashSet::new()
    }
}

fn list_local_share_exports() -> Vec<ShareExport> {
    #[cfg(windows)]
    {
        list_local_share_exports_windows()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[derive(Debug)]
enum QueryError {
    #[allow(dead_code)] // returned only on non-Windows builds
    Unsupported,
    PermissionDenied(String),
    Other(String),
}

#[derive(Debug)]
enum DeleteError {
    #[allow(dead_code)] // returned only on non-Windows builds
    Unsupported,
    PermissionDenied(String),
    AlreadyGone,
    Other(String),
}

fn query_sessions() -> Result<Vec<SmbSessionRow>, QueryError> {
    #[cfg(windows)]
    {
        query_sessions_windows()
    }
    #[cfg(not(windows))]
    {
        Err(QueryError::Unsupported)
    }
}

fn delete_session(client: &str, user: &str) -> Result<(), DeleteError> {
    #[cfg(windows)]
    {
        delete_session_windows(client, user)
    }
    #[cfg(not(windows))]
    {
        let _ = (client, user);
        Err(DeleteError::Unsupported)
    }
}

#[cfg(windows)]
fn query_sessions_windows() -> Result<Vec<SmbSessionRow>, QueryError> {
    use std::slice;
    use windows::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_MORE_DATA, ERROR_SUCCESS, WIN32_ERROR,
    };
    use windows::Win32::NetworkManagement::NetManagement::{
        NetApiBufferFree, MAX_PREFERRED_LENGTH,
    };
    use windows::Win32::Storage::FileSystem::{NetSessionEnum, SESSION_INFO_2};

    let mut buf_ptr: *mut u8 = std::ptr::null_mut();
    let mut entries_read: u32 = 0;
    let mut total_entries: u32 = 0;
    let mut resume_handle: u32 = 0;

    // Level 2: client, user, num_opens, time (exists), idle_time — mirrors MSFT_SmbSession diagnostics.
    let status = unsafe {
        NetSessionEnum(
            None,
            None,
            None,
            2,
            &mut buf_ptr,
            MAX_PREFERRED_LENGTH,
            &mut entries_read,
            &mut total_entries,
            Some(&mut resume_handle),
        )
    };

    let status = WIN32_ERROR(status);
    if status == ERROR_ACCESS_DENIED {
        return Err(QueryError::PermissionDenied(
            "NetSessionEnum: Zugriff verweigert".into(),
        ));
    }
    if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
        return Err(QueryError::Other(format!(
            "NetSessionEnum fehlgeschlagen (Win32 {})",
            status.0
        )));
    }

    let mut out = Vec::new();
    if !buf_ptr.is_null() && entries_read > 0 {
        let rows = unsafe {
            slice::from_raw_parts(buf_ptr as *const SESSION_INFO_2, entries_read as usize)
        };
        for (idx, row) in rows.iter().enumerate() {
            let client = pwstr_to_string(row.sesi2_cname);
            let user = pwstr_to_string(row.sesi2_username);
            // NetAPI has no CIM SessionId; stable composite key for UI / close.
            let session_id = format!("{client}|{user}|{idx}");
            out.push(SmbSessionRow {
                session_id,
                client_computer_name: client,
                client_user_name: user,
                seconds_idle: u64::from(row.sesi2_idle_time),
                seconds_exists: u64::from(row.sesi2_time),
                num_opens: row.sesi2_num_opens,
                on_focus_share: false,
                ats_hint: None,
            });
        }
    }
    if !buf_ptr.is_null() {
        unsafe {
            let _ = NetApiBufferFree(Some(buf_ptr as *const _));
        }
    }

    out.sort_by(|a, b| {
        b.seconds_idle
            .cmp(&a.seconds_idle)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    Ok(out)
}

#[cfg(windows)]
fn delete_session_windows(client: &str, user: &str) -> Result<(), DeleteError> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SUCCESS, WIN32_ERROR};
    use windows::Win32::Storage::FileSystem::NetSessionDel;

    // NERR_ClientNameNotFound / NERR_UserNotFound — session already gone.
    const NERR_CLIENT_NAME_NOT_FOUND: u32 = 2312;
    const NERR_USER_NOT_FOUND: u32 = 2221;

    let client_hs = HSTRING::from(client);
    let user_hs = HSTRING::from(user);
    let status = unsafe { NetSessionDel(None, &client_hs, &user_hs) };
    let status = WIN32_ERROR(status);

    if status == ERROR_SUCCESS {
        return Ok(());
    }
    if status == ERROR_ACCESS_DENIED {
        return Err(DeleteError::PermissionDenied(
            "NetSessionDel: Zugriff verweigert".into(),
        ));
    }
    if status.0 == NERR_CLIENT_NAME_NOT_FOUND || status.0 == NERR_USER_NOT_FOUND {
        return Err(DeleteError::AlreadyGone);
    }
    Err(DeleteError::Other(format!(
        "NetSessionDel fehlgeschlagen (Win32 {})",
        status.0
    )))
}

#[cfg(windows)]
fn list_local_share_exports_windows() -> Vec<ShareExport> {
    use std::slice;
    use windows::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_MORE_DATA, ERROR_SUCCESS, WIN32_ERROR,
    };
    use windows::Win32::NetworkManagement::NetManagement::{
        NetApiBufferFree, MAX_PREFERRED_LENGTH,
    };
    use windows::Win32::Storage::FileSystem::{NetShareEnum, SHARE_INFO_2};

    let mut buf_ptr: *mut u8 = std::ptr::null_mut();
    let mut entries_read: u32 = 0;
    let mut total_entries: u32 = 0;
    let mut resume_handle: u32 = 0;

    let status = unsafe {
        NetShareEnum(
            None,
            2,
            &mut buf_ptr,
            MAX_PREFERRED_LENGTH,
            &mut entries_read,
            &mut total_entries,
            Some(&mut resume_handle),
        )
    };
    let status = WIN32_ERROR(status);
    if status == ERROR_ACCESS_DENIED {
        return Vec::new();
    }
    if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
        return Vec::new();
    }

    let mut out = Vec::new();
    if !buf_ptr.is_null() && entries_read > 0 {
        let rows = unsafe {
            slice::from_raw_parts(buf_ptr as *const SHARE_INFO_2, entries_read as usize)
        };
        for row in rows {
            let name = pwstr_to_string(row.shi2_netname);
            let path = pwstr_to_string(row.shi2_path);
            if name.is_empty() || path.is_empty() || name.ends_with('$') {
                continue;
            }
            out.push(ShareExport {
                share_name: name,
                local_path: path,
            });
        }
    }
    if !buf_ptr.is_null() {
        unsafe {
            let _ = NetApiBufferFree(Some(buf_ptr as *const _));
        }
    }
    out
}

#[cfg(windows)]
fn list_focus_share_clients_windows(share_name: &str) -> HashSet<String> {
    use std::slice;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_MORE_DATA, ERROR_SUCCESS, WIN32_ERROR,
    };
    use windows::Win32::NetworkManagement::NetManagement::{
        NetApiBufferFree, MAX_PREFERRED_LENGTH,
    };
    use windows::Win32::Storage::FileSystem::{NetConnectionEnum, CONNECTION_INFO_1};

    let share = share_name.trim();
    if share.is_empty() {
        return HashSet::new();
    }

    let mut buf_ptr: *mut u8 = std::ptr::null_mut();
    let mut entries_read: u32 = 0;
    let mut total_entries: u32 = 0;
    let mut resume_handle: u32 = 0;
    let share_hs = HSTRING::from(share);

    let status = unsafe {
        NetConnectionEnum(
            None,
            &share_hs,
            1,
            &mut buf_ptr,
            MAX_PREFERRED_LENGTH,
            &mut entries_read,
            &mut total_entries,
            Some(&mut resume_handle),
        )
    };
    let status = WIN32_ERROR(status);
    if status == ERROR_ACCESS_DENIED {
        return HashSet::new();
    }
    if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
        return HashSet::new();
    }

    let mut out = HashSet::new();
    if !buf_ptr.is_null() && entries_read > 0 {
        let rows = unsafe {
            slice::from_raw_parts(buf_ptr as *const CONNECTION_INFO_1, entries_read as usize)
        };
        for row in rows {
            let client = pwstr_to_string(row.coni1_netname);
            let key = normalize_smb_client_token(&client);
            if !key.is_empty() {
                out.insert(key);
            }
        }
    }
    if !buf_ptr.is_null() {
        unsafe {
            let _ = NetApiBufferFree(Some(buf_ptr as *const _));
        }
    }
    out
}

#[cfg(windows)]
fn pwstr_to_string(ptr: windows::core::PWSTR) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { ptr.to_string().unwrap_or_default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(idle: u64, opens: u32) -> SmbSessionRow {
        SmbSessionRow {
            session_id: format!("\\\\PC|user|{idle}-{opens}"),
            client_computer_name: "\\\\PC".into(),
            client_user_name: "user".into(),
            seconds_idle: idle,
            seconds_exists: idle + 10,
            num_opens: opens,
            on_focus_share: false,
            ats_hint: None,
        }
    }

    fn row_client(client: &str, idle: u64, opens: u32, on_focus: bool) -> SmbSessionRow {
        SmbSessionRow {
            session_id: format!("{client}|user|{idle}-{opens}"),
            client_computer_name: client.into(),
            client_user_name: "user".into(),
            seconds_idle: idle,
            seconds_exists: idle + 10,
            num_opens: opens,
            on_focus_share: on_focus,
            ats_hint: None,
        }
    }

    #[test]
    fn parse_threshold_and_poll_defaults() {
        assert_eq!(parse_warn_threshold(""), DEFAULT_WARN_THRESHOLD);
        assert_eq!(parse_warn_threshold("8"), 8);
        assert_eq!(parse_warn_threshold("0"), 1);
        assert_eq!(parse_warn_threshold("nope"), DEFAULT_WARN_THRESHOLD);
        assert_eq!(parse_poll_seconds(""), DEFAULT_POLL_SECONDS);
        assert_eq!(parse_poll_seconds("30"), 30);
        assert_eq!(parse_poll_seconds("1"), 5);
        assert_eq!(parse_poll_seconds("999"), 300);
    }

    #[test]
    fn parse_idle_min_defaults_and_bounds() {
        assert_eq!(parse_idle_min_seconds(""), DEFAULT_IDLE_MIN_SECONDS);
        assert_eq!(parse_idle_min_seconds("600"), 600);
        assert_eq!(parse_idle_min_seconds("10"), 60);
        assert_eq!(parse_idle_min_seconds("999999"), 86_400);
        assert_eq!(parse_idle_min_seconds("x"), DEFAULT_IDLE_MIN_SECONDS);
    }

    #[test]
    fn parse_auto_close_defaults_off() {
        assert!(!parse_auto_close_enabled(""));
        assert!(!parse_auto_close_enabled("false"));
        assert!(!parse_auto_close_enabled("0"));
        assert!(!parse_auto_close_enabled("nope"));
        assert!(parse_auto_close_enabled("true"));
        assert!(parse_auto_close_enabled("1"));
        assert!(parse_auto_close_enabled("ON"));
        assert_eq!(DEFAULT_AUTO_CLOSE_ENABLED, false);
    }

    #[test]
    fn safe_close_requires_idle_and_zero_opens() {
        assert!(is_safe_to_close(&row(600, 0), 600));
        assert!(is_safe_to_close(&row(601, 0), 600));
        assert!(!is_safe_to_close(&row(599, 0), 600));
        assert!(!is_safe_to_close(&row(600, 1), 600));
        assert!(!is_safe_to_close(&row(10_000, 2), 600));
        assert!(!is_safe_to_close(&row(0, 0), 600));
    }

    #[test]
    fn filter_rejects_unsafe_sessions() {
        let sessions = vec![row(700, 0), row(800, 1), row(100, 0), row(900, 0)];
        let safe = filter_safe_close_candidates(&sessions, 600, false);
        assert_eq!(safe.len(), 2);
        assert_eq!(safe[0].seconds_idle, 700);
        assert_eq!(safe[1].seconds_idle, 900);
    }

    #[test]
    fn bulk_close_respects_focus_share_when_known() {
        let sessions = vec![
            row_client(r"\\ATS-A", 700, 0, true),
            row_client(r"\\OTHER", 800, 0, false),
            row_client(r"\\ATS-B", 900, 1, true),
        ];
        let safe = filter_safe_close_candidates(&sessions, 600, true);
        assert_eq!(safe.len(), 1);
        assert_eq!(safe[0].client_computer_name, r"\\ATS-A");
        assert!(!is_bulk_close_candidate(&sessions[1], 600, true));
        assert!(is_bulk_close_candidate(&sessions[1], 600, false));
    }

    #[test]
    fn auto_close_disabled_returns_none() {
        assert!(run_auto_close_if_enabled(false, 600, "").is_none());
    }

    #[test]
    fn warn_when_at_or_above_threshold() {
        assert!(!should_warn(&SmbSessionQueryStatus::Ok, 7, 8));
        assert!(should_warn(&SmbSessionQueryStatus::Ok, 8, 8));
        assert!(should_warn(&SmbSessionQueryStatus::Ok, 12, 8));
    }

    #[test]
    fn no_warn_on_permission_denied() {
        assert!(!should_warn(
            &SmbSessionQueryStatus::PermissionDenied,
            0,
            8
        ));
        assert!(!should_warn(
            &SmbSessionQueryStatus::PermissionDenied,
            100,
            8
        ));
    }

    #[test]
    fn no_warn_when_unsupported() {
        assert!(!should_warn(&SmbSessionQueryStatus::Unsupported, 100, 8));
    }

    #[test]
    fn no_warn_on_error_status() {
        assert!(!should_warn(&SmbSessionQueryStatus::Error, 0, 8));
        assert!(!should_warn(&SmbSessionQueryStatus::Error, 8, 8));
    }

    #[test]
    fn normalize_client_strips_unc_and_case() {
        assert_eq!(normalize_smb_client_token(r"\\Studio-PC"), "studio-pc");
        assert_eq!(normalize_smb_client_token("Studio-PC."), "studio-pc");
        assert_eq!(normalize_smb_client_token("192.168.1.10"), "192.168.1.10");
    }

    #[test]
    fn pick_focus_prefers_monitor_path_then_aktuell() {
        let exports = vec![
            ShareExport {
                share_name: "other".into(),
                local_path: r"D:\Other".into(),
            },
            ShareExport {
                share_name: "aktuell".into(),
                local_path: r"D:\Shares\aktuell".into(),
            },
        ];
        assert_eq!(
            pick_focus_share_name(&exports, r"D:\Shares\aktuell"),
            Some("aktuell".into())
        );
        assert_eq!(
            pick_focus_share_name(&exports, r"D:\Elsewhere"),
            Some("aktuell".into())
        );
        assert_eq!(
            pick_focus_share_name(
                &[ShareExport {
                    share_name: "media".into(),
                    local_path: r"E:\Media".into(),
                }],
                r"E:\Media"
            ),
            Some("media".into())
        );
    }

    #[test]
    fn ats_hint_matches_hostname_or_label() {
        let hosts = vec![
            AtsHostRef {
                hostname: "Studio-PC".into(),
                display_label: "Studio PC".into(),
            },
            AtsHostRef {
                hostname: "backup".into(),
                display_label: "Backup".into(),
            },
        ];
        assert_eq!(
            match_ats_host_hint(r"\\Studio-PC", &hosts).as_deref(),
            Some("Studio PC")
        );
        assert_eq!(
            match_ats_host_hint("backup", &hosts).as_deref(),
            Some("Backup")
        );
        assert!(match_ats_host_hint(r"\\Unknown", &hosts).is_none());
    }

    #[test]
    fn enrich_marks_focus_and_ats_and_sorts() {
        let mut focus = HashSet::new();
        focus.insert("ats-a".into());
        let hosts = vec![AtsHostRef {
            hostname: "ATS-A".into(),
            display_label: "ATS Alpha".into(),
        }];
        let sessions = enrich_sessions(
            vec![
                row_client(r"\\OTHER", 900, 0, false),
                row_client(r"\\ATS-A", 100, 0, false),
            ],
            &focus,
            true,
            &hosts,
        );
        assert!(sessions[0].on_focus_share);
        assert_eq!(sessions[0].ats_hint.as_deref(), Some("ATS Alpha"));
        assert!(!sessions[1].on_focus_share);
        assert!(sessions[1].ats_hint.is_none());
    }

    #[test]
    fn unsupported_snapshot_on_non_windows() {
        if cfg!(windows) {
            return;
        }
        let snap = collect_snapshot(8, 30, 600, false, "", &[]);
        assert_eq!(snap.status, SmbSessionQueryStatus::Unsupported);
        assert!(!snap.warn);
        assert!(snap.sessions.is_empty());
        assert_eq!(snap.warn_threshold, 8);
        assert_eq!(snap.poll_seconds, 30);
        assert_eq!(snap.idle_min_seconds, 600);
        assert!(!snap.auto_close_enabled);
    }

    #[test]
    fn snapshot_normalizes_config_bounds() {
        let snap = collect_snapshot(0, 1, 10, true, "", &[]);
        assert_eq!(snap.warn_threshold, 1);
        assert_eq!(snap.poll_seconds, 5);
        assert_eq!(snap.idle_min_seconds, 60);
        assert!(snap.auto_close_enabled);
    }

    #[test]
    fn close_on_non_windows_is_unsupported() {
        if cfg!(windows) {
            return;
        }
        let report = close_all_safe_idle(600, SmbSessionCloseMode::Manual, "");
        assert_eq!(report.closed, 0);
        assert_eq!(report.failed, 1);
        assert!(!report.permission_denied);
        assert!(report.message.contains("Windows"));
    }

    #[test]
    fn parse_composite_client_user_splits_tail_index() {
        let (client, user) =
            parse_composite_client_user(r"\\HOST|alice|3").expect("parse");
        assert_eq!(client, r"\\HOST");
        assert_eq!(user, "alice");
    }
}
