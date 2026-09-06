//! Non-loopback IPv4 addresses of this host (for smb:// export URL hints).

use std::net::Ipv4Addr;

/// Host identifiers to build client `smb://` URLs (IPs first, then hostname).
pub fn list_smb_host_endpoints() -> Vec<String> {
    let mut out = list_local_ipv4_addresses();
    let hostname = hostname_label();
    if !hostname.is_empty()
        && hostname != "localhost"
        && !out.iter().any(|h| h.eq_ignore_ascii_case(&hostname))
    {
        out.push(hostname);
    }
    out
}

fn list_local_ipv4_addresses() -> Vec<String> {
    let mut ips: Vec<Ipv4Addr> = Vec::new();

    #[cfg(windows)]
    collect_windows_ipv4(&mut ips);
    #[cfg(target_os = "linux")]
    collect_linux_ipv4(&mut ips);
    #[cfg(target_os = "macos")]
    collect_macos_ipv4(&mut ips);

    let mut strings: Vec<String> = ips.into_iter().map(|ip| ip.to_string()).collect();
    strings.sort_by(|a, b| ip_priority(a).cmp(&ip_priority(b)));
    strings.dedup();
    strings
}

/// Lower = higher priority in combobox (link-local / ATS setups first, then LAN).
fn ip_priority(ip: &str) -> (u8, Ipv4Addr) {
    let parsed = ip.parse::<Ipv4Addr>().unwrap_or(Ipv4Addr::UNSPECIFIED);
    let bucket = if parsed.octets()[0] == 169 && parsed.octets()[1] == 254 {
        0
    } else if parsed.octets()[0] == 10
        || (parsed.octets()[0] == 172 && (16..=31).contains(&parsed.octets()[1]))
        || (parsed.octets()[0] == 192 && parsed.octets()[1] == 168)
    {
        1
    } else {
        2
    };
    (bucket, parsed)
}

fn hostname_label() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_default()
}

#[cfg(windows)]
fn collect_windows_ipv4(out: &mut Vec<Ipv4Addr>) {
    // Prefer `ipconfig` over PowerShell: much faster cold start and no console flash
    // when spawned with `hidden_command` (CREATE_NO_WINDOW).
    let Ok(output) = crate::util::process::hidden_command("ipconfig").output() else {
        return;
    };
    if !output.status.success() {
        return;
    }
    parse_ipv4_lines(&String::from_utf8_lossy(&output.stdout), out);
}

#[cfg(target_os = "linux")]
fn collect_linux_ipv4(out: &mut Vec<Ipv4Addr>) {
    if let Ok(output) = crate::util::process::hidden_command("ip")
        .args(["-4", "-o", "addr", "show"])
        .output()
    {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                for (i, token) in parts.iter().enumerate() {
                    if *token == "inet" {
                        if let Some(ip_part) = parts.get(i + 1) {
                            push_ipv4_str(out, ip_part.split('/').next().unwrap_or(""));
                        }
                    }
                }
            }
        }
    }
    if out.is_empty() {
        if let Ok(output) = crate::util::process::hidden_command("hostname")
            .args(["-I"])
            .output()
        {
            if output.status.success() {
                for ip in String::from_utf8_lossy(&output.stdout).split_whitespace() {
                    push_ipv4_str(out, ip);
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn collect_macos_ipv4(out: &mut Vec<Ipv4Addr>) {
    let Ok(output) = crate::util::process::hidden_command("ifconfig").output() else {
        return;
    };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("inet ") || trimmed.starts_with("inet 127.") {
            continue;
        }
        if let Some(ip) = trimmed.split_whitespace().nth(1) {
            push_ipv4_str(out, ip);
        }
    }
}

fn parse_ipv4_lines(text: &str, out: &mut Vec<Ipv4Addr>) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        // ipconfig EN/DE: "IPv4 Address" / "IPv4-Adresse" — skip gateway/mask rows.
        if lower.contains("ipv4") {
            if let Some(rest) = line.rsplit(':').next() {
                push_ipv4_str(out, rest.trim());
            }
            continue;
        }
        // Bare IP per line from other helpers.
        if !line.contains(' ') && !line.contains('\t') {
            push_ipv4_str(out, line);
        }
    }
}

fn push_ipv4_str(out: &mut Vec<Ipv4Addr>, raw: &str) {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with("127.") {
        return;
    }
    if let Ok(ip) = raw.parse::<Ipv4Addr>() {
        if !ip.is_loopback() && !out.contains(&ip) {
            out.push(ip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_local_ip_sorts_before_lan() {
        assert!(ip_priority("169.254.169.254") < ip_priority("192.168.178.89"));
        assert!(ip_priority("192.168.1.5") < ip_priority("8.8.8.8"));
    }

    #[test]
    fn parses_ipconfig_ipv4_lines() {
        let sample = "\
Windows IP Configuration

Ethernet adapter Ethernet:

   Connection-specific DNS Suffix  . :
   IPv4 Address. . . . . . . . . . . : 192.168.178.89
   Subnet Mask . . . . . . . . . . . : 255.255.255.0
   Default Gateway . . . . . . . . . : 192.168.178.1

Wireless LAN adapter Wi-Fi:

   IPv4-Adresse . . . . . . . . . .  : 169.254.169.254
   Subnetzmaske . . . . . . . . . .  : 255.255.0.0
";
        let mut out = Vec::new();
        parse_ipv4_lines(sample, &mut out);
        assert_eq!(
            out,
            vec![
                "192.168.178.89".parse::<Ipv4Addr>().unwrap(),
                "169.254.169.254".parse::<Ipv4Addr>().unwrap(),
            ]
        );
    }
}
