//! mDNS advertise for AMS LAN Bridge (Phase 13 / P4).
//! Service type: `_ams-bridge._tcp.local.` — Token is never published.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use mdns_sd::{ServiceDaemon, ServiceInfo};

use super::types::P3_CAPABILITIES;
use crate::storage::logging;

/// DNS-SD type (service name ≤ 15 bytes per mDNS convention).
pub const SERVICE_TYPE: &str = "_ams-bridge._tcp.local.";
pub const TXT_APP: &str = "AeroMediaService";

pub struct MdnsAdvertiser {
    daemon: ServiceDaemon,
    fullname: String,
}

impl MdnsAdvertiser {
    /// Publish bridge on LAN. Soft-fail: returns `None` if mDNS cannot start (bridge HTTP still runs).
    pub fn start(
        local_addr: SocketAddr,
        version: &str,
        monitor_path: &str,
        display_name: &str,
        instance_id: &str,
    ) -> Option<Self> {
        let daemon = match ServiceDaemon::new() {
            Ok(d) => d,
            Err(e) => {
                logging::log_warn(&format!(
                    "AMS-Bridge mDNS: Daemon nicht gestartet ({e}) — Bridge HTTP läuft weiter."
                ));
                return None;
            }
        };

        let port = local_addr.port();
        let instance = super::identity::instance_dns_label(display_name);
        let host = host_name_dns();
        let props = txt_properties(version, monitor_path, display_name, instance_id);

        let service = match build_service_info(local_addr.ip(), port, &instance, &host, props) {
            Ok(s) => s,
            Err(e) => {
                logging::log_warn(&format!(
                    "AMS-Bridge mDNS: ServiceInfo ungültig ({e}) — Bridge HTTP läuft weiter."
                ));
                let _ = daemon.shutdown();
                return None;
            }
        };
        let fullname = service.get_fullname().to_string();

        if let Err(e) = daemon.register(service) {
            logging::log_warn(&format!(
                "AMS-Bridge mDNS: Register fehlgeschlagen ({e}) — Bridge HTTP läuft weiter."
            ));
            let _ = daemon.shutdown();
            return None;
        }

        logging::log_info(&format!(
            "AMS-Bridge mDNS aktiv: {fullname} (type {SERVICE_TYPE}, port {port})"
        ));
        Some(Self { daemon, fullname })
    }

    pub fn stop(self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
        logging::log_info("AMS-Bridge mDNS gestoppt.");
    }
}

fn build_service_info(
    ip: IpAddr,
    port: u16,
    instance: &str,
    host: &str,
    props: HashMap<String, String>,
) -> Result<ServiceInfo, String> {
    let unspecified = matches!(ip, IpAddr::V4(v) if v.is_unspecified())
        || matches!(ip, IpAddr::V6(v) if v.is_unspecified());

    if unspecified {
        ServiceInfo::new(SERVICE_TYPE, instance, host, "", port, Some(props))
            .map(|s| s.enable_addr_auto())
            .map_err(|e| e.to_string())
    } else {
        let ip_str = ip.to_string();
        ServiceInfo::new(
            SERVICE_TYPE,
            instance,
            host,
            ip_str.as_str(),
            port,
            Some(props),
        )
        .map_err(|e| e.to_string())
    }
}

fn txt_properties(
    version: &str,
    monitor_path: &str,
    display_name: &str,
    instance_id: &str,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("app".into(), TXT_APP.into());
    map.insert("ver".into(), version.chars().take(32).collect());
    map.insert("caps".into(), P3_CAPABILITIES.join(","));
    let name = display_name.trim();
    if !name.is_empty() {
        let truncated: String = name.chars().take(180).collect();
        map.insert("name".into(), truncated);
    }
    let id = instance_id.trim();
    if !id.is_empty() {
        map.insert("id".into(), id.chars().take(64).collect());
    }
    let path = monitor_path.trim();
    if !path.is_empty() {
        // TXT value length budget — keep short for UNC paths.
        let truncated: String = path.chars().take(180).collect();
        map.insert("path".into(), truncated);
    }
    map
}

fn host_name_dns() -> String {
    let host = hostname_raw();
    let safe = sanitize_dns_label(&host);
    if safe.is_empty() {
        "aeromediaservice.local.".into()
    } else {
        format!("{safe}.local.")
    }
}

fn hostname_raw() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_default()
}

pub fn sanitize_dns_label(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '_' || ch == ' ' || ch == '.' {
            if !out.ends_with('-') && !out.is_empty() {
                out.push('-');
            }
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out.chars().take(63).collect()
}

/// Prefer IPv4 (esp. link-local / private) when building http:// base URLs for ATS.
pub fn prefer_http_host(addrs: impl IntoIterator<Item = IpAddr>) -> Option<String> {
    let mut v4: Vec<IpAddr> = Vec::new();
    let mut v6: Vec<IpAddr> = Vec::new();
    for a in addrs {
        match a {
            IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => v4.push(a),
            IpAddr::V6(ip) if !ip.is_unspecified() && !ip.is_loopback() => v6.push(a),
            _ => {}
        }
    }
    v4.sort_by_key(|a| match a {
        IpAddr::V4(ip) if ip.is_link_local() => 0u8,
        IpAddr::V4(ip) if ip.is_private() => 1,
        _ => 2,
    });
    v4.into_iter()
        .next()
        .or_else(|| v6.into_iter().next())
        .map(|ip| match ip {
            IpAddr::V6(v) => format!("[{v}]"),
            IpAddr::V4(v) => v.to_string(),
        })
}

pub fn base_url_for(host: &str, port: u16) -> String {
    format!("http://{host}:{port}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn sanitize_hostname() {
        assert_eq!(sanitize_dns_label("DESKTOP-ABC 1"), "desktop-abc-1");
        assert_eq!(sanitize_dns_label("!!!"), "");
    }

    #[test]
    fn prefer_link_local_v4() {
        let host = prefer_http_host([
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(169, 254, 1, 2)),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)),
        ])
        .unwrap();
        assert_eq!(host, "169.254.1.2");
    }

    #[test]
    fn base_url_format() {
        assert_eq!(base_url_for("169.254.1.2", 8787), "http://169.254.1.2:8787");
        assert_eq!(base_url_for("[fe80::1]", 8787), "http://[fe80::1]:8787");
    }

    #[test]
    fn service_type_name_within_15_bytes() {
        assert!(SERVICE_TYPE.starts_with("_ams-bridge._tcp"));
        assert_eq!("ams-bridge".len(), 10);
    }
}
