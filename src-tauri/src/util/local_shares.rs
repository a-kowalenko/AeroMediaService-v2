//! Locally visible network shares on the AMS host (mapped drives, mounts, exports).

use serde::Serialize;

use crate::bridge::to_smb_url;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalShareKind {
    Monitor,
    MappedDrive,
    Mount,
    LocalExport,
    SavedPrimary,
    SavedBackup,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalShareCandidate {
    /// Wire-preferred path (`smb://…`) or local absolute path.
    pub path: String,
    pub label: String,
    pub kind: LocalShareKind,
}

/// Collect de-duplicated share candidates visible on this machine.
pub fn list_local_share_candidates(
    monitor_path: &str,
    primary_raw: &str,
    backup_raw: &str,
) -> Vec<LocalShareCandidate> {
    let mut out: Vec<LocalShareCandidate> = Vec::new();

    push_unique(
        &mut out,
        LocalShareCandidate {
            path: normalize_candidate_path(monitor_path),
            label: "Monitor-Ordner".into(),
            kind: LocalShareKind::Monitor,
        },
    );

    push_unique(
        &mut out,
        LocalShareCandidate {
            path: normalize_candidate_path(primary_raw),
            label: "Gespeicherter Primär-Share".into(),
            kind: LocalShareKind::SavedPrimary,
        },
    );

    push_unique(
        &mut out,
        LocalShareCandidate {
            path: normalize_candidate_path(backup_raw),
            label: "Gespeicherter Backup-Share".into(),
            kind: LocalShareKind::SavedBackup,
        },
    );

    #[cfg(target_os = "windows")]
    collect_windows(&mut out);

    #[cfg(target_os = "macos")]
    collect_macos(&mut out);

    #[cfg(target_os = "linux")]
    collect_linux(&mut out);

    sort_candidates(&mut out);
    out
}

fn normalize_candidate_path(raw: &str) -> String {
    to_smb_url(raw)
}

fn push_unique(out: &mut Vec<LocalShareCandidate>, candidate: LocalShareCandidate) {
    let path = to_smb_url(&candidate.path);
    if path.is_empty() {
        return;
    }
    let key = normalize_smb_for_dedupe(&path);
    if out.iter().any(|c| normalize_smb_for_dedupe(&c.path) == key) {
        return;
    }
    out.push(LocalShareCandidate {
        path,
        label: candidate.label,
        kind: candidate.kind,
    });
}

fn normalize_smb_for_dedupe(raw: &str) -> String {
    let mut s = to_smb_url(raw).to_ascii_lowercase();
    s = s.replace('\\', "/");
    while s.ends_with('/') && s.len() > 1 {
        s.pop();
    }
    s
}

fn sort_candidates(out: &mut [LocalShareCandidate]) {
    out.sort_by(|a, b| {
        kind_rank(&a.kind)
            .cmp(&kind_rank(&b.kind))
            .then_with(|| a.label.cmp(&b.label))
            .then_with(|| a.path.cmp(&b.path))
    });
}

fn kind_rank(kind: &LocalShareKind) -> u8 {
    match kind {
        LocalShareKind::Monitor => 0,
        LocalShareKind::MappedDrive => 1,
        LocalShareKind::Mount => 2,
        LocalShareKind::LocalExport => 3,
        LocalShareKind::SavedPrimary => 4,
        LocalShareKind::SavedBackup => 5,
    }
}

#[cfg(target_os = "windows")]
fn collect_windows(out: &mut Vec<LocalShareCandidate>) {
    collect_windows_mapped_drives(out);
    collect_windows_local_exports(out);
}

#[cfg(target_os = "windows")]
fn collect_windows_mapped_drives(out: &mut Vec<LocalShareCandidate>) {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let Ok(network) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Network") else {
        return;
    };

    for name in network.enum_keys().flatten() {
        if name.len() != 1 {
            continue;
        }
        let Ok(drive_key) = network.open_subkey(&name) else {
            continue;
        };
        let Ok(remote) = drive_key.get_value::<String, _>("RemotePath") else {
            continue;
        };
        let remote = remote.trim();
        if remote.is_empty() {
            continue;
        }
        let letter = format!("{name}:");
        push_unique(
            out,
            LocalShareCandidate {
                path: to_smb_url(remote),
                label: format!("Netzlaufwerk {letter} ({remote})"),
                kind: LocalShareKind::MappedDrive,
            },
        );
    }
}

#[cfg(target_os = "windows")]
fn collect_windows_local_exports(out: &mut Vec<LocalShareCandidate>) {
    let host = local_host_label();
    let output = std::process::Command::new("net")
        .args(["share"])
        .output()
        .ok();
    let Some(output) = output else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines().skip(2) {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('-') {
            continue;
        }
        let Some((share_name, _rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let share_name = share_name.trim();
        if share_name.is_empty()
            || share_name.eq_ignore_ascii_case("Share name")
            || share_name.ends_with('$')
        {
            continue;
        }
        let unc = format!(r"\\{host}\{share_name}");
        push_unique(
            out,
            LocalShareCandidate {
                path: to_smb_url(&unc),
                label: format!("Lokale Freigabe {share_name}"),
                kind: LocalShareKind::LocalExport,
            },
        );
    }
}

#[cfg(target_os = "macos")]
fn collect_macos(out: &mut Vec<LocalShareCandidate>) {
    collect_mount_table(out, &["mount"]);
}

#[cfg(target_os = "linux")]
fn collect_linux(out: &mut Vec<LocalShareCandidate>) {
    collect_mount_table(out, &["mount"]);
    collect_proc_mounts(out);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn collect_mount_table(out: &mut Vec<LocalShareCandidate>, cmd: &[&str]) {
    let Ok(output) = std::process::Command::new(cmd[0]).args(&cmd[1..]).output() else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        parse_mount_line(out, line);
    }
}

#[cfg(target_os = "linux")]
fn collect_proc_mounts(out: &mut Vec<LocalShareCandidate>) {
    let Ok(text) = std::fs::read_to_string("/proc/mounts") else {
        return;
    };
    for line in text.lines() {
        let Some((source, _mount_point, fs_type)) = parse_proc_mount_fields(line) else {
            continue;
        };
        if !is_smb_fs_type(fs_type) {
            continue;
        }
        push_mount_source(out, source, _mount_point);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn parse_mount_line(out: &mut Vec<LocalShareCandidate>, line: &str) {
    // //user@host/share on /Volumes/share (smbfs, …)
    // //host/share on /mnt/share type cifs (...)
    let Some(on_idx) = line.find(" on ") else {
        return;
    };
    let source = line[..on_idx].trim();
    let after_on = &line[on_idx + 4..];
    let mount_point = after_on
        .split([' ', '('])
        .next()
        .unwrap_or("")
        .trim();
    if mount_point.is_empty() {
        return;
    }
    let fs_type = after_on
        .split('(')
        .nth(1)
        .and_then(|s| s.split(',').next())
        .unwrap_or("")
        .trim();
    if !source.starts_with("//") && !is_smb_fs_type(fs_type) {
        return;
    }
    if source.starts_with("//") {
        push_mount_source(out, source, mount_point);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn push_mount_source(out: &mut Vec<LocalShareCandidate>, source: &str, mount_point: &str) {
    let source = source.trim();
    let mount_point = mount_point.trim();
    if source.is_empty() {
        return;
    }
    let smb_path = to_smb_url(source);
    let label = if mount_point.is_empty() {
        format!("Gemountet ({smb_path})")
    } else {
        format!("Gemountet {mount_point}")
    };
    push_unique(
        out,
        LocalShareCandidate {
            path: smb_path,
            label,
            kind: LocalShareKind::Mount,
        },
    );
}

#[cfg(target_os = "linux")]
fn parse_proc_mount_fields(line: &str) -> Option<(&str, &str, &str)> {
    let mut parts = line.split_whitespace();
    let source = parts.next()?;
    let mount_point = parts.next()?;
    let fs_type = parts.next()?;
    Some((source, mount_point, fs_type))
}

fn is_smb_fs_type(fs_type: &str) -> bool {
    matches!(
        fs_type.to_ascii_lowercase().as_str(),
        "smbfs" | "cifs" | "smb3" | "fuse.smb" | "fuse.cifs"
    )
}

fn local_host_label() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "localhost".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_smb_prefix_on_local_windows_path() {
        assert_eq!(
            to_smb_url(r"smb://C:\Shares\aktuell"),
            r"C:\Shares\aktuell"
        );
    }

    #[test]
    fn dedupes_monitor_and_saved_primary() {
        let list = list_local_share_candidates(
            "smb://host/aktuell",
            "smb://host/aktuell",
            "",
        );
        assert_eq!(
            list.iter()
                .filter(|c| c.path.eq_ignore_ascii_case("smb://host/aktuell"))
                .count(),
            1
        );
        assert!(list.iter().any(|c| c.kind == LocalShareKind::Monitor));
        assert!(!list.iter().any(|c| c.kind == LocalShareKind::SavedPrimary));
    }

    #[test]
    fn normalizes_unc_monitor_to_smb() {
        let list = list_local_share_candidates(r"\\host\aktuell", "", "");
        assert!(list.iter().any(|c| {
            c.kind == LocalShareKind::Monitor && c.path == "smb://host/aktuell"
        }));
    }

    #[test]
    fn keeps_local_monitor_path() {
        let list = list_local_share_candidates(r"D:\Shares\aktuell", "", "");
        assert!(list.iter().any(|c| {
            c.kind == LocalShareKind::Monitor && c.path == r"D:\Shares\aktuell"
        }));
    }

    #[test]
    fn includes_saved_backup_when_distinct() {
        let list = list_local_share_candidates(
            "smb://host/aktuell",
            "smb://host/aktuell",
            "smb://host/aktuell-backup",
        );
        assert!(list.iter().any(|c| c.kind == LocalShareKind::SavedBackup));
        assert!(list.iter().any(|c| c.path == "smb://host/aktuell-backup"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_proc_mounts_cifs_line() {
        let mut out = Vec::new();
        let line = "//server/share /mnt/aktuell cifs rw,relatime,vers=3.0 0 0";
        let Some((source, mount_point, fs_type)) = parse_proc_mount_fields(line) else {
            panic!("parse failed");
        };
        assert_eq!(fs_type, "cifs");
        if is_smb_fs_type(fs_type) {
            push_mount_source(&mut out, source, mount_point);
        }
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, "smb://server/share");
    }
}
