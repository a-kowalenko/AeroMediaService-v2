//! Tauri IPC for SMB server session diagnostics + Safe-Close (Phase 20 / 20d).

use tauri::State;

use crate::commands::ConfigState;
use crate::storage::ats_presence::AtsPresenceState;
use crate::storage::logging::log_warn;
use crate::util::smb_elevate::{
    elevated_close_by_id, elevated_close_safe_idle, elevated_list_snapshot, ElevateError,
};
use crate::util::smb_sessions::{
    close_all_safe_idle, close_session_by_id, collect_snapshot, parse_auto_close_enabled,
    parse_idle_min_seconds, parse_poll_seconds, parse_warn_threshold, run_auto_close_if_enabled,
    should_warn, AtsHostRef, SmbSessionCloseMode, SmbSessionCloseOutcome, SmbSessionCloseReport,
    SmbSessionQueryStatus, SmbSessionSnapshot,
};

fn read_idle_min(config: &ConfigState) -> u64 {
    parse_idle_min_seconds(
        &config
            .get("smb_session_idle_min_seconds", Some("600"))
            .unwrap_or_else(|_| "600".into()),
    )
}

fn read_auto_close(config: &ConfigState) -> bool {
    parse_auto_close_enabled(
        &config
            .get("smb_session_auto_close_enabled", Some("false"))
            .unwrap_or_else(|_| "false".into()),
    )
}

fn read_monitor_path(config: &ConfigState) -> String {
    config
        .get("monitor_path", Some(""))
        .unwrap_or_default()
}

fn ats_host_refs(presence: &AtsPresenceState) -> Vec<AtsHostRef> {
    presence
        .get_hosts_summary(60)
        .unwrap_or_default()
        .into_iter()
        .map(|h| AtsHostRef {
            hostname: h.hostname,
            display_label: h.display_label,
        })
        .collect()
}

fn map_elevate_err_to_snapshot(err: ElevateError, base: SmbSessionSnapshot) -> SmbSessionSnapshot {
    let message = err.user_message();
    log_warn(&format!("SMB elevated list failed: {message}"));
    let status = match err {
        ElevateError::Unsupported => SmbSessionQueryStatus::Unsupported,
        ElevateError::Cancelled => SmbSessionQueryStatus::PermissionDenied,
        ElevateError::HelperMissing(_)
        | ElevateError::TimedOut
        | ElevateError::Failed(_)
        | ElevateError::BadJson(_) => SmbSessionQueryStatus::Error,
    };
    // Elevate failure is not overload — keep chip warn off (dialog shows status).
    let warn = should_warn(&status, 0, base.warn_threshold);
    SmbSessionSnapshot {
        status,
        message,
        session_count: 0,
        warn_threshold: base.warn_threshold,
        poll_seconds: base.poll_seconds,
        idle_min_seconds: base.idle_min_seconds,
        auto_close_enabled: base.auto_close_enabled,
        focus_share_name: base.focus_share_name,
        focus_session_count: 0,
        warn,
        sessions: vec![],
        platform: base.platform,
    }
}

fn map_elevate_err_to_close(err: ElevateError) -> SmbSessionCloseReport {
    let message = err.user_message();
    log_warn(&format!("SMB elevated close failed: {message}"));
    let permission_denied = matches!(
        err,
        ElevateError::Cancelled | ElevateError::Failed(_)
    ) && (message.contains("Admin")
        || message.contains("UAC")
        || message.contains("abgebrochen")
        || message.contains("verweigert"));
    let outcome = match err {
        ElevateError::Unsupported => SmbSessionCloseOutcome::Unsupported,
        ElevateError::Cancelled => SmbSessionCloseOutcome::PermissionDenied,
        _ if permission_denied => SmbSessionCloseOutcome::PermissionDenied,
        _ => SmbSessionCloseOutcome::Error,
    };
    SmbSessionCloseReport {
        closed: 0,
        skipped: 0,
        failed: 1,
        permission_denied: matches!(
            outcome,
            SmbSessionCloseOutcome::PermissionDenied
        ) || matches!(err, ElevateError::Cancelled),
        message,
        details: vec![],
    }
}

/// Snapshot of local SMB server sessions (Windows) for Clients chip + dialog.
#[tauri::command]
pub fn get_smb_session_snapshot(
    config: State<'_, ConfigState>,
    presence: State<'_, AtsPresenceState>,
) -> SmbSessionSnapshot {
    let warn_threshold = parse_warn_threshold(
        &config
            .inner()
            .get("smb_session_warn_threshold", Some("8"))
            .unwrap_or_else(|_| "8".into()),
    );
    let poll_seconds = parse_poll_seconds(
        &config
            .inner()
            .get("smb_session_poll_seconds", Some("30"))
            .unwrap_or_else(|_| "30".into()),
    );
    let idle_min_seconds = read_idle_min(config.inner());
    let auto_close_enabled = read_auto_close(config.inner());
    let monitor_path = read_monitor_path(config.inner());
    let hosts = ats_host_refs(presence.inner());
    // NetAPI is sync; keep the command sync so Tauri does not need spawn_blocking.
    collect_snapshot(
        warn_threshold,
        poll_seconds,
        idle_min_seconds,
        auto_close_enabled,
        &monitor_path,
        &hosts,
    )
}

/// Elevated List via `ams-smb-helper` + UAC (Phase 20d). AMS stays unelevated.
#[tauri::command]
pub fn get_smb_session_snapshot_elevated(
    config: State<'_, ConfigState>,
    presence: State<'_, AtsPresenceState>,
) -> SmbSessionSnapshot {
    let warn_threshold = parse_warn_threshold(
        &config
            .inner()
            .get("smb_session_warn_threshold", Some("8"))
            .unwrap_or_else(|_| "8".into()),
    );
    let poll_seconds = parse_poll_seconds(
        &config
            .inner()
            .get("smb_session_poll_seconds", Some("30"))
            .unwrap_or_else(|_| "30".into()),
    );
    let idle_min_seconds = read_idle_min(config.inner());
    let auto_close_enabled = read_auto_close(config.inner());
    let monitor_path = read_monitor_path(config.inner());
    let hosts = ats_host_refs(presence.inner());
    let base = collect_snapshot(
        warn_threshold,
        poll_seconds,
        idle_min_seconds,
        auto_close_enabled,
        &monitor_path,
        &hosts,
    );
    // If unelevated already works, skip UAC.
    if base.status == SmbSessionQueryStatus::Ok {
        return base;
    }
    match elevated_list_snapshot(
        warn_threshold,
        poll_seconds,
        idle_min_seconds,
        auto_close_enabled,
        &monitor_path,
        &hosts,
    ) {
        Ok(snap) => snap,
        Err(err) => map_elevate_err_to_snapshot(err, base),
    }
}

/// Close one session by id — only if Safe-Close rules still hold.
#[tauri::command]
pub fn close_smb_session(
    config: State<'_, ConfigState>,
    session_id: String,
) -> SmbSessionCloseReport {
    let idle_min_seconds = read_idle_min(config.inner());
    close_session_by_id(&session_id, idle_min_seconds, SmbSessionCloseMode::Manual)
}

/// Elevated Safe-Close one session (UAC). AMS stays unelevated.
#[tauri::command]
pub fn close_smb_session_elevated(
    config: State<'_, ConfigState>,
    session_id: String,
) -> SmbSessionCloseReport {
    let idle_min_seconds = read_idle_min(config.inner());
    // Try in-process first when already permitted.
    let in_process =
        close_session_by_id(&session_id, idle_min_seconds, SmbSessionCloseMode::Manual);
    if !in_process.permission_denied
        && in_process
            .details
            .iter()
            .all(|d| d.outcome != SmbSessionCloseOutcome::PermissionDenied)
    {
        return in_process;
    }
    match elevated_close_by_id(&session_id, idle_min_seconds) {
        Ok(report) => report,
        Err(err) => map_elevate_err_to_close(err),
    }
}

/// Close all sessions that currently pass Idle ≥ min, NumOpens == 0, and focus-share (when known).
#[tauri::command]
pub fn close_safe_idle_smb_sessions(config: State<'_, ConfigState>) -> SmbSessionCloseReport {
    let idle_min_seconds = read_idle_min(config.inner());
    let monitor_path = read_monitor_path(config.inner());
    close_all_safe_idle(
        idle_min_seconds,
        SmbSessionCloseMode::Manual,
        &monitor_path,
    )
}

/// Elevated bulk Safe-Close (UAC). AMS stays unelevated.
#[tauri::command]
pub fn close_safe_idle_smb_sessions_elevated(
    config: State<'_, ConfigState>,
) -> SmbSessionCloseReport {
    let idle_min_seconds = read_idle_min(config.inner());
    let monitor_path = read_monitor_path(config.inner());
    let in_process = close_all_safe_idle(
        idle_min_seconds,
        SmbSessionCloseMode::Manual,
        &monitor_path,
    );
    if !in_process.permission_denied {
        return in_process;
    }
    match elevated_close_safe_idle(idle_min_seconds, &monitor_path) {
        Ok(report) => report,
        Err(err) => map_elevate_err_to_close(err),
    }
}

/// Optional Auto-Close tick (no-op unless `smb_session_auto_close_enabled`); Safe + focus only.
/// Never triggers UAC (Phase 20d) — permission denied is reported, not elevated.
#[tauri::command]
pub fn auto_close_safe_idle_smb_sessions(
    config: State<'_, ConfigState>,
) -> Option<SmbSessionCloseReport> {
    let enabled = read_auto_close(config.inner());
    let idle_min_seconds = read_idle_min(config.inner());
    let monitor_path = read_monitor_path(config.inner());
    run_auto_close_if_enabled(enabled, idle_min_seconds, &monitor_path)
}
