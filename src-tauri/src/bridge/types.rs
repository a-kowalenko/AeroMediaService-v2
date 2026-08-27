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
pub const CAPABILITY_HANDOFF_CANCEL: &str = "handoff-cancel";
pub const CAPABILITY_APPEND_V1: &str = "append-v1";
pub const CAPABILITY_PATHS_V1: &str = "paths-v1";

/// Base capabilities advertised by AMS (P3 + append + handoff cancel).
/// `paths-v1` is added dynamically when `ats_primary_smb_url` is set (P6a).
pub const P3_CAPABILITIES: [&str; 6] = [
    CAPABILITY_MANIFEST_V1,
    CAPABILITY_STATUS_OUTBOX,
    CAPABILITY_LOOKUP,
    CAPABILITY_READY,
    CAPABILITY_HANDOFF_CANCEL,
    CAPABILITY_APPEND_V1,
];

pub type BridgeCapabilities = Vec<String>;

/// Client-taugliche SMB-Hints für ATS (HANDOFF.md §9.3). Wire-Format bevorzugt `smb://`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AtsPathsHint {
    pub primary_smb_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub backup_smb_url: String,
}

impl AtsPathsHint {
    pub fn from_settings(primary: &str, backup: &str) -> Option<Self> {
        let primary_smb_url = to_smb_url(primary);
        if primary_smb_url.is_empty() {
            return None;
        }
        Some(Self {
            primary_smb_url,
            backup_smb_url: to_smb_url(backup),
        })
    }
}

/// Normalize operator input to wire-preferred `smb://` (UNC → smb).
pub fn to_smb_url(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.len() >= 6 && t[..6].eq_ignore_ascii_case("smb://") {
        // Keep scheme lower-case; normalize path separators.
        let rest = &t[6..];
        return format!("smb://{}", rest.replace('\\', "/"));
    }
    let unc = t
        .strip_prefix(r"\\")
        .or_else(|| t.strip_prefix("//"));
    if let Some(rest) = unc {
        return format!("smb://{}", rest.replace('\\', "/"));
    }
    t.to_string()
}

/// Whether `monitor_path` is UNC or `smb://` (P6d drift checks apply only then).
pub fn is_network_share_path(raw: &str) -> bool {
    let t = raw.trim();
    if t.is_empty() {
        return false;
    }
    if t.len() >= 6 && t[..6].eq_ignore_ascii_case("smb://") {
        return true;
    }
    t.starts_with(r"\\") || t.starts_with("//")
}

/// Case-insensitive SMB URL comparison with trailing slashes trimmed.
pub fn normalize_smb_for_compare(raw: &str) -> String {
    let mut s = to_smb_url(raw).to_ascii_lowercase();
    while s.ends_with('/') && s.len() > "smb://".len() {
        s.pop();
    }
    s
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PathHintsDrift {
    Disabled,
    Ok,
    MissingPrimary,
    Drift,
}

/// AMS-side path-hint diagnostics for Settings / header (P6d).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PathHintsStatus {
    pub bridge_enabled: bool,
    pub paths_v1: bool,
    pub monitor_is_network_share: bool,
    pub suggested_primary_smb_url: String,
    pub primary_smb_url: String,
    pub monitor_smb_url: String,
    pub drift: PathHintsDrift,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

impl PathHintsStatus {
    pub fn evaluate(bridge_enabled: bool, monitor_path: &str, primary_raw: &str) -> Self {
        let primary_smb_url = to_smb_url(primary_raw);
        let paths_v1 = !primary_smb_url.is_empty();
        let monitor_is_network_share = is_network_share_path(monitor_path);
        let monitor_smb_url = if monitor_is_network_share {
            to_smb_url(monitor_path)
        } else {
            String::new()
        };
        let suggested_primary_smb_url = monitor_smb_url.clone();

        let drift = if !bridge_enabled {
            PathHintsDrift::Disabled
        } else if !paths_v1 {
            PathHintsDrift::MissingPrimary
        } else if monitor_is_network_share {
            if normalize_smb_for_compare(&primary_smb_url)
                == normalize_smb_for_compare(&monitor_smb_url)
            {
                PathHintsDrift::Ok
            } else {
                PathHintsDrift::Drift
            }
        } else {
            PathHintsDrift::Ok
        };

        let warning = match drift {
            PathHintsDrift::Disabled | PathHintsDrift::Ok => None,
            PathHintsDrift::MissingPrimary => {
                if monitor_is_network_share && !monitor_smb_url.is_empty() {
                    Some(format!(
                        "Primär-Share fehlt — paths-v1 ist inaktiv. Monitor: {monitor_smb_url}"
                    ))
                } else {
                    Some(
                        "Primär-Share fehlt — Capability paths-v1 ist nicht aktiv.".into(),
                    )
                }
            }
            PathHintsDrift::Drift => Some(format!(
                "Primär-Share weicht vom Monitor-Pfad ab (Primär: {primary_smb_url}, Monitor: {monitor_smb_url})."
            )),
        };

        Self {
            bridge_enabled,
            paths_v1,
            monitor_is_network_share,
            suggested_primary_smb_url,
            primary_smb_url,
            monitor_smb_url,
            drift,
            warning,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthResponse {
    pub online: bool,
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub instance_id: String,
    pub monitor_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ats_paths: Option<AtsPathsHint>,
    pub capabilities: BridgeCapabilities,
}

impl HealthResponse {
    pub fn p3(
        version: impl Into<String>,
        monitor_path: impl Into<String>,
        display_name: impl Into<String>,
        instance_id: impl Into<String>,
    ) -> Self {
        Self::with_paths(version, monitor_path, display_name, instance_id, None)
    }

    pub fn with_paths(
        version: impl Into<String>,
        monitor_path: impl Into<String>,
        display_name: impl Into<String>,
        instance_id: impl Into<String>,
        ats_paths: Option<AtsPathsHint>,
    ) -> Self {
        let mut capabilities: BridgeCapabilities =
            P3_CAPABILITIES.iter().map(|s| (*s).to_string()).collect();
        let ats_paths = ats_paths.filter(|p| !p.primary_smb_url.trim().is_empty());
        if ats_paths.is_some() {
            capabilities.push(CAPABILITY_PATHS_V1.to_string());
        }
        Self {
            online: true,
            version: version.into(),
            display_name: display_name.into(),
            instance_id: instance_id.into(),
            monitor_path: monitor_path.into(),
            ats_paths,
            capabilities,
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

/// `POST /v1/handoff/cancel` — ATS aborted upload; drop pending handoff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HandoffCancelRequest {
    #[serde(default)]
    pub correlation_id: String,
    #[serde(default)]
    pub folder_name: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffCancelResponse {
    pub ok: bool,
    pub cancelled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<LookupErrorBody>,
}

impl HandoffCancelResponse {
    pub fn cancelled() -> Self {
        Self {
            ok: true,
            cancelled: true,
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_advertises_p3_capabilities_including_ready() {
        let h = HealthResponse::p3("0.1.0", r"\\host\aktuell", "Dropzone", "id-1");
        assert_eq!(h.display_name, "Dropzone");
        assert_eq!(h.instance_id, "id-1");
        assert!(h.online);
        assert!(h.ats_paths.is_none());
        assert_eq!(
            h.capabilities,
            vec![
                "manifest-v1",
                "status-outbox",
                "lookup",
                "ready",
                "handoff-cancel",
                "append-v1",
            ]
        );
        assert!(h.capabilities.iter().any(|c| c == "ready"));
        assert!(h.capabilities.iter().any(|c| c == "handoff-cancel"));
        assert!(!h.capabilities.iter().any(|c| c == "paths-v1"));
    }

    #[test]
    fn health_paths_v1_only_when_primary_set() {
        let with = HealthResponse::with_paths(
            "0.1.0",
            r"D:\Shares\aktuell",
            "Dropzone",
            "id-1",
            AtsPathsHint::from_settings(r"\\169.254.169.254\aktuell", "smb://host/aktuell-backup"),
        );
        assert!(with.capabilities.iter().any(|c| c == "paths-v1"));
        let paths = with.ats_paths.expect("ats_paths");
        assert_eq!(paths.primary_smb_url, "smb://169.254.169.254/aktuell");
        assert_eq!(paths.backup_smb_url, "smb://host/aktuell-backup");

        let without = HealthResponse::with_paths(
            "0.1.0",
            r"D:\Shares\aktuell",
            "Dropzone",
            "id-1",
            AtsPathsHint::from_settings("", "smb://host/backup"),
        );
        assert!(without.ats_paths.is_none());
        assert!(!without.capabilities.iter().any(|c| c == "paths-v1"));
    }

    #[test]
    fn to_smb_url_normalizes_unc() {
        assert_eq!(to_smb_url(r"\\host\aktuell"), "smb://host/aktuell");
        assert_eq!(to_smb_url("//host/aktuell"), "smb://host/aktuell");
        assert_eq!(to_smb_url("SMB://Host/Share"), "smb://Host/Share");
        assert_eq!(to_smb_url(""), "");
        assert_eq!(to_smb_url("  "), "");
    }

    #[test]
    fn is_network_share_path_detects_unc_and_smb() {
        assert!(is_network_share_path(r"\\169.254.169.254\aktuell"));
        assert!(is_network_share_path("smb://host/share"));
        assert!(is_network_share_path("//host/share"));
        assert!(!is_network_share_path(r"D:\Shares\aktuell"));
        assert!(!is_network_share_path(""));
    }

    #[test]
    fn path_hints_missing_primary_when_bridge_on() {
        let s = PathHintsStatus::evaluate(true, r"\\host\aktuell", "");
        assert!(!s.paths_v1);
        assert_eq!(s.drift, PathHintsDrift::MissingPrimary);
        assert!(s.warning.as_ref().unwrap().contains("paths-v1"));
        assert_eq!(s.suggested_primary_smb_url, "smb://host/aktuell");
    }

    #[test]
    fn path_hints_drift_when_primary_differs_from_monitor() {
        let s = PathHintsStatus::evaluate(
            true,
            r"\\169.254.169.254\aktuell",
            "smb://other/aktuell",
        );
        assert!(s.paths_v1);
        assert_eq!(s.drift, PathHintsDrift::Drift);
        assert!(s.warning.as_ref().unwrap().contains("weicht"));
    }

    #[test]
    fn path_hints_ok_when_normalized_match() {
        let s = PathHintsStatus::evaluate(
            true,
            r"\\Host\Aktuell\",
            "SMB://host/Aktuell",
        );
        assert_eq!(s.drift, PathHintsDrift::Ok);
        assert!(s.paths_v1);
        assert!(s.warning.is_none());
    }

    #[test]
    fn path_hints_ok_for_local_monitor_with_primary() {
        let s = PathHintsStatus::evaluate(true, r"D:\Shares\aktuell", "smb://host/aktuell");
        assert_eq!(s.drift, PathHintsDrift::Ok);
        assert!(!s.monitor_is_network_share);
    }

    #[test]
    fn path_hints_disabled_when_bridge_off() {
        let s = PathHintsStatus::evaluate(false, r"\\host\aktuell", "");
        assert_eq!(s.drift, PathHintsDrift::Disabled);
        assert!(s.warning.is_none());
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
