//! File logging plus GUI events (`log-message`), port of legacy `core/logger.py`.

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use once_cell::sync::Lazy;
use serde::Serialize;

use crate::constants::DEBUG_LOG_FILE;
use crate::storage::app_config_dir;

const RING_CAPACITY: usize = 2000;
const DEBUG_MAX_BYTES: u64 = 5 * 1024 * 1024;
const DEBUG_BACKUP_COUNT: u32 = 3;

/// Python `logging` level numbers (legacy `log_message = Signal(int, str)`).
pub const LEVEL_DEBUG: i32 = 10;
pub const LEVEL_INFO: i32 = 20;
pub const LEVEL_WARNING: i32 = 30;
pub const LEVEL_ERROR: i32 = 40;

type LogEmitter = Box<dyn Fn(&LogMessage) + Send + Sync>;

static LOG_DIR: Lazy<Mutex<Option<PathBuf>>> = Lazy::new(|| Mutex::new(None));
static RING: Lazy<Mutex<VecDeque<LogMessage>>> = Lazy::new(|| Mutex::new(VecDeque::new()));
static EMITTER: Lazy<Mutex<Option<LogEmitter>>> = Lazy::new(|| Mutex::new(None));

#[derive(Debug, Clone, Serialize)]
pub struct LogMessage {
    pub level: i32,
    pub level_name: String,
    pub message: String,
}

/// Initialize logging. Empty/`None` uses the app-data directory (not the process cwd).
pub fn init_logging(log_dir: Option<&str>) -> Result<PathBuf, String> {
    let dir = resolve_log_dir(log_dir)?;
    set_log_dir_path(dir.clone())?;
    let version = env!("CARGO_PKG_VERSION");
    append_line(
        LEVEL_INFO,
        "INFO",
        "app",
        &format!("Logging-System initialisiert (Aero Media Service v{version})"),
    )?;
    log_path().ok_or_else(|| "log path not set".into())
}

pub fn set_log_dir(log_dir: &str) -> Result<PathBuf, String> {
    let dir = resolve_log_dir(Some(log_dir))?;
    set_log_dir_path(dir.clone())?;
    append_line(
        LEVEL_INFO,
        "INFO",
        "app",
        &format!("Log-Verzeichnis: {}", dir.display()),
    )?;
    Ok(dir)
}

pub fn set_log_emitter<F>(f: F)
where
    F: Fn(&LogMessage) + Send + Sync + 'static,
{
    if let Ok(mut guard) = EMITTER.lock() {
        *guard = Some(Box::new(f));
    }
}

pub fn log_path() -> Option<PathBuf> {
    LOG_DIR
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .map(|dir| dir.join(DEBUG_LOG_FILE))
}

#[allow(dead_code)]
pub fn log_debug(message: &str) {
    let _ = append_line(LEVEL_DEBUG, "DEBUG", "app", message);
}

pub fn log_info(message: &str) {
    let _ = append_line(LEVEL_INFO, "INFO", "app", message);
}

#[allow(dead_code)]
pub fn log_warn(message: &str) {
    let _ = append_line(LEVEL_WARNING, "WARN", "app", message);
}

#[allow(dead_code)]
pub fn log_error(message: &str) {
    let _ = append_line(LEVEL_ERROR, "ERROR", "app", message);
}

/// Snapshot of GUI-visible log lines (oldest → newest).
pub fn recent_logs(limit: Option<usize>) -> Vec<LogMessage> {
    let Ok(guard) = RING.lock() else {
        return Vec::new();
    };
    let cap = limit.unwrap_or(RING_CAPACITY).min(guard.len());
    guard
        .iter()
        .rev()
        .take(cap)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

#[allow(dead_code)]
pub fn clear_ring_buffer() {
    if let Ok(mut guard) = RING.lock() {
        guard.clear();
    }
}

fn resolve_log_dir(log_dir: Option<&str>) -> Result<PathBuf, String> {
    let trimmed = log_dir.map(str::trim).filter(|s| !s.is_empty());
    match trimmed {
        Some(path) => Ok(PathBuf::from(path)),
        None => app_config_dir().map_err(|e| e.to_string()),
    }
}

fn set_log_dir_path(dir: PathBuf) -> Result<(), String> {
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut guard = LOG_DIR.lock().map_err(|e| e.to_string())?;
    *guard = Some(dir);
    Ok(())
}

fn append_line(level: i32, level_name: &str, source: &str, message: &str) -> Result<(), String> {
    let dir = {
        let guard = LOG_DIR.lock().map_err(|e| e.to_string())?;
        match guard.as_ref() {
            Some(p) => p.clone(),
            None => {
                let dir = app_config_dir().map_err(|e| e.to_string())?;
                fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                dir
            }
        }
    };

    let path = dir.join(DEBUG_LOG_FILE);
    rotate_if_needed(&path, DEBUG_MAX_BYTES, DEBUG_BACKUP_COUNT);

    let now = chrono::Local::now();
    let file_ts = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let gui_ts = now.format("%H:%M:%S").to_string();
    let file_line = format!("{file_ts} - {source} - {level_name} - {message}\n");
    let gui_message = format!("{gui_ts} [{level_name}]: {message}");

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    if file.metadata().map(|m| m.len()).unwrap_or(1) == 0 {
        file.write_all(&[0xEF, 0xBB, 0xBF])
            .map_err(|e| e.to_string())?;
    }
    file.write_all(file_line.as_bytes())
        .map_err(|e| e.to_string())?;

    // GUI handler in legacy was INFO+; DEBUG stays file-only.
    if level >= LEVEL_INFO {
        let entry = LogMessage {
            level,
            level_name: level_name.to_string(),
            message: gui_message,
        };
        push_ring(entry.clone());
        emit_log_line(&entry);
    }

    Ok(())
}

fn push_ring(entry: LogMessage) {
    if let Ok(mut guard) = RING.lock() {
        if guard.len() >= RING_CAPACITY {
            guard.pop_front();
        }
        guard.push_back(entry);
    }
}

fn emit_log_line(entry: &LogMessage) {
    if let Ok(guard) = EMITTER.lock() {
        if let Some(emit) = guard.as_ref() {
            emit(entry);
        }
    }
}

fn rotate_if_needed(path: &Path, max_bytes: u64, backup_count: u32) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.len() < max_bytes || backup_count == 0 {
        return;
    }

    let oldest = backup_name(path, backup_count);
    let _ = fs::remove_file(&oldest);
    for i in (1..backup_count).rev() {
        let from = backup_name(path, i);
        let to = backup_name(path, i + 1);
        if from.is_file() {
            let _ = fs::rename(&from, &to);
        }
    }
    let _ = fs::rename(path, backup_name(path, 1));
}

fn backup_name(path: &Path, index: u32) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{index}"));
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn init_logging_writes_file_and_gui_ring() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_ring_buffer();
        let dir = tempdir().unwrap();
        let path = init_logging(Some(dir.path().to_str().unwrap())).expect("init");
        assert!(path.ends_with(DEBUG_LOG_FILE));
        assert!(path.is_file());

        log_info("unit-test-info");
        log_debug("unit-test-debug");
        log_error("unit-test-error");

        let content = fs::read_to_string(&path).expect("read log");
        assert!(content.contains("Logging-System initialisiert"));
        assert!(content.contains("unit-test-info"));
        assert!(content.contains("unit-test-debug"));
        assert!(content.contains("unit-test-error"));

        let recent = recent_logs(Some(50));
        assert!(recent.iter().any(|e| e.message.contains("unit-test-info")));
        assert!(recent.iter().any(|e| e.level == LEVEL_ERROR));
        assert!(!recent.iter().any(|e| e.message.contains("unit-test-debug")));
        assert!(recent
            .iter()
            .any(|e| e.message.contains("[INFO]:") && e.level_name == "INFO"));
    }

    #[test]
    fn rotate_renames_oversized_file() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempdir().unwrap();
        let path = dir.path().join(DEBUG_LOG_FILE);
        fs::write(&path, "abcdefghij").unwrap();
        rotate_if_needed(&path, 5, 2);
        assert!(!path.is_file());
        assert!(backup_name(&path, 1).is_file());
    }
}
