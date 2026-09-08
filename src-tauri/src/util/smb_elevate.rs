//! Spawn elevated `ams-smb-helper` via UAC (`runas`) — Phase 20d.
//!
//! AMS stays unelevated. No PowerShell. IPC = JSON temp file under `%TEMP%`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::storage::logging::{log_info, log_warn};
use crate::util::smb_helper::{build_helper_params, validate_out_path};
use crate::util::smb_sessions::{
    enrich_sessions, SmbSessionCloseReport, SmbSessionQueryStatus, SmbSessionSnapshot, AtsHostRef,
};

const HELPER_EXE_NAME: &str = if cfg!(windows) {
    "ams-smb-helper.exe"
} else {
    "ams-smb-helper"
};

const HELPER_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevateError {
    /// Non-Windows builds only.
    #[allow(dead_code)]
    Unsupported,
    HelperMissing(String),
    Cancelled,
    TimedOut,
    Failed(String),
    BadJson(String),
}

impl ElevateError {
    pub fn user_message(&self) -> String {
        match self {
            Self::Unsupported => {
                "Elevierter SMB-Helper ist nur unter Windows verfügbar.".into()
            }
            Self::HelperMissing(path) => {
                format!(
                    "SMB-Helper nicht gefunden ({path}). Bitte App neu bauen/installieren."
                )
            }
            Self::Cancelled => {
                "Administrator-Zustimmung abgebrochen (UAC). Keine Änderung.".into()
            }
            Self::TimedOut => {
                "SMB-Helper hat nicht rechtzeitig geantwortet (Timeout).".into()
            }
            Self::Failed(detail) => detail.clone(),
            Self::BadJson(detail) => {
                format!("Antwort des SMB-Helpers ungültig: {detail}")
            }
        }
    }
}

/// Resolve helper next to the AMS executable (dev + bundled externalBin).
pub fn resolve_helper_path() -> Result<PathBuf, ElevateError> {
    let exe = std::env::current_exe().map_err(|e| {
        ElevateError::Failed(format!("current_exe: {e}"))
    })?;
    let dir = exe
        .parent()
        .ok_or_else(|| ElevateError::Failed("current_exe ohne Verzeichnis".into()))?;
    let candidate = dir.join(HELPER_EXE_NAME);
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(ElevateError::HelperMissing(candidate.display().to_string()))
}

fn create_out_file() -> Result<PathBuf, ElevateError> {
    let name = format!(
        "ams_smb_helper_{}_{}.json",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    );
    let path = std::env::temp_dir().join(name);
    validate_out_path(&path).map_err(ElevateError::Failed)?;
    // Touch empty file so ACL/parent exist; helper overwrites.
    fs::write(&path, b"{}").map_err(|e| ElevateError::Failed(format!("temp create: {e}")))?;
    Ok(path)
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, ElevateError> {
    let raw = fs::read_to_string(path).map_err(|e| {
        ElevateError::BadJson(format!("lesen {}: {e}", path.display()))
    })?;
    // Prefer typed parse; surface helper error envelope if present.
    if let Ok(err) = serde_json::from_str::<serde_json::Value>(&raw) {
        if let Some(msg) = err.get("error").and_then(|v| v.as_str()) {
            let code = err
                .get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("helper_error");
            return Err(ElevateError::Failed(format!("{code}: {msg}")));
        }
    }
    serde_json::from_str(&raw).map_err(|e| ElevateError::BadJson(e.to_string()))
}

/// Run elevated helper and deserialize JSON result.
pub fn run_elevated_json<T: DeserializeOwned>(
    command: &str,
    pairs: &[(&str, &str)],
) -> Result<T, ElevateError> {
    #[cfg(not(windows))]
    {
        let _ = (command, pairs);
        return Err(ElevateError::Unsupported);
    }
    #[cfg(windows)]
    {
        run_elevated_json_windows(command, pairs)
    }
}

#[cfg(windows)]
fn run_elevated_json_windows<T: DeserializeOwned>(
    command: &str,
    pairs: &[(&str, &str)],
) -> Result<T, ElevateError> {
    let helper = resolve_helper_path()?;
    let out = create_out_file()?;
    let out_str = out.to_string_lossy().to_string();

    let mut all_pairs: Vec<(&str, &str)> = Vec::with_capacity(pairs.len() + 1);
    all_pairs.push(("--out", out_str.as_str()));
    all_pairs.extend_from_slice(pairs);

    let params = build_helper_params(command, &all_pairs).map_err(ElevateError::Failed)?;

    log_info(&format!(
        "SMB-Helper elevate: cmd={command} helper={} out={}",
        helper.display(),
        out.display()
    ));

    let exit = spawn_runas_and_wait(&helper, &params)?;
    if exit != 0 {
        // Try to surface JSON error from out file before generic failure.
        if let Ok(err_val) = read_json_file::<serde_json::Value>(&out) {
            if let Some(msg) = err_val.get("error").and_then(|v| v.as_str()) {
                let _ = fs::remove_file(&out);
                return Err(ElevateError::Failed(format!(
                    "Helper exit {exit}: {msg}"
                )));
            }
        }
        let _ = fs::remove_file(&out);
        return Err(ElevateError::Failed(format!(
            "SMB-Helper beendete mit Exit-Code {exit}"
        )));
    }

    let result = read_json_file::<T>(&out);
    let _ = fs::remove_file(&out);
    result
}

#[cfg(windows)]
fn spawn_runas_and_wait(helper: &Path, params: &str) -> Result<u32, ElevateError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_CANCELLED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, TerminateProcess, WaitForSingleObject,
    };
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    fn to_wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let file_w = to_wide(&helper.to_string_lossy());
    let params_w = to_wide(params);
    let verb_w = to_wide("runas");
    let dir_w = helper
        .parent()
        .map(|p| to_wide(&p.to_string_lossy()))
        .unwrap_or_else(|| to_wide(""));

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: Default::default(),
        lpVerb: PCWSTR(verb_w.as_ptr()),
        lpFile: PCWSTR(file_w.as_ptr()),
        lpParameters: PCWSTR(params_w.as_ptr()),
        lpDirectory: if dir_w.len() > 1 {
            PCWSTR(dir_w.as_ptr())
        } else {
            PCWSTR::null()
        },
        nShow: SW_HIDE.0 as i32,
        ..Default::default()
    };

    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok.is_err() {
        let err = unsafe { GetLastError() };
        if err == ERROR_CANCELLED {
            log_warn("SMB-Helper: UAC abgebrochen");
            return Err(ElevateError::Cancelled);
        }
        return Err(ElevateError::Failed(format!(
            "ShellExecuteEx(runas) fehlgeschlagen (Win32 {})",
            err.0
        )));
    }

    let handle = info.hProcess;
    if handle.is_invalid() {
        return Err(ElevateError::Failed(
            "ShellExecuteEx lieferte kein Prozess-Handle".into(),
        ));
    }

    let timeout_ms = HELPER_TIMEOUT.as_millis().min(u32::MAX as u128) as u32;
    let wait = unsafe { WaitForSingleObject(handle, timeout_ms) };
    if wait == WAIT_TIMEOUT {
        unsafe {
            let _ = TerminateProcess(handle, 1);
            let _ = CloseHandle(handle);
        }
        return Err(ElevateError::TimedOut);
    }
    if wait != WAIT_OBJECT_0 {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Err(ElevateError::Failed(format!(
            "WaitForSingleObject fehlgeschlagen ({})",
            wait.0
        )));
    }

    let mut exit_code: u32 = 1;
    let got = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
    unsafe {
        let _ = CloseHandle(handle);
    }
    if got.is_err() {
        return Err(ElevateError::Failed(format!(
            "GetExitCodeProcess fehlgeschlagen (Win32 {})",
            unsafe { GetLastError() }.0
        )));
    }
    Ok(exit_code)
}

/// Elevated list; re-apply ATS hints in the unelevated parent.
pub fn elevated_list_snapshot(
    warn_threshold: u32,
    poll_seconds: u32,
    idle_min_seconds: u64,
    auto_close_enabled: bool,
    monitor_path: &str,
    ats_hosts: &[AtsHostRef],
) -> Result<SmbSessionSnapshot, ElevateError> {
    let warn_s = warn_threshold.to_string();
    let poll_s = poll_seconds.to_string();
    let idle_s = idle_min_seconds.to_string();
    let auto_s = if auto_close_enabled { "true" } else { "false" };
    let mut snap: SmbSessionSnapshot = run_elevated_json(
        "list",
        &[
            ("--idle-min", idle_s.as_str()),
            ("--warn-threshold", warn_s.as_str()),
            ("--poll-seconds", poll_s.as_str()),
            ("--auto-close", auto_s),
            ("--monitor-path", monitor_path),
        ],
    )?;

    // Re-apply ATS presence hints (helper has no presence DB).
    if !ats_hosts.is_empty() && snap.status == SmbSessionQueryStatus::Ok {
        let focus_clients: std::collections::HashSet<String> = snap
            .sessions
            .iter()
            .filter(|s| s.on_focus_share)
            .map(|s| {
                crate::util::smb_sessions::normalize_smb_client_token(&s.client_computer_name)
            })
            .filter(|s| !s.is_empty())
            .collect();
        let focus_known = snap.focus_share_name.is_some();
        snap.sessions = enrich_sessions(snap.sessions, &focus_clients, focus_known, ats_hosts);
        snap.focus_session_count = snap.sessions.iter().filter(|s| s.on_focus_share).count();
    }

    if snap.message.trim().is_empty() {
        snap.message = "SMB-Sessions mit Admin-Rechten geladen.".into();
    } else if !snap.message.contains("Admin") {
        snap.message = format!("{} (via Admin-Helper)", snap.message);
    }

    log_info(&format!(
        "SMB-Helper list ok: status={:?} count={} exit logged in parent",
        snap.status, snap.session_count
    ));
    Ok(snap)
}

pub fn elevated_close_by_id(
    session_id: &str,
    idle_min_seconds: u64,
) -> Result<SmbSessionCloseReport, ElevateError> {
    let idle_s = idle_min_seconds.to_string();
    let report: SmbSessionCloseReport = run_elevated_json(
        "close-id",
        &[
            ("--session-id", session_id),
            ("--idle-min", idle_s.as_str()),
        ],
    )?;
    audit_elevated_report(&report);
    Ok(report)
}

pub fn elevated_close_safe_idle(
    idle_min_seconds: u64,
    monitor_path: &str,
) -> Result<SmbSessionCloseReport, ElevateError> {
    let idle_s = idle_min_seconds.to_string();
    let report: SmbSessionCloseReport = run_elevated_json(
        "close-safe-idle",
        &[
            ("--idle-min", idle_s.as_str()),
            ("--monitor-path", monitor_path),
        ],
    )?;
    audit_elevated_report(&report);
    Ok(report)
}

fn audit_elevated_report(report: &SmbSessionCloseReport) {
    for d in &report.details {
        let line = format!(
            "SMB-Session-Close [elevated-helper] id={} client={} user={} idle={}s opens={} → {:?}: {}",
            d.session_id,
            d.client_computer_name,
            d.client_user_name,
            d.seconds_idle,
            d.num_opens,
            d.outcome,
            d.message
        );
        match d.outcome {
            crate::util::smb_sessions::SmbSessionCloseOutcome::Closed
            | crate::util::smb_sessions::SmbSessionCloseOutcome::SkippedUnsafe
            | crate::util::smb_sessions::SmbSessionCloseOutcome::SkippedOffFocus
            | crate::util::smb_sessions::SmbSessionCloseOutcome::NotFound => log_info(&line),
            _ => log_warn(&line),
        }
    }
    log_info(&format!(
        "SMB-Helper close summary: closed={} skipped={} failed={} permission_denied={} — {}",
        report.closed, report.skipped, report.failed, report.permission_denied, report.message
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevate_error_messages_distinguish_cancel() {
        let cancel = ElevateError::Cancelled.user_message();
        assert!(cancel.contains("abgebrochen") || cancel.contains("UAC"));
        let denied = ElevateError::Failed("Zugriff verweigert".into()).user_message();
        assert!(!denied.contains("abgebrochen"));
    }

    #[test]
    fn helper_name_is_fixed() {
        assert!(HELPER_EXE_NAME.starts_with("ams-smb-helper"));
    }
}
