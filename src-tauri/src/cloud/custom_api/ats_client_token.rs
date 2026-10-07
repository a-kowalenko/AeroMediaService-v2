//! Phase 22 / A0–A2 — Cloud base + API-Key readiness, JWT issue proxy, Health hint.
//!
//! Reuses Skydive-Media secrets (`custom_api_url`, `custom_api_bearer_token`).
//! Cloud endpoint: `POST {origin}/api/ats/v1/client-token` (Permission `ats_client_token`).
//! Bridge route: `POST /v1/client-token` (A1).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{api_origin, summarize_api_error_body};
use crate::storage::logging;
use crate::storage::secrets::{self, SecretError};

/// Keyring key: Cloud / Skydive Media base URL (often ends with `/api`).
pub const SECRET_CLOUD_BASE_URL: &str = "custom_api_url";
/// Keyring key: Cloud API key (`keyId.secret`); must include permission `ats_client_token`.
pub const SECRET_CLOUD_API_KEY: &str = "custom_api_bearer_token";

/// Cloud permission required on the AMS API key (Cloud Admin / Key-Update).
pub const CLOUD_PERMISSION_ATS_CLIENT_TOKEN: &str = "ats_client_token";

/// Relative path on the Cloud origin (after stripping trailing `/api`).
pub const CLOUD_CLIENT_TOKEN_PATH: &str = "/api/ats/v1/client-token";

/// Bridge/API error code when `custom_api_url` is missing.
pub const ERR_CLOUD_BASE_MISSING: &str = "cloud_base_missing";
/// Bridge/API error code when `custom_api_bearer_token` is missing.
pub const ERR_CLOUD_API_KEY_MISSING: &str = "cloud_api_key_missing";
/// Bridge error when `X-Ats-Instance-Id` is missing/empty.
pub const ERR_ATS_INSTANCE_ID_REQUIRED: &str = "ats_instance_id_required";
/// Bridge error when Cloud is unreachable / transport fails.
pub const ERR_CLOUD_UNREACHABLE: &str = "cloud_unreachable";
/// Bridge error when Cloud rejects the AMS API key (401).
pub const ERR_CLOUD_UNAUTHORIZED: &str = "cloud_unauthorized";
/// Bridge error when Cloud key lacks `ats_client_token` (403).
pub const ERR_CLOUD_FORBIDDEN: &str = "cloud_forbidden";
/// Bridge error for other Cloud issue failures.
pub const ERR_CLOUD_ISSUE_FAILED: &str = "cloud_issue_failed";

const ISSUE_TIMEOUT_SECS: u64 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudIssueCredentials {
    /// Raw base from secrets (may include `/api`).
    pub api_base_url: String,
    /// Bearer API key (not logged).
    pub api_key: String,
    /// Public Cloud origin for ATS (`cloud_base_url` in issue response).
    pub cloud_base_url: String,
    /// Absolute URL for `POST …/client-token`.
    pub issue_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudIssueConfigError {
    MissingBaseUrl,
    MissingApiKey,
}

impl CloudIssueConfigError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingBaseUrl => ERR_CLOUD_BASE_MISSING,
            Self::MissingApiKey => ERR_CLOUD_API_KEY_MISSING,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::MissingBaseUrl => {
                "Cloud-Base-URL fehlt (Secret custom_api_url). Für ATS Client-Token Issue erforderlich."
            }
            Self::MissingApiKey => {
                "Cloud-API-Key fehlt (Secret custom_api_bearer_token). Key braucht Permission ats_client_token."
            }
        }
    }
}

impl std::fmt::Display for CloudIssueConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message(), self.code())
    }
}

impl std::error::Error for CloudIssueConfigError {}

/// ATS identity fields forwarded to Cloud issue (from Bridge headers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtsClientIdentity {
    pub ats_instance_id: String,
    pub ats_hostname: String,
    pub ats_version: String,
    pub ats_app: String,
}

impl AtsClientIdentity {
    /// Require a real `X-Ats-Instance-Id` (not empty / not degraded `unknown:…`).
    pub fn require_instance_id(
        instance_id: Option<&str>,
        hostname: Option<&str>,
        version: Option<&str>,
        app: Option<&str>,
    ) -> Result<Self, ClientTokenIssueError> {
        let ats_instance_id = instance_id.map(str::trim).unwrap_or("").to_string();
        if ats_instance_id.is_empty() || ats_instance_id.starts_with("unknown:") {
            return Err(ClientTokenIssueError {
                code: ERR_ATS_INSTANCE_ID_REQUIRED.into(),
                message: "X-Ats-Instance-Id ist Pflicht für Client-Token Issue.".into(),
                http_status: 400,
            });
        }
        Ok(Self {
            ats_instance_id,
            ats_hostname: hostname.map(str::trim).unwrap_or("").to_string(),
            ats_version: version.map(str::trim).unwrap_or("").to_string(),
            ats_app: {
                let a = app.map(str::trim).unwrap_or("");
                if a.is_empty() {
                    "AeroTandemStudio".into()
                } else {
                    a.to_string()
                }
            },
        })
    }
}

/// Body AMS sends to Cloud `POST /api/ats/v1/client-token`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudClientTokenRequest {
    pub ats_instance_id: String,
    pub ams_server_instance_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ats_hostname: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ats_version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ats_app: String,
}

impl CloudClientTokenRequest {
    pub fn build(identity: &AtsClientIdentity, ams_server_instance_id: &str) -> Self {
        Self {
            ats_instance_id: identity.ats_instance_id.clone(),
            ams_server_instance_id: ams_server_instance_id.trim().to_string(),
            ats_hostname: identity.ats_hostname.clone(),
            ats_version: identity.ats_version.clone(),
            ats_app: identity.ats_app.clone(),
        }
    }
}

/// Success payload returned to ATS on Bridge `POST /v1/client-token`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClientTokenResponse {
    pub access_token: String,
    #[serde(default = "default_token_type")]
    pub token_type: String,
    pub expires_at: String,
    pub expires_in: i64,
    pub cloud_base_url: String,
    #[serde(default)]
    pub scope: Vec<String>,
}

fn default_token_type() -> String {
    "Bearer".into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientTokenIssueError {
    pub code: String,
    pub message: String,
    pub http_status: u16,
}

impl ClientTokenIssueError {
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
                    "Cloud verweigert Client-Token (403). Permission „{CLOUD_PERMISSION_ATS_CLIENT_TOKEN}“ am API-Key fehlt. Details: {snippet}"
                ),
                http_status: 502,
            },
            400 | 422 => Self {
                code: ERR_CLOUD_ISSUE_FAILED.into(),
                message: format!(
                    "Cloud Client-Token Validierung fehlgeschlagen ({status}): {snippet}"
                ),
                http_status: 502,
            },
            _ => Self {
                code: ERR_CLOUD_ISSUE_FAILED.into(),
                message: format!(
                    "Cloud Client-Token Issue fehlgeschlagen (HTTP {status}): {snippet}"
                ),
                http_status: 502,
            },
        }
    }

    pub fn unreachable(err: impl std::fmt::Display) -> Self {
        Self {
            code: ERR_CLOUD_UNREACHABLE.into(),
            message: format!("Cloud nicht erreichbar für Client-Token Issue: {err}"),
            http_status: 502,
        }
    }

    pub fn invalid_response(detail: impl Into<String>) -> Self {
        Self {
            code: ERR_CLOUD_ISSUE_FAILED.into(),
            message: detail.into(),
            http_status: 502,
        }
    }
}

impl std::fmt::Display for ClientTokenIssueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}]", self.message, self.code)
    }
}

impl std::error::Error for ClientTokenIssueError {}

/// Settings / diagnostics status (no secrets in the payload).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CloudLookupIssueStatus {
    pub configured: bool,
    pub has_base_url: bool,
    pub has_api_key: bool,
    /// Public origin when base is set (safe to show in UI).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cloud_base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Operator reminder: Cloud key must include this permission.
    pub required_permission: String,
}

impl CloudLookupIssueStatus {
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
                CLOUD_PERMISSION_ATS_CLIENT_TOKEN
            )
        });

        Self {
            configured: error.is_none(),
            has_base_url,
            has_api_key,
            cloud_base_url,
            error_code: error.as_ref().map(|e| e.code().to_string()),
            warning,
            required_permission: CLOUD_PERMISSION_ATS_CLIENT_TOKEN.to_string(),
        }
    }
}

/// Build issue URL and credentials from raw secret values (no keyring I/O).
pub fn credentials_from_values(
    api_base_url: &str,
    api_key: &str,
) -> Result<CloudIssueCredentials, CloudIssueConfigError> {
    let api_base_url = api_base_url.trim();
    let api_key = api_key.trim();
    if api_base_url.is_empty() {
        return Err(CloudIssueConfigError::MissingBaseUrl);
    }
    if api_key.is_empty() {
        return Err(CloudIssueConfigError::MissingApiKey);
    }
    let cloud_base_url = api_origin(api_base_url);
    let issue_url = format!(
        "{}{}",
        cloud_base_url.trim_end_matches('/'),
        CLOUD_CLIENT_TOKEN_PATH
    );
    Ok(CloudIssueCredentials {
        api_base_url: api_base_url.to_string(),
        api_key: api_key.to_string(),
        cloud_base_url,
        issue_url,
    })
}

/// Read Skydive-Media secrets and resolve Cloud issue credentials.
/// Logs a clear warn (with error code) when Base or Key is missing — no silent fail.
pub fn resolve_cloud_issue_credentials() -> Result<CloudIssueCredentials, CloudIssueConfigError> {
    let base = read_optional_secret(SECRET_CLOUD_BASE_URL);
    let key = read_optional_secret(SECRET_CLOUD_API_KEY);
    match credentials_from_values(base.as_deref().unwrap_or(""), key.as_deref().unwrap_or("")) {
        Ok(creds) => Ok(creds),
        Err(err) => {
            logging::log_warn(&format!(
                "Cloud-Lookup client-token nicht konfiguriert: {} [{}]",
                err.message(),
                err.code()
            ));
            Err(err)
        }
    }
}

/// Status snapshot for Settings / IPC (reads secrets; never returns the key).
pub fn cloud_lookup_issue_status() -> CloudLookupIssueStatus {
    let base = read_optional_secret(SECRET_CLOUD_BASE_URL);
    let key = read_optional_secret(SECRET_CLOUD_API_KEY);
    CloudLookupIssueStatus::evaluate(base.as_deref(), key.as_deref())
}

/// Map Cloud JSON into the Bridge response; fill `cloud_base_url` from AMS creds if absent.
pub fn map_cloud_issue_response(
    value: &Value,
    fallback_cloud_base_url: &str,
) -> Result<ClientTokenResponse, ClientTokenIssueError> {
    let access_token = value
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ClientTokenIssueError::invalid_response(
                "Cloud Client-Token Response ohne access_token.".to_string(),
            )
        })?
        .to_string();

    let expires_at = value
        .get("expires_at")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_string();

    let expires_in = value
        .get("expires_in")
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_u64().map(|n| n as i64))
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(0);

    if expires_at.is_empty() && expires_in <= 0 {
        return Err(ClientTokenIssueError::invalid_response(
            "Cloud Client-Token Response ohne expires_at/expires_in.".to_string(),
        ));
    }

    let cloud_base_url = value
        .get("cloud_base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_cloud_base_url)
        .trim()
        .to_string();
    if cloud_base_url.is_empty() {
        return Err(ClientTokenIssueError::invalid_response(
            "cloud_base_url fehlt in Cloud-Response und AMS-Config.".to_string(),
        ));
    }

    let token_type = value
        .get("token_type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Bearer")
        .to_string();

    let scope = match value.get("scope") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) if !s.trim().is_empty() => vec![s.trim().to_string()],
        _ => vec!["customer.lookup".into()],
    };

    Ok(ClientTokenResponse {
        access_token,
        token_type,
        expires_at,
        expires_in,
        cloud_base_url,
        scope,
    })
}

/// Proxy Cloud C1 issue with AMS API key. Does not log tokens.
pub async fn issue_client_token(
    creds: &CloudIssueCredentials,
    identity: &AtsClientIdentity,
    ams_server_instance_id: &str,
) -> Result<ClientTokenResponse, ClientTokenIssueError> {
    issue_client_token_at_url(
        &creds.issue_url,
        &creds.api_key,
        &creds.cloud_base_url,
        identity,
        ams_server_instance_id,
    )
    .await
}

/// Same as [`issue_client_token`] but with explicit URL (tests / overrides).
pub async fn issue_client_token_at_url(
    issue_url: &str,
    api_key: &str,
    fallback_cloud_base_url: &str,
    identity: &AtsClientIdentity,
    ams_server_instance_id: &str,
) -> Result<ClientTokenResponse, ClientTokenIssueError> {
    let body = CloudClientTokenRequest::build(identity, ams_server_instance_id);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(ISSUE_TIMEOUT_SECS))
        .build()
        .map_err(ClientTokenIssueError::unreachable)?;

    let response = client
        .post(issue_url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(ClientTokenIssueError::unreachable)?;

    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|e| ClientTokenIssueError::unreachable(e))?;

    if !(200..300).contains(&status) {
        logging::log_warn(&format!(
            "Cloud client-token Issue HTTP {status}: {}",
            summarize_api_error_body(&text, 160)
        ));
        return Err(ClientTokenIssueError::from_cloud_status(status, &text));
    }

    let value: Value = serde_json::from_str(&text).map_err(|e| {
        ClientTokenIssueError::invalid_response(format!(
            "Cloud Client-Token Response ist kein JSON: {e}"
        ))
    })?;

    map_cloud_issue_response(&value, fallback_cloud_base_url)
}

fn read_optional_secret(key: &str) -> Option<String> {
    match secrets::get_secret(key) {
        Ok(v) => v.filter(|s| !s.trim().is_empty()),
        Err(SecretError::EmptyKey) => None,
        Err(e) => {
            logging::log_warn(&format!(
                "Cloud-Lookup: Secret „{key}“ konnte nicht gelesen werden: {e}"
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let s = CloudLookupIssueStatus::evaluate(None, Some("key"));
        assert!(!s.configured);
        assert!(!s.has_base_url);
        assert!(s.has_api_key);
        assert_eq!(s.error_code.as_deref(), Some(ERR_CLOUD_BASE_MISSING));
        assert!(s.warning.as_ref().unwrap().contains("custom_api_url"));
        assert_eq!(s.required_permission, CLOUD_PERMISSION_ATS_CLIENT_TOKEN);
    }

    #[test]
    fn evaluate_missing_key() {
        let s = CloudLookupIssueStatus::evaluate(Some("https://cloud.example/api"), None);
        assert!(!s.configured);
        assert!(s.has_base_url);
        assert!(!s.has_api_key);
        assert_eq!(s.error_code.as_deref(), Some(ERR_CLOUD_API_KEY_MISSING));
        assert_eq!(s.cloud_base_url, "https://cloud.example");
        assert!(s.warning.as_ref().unwrap().contains("ats_client_token"));
    }

    #[test]
    fn evaluate_ok() {
        let s = CloudLookupIssueStatus::evaluate(
            Some("https://cloud.example/api/"),
            Some("kid.secret"),
        );
        assert!(s.configured);
        assert!(s.error_code.is_none());
        assert!(s.warning.is_none());
        assert_eq!(s.cloud_base_url, "https://cloud.example");
    }

    #[test]
    fn credentials_build_issue_url() {
        let c = credentials_from_values("https://host.example/api", "tok").unwrap();
        assert_eq!(c.cloud_base_url, "https://host.example");
        assert_eq!(
            c.issue_url,
            "https://host.example/api/ats/v1/client-token"
        );
        assert_eq!(c.api_key, "tok");
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
            resolve_cloud_issue_credentials().unwrap_err().code(),
            ERR_CLOUD_BASE_MISSING
        );

        save_secret(SECRET_CLOUD_BASE_URL, "https://cloud.test/api").unwrap();
        assert_eq!(
            resolve_cloud_issue_credentials().unwrap_err().code(),
            ERR_CLOUD_API_KEY_MISSING
        );

        save_secret(SECRET_CLOUD_API_KEY, "key.secret").unwrap();
        let c = resolve_cloud_issue_credentials().unwrap();
        assert_eq!(c.issue_url, "https://cloud.test/api/ats/v1/client-token");
        assert_eq!(
            cloud_lookup_issue_status().cloud_base_url,
            "https://cloud.test"
        );
        assert!(cloud_lookup_issue_status().configured);
        clear_test_secrets();
    }

    #[test]
    fn identity_requires_instance_id() {
        let err = AtsClientIdentity::require_instance_id(None, Some("PC"), None, None).unwrap_err();
        assert_eq!(err.code, ERR_ATS_INSTANCE_ID_REQUIRED);
        assert_eq!(err.http_status, 400);

        let degraded =
            AtsClientIdentity::require_instance_id(Some("unknown:pc"), Some("PC"), None, None)
                .unwrap_err();
        assert_eq!(degraded.code, ERR_ATS_INSTANCE_ID_REQUIRED);

        let ok = AtsClientIdentity::require_instance_id(
            Some(" ats-uuid "),
            Some(" Studio "),
            Some("1.2.3"),
            None,
        )
        .unwrap();
        assert_eq!(ok.ats_instance_id, "ats-uuid");
        assert_eq!(ok.ats_hostname, "Studio");
        assert_eq!(ok.ats_version, "1.2.3");
        assert_eq!(ok.ats_app, "AeroTandemStudio");
    }

    #[test]
    fn builds_cloud_request_body() {
        let identity = AtsClientIdentity {
            ats_instance_id: "ats-1".into(),
            ats_hostname: "Studio-PC".into(),
            ats_version: "2.0.0".into(),
            ats_app: "AeroTandemStudio".into(),
        };
        let body = CloudClientTokenRequest::build(&identity, " ams-9 ");
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            json!({
                "ats_instance_id": "ats-1",
                "ams_server_instance_id": "ams-9",
                "ats_hostname": "Studio-PC",
                "ats_version": "2.0.0",
                "ats_app": "AeroTandemStudio",
            })
        );
    }

    #[test]
    fn maps_cloud_response_and_fills_base_url() {
        let mapped = map_cloud_issue_response(
            &json!({
                "access_token": " jwt.token ",
                "expires_at": "2026-10-09T12:00:00.000Z",
                "expires_in": 172800,
                "scope": ["customer.lookup"]
            }),
            "https://fallback.example",
        )
        .unwrap();
        assert_eq!(mapped.access_token, "jwt.token");
        assert_eq!(mapped.token_type, "Bearer");
        assert_eq!(mapped.cloud_base_url, "https://fallback.example");
        assert_eq!(mapped.expires_in, 172800);
        assert_eq!(mapped.scope, vec!["customer.lookup"]);
    }

    #[test]
    fn maps_cloud_error_status() {
        let u = ClientTokenIssueError::from_cloud_status(401, r#"{"error":"bad key"}"#);
        assert_eq!(u.code, ERR_CLOUD_UNAUTHORIZED);
        assert_eq!(u.http_status, 502);

        let f = ClientTokenIssueError::from_cloud_status(403, "forbidden");
        assert_eq!(f.code, ERR_CLOUD_FORBIDDEN);
        assert!(f.message.contains(CLOUD_PERMISSION_ATS_CLIENT_TOKEN));

        let o = ClientTokenIssueError::from_cloud_status(500, "boom");
        assert_eq!(o.code, ERR_CLOUD_ISSUE_FAILED);
    }

    #[derive(Clone)]
    struct MockCloudState {
        last_auth: Arc<Mutex<Option<String>>>,
        last_body: Arc<Mutex<Option<CloudClientTokenRequest>>>,
        status: u16,
        body: Value,
    }

    async fn mock_cloud_issue(
        State(state): State<MockCloudState>,
        headers: HeaderMap,
        Json(body): Json<CloudClientTokenRequest>,
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

    async fn start_mock_cloud(status: u16, body: Value) -> (String, oneshot::Sender<()>, MockCloudState) {
        let state = MockCloudState {
            last_auth: Arc::new(Mutex::new(None)),
            last_body: Arc::new(Mutex::new(None)),
            status,
            body,
        };
        let app = Router::new()
            .route("/api/ats/v1/client-token", post(mock_cloud_issue))
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
    async fn issue_proxies_to_cloud_and_maps_success() {
        let (base, shutdown, state) = start_mock_cloud(
            200,
            json!({
                "access_token": "eyJhbGciOiJIUzI1NiJ9.ok",
                "token_type": "Bearer",
                "expires_at": "2026-10-09T12:00:00.000Z",
                "expires_in": 172800,
                "cloud_base_url": "https://cloud.from.response",
                "scope": ["customer.lookup"]
            }),
        )
        .await;

        let identity = AtsClientIdentity {
            ats_instance_id: "ats-inst".into(),
            ats_hostname: "Host".into(),
            ats_version: "9.9.9".into(),
            ats_app: "AeroTandemStudio".into(),
        };
        let issue_url = format!("{base}/api/ats/v1/client-token");
        let resp = issue_client_token_at_url(
            &issue_url,
            "kid.secret",
            "https://fallback",
            &identity,
            "ams-inst",
        )
        .await
        .unwrap();

        assert_eq!(resp.access_token, "eyJhbGciOiJIUzI1NiJ9.ok");
        assert_eq!(resp.cloud_base_url, "https://cloud.from.response");
        assert_eq!(
            state.last_auth.lock().unwrap().as_deref(),
            Some("Bearer kid.secret")
        );
        let sent = state.last_body.lock().unwrap().clone().unwrap();
        assert_eq!(sent.ats_instance_id, "ats-inst");
        assert_eq!(sent.ams_server_instance_id, "ams-inst");

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn issue_maps_cloud_403() {
        let (base, shutdown, _) = start_mock_cloud(403, json!({"error":"no permission"})).await;
        let identity = AtsClientIdentity {
            ats_instance_id: "ats-inst".into(),
            ats_hostname: String::new(),
            ats_version: String::new(),
            ats_app: "AeroTandemStudio".into(),
        };
        let err = issue_client_token_at_url(
            &format!("{base}/api/ats/v1/client-token"),
            "kid.secret",
            "https://fallback",
            &identity,
            "ams-inst",
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ERR_CLOUD_FORBIDDEN);
        let _ = shutdown.send(());
    }
}
