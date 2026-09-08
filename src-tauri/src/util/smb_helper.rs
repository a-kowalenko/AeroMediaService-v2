//! CLI entry for `ams-smb-helper` (Phase 20d).
//!
//! Strict arg whitelist: `list` | `close-id` | `close-safe-idle`.
//! Writes JSON to `--out` (parent-created temp file). No PowerShell.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::util::smb_sessions::{
    close_all_safe_idle, close_session_by_id, collect_snapshot, parse_auto_close_enabled,
    parse_idle_min_seconds, parse_poll_seconds, parse_warn_threshold, SmbSessionCloseMode,
};

const HELPER_NAME: &str = "ams-smb-helper";

#[derive(Debug)]
struct HelperArgs {
    command: HelperCommand,
    out: PathBuf,
    idle_min_seconds: u64,
    warn_threshold: u32,
    poll_seconds: u32,
    auto_close_enabled: bool,
    monitor_path: String,
    session_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelperCommand {
    List,
    CloseId,
    CloseSafeIdle,
}

#[derive(Debug, Serialize)]
struct HelperErrorBody {
    error: String,
    code: &'static str,
}

/// Process CLI args; write JSON to `--out`; return process exit code.
pub fn run(args: Vec<String>) -> i32 {
    match parse_args(&args) {
        Ok(parsed) => match execute(&parsed) {
            Ok(()) => 0,
            Err(err) => {
                let _ = write_error(&parsed.out, &err, "exec_failed");
                1
            }
        },
        Err(err) => {
            // Best-effort: if `--out` was present, write error JSON there.
            if let Some(out) = peek_out_path(&args) {
                let _ = write_error(&out, &err, "bad_args");
            }
            eprintln!("{HELPER_NAME}: {err}");
            2
        }
    }
}

fn execute(args: &HelperArgs) -> Result<(), String> {
    match args.command {
        HelperCommand::List => {
            let snap = collect_snapshot(
                args.warn_threshold,
                args.poll_seconds,
                args.idle_min_seconds,
                args.auto_close_enabled,
                &args.monitor_path,
                &[],
            );
            write_json(&args.out, &snap)
        }
        HelperCommand::CloseId => {
            let report = close_session_by_id(
                &args.session_id,
                args.idle_min_seconds,
                SmbSessionCloseMode::ElevatedHelper,
            );
            write_json(&args.out, &report)
        }
        HelperCommand::CloseSafeIdle => {
            let report = close_all_safe_idle(
                args.idle_min_seconds,
                SmbSessionCloseMode::ElevatedHelper,
                &args.monitor_path,
            );
            write_json(&args.out, &report)
        }
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    validate_out_path(path)?;
    let body = serde_json::to_vec_pretty(value).map_err(|e| format!("JSON serialize: {e}"))?;
    fs::write(path, body).map_err(|e| format!("write {}: {e}", path.display()))
}

fn write_error(path: &Path, message: &str, code: &'static str) -> Result<(), String> {
    write_json(
        path,
        &HelperErrorBody {
            error: message.to_string(),
            code,
        },
    )
}

fn parse_args(args: &[String]) -> Result<HelperArgs, String> {
    if args.is_empty() {
        return Err(usage());
    }
    let command = match args[0].as_str() {
        "list" => HelperCommand::List,
        "close-id" => HelperCommand::CloseId,
        "close-safe-idle" => HelperCommand::CloseSafeIdle,
        other => {
            return Err(format!(
                "Unbekannter Befehl '{other}'. Erlaubt: list | close-id | close-safe-idle"
            ));
        }
    };

    let mut out: Option<PathBuf> = None;
    let mut idle_min_seconds = 600u64;
    let mut warn_threshold = 8u32;
    let mut poll_seconds = 30u32;
    let mut auto_close_enabled = false;
    let mut monitor_path = String::new();
    let mut session_id = String::new();

    let mut i = 1;
    while i < args.len() {
        let key = args[i].as_str();
        if !key.starts_with("--") {
            return Err(format!("Unerwartetes Argument '{key}' (nur --flags erlaubt)"));
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("Wert fehlt für {key}"))?
            .clone();
        if value.starts_with("--") {
            return Err(format!("Wert fehlt für {key}"));
        }
        // Reject shell metacharacters in values.
        if value.chars().any(|c| matches!(c, '&' | '|' | '<' | '>' | '^' | '\n' | '\r' | '`')) {
            return Err(format!("Ungültige Zeichen in {key}"));
        }
        match key {
            "--out" => {
                let path = PathBuf::from(&value);
                validate_out_path(&path)?;
                out = Some(path);
            }
            "--idle-min" => idle_min_seconds = parse_idle_min_seconds(&value),
            "--warn-threshold" => warn_threshold = parse_warn_threshold(&value),
            "--poll-seconds" => poll_seconds = parse_poll_seconds(&value),
            "--auto-close" => auto_close_enabled = parse_auto_close_enabled(&value),
            "--monitor-path" => {
                validate_path_value(&value, "--monitor-path")?;
                monitor_path = value;
            }
            "--session-id" => {
                validate_session_id(&value)?;
                session_id = value;
            }
            other => return Err(format!("Unbekanntes Flag '{other}'")),
        }
        i += 2;
    }

    let out = out.ok_or_else(|| "--out <path> ist Pflicht".to_string())?;
    if command == HelperCommand::CloseId && session_id.trim().is_empty() {
        return Err("--session-id ist Pflicht für close-id".into());
    }

    Ok(HelperArgs {
        command,
        out,
        idle_min_seconds,
        warn_threshold,
        poll_seconds,
        auto_close_enabled,
        monitor_path,
        session_id,
    })
}

fn peek_out_path(args: &[String]) -> Option<PathBuf> {
    args.windows(2).find_map(|pair| {
        if pair[0] == "--out" {
            Some(PathBuf::from(&pair[1]))
        } else {
            None
        }
    })
}

fn usage() -> String {
    format!(
        "Usage: {HELPER_NAME} <list|close-id|close-safe-idle> --out <file> [options]\n\
         Options: --idle-min --warn-threshold --poll-seconds --auto-close --monitor-path --session-id"
    )
}

/// Out path must be absolute and under the process temp directory.
pub fn validate_out_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("--out muss ein absoluter Pfad sein".into());
    }
    let temp = std::env::temp_dir();
    let temp_canon = temp.canonicalize().unwrap_or(temp.clone());
    // Parent may create the file after validation; check parent dir is under temp.
    let parent = path.parent().ok_or_else(|| "--out ohne Verzeichnis".to_string())?;
    let parent_canon = if parent.exists() {
        parent.canonicalize().map_err(|e| format!("--out parent: {e}"))?
    } else {
        parent.to_path_buf()
    };
    let parent_str = parent_canon.to_string_lossy().to_ascii_lowercase();
    let temp_str = temp_canon.to_string_lossy().to_ascii_lowercase();
    if !parent_str.starts_with(temp_str.trim_end_matches(['\\', '/'])) {
        return Err("--out muss unter %TEMP% liegen".into());
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if name.is_empty()
        || name.chars().any(|c| !(c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'))
    {
        return Err("--out Dateiname ungültig".into());
    }
    Ok(())
}

fn validate_path_value(value: &str, flag: &str) -> Result<(), String> {
    if value.len() > 512 {
        return Err(format!("{flag} zu lang"));
    }
    Ok(())
}

fn validate_session_id(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 256 {
        return Err("--session-id ungültig".into());
    }
    // Composite key: client|user|idx — allow UNC backslashes and dots.
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '|' | '\\' | '/' | '.' | '-' | '_' | ' ' | '$'))
    {
        return Err("--session-id enthält unerlaubte Zeichen".into());
    }
    Ok(())
}

/// Quote a single Windows command-line argument for `ShellExecuteEx` lpParameters.
pub fn quote_win_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }
    let needs_quotes = arg.chars().any(|c| c.is_whitespace() || c == '"');
    if !needs_quotes {
        return arg.to_string();
    }
    let mut out = String::from('"');
    for ch in arg.chars() {
        if ch == '"' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// Build lpParameters string for the helper (no exe name).
pub fn build_helper_params(command: &str, pairs: &[(&str, &str)]) -> Result<String, String> {
    match command {
        "list" | "close-id" | "close-safe-idle" => {}
        other => return Err(format!("Befehl nicht erlaubt: {other}")),
    }
    let mut parts = vec![command.to_string()];
    for &(key, value) in pairs {
        if !key.starts_with("--") {
            return Err(format!("Flag ungültig: {key}"));
        }
        if value.chars().any(|c| matches!(c, '&' | '|' | '<' | '>' | '^' | '\n' | '\r' | '`')) {
            return Err(format!("Wert für {key} enthält unerlaubte Zeichen"));
        }
        parts.push(key.to_string());
        parts.push(quote_win_arg(value));
    }
    Ok(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn parse_list_requires_out() {
        let err = parse_args(&["list".into()]).unwrap_err();
        assert!(err.contains("--out"));
    }

    #[test]
    fn rejects_unknown_command() {
        let err = parse_args(&["kill-all".into(), "--out".into(), "x".into()]).unwrap_err();
        assert!(err.contains("Unbekannter Befehl"));
    }

    #[test]
    fn rejects_shell_metacharacters() {
        let temp = env::temp_dir();
        let out = temp.join("ams_smb_helper_test_out.json");
        let err = parse_args(&[
            "list".into(),
            "--out".into(),
            out.to_string_lossy().into(),
            "--monitor-path".into(),
            "D:\\share & calc.exe".into(),
        ])
        .unwrap_err();
        assert!(err.contains("Ungültige Zeichen"));
    }

    #[test]
    fn parse_list_ok_under_temp() {
        let temp = env::temp_dir();
        let out = temp.join("ams_smb_helper_parse_ok.json");
        let parsed = parse_args(&[
            "list".into(),
            "--out".into(),
            out.to_string_lossy().into_owned(),
            "--idle-min".into(),
            "600".into(),
            "--warn-threshold".into(),
            "8".into(),
        ])
        .expect("parse");
        assert_eq!(parsed.command, HelperCommand::List);
        assert_eq!(parsed.idle_min_seconds, 600);
        assert_eq!(parsed.warn_threshold, 8);
    }

    #[test]
    fn close_id_requires_session() {
        let temp = env::temp_dir();
        let out = temp.join("ams_smb_helper_close_id.json");
        let err = parse_args(&[
            "close-id".into(),
            "--out".into(),
            out.to_string_lossy().into_owned(),
        ])
        .unwrap_err();
        assert!(err.contains("session-id"));
    }

    #[test]
    fn quote_and_build_params() {
        let p = build_helper_params(
            "list",
            &[("--out", r"C:\Temp\a b.json"), ("--idle-min", "600")],
        )
        .unwrap();
        assert!(p.starts_with("list "));
        assert!(p.contains(r#""C:\Temp\a b.json""#));
        assert!(p.contains("--idle-min 600"));
    }

    #[test]
    fn build_params_rejects_bad_command() {
        assert!(build_helper_params("powershell", &[]).is_err());
    }

    #[test]
    fn validate_session_id_allows_composite() {
        assert!(validate_session_id(r"\\HOST|user|0").is_ok());
        assert!(validate_session_id("evil&cmd").is_err());
    }
}
