//! Open URLs and paths in the user's default browser or file manager.
//!
//! On Linux AppImages, delegates to host `xdg-open` with a sanitized environment so
//! child processes do not inherit bundle-specific library paths.

use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

use crate::model::validation::is_valid_share_link;

#[cfg(target_os = "linux")]
use super::appimage_env;

pub fn open_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if !is_valid_share_link(url) {
        return Err("URL muss mit http:// oder https:// beginnen.".into());
    }
    open_target(url)
}

pub fn open_path(path: &Path) -> Result<(), String> {
    path.metadata()
        .map_err(|e| format!("Pfad nicht gefunden: {e}"))?;
    open_target(path.as_os_str())
}

fn open_target(target: impl AsRef<std::ffi::OsStr>) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        spawn_linux_opener(target.as_ref())
    }
    #[cfg(not(target_os = "linux"))]
    {
        open::that_detached(target).map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "linux")]
fn spawn_linux_opener(target: &std::ffi::OsStr) -> Result<(), String> {
    let mut errors: Vec<String> = Vec::new();
    for program in linux_opener_candidates() {
        let mut cmd = Command::new(&program);
        match program.as_str() {
            "gio" => {
                cmd.arg("open");
            }
            _ => {}
        }
        cmd.arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        appimage_env::sanitize_std_command(&mut cmd);

        match cmd.spawn() {
            Ok(_) => return Ok(()),
            Err(err) => errors.push(format!("{program}: {err}")),
        }
    }
    Err(format!(
        "Externes Programm konnte nicht gestartet werden: {}",
        errors.join("; ")
    ))
}

#[cfg(target_os = "linux")]
fn linux_opener_candidates() -> Vec<String> {
    let mut candidates = vec![
        "/usr/bin/xdg-open".to_string(),
        "xdg-open".to_string(),
        "/usr/bin/gio".to_string(),
        "gio".to_string(),
    ];
    candidates.dedup();
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_urls() {
        assert!(open_url("ftp://example.com").is_err());
        assert!(open_url("").is_err());
        assert!(open_url("HTTPS://example.com").is_err());
    }

    #[test]
    fn accepts_valid_urls() {
        assert!(is_valid_share_link("https://example.com/x"));
        assert!(is_valid_share_link("http://example.com"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_opener_prefers_host_xdg_open() {
        let candidates = linux_opener_candidates();
        assert_eq!(candidates.first().map(String::as_str), Some("/usr/bin/xdg-open"));
    }
}
