//! Phase 23 / B0 — Cloud diagnostics ingest forward (AMS API key → Cloud D2).
//!
//! Reuses Skydive-Media secrets (`custom_api_url`, `custom_api_bearer_token`).
//! Cloud endpoint: `POST {origin}/api/ats/v1/diagnostics/events`
//! (Permission `ats_diagnostics_ingest`). ATS JSON body is forwarded 1:1.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ats_client_token::{
    CloudIssueConfigError, ERR_CLOUD_FORBIDDEN, ERR_CLOUD_UNAUTHORIZED,
    ERR_CLOUD_UNREACHABLE, SECRET_CLOUD_API_KEY, SECRET_CLOUD_BASE_URL,
};
use super::{api_origin, summarize_api_error_body};
use crate::storage::logging;
use crate::storage::secrets::{self, SecretError};

/// Cloud permission required on the AMS API key (Cloud Admin / Key-Update).
pub const CLOUD_PERMISSION_ATS_DIAGNOSTICS_INGEST: &str = "ats_diagnostics_ingest";

/// Relative path on the Cloud origin (after stripping trailing `/api`).
pub const CLOUD_DIAGNOSTICS_EVENTS_PATH: &str = "/api/ats/v1/diagnostics/events";

/// Bridge/API error for other Cloud ingest failures.
pub const ERR_CLOUD_INGEST_FAILED: &str = "cloud_ingest_failed";

const INGEST_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudDiagnosticsCredentials {
    /// Raw base from secrets (may include `/api`).
    pub api_base_url: String,
    /// Bearer API key (not logged).
    pub api_key: String,
    /// Public Cloud origin.
    pub cloud_base_url: String,
    /// Absolute URL for `POST …/diagnostics/events`.
    pub ingest_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsIngestError {
    pub code: String,
    pub message: String,
    pub http_status: u16,
}

impl DiagnosticsIngestError {
    pub fn config(err: CloudIssueConfigError) -> Self {
        Self {
            code: err.code().into(),
            message: err.message().into(),
            http_status: 503,
        }
    }

    pub fn from_cloud_status(status: u16, body: &str) -> Self {
        let snippet = summarize_api_error_body(body, 200);
        match status {
            401 => Self {
                code: ERR_CLOUD_UNAUTHORIZED.into(),
                message: format!(
                    "Cloud lehnt API-Key ab (401). Key prüfen. Details: {snippet}"
                ),
                http_status: 502,
            },
            403 => Self {
                code: ERR_CLOUD_FORBIDDEN.into(),
                message: format!(
                    "Cloud verweigert Diagnostik-Ingest (403). Permission „{CLOUD_PERMISSION_ATS_DIAGNOSTICS_INGEST}“ am API-Key fehlt. Details: {snippet}"
                ),
                http_status: 502,
            },
            400 | 413 | 422 => Self {
                code: ERR_CLOUD_INGEST_FAILED.into(),
                message: format!(
                    "Cloud Diagnostik-Validierung fehlgeschlagen ({status}): {snippet}"
                ),
                http_status: 502,
            },
            429 => Self {
                code: ERR_CLOUD_INGEST_FAILED.into(),
                message: format!(
                    "Cloud Diagnostik Rate-Limit (429): {snippet}"
                ),
                http_status: 502,
            },
            _ => Self {
                code: ERR_CLOUD_INGEST_FAILED.into(),
                message: format!(
                    "Cloud Diagnostik-Ingest fehlgeschlagen (HTTP {status}): {snippet}"
                ),
                http_status: 502,
            },
        }
    }

    pub fn unreachable(err: impl std::fmt::Display) -> Self {
        Self {
            code: ERR_CLOUD_UNREACHABLE.into(),
            message: format!("Cloud nicht erreichbar für Diagnostik-Ingest: {err}"),
            http_status: 502,
        }
    }

    pub fn invalid_response(detail: impl Into<String>) -> Self {
        Self {
            code: ERR_CLOUD_INGEST_FAILED.into(),
            message: detail.into(),
            http_status: 502,
        }
    }
}

impl std::fmt::Display for DiagnosticsIngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}]", self.message, self.code)
    }
}

impl std::error::Error for DiagnosticsIngestError {}

/// Success payload mapped from Cloud D2 (forwarded to ATS on Bridge in B1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticsIngestResponse {
    pub ok: bool,
    pub accepted: u64,
    pub duplicates: u64,
}

/// Settings / diagnostics status (no secrets in the payload).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CloudDiagnosticsIngestStatus {
    pub configured: bool,
    pub has_base_url: bool,
    pub has_api_key: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cloud_base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    pub required_permission: String,
}

impl CloudDiagnosticsIngestStatus {
    pub fn evaluate(base_url: Option<&str>, api_key: Option<&str>) -> Self {
        let base = base_url.map(str::trim).filter(|s| !s.is_empty());
        let key = api_key.map(str::trim).filter(|s| !s.is_empty());
        let has_base_url = base.is_some();
        let has_api_key = key.is_some();
        let cloud_base_url = base.map(api_origin).unwrap_or_default();

        let error = if !has_base_url {
            Some(CloudIssueConfigError::MissingBaseUrl)
        } else if !has_api_key {
            Some(CloudIssueConfigError::MissingApiKey)
        } else {
            None
        };

        let warning = error.as_ref().map(|e| {
            format!(
                "{} Cloud-Admin: Permission „{}“ am API-Key setzen.",
                e.message(),
                CLOUD_PERMISSION_ATS_DIAGNOSTICS_INGEST
            )
        });

        Self {
            configured: error.is_none(),
            has_base_url,
            has_api_key,
            cloud_base_url,
            error_code: error.as_ref().map(|e| e.code().to_string()),
            warning,
            required_permission: CLOUD_PERMISSION_ATS_DIAGNOSTICS_INGEST.to_string(),
        }
    }
}

/// Build ingest URL and credentials from raw secret values (no keyring I/O).
pub fn credentials_from_values(
    api_base_url: &str,
    api_key: &str,
) -> Result<CloudDiagnosticsCredentials, CloudIssueConfigError> {
    let api_base_url = api_base_url.trim();
    let api_key = api_key.trim();
    if api_base_url.is_empty() {
        return Err(CloudIssueConfigError::MissingBaseUrl);
    }
    if api_key.is_empty() {
        return Err(CloudIssueConfigError::MissingApiKey);
    }
    let cloud_base_url = api_origin(api_base_url);
    let ingest_url = format!(
        "{}{}",
        cloud_base_url.trim_end_matches('/'),
        CLOUD_DIAGNOSTICS_EVENTS_PATH
    );
    Ok(CloudDiagnosticsCredentials {
        api_base_url: api_base_url.to_string(),
        api_key: api_key.to_string(),
        cloud_base_url,
        ingest_url,
    })
}

/// Read Skydive-Media secrets and resolve Cloud ingest credentials.
pub fn resolve_cloud_diagnostics_credentials(
) -> Result<CloudDiagnosticsCredentials, CloudIssueConfigError> {
    let base = read_optional_secret(SECRET_CLOUD_BASE_URL);
    let key = read_optional_secret(SECRET_CLOUD_API_KEY);
    match credentials_from_values(base.as_deref().unwrap_or(""), key.as_deref().unwrap_or("")) {
        Ok(creds) => Ok(creds),
        Err(err) => {
            logging::log_warn(&format!(
                "Cloud-Diagnostik-Ingest nicht konfiguriert: {} [{}]",
                err.message(),
                err.code()
            ));
            Err(err)
        }
    }
}

/// Status snapshot for Settings / IPC (reads secrets; never returns the key).
pub fn cloud_diagnostics_ingest_status() -> CloudDiagnosticsIngestStatus {
    let base = read_optional_secret(SECRET_CLOUD_BASE_URL);
    let key = read_optional_secret(SECRET_CLOUD_API_KEY);
    CloudDiagnosticsIngestStatus::evaluate(base.as_deref(), key.as_deref())
}

/// Map Cloud JSON into the ingest response for ATS.
pub fn map_cloud_ingest_response(value: &Value) -> Result<DiagnosticsIngestResponse, DiagnosticsIngestError> {
    let ok = value
        .get("ok")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| {
            DiagnosticsIngestError::invalid_response(
                "Cloud Diagnostik-Response ohne ok.".to_string(),
            )
        })?;

    let accepted = value
        .get("accepted")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_i64().map(|n| n.max(0) as u64))
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(0);

    let duplicates = value
        .get("duplicates")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_i64().map(|n| n.max(0) as u64))
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(0);

    Ok(DiagnosticsIngestResponse {
        ok,
        accepted,
        duplicates,
    })
}

/// Forward ATS diagnostics batch JSON to Cloud with AMS API key.
pub async fn forward_diagnostics_events(
    creds: &CloudDiagnosticsCredentials,
    body: &Value,
) -> Result<DiagnosticsIngestResponse, DiagnosticsIngestError> {
    forward_diagnostics_events_at_url(&creds.ingest_url, &creds.api_key, body).await
}

/// Same as [`forward_diagnostics_events`] but with explicit URL (tests / overrides).
pub async fn forward_diagnostics_events_at_url(
    ingest_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<DiagnosticsIngestResponse, DiagnosticsIngestError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(INGEST_TIMEOUT_SECS))
        .build()
        .map_err(DiagnosticsIngestError::unreachable)?;

    let response = client
        .post(ingest_url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .map_err(DiagnosticsIngestError::unreachable)?;

    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(DiagnosticsIngestError::unreachable)?;

    if !(200..300).contains(&status) {
        logging::log_warn(&format!(
            "Cloud diagnostics ingest HTTP {status}: {}",
            summarize_api_error_body(&text, 160)
        ));
        return Err(DiagnosticsIngestError::from_cloud_status(status, &text));
    }

    let value: Value = serde_json::from_str(&text).map_err(|e| {
        DiagnosticsIngestError::invalid_response(format!(
            "Cloud Diagnostik-Response ist kein JSON: {e}"
        ))
    })?;

    map_cloud_ingest_response(&value)
}

fn read_optional_secret(key: &str) -> Option<String> {
    match secrets::get_secret(key) {
        Ok(v) => v.filter(|s| !s.trim().is_empty()),
        Err(SecretError::EmptyKey) => None,
        Err(e) => {
            logging::log_warn(&format!(
                "Cloud-Diagnostik: Secret „{key}“ konnte nicht gelesen werden: {e}"
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ats_client_token::{ERR_CLOUD_API_KEY_MISSING, ERR_CLOUD_BASE_MISSING};
    use crate::storage::secrets::{clear_test_secrets, save_secret};
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::json;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;

    #[test]
    fn evaluate_missing_base() {
        let s = CloudDiagnosticsIngestStatus::evaluate(None, Some("key"));
        assert!(!s.configured);
        assert_eq!(s.error_code.as_deref(), Some(ERR_CLOUD_BASE_MISSING));
        assert_eq!(
            s.required_permission,
            CLOUD_PERMISSION_ATS_DIAGNOSTICS_INGEST
        );
    }

    #[test]
    fn evaluate_missing_key() {
        let s = CloudDiagnosticsIngestStatus::evaluate(Some("https://cloud.example/api"), None);
        assert!(!s.configured);
        assert_eq!(s.error_code.as_deref(), Some(ERR_CLOUD_API_KEY_MISSING));
        assert_eq!(s.cloud_base_url, "https://cloud.example");
        assert!(s.warning.as_ref().unwrap().contains("ats_diagnostics_ingest"));
    }

    #[test]
    fn credentials_build_ingest_url() {
        let c = credentials_from_values("https://host.example/api", "tok").unwrap();
        assert_eq!(c.cloud_base_url, "https://host.example");
        assert_eq!(
            c.ingest_url,
            "https://host.example/api/ats/v1/diagnostics/events"
        );
    }

    #[test]
    fn credentials_error_codes() {
        assert_eq!(
            credentials_from_values("", "k").unwrap_err().code(),
            ERR_CLOUD_BASE_MISSING
        );
        assert_eq!(
            credentials_from_values("https://x", "  ").unwrap_err().code(),
            ERR_CLOUD_API_KEY_MISSING
        );
    }

    #[test]
    fn resolve_from_secrets_ok_and_missing() {
        let _lock = crate::storage::secrets::test_secrets_lock();
        clear_test_secrets();
        assert_eq!(
            resolve_cloud_diagnostics_credentials().unwrap_err().code(),
            ERR_CLOUD_BASE_MISSING
        );

        save_secret(SECRET_CLOUD_BASE_URL, "https://cloud.test/api").unwrap();
        save_secret(SECRET_CLOUD_API_KEY, "key.secret").unwrap();
        let c = resolve_cloud_diagnostics_credentials().unwrap();
        assert_eq!(
            c.ingest_url,
            "https://cloud.test/api/ats/v1/diagnostics/events"
        );
        assert!(cloud_diagnostics_ingest_status().configured);
        clear_test_secrets();
    }

    #[test]
    fn maps_cloud_ingest_response() {
        let mapped = map_cloud_ingest_response(&json!({
            "ok": true,
            "accepted": 2,
            "duplicates": 1
        }))
        .unwrap();
        assert!(mapped.ok);
        assert_eq!(mapped.accepted, 2);
        assert_eq!(mapped.duplicates, 1);
    }

    #[test]
    fn maps_cloud_error_status() {
        let u = DiagnosticsIngestError::from_cloud_status(401, r#"{"error":"bad key"}"#);
        assert_eq!(u.code, ERR_CLOUD_UNAUTHORIZED);

        let f = DiagnosticsIngestError::from_cloud_status(403, "forbidden");
        assert_eq!(f.code, ERR_CLOUD_FORBIDDEN);
        assert!(f.message.contains(CLOUD_PERMISSION_ATS_DIAGNOSTICS_INGEST));

        let r = DiagnosticsIngestError::from_cloud_status(429, "slow down");
        assert_eq!(r.code, ERR_CLOUD_INGEST_FAILED);
        assert!(r.message.contains("429"));
    }

    #[derive(Clone)]
    struct MockCloudState {
        last_auth: Arc<Mutex<Option<String>>>,
        last_body: Arc<Mutex<Option<Value>>>,
        status: u16,
        body: Value,
    }

    async fn mock_cloud_ingest(
        State(state): State<MockCloudState>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        *state.last_auth.lock().unwrap() = auth;
        *state.last_body.lock().unwrap() = Some(body);
        (
            StatusCode::from_u16(state.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(state.body.clone()),
        )
    }

    async fn start_mock_cloud(
        status: u16,
        body: Value,
    ) -> (String, oneshot::Sender<()>, MockCloudState) {
        let state = MockCloudState {
            last_auth: Arc::new(Mutex::new(None)),
            last_body: Arc::new(Mutex::new(None)),
            status,
            body,
        };
        let app = Router::new()
            .route(
                "/api/ats/v1/diagnostics/events",
                post(mock_cloud_ingest),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await
                .ok();
        });
        (format!("http://{addr}"), tx, state)
    }

    #[tokio::test]
    async fn forward_proxies_body_and_maps_success() {
        let (base, shutdown, state) = start_mock_cloud(
            200,
            json!({ "ok": true, "accepted": 1, "duplicates": 0 }),
        )
        .await;

        let batch = json!({
            "events": [{
                "event_id": "ev-1",
                "severity": "error",
                "message": "test"
            }]
        });
        let ingest_url = format!("{base}/api/ats/v1/diagnostics/events");
        let resp = forward_diagnostics_events_at_url(&ingest_url, "kid.secret", &batch)
            .await
            .unwrap();

        assert!(resp.ok);
        assert_eq!(resp.accepted, 1);
        assert_eq!(resp.duplicates, 0);
        assert_eq!(
            state.last_auth.lock().unwrap().as_deref(),
            Some("Bearer kid.secret")
        );
        assert_eq!(
            state.last_body.lock().unwrap().clone().unwrap(),
            batch
        );

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn forward_maps_cloud_403() {
        let (base, shutdown, _) =
            start_mock_cloud(403, json!({"error": "no permission"})).await;
        let err = forward_diagnostics_events_at_url(
            &format!("{base}/api/ats/v1/diagnostics/events"),
            "kid.secret",
            &json!({"events": []}),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ERR_CLOUD_FORBIDDEN);
        let _ = shutdown.send(());
    }
}
