//! Dropbox OAuth2 no-redirect flow with PKCE (port of DropboxOAuth2FlowNoRedirect).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::cloud::traits::CloudError;
use crate::storage::logging;
use crate::storage::secrets;

const AUTHORIZE_URL: &str = "https://www.dropbox.com/oauth2/authorize";
const TOKEN_URL: &str = "https://api.dropboxapi.com/oauth2/token";

#[derive(Debug, Clone, Serialize)]
pub struct OauthStart {
    pub authorize_url: String,
    pub code_verifier: String,
}

/// Builds the Dropbox authorize URL and a PKCE code verifier for the no-redirect flow.
pub fn start_oauth(app_key: &str) -> Result<OauthStart, CloudError> {
    let app_key = app_key.trim();
    if app_key.is_empty() {
        return Err(CloudError::Message("App Key fehlt für OAuth.".into()));
    }
    let code_verifier = generate_code_verifier();
    let challenge = code_challenge_s256(&code_verifier);
    let authorize_url = format!(
        "{AUTHORIZE_URL}?client_id={}&response_type=code&token_access_type=offline\
         &code_challenge={}&code_challenge_method=S256",
        urlencoding_minimal(app_key),
        urlencoding_minimal(&challenge),
    );
    Ok(OauthStart {
        authorize_url,
        code_verifier,
    })
}

/// Exchanges an auth code for tokens and stores the refresh token under `refresh_key`.
pub async fn finish_oauth(
    app_key: &str,
    app_secret: &str,
    auth_code: &str,
    code_verifier: &str,
    refresh_key: &str,
) -> Result<(String, String), CloudError> {
    let app_key = app_key.trim();
    let app_secret = app_secret.trim();
    let auth_code = auth_code.trim();
    let code_verifier = code_verifier.trim();
    if app_key.is_empty() || app_secret.is_empty() {
        return Err(CloudError::Message(
            "App Key/Secret fehlen für OAuth.".into(),
        ));
    }
    if auth_code.is_empty() {
        return Err(CloudError::Message("Auth-Code fehlt.".into()));
    }
    if code_verifier.is_empty() {
        return Err(CloudError::Message("PKCE code_verifier fehlt.".into()));
    }

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let response = http
        .post(TOKEN_URL)
        .form(&[
            ("code", auth_code),
            ("grant_type", "authorization_code"),
            ("client_id", app_key),
            ("client_secret", app_secret),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .map_err(|e| CloudError::Http(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        logging::log_error(&format!(
            "OAuth Token-Austausch fehlgeschlagen: {status} {body}"
        ));
        return Err(CloudError::Http(format!(
            "OAuth Token-Austausch fehlgeschlagen: {status}"
        )));
    }

    let payload: Value = response
        .json()
        .await
        .map_err(|e| CloudError::Http(e.to_string()))?;
    let access = payload
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| CloudError::Http("OAuth-Antwort ohne access_token".into()))?
        .to_string();
    let refresh = payload
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| CloudError::Http("OAuth-Antwort ohne refresh_token".into()))?
        .to_string();

    secrets::save_secret(refresh_key, &refresh).map_err(|e| CloudError::Message(e.to_string()))?;
    logging::log_info(&format!(
        "Refresh-Token im Keyring gespeichert ({refresh_key})."
    ));
    Ok((access, refresh))
}

/// Reads app key/secret from keyring and starts OAuth for the given secret-key set.
pub fn start_oauth_for_keys(
    app_key_name: &str,
    app_secret_name: &str,
) -> Result<OauthStart, CloudError> {
    let app_key = secrets::get_secret(app_key_name)
        .map_err(|e| CloudError::Message(e.to_string()))?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| CloudError::Message("App Key fehlt im Keyring.".into()))?;
    let _app_secret = secrets::get_secret(app_secret_name)
        .map_err(|e| CloudError::Message(e.to_string()))?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| CloudError::Message("App Secret fehlt im Keyring.".into()))?;
    start_oauth(&app_key)
}

/// Completes OAuth using keyring credentials for the given secret-key set.
pub async fn finish_oauth_for_keys(
    app_key_name: &str,
    app_secret_name: &str,
    refresh_key: &str,
    auth_code: &str,
    code_verifier: &str,
) -> Result<(String, String), CloudError> {
    let app_key = secrets::get_secret(app_key_name)
        .map_err(|e| CloudError::Message(e.to_string()))?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| CloudError::Message("App Key fehlt im Keyring.".into()))?;
    let app_secret = secrets::get_secret(app_secret_name)
        .map_err(|e| CloudError::Message(e.to_string()))?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| CloudError::Message("App Secret fehlt im Keyring.".into()))?;
    finish_oauth(&app_key, &app_secret, auth_code, code_verifier, refresh_key).await
}

fn generate_code_verifier() -> String {
    let mut raw = [0u8; 32];
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    raw[..16].copy_from_slice(a.as_bytes());
    raw[16..].copy_from_slice(b.as_bytes());
    URL_SAFE_NO_PAD.encode(raw)
}

fn code_challenge_s256(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn urlencoding_minimal(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[allow(dead_code)]
pub fn is_unauthorized(status: StatusCode) -> bool {
    status == StatusCode::UNAUTHORIZED || status == StatusCode::BAD_REQUEST
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_verifier_is_url_safe_and_long_enough() {
        let v = generate_code_verifier();
        assert!(v.len() >= 43);
        assert!(v
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn challenge_is_deterministic() {
        let v = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG";
        let c1 = code_challenge_s256(v);
        let c2 = code_challenge_s256(v);
        assert_eq!(c1, c2);
        assert!(!c1.contains('+'));
        assert!(!c1.contains('/'));
        assert!(!c1.contains('='));
    }

    #[test]
    fn authorize_url_contains_pkce_params() {
        let start = start_oauth("test-app-key").unwrap();
        assert!(start.authorize_url.contains("client_id=test-app-key"));
        assert!(start.authorize_url.contains("code_challenge_method=S256"));
        assert!(start.authorize_url.contains("token_access_type=offline"));
        assert!(!start.code_verifier.is_empty());
    }
}
