//! Parse locally exported SMB shares (Samba / Windows) for client URL hints.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareExport {
    pub share_name: String,
    pub local_path: String,
}

/// Parse `[share]` sections with `path = …` from smb.conf / testparm output.
pub fn parse_smb_conf_exports(text: &str) -> Vec<ShareExport> {
    let mut out = Vec::new();
    let mut current_section: Option<String> = None;
    let mut current_path: Option<String> = None;

    for raw_line in text.lines() {
        let stripped = strip_smb_comment(raw_line);
        let line = stripped.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            flush_smb_section(&mut out, &mut current_section, &mut current_path);
            let name = line[1..line.len() - 1].trim();
            if is_skipped_smb_section(name) {
                current_section = None;
            } else {
                current_section = Some(name.to_string());
            }
            current_path = None;
            continue;
        }
        if current_section.is_none() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("path") {
            current_path = Some(unquote_smb_value(value));
        }
    }
    flush_smb_section(&mut out, &mut current_section, &mut current_path);
    out
}

/// Parse `sharing -l` output from macOS File Sharing.
pub fn parse_macos_sharing_list(text: &str) -> Vec<ShareExport> {
    let mut out = Vec::new();
    let mut pending_name: Option<String> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.eq_ignore_ascii_case("List of Share Points") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("name:") {
            pending_name = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("path:") {
            let local_path = rest.trim();
            let Some(name) = pending_name.take() else {
                continue;
            };
            if name.is_empty() || local_path.is_empty() {
                continue;
            }
            out.push(ShareExport {
                share_name: name,
                local_path: local_path.to_string(),
            });
        }
    }
    out
}

fn flush_smb_section(
    out: &mut Vec<ShareExport>,
    section: &mut Option<String>,
    path: &mut Option<String>,
) {
    let Some(name) = section.take() else {
        path.take();
        return;
    };
    let Some(local_path) = path.take() else {
        return;
    };
    if name.is_empty() || local_path.is_empty() || name.ends_with('$') {
        return;
    }
    out.push(ShareExport {
        share_name: name,
        local_path,
    });
}

fn is_skipped_smb_section(name: &str) -> bool {
    name.eq_ignore_ascii_case("global")
        || name.eq_ignore_ascii_case("homes")
        || name.eq_ignore_ascii_case("printers")
        || name.eq_ignore_ascii_case("print$")
}

fn strip_smb_comment(line: &str) -> String {
    let mut out = String::new();
    let mut in_string = false;
    for ch in line.chars() {
        if ch == '"' {
            in_string = !in_string;
            out.push(ch);
            continue;
        }
        if !in_string && (ch == '#' || ch == ';') {
            break;
        }
        out.push(ch);
    }
    out
}

fn unquote_smb_value(raw: &str) -> String {
    let t = raw.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        return t[1..t.len() - 1].to_string();
    }
    t.to_string()
}

/// Compare local filesystem paths for monitor ↔ export matching.
pub fn local_paths_match(a: &str, b: &str) -> bool {
    let a = normalize_local_path(a);
    let b = normalize_local_path(b);
    !a.is_empty() && a == b
}

pub fn normalize_local_path(raw: &str) -> String {
    let mut s = raw.trim().trim_end_matches(['/', '\\']).to_string();
    if cfg!(windows) {
        s = s.replace('/', "\\");
    } else {
        s = s.replace('\\', "/");
    }
    if cfg!(windows) || cfg!(target_os = "macos") {
        s.to_ascii_lowercase()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_smb_conf_share_section() {
        let text = r#"
[global]
    workgroup = WORKGROUP

[aktuell]
    path = /home/coilnova/Desktop/aktuell
    read only = no
"#;
        let exports = parse_smb_conf_exports(text);
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].share_name, "aktuell");
        assert_eq!(exports[0].local_path, "/home/coilnova/Desktop/aktuell");
    }

    #[test]
    fn skips_global_and_hidden_shares() {
        let text = r#"
[global]
    path = /tmp

[print$]
    path = /var/spool/samba

[share]
    path = /data
"#;
        let exports = parse_smb_conf_exports(text);
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].share_name, "share");
    }

    #[test]
    fn paths_match_ignore_trailing_slash() {
        assert!(local_paths_match(
            "/home/coilnova/Desktop/aktuell/",
            "/home/coilnova/Desktop/aktuell"
        ));
        assert!(local_paths_match(
            r"D:\Aktuell",
            r"d:/Aktuell/"
        ));
    }

    #[test]
    fn parses_macos_sharing_list() {
        let text = r#"List of Share Points
name:		aktuell
path:		/Users/coilnova/Desktop/aktuell
	smb:	{
		name:	aktuell
	}
name:		Public
path:		/Users/coilnova/Public
"#;
        let exports = parse_macos_sharing_list(text);
        assert_eq!(exports.len(), 2);
        assert_eq!(exports[0].share_name, "aktuell");
        assert_eq!(exports[0].local_path, "/Users/coilnova/Desktop/aktuell");
        assert_eq!(exports[1].share_name, "Public");
    }
}
