//! Bridge request/response DTOs (HANDOFF.md §9).

use serde::{Deserialize, Serialize};

use crate::model::handoff::StatusOutboxV1;
use crate::model::kunde::Kunde;
use crate::model::marker::LookupMode;

pub const DEFAULT_BRIDGE_BIND: &str = "0.0.0.0:8787";

pub const CAPABILITY_MANIFEST_V1: &str = "manifest-v1";
pub const CAPABILITY_STATUS_OUTBOX: &str = "status-outbox";
pub const CAPABILITY_LOOKUP: &str = "lookup";
pub const CAPABILITY_READY: &str = "ready";
pub const CAPABILITY_APPEND_V1: &str = "append-v1";

/// Capabilities advertised by AMS (P3 + append).
pub const P3_CAPABILITIES: [&str; 5] = [
    CAPABILITY_MANIFEST_V1,
    CAPABILITY_STATUS_OUTBOX,
    CAPABILITY_LOOKUP,
    CAPABILITY_READY,
    CAPABILITY_APPEND_V1,
];

pub type BridgeCapabilities = Vec<String>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthResponse {
    pub online: bool,
    pub version: String,
    pub monitor_path: String,
    pub capabilities: BridgeCapabilities,
}

impl HealthResponse {
    pub fn p3(version: impl Into<String>, monitor_path: impl Into<String>) -> Self {
        Self {
            online: true,
            version: version.into(),
            monitor_path: monitor_path.into(),
            capabilities: P3_CAPABILITIES.iter().map(|s| (*s).to_string()).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LookupRequest {
    pub customer_id: String,
    pub booking_id: String,
    #[serde(rename = "type")]
    pub marker_type: String,
    /// `"hash"` (QR) or `"id"` (manual). Default: `hash`.
    #[serde(default = "default_lookup_mode")]
    pub mode: String,
}

fn default_lookup_mode() -> String {
    "hash".into()
}

impl LookupRequest {
    pub fn lookup_mode(&self) -> Result<LookupMode, String> {
        match self.mode.trim().to_ascii_lowercase().as_str() {
            "" | "hash" => Ok(LookupMode::Hash),
            "id" => Ok(LookupMode::Id),
            other => Err(format!(
                "Ungültiger lookup mode '{other}' (erwartet: hash|id)."
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LookupErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LookupResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer: Option<Kunde>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<LookupErrorBody>,
}

impl LookupResponse {
    pub fn success(customer: Kunde) -> Self {
        Self {
            ok: true,
            customer: Some(customer),
            error: None,
        }
    }

    pub fn failure(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            customer: None,
            error: Some(LookupErrorBody {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

/// `GET /v1/jobs/{correlation_id}` — mirrors status outbox (HANDOFF.md §8/§9).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobStatusResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<StatusOutboxV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<LookupErrorBody>,
}

impl JobStatusResponse {
    pub fn found(job: StatusOutboxV1) -> Self {
        Self {
            ok: true,
            job: Some(job),
            error: None,
        }
    }

    pub fn not_found(correlation_id: &str) -> Self {
        Self {
            ok: false,
            job: None,
            error: Some(LookupErrorBody {
                code: "job_not_found".into(),
                message: format!(
                    "Kein Status für correlation_id '{correlation_id}' (Outbox fehlt)."
                ),
            }),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            job: None,
            error: Some(LookupErrorBody {
                code: "invalid_request".into(),
                message: message.into(),
            }),
        }
    }
}

/// `POST /v1/handoff/ready` — wake monitor; no upload bypass.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HandoffReadyRequest {
    #[serde(default)]
    pub correlation_id: String,
    #[serde(default)]
    pub folder_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffReadyResponse {
    pub ok: bool,
    /// Monitor scan loop was signaled (wake). Does not enqueue uploads.
    pub woken: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<LookupErrorBody>,
}

impl HandoffReadyResponse {
    pub fn woken() -> Self {
        Self {
            ok: true,
            woken: true,
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_advertises_p3_capabilities_including_ready() {
        let h = HealthResponse::p3("0.1.0", r"\\host\aktuell");
        assert!(h.online);
        assert_eq!(
            h.capabilities,
            vec!["manifest-v1", "status-outbox", "lookup", "ready", "append-v1",]
        );
        assert!(h.capabilities.iter().any(|c| c == "ready"));
    }

    #[test]
    fn lookup_mode_parses() {
        let mut req = LookupRequest {
            customer_id: "c".into(),
            booking_id: "b".into(),
            marker_type: "Handcam".into(),
            mode: "hash".into(),
        };
        assert_eq!(req.lookup_mode().unwrap(), LookupMode::Hash);
        req.mode = "id".into();
        assert_eq!(req.lookup_mode().unwrap(), LookupMode::Id);
        req.mode = "nope".into();
        assert!(req.lookup_mode().is_err());
    }
}
