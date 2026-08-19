use axum::http::{HeaderMap, StatusCode};
use serde_json::Value;

use crate::storage::ats_presence::{
    clamp_activity_payload_json, AtsActivityInput, AtsIdentityInput, AtsPresenceState,
};
use crate::storage::logging;

pub const HEADER_INSTANCE_ID: &str = "x-ats-instance-id";
pub const HEADER_HOSTNAME: &str = "x-ats-hostname";
pub const HEADER_VERSION: &str = "x-ats-version";
pub const HEADER_APP: &str = "x-ats-app";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeEventKind {
    Health,
    CustomerLookup,
    JobStatus,
    HandoffReady,
}

impl BridgeEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BridgeEventKind::Health => "health",
            BridgeEventKind::CustomerLookup => "customer_lookup",
            BridgeEventKind::JobStatus => "job_status",
            BridgeEventKind::HandoffReady => "handoff_ready",
        }
    }
}

pub fn parse_identity(headers: &HeaderMap) -> AtsIdentityInput {
    let hostname = header_value(headers, HEADER_HOSTNAME)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unbekannter ATS Host".into());
    let instance_raw = header_value(headers, HEADER_INSTANCE_ID);
    let degraded_identity = instance_raw.as_deref().unwrap_or("").trim().is_empty();
    let instance_id = instance_raw
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            if hostname.trim().is_empty() || hostname == "Unbekannter ATS Host" {
                "unknown:missing-instance-id".into()
            } else {
                format!("unknown:{}", slugify(&hostname))
            }
        });
    AtsIdentityInput {
        instance_id,
        hostname,
        ats_version: header_value(headers, HEADER_VERSION).unwrap_or_default(),
        ats_app: header_value(headers, HEADER_APP).unwrap_or_else(|| "AeroTandemStudio".into()),
        degraded_identity,
    }
}

pub fn record_bridge_event(
    presence: &AtsPresenceState,
    headers: &HeaderMap,
    kind: BridgeEventKind,
    route: &str,
    method: &str,
    status: StatusCode,
    correlation_id: Option<&str>,
    folder_name: Option<&str>,
    payload: Option<Value>,
) {
    let identity = parse_identity(headers);
    let payload_json = payload
        .map(clamp_activity_payload_json)
        .unwrap_or_default();
    let activity = AtsActivityInput {
        event_type: kind.as_str().into(),
        route: route.into(),
        method: method.into(),
        status_code_class: status_class(status),
        correlation_id: correlation_id.unwrap_or("").trim().to_string(),
        folder_name: folder_name.unwrap_or("").trim().to_string(),
        payload_json,
    };
    if let Err(e) = presence.record_event(identity, activity) {
        logging::log_warn(&format!(
            "ATS-Presence Recording fehlgeschlagen für {} {}: {e}",
            method, route
        ));
    }
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(name)?.to_str().ok()?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(raw.chars().take(160).collect())
}

fn status_class(status: StatusCode) -> String {
    format!("{}xx", status.as_u16() / 100)
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if (ch.is_ascii_whitespace() || ch == '-' || ch == '_') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "missing-hostname".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn parses_complete_identity() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER_INSTANCE_ID, HeaderValue::from_static("abc-123"));
        headers.insert(HEADER_HOSTNAME, HeaderValue::from_static("Studio-PC"));
        headers.insert(HEADER_VERSION, HeaderValue::from_static("1.2.3"));
        headers.insert(HEADER_APP, HeaderValue::from_static("ATS"));
        let identity = parse_identity(&headers);
        assert_eq!(identity.instance_id, "abc-123");
        assert_eq!(identity.hostname, "Studio-PC");
        assert_eq!(identity.ats_version, "1.2.3");
        assert_eq!(identity.ats_app, "ATS");
        assert!(!identity.degraded_identity);
    }

    #[test]
    fn falls_back_when_instance_id_missing() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER_HOSTNAME, HeaderValue::from_static("Mein Rechner"));
        let identity = parse_identity(&headers);
        assert_eq!(identity.instance_id, "unknown:mein-rechner");
        assert_eq!(identity.hostname, "Mein Rechner");
        assert!(identity.degraded_identity);
        assert_eq!(identity.ats_app, "AeroTandemStudio");
    }
}
