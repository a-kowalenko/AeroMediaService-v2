//! Dropbox HTTP client: refresh-token auth, OAuth/PKCE, chunk upload, share links.
//! Port of legacy `services/dropbox_client.py`.
//!
//! Secrets are read only from the OS keyring.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::{json, Value};

use crate::cloud::dropbox_batch::{self, HybridUploaded};
use crate::cloud::guards::{assert_checkpoint_binding_matches, merge_checkpoint_binding};
use crate::cloud::oauth::{self, OauthStart};
use crate::cloud::traits::{should_skip_upload_file, CloudClient, CloudError};
use crate::events;
use crate::model::kunde::Kunde;
use crate::storage::logging;
use crate::storage::secrets;
use crate::upload::checkpoint::{
    clear_checkpoint, load_checkpoint, manifest_fingerprint, save_checkpoint,
    ThrottledCheckpointSaver,
};
use crate::upload::control::UploadControl;
use crate::util::link_shortener;

/// Dropbox session chunk size (Performance Guide: multiples of 4 MiB).
pub const CHUNK_SIZE: usize = 32 * 1024 * 1024;
/// Direct `/files/upload` threshold (hybrid small vs large).
pub const SMALL_FILE_BYTES: usize = 4 * 1024 * 1024;
pub const CK_MIN_INTERVAL_SECS: f64 = 10.0;
pub const BATCH_PARALLEL_WORKERS: usize = 4;
pub const BATCH_MAX_FILES: usize = 1000;

const TOKEN_URL: &str = "https://api.dropboxapi.com/oauth2/token";
const API_URL: &str = "https://api.dropboxapi.com/2";
const CONTENT_URL: &str = "https://content.dropboxapi.com/2";
const MAX_RETRY_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone)]
pub struct UploadFile {
    pub local_path: PathBuf,
    pub dropbox_path: String,
    pub size: u64,
    pub rel_norm: String,
}

/// Which Dropbox credential pool (never mix tokens across pools).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DropboxPool {
    Native,
    CustomApi,
}

impl DropboxPool {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::CustomApi => "custom_api",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, CloudError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "native" | "dropbox" => Ok(Self::Native),
            "custom" | "custom_api" | "custom_dropbox" => Ok(Self::CustomApi),
            other => Err(CloudError::Message(format!(
                "Unbekannter Dropbox-Pool: {other}"
            ))),
        }
    }

    pub fn active_setting_key(self) -> &'static str {
        match self {
            Self::Native => "active_dropbox_account_id",
            Self::CustomApi => "active_custom_dropbox_account_id",
        }
    }

    pub fn legacy_keys(self) -> DropboxSecretKeys {
        match self {
            Self::Native => DropboxSecretKeys::native(),
            Self::CustomApi => DropboxSecretKeys::custom_api(),
        }
    }
}

/// Live account snapshot for Settings (native or Custom-API Dropbox).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DropboxAccountInfo {
    /// Dropbox `account_id` from `users/get_current_account`.
    pub account_id: String,
    pub display_name: String,
    pub email: String,
    /// Profile photo URL from Dropbox (`profile_photo_url`), if set.
    pub profile_photo_url: String,
    /// App-folder name under `/Apps/…` when discoverable (Full Dropbox).
    pub app_name: String,
    /// Truncated App Key (fallback when `app_name` is empty).
    pub app_key_hint: String,
    pub token_valid: bool,
    pub used_bytes: u64,
    /// `None` when Dropbox does not report a fixed quota.
    pub allocated_bytes: Option<u64>,
}

/// Keyring key names for a Dropbox credential set (legacy pool-wide or per AMS profile).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropboxSecretKeys {
    pub app_key: String,
    pub app_secret: String,
    pub refresh_token: String,
}

impl DropboxSecretKeys {
    pub fn native() -> Self {
        Self {
            app_key: "db_app_key".into(),
            app_secret: "db_app_secret".into(),
            refresh_token: "db_refresh_token".into(),
        }
    }

    pub fn custom_api() -> Self {
        Self {
            app_key: "custom_db_app_key".into(),
            app_secret: "custom_db_app_secret".into(),
            refresh_token: "custom_db_refresh_token".into(),
        }
    }

    /// Namespaced keys for one AMS Dropbox profile (`db_*_<ams_id>` / `custom_db_*_<ams_id>`).
    pub fn for_account(pool: DropboxPool, ams_id: &str) -> Self {
        let id = ams_id.trim();
        match pool {
            DropboxPool::Native => Self {
                app_key: format!("db_app_key_{id}"),
                app_secret: format!("db_app_secret_{id}"),
                refresh_token: format!("db_refresh_token_{id}"),
            },
            DropboxPool::CustomApi => Self {
                app_key: format!("custom_db_app_key_{id}"),
                app_secret: format!("custom_db_app_secret_{id}"),
                refresh_token: format!("custom_db_refresh_token_{id}"),
            },
        }
    }

    /// Pool inferred from keyring key prefix (`custom_db_*` vs `db_*`).
    pub fn pool_from_keys(keys: &DropboxSecretKeys) -> DropboxPool {
        if keys.app_key.starts_with("custom_db_") {
            DropboxPool::CustomApi
        } else {
            DropboxPool::Native
        }
    }

    /// AMS profile id from namespaced keys (`db_app_key_<ams_id>`), if any.
    pub fn ams_id_from_keys(keys: &DropboxSecretKeys) -> Option<String> {
        for prefix in ["custom_db_app_key_", "db_app_key_"] {
            if let Some(rest) = keys.app_key.strip_prefix(prefix) {
                let id = rest.trim();
                if !id.is_empty() {
                    return Some(id.to_string());
                }
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct DropboxCursor {
    pub session_id: String,
    pub offset: u64,
}

#[derive(Debug, Clone)]
pub struct DropboxSessionResume {
    pub session_id: String,
    pub offset: u64,
    pub rel_path: String,
}

#[derive(Clone)]
pub struct DropboxClient {
    http: reqwest::Client,
    access_token: Arc<Mutex<Option<String>>>,
    connection_verified: Arc<AtomicBool>,
    keys: DropboxSecretKeys,
}

/// True for `Verbunden` and legacy variants like `Verbunden (…)`.
pub fn is_connected_status(status: &str) -> bool {
    status.trim().starts_with("Verbunden")
}

impl Default for DropboxClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DropboxClient {
    pub fn new() -> Self {
        Self::with_keys(DropboxSecretKeys::native())
    }

    pub fn for_custom_api() -> Self {
        Self::with_keys(DropboxSecretKeys::custom_api())
    }

    pub fn with_keys(keys: DropboxSecretKeys) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            access_token: Arc::new(Mutex::new(None)),
            connection_verified: Arc::new(AtomicBool::new(false)),
            keys,
        }
    }

    fn token(&self) -> Option<String> {
        self.access_token.lock().ok().and_then(|g| g.clone())
    }

    fn set_token(&self, token: Option<String>) {
        if let Ok(mut guard) = self.access_token.lock() {
            *guard = token;
        }
    }

    /// Refresh access token from keyring credentials.
    ///
    /// Does **not** emit global connection-status events: callers such as
    /// `connection_status_verified` (Settings checks both native and custom)
    /// and mid-upload `ensure_token` must not overwrite the active cloud status.
    async fn refresh_access_token(&self) -> Result<String, CloudError> {
        let app_key = secrets::get_secret(&self.keys.app_key)
            .map_err(|e| CloudError::Message(e.to_string()))?
            .filter(|s| !s.is_empty());
        let app_secret = secrets::get_secret(&self.keys.app_secret)
            .map_err(|e| CloudError::Message(e.to_string()))?
            .filter(|s| !s.is_empty());
        let refresh_token = secrets::get_secret(&self.keys.refresh_token)
            .map_err(|e| CloudError::Message(e.to_string()))?
            .filter(|s| !s.is_empty());

        let (Some(app_key), Some(app_secret)) = (app_key, app_secret) else {
            logging::log_warn("App Key oder App Secret für Dropbox fehlen.");
            return Err(CloudError::NotConnected(
                "App Key oder App Secret für Dropbox fehlen.".into(),
            ));
        };
        let Some(refresh_token) = refresh_token else {
            return Err(CloudError::NotConnected(
                "Kein Dropbox-Refresh-Token im Keyring.".into(),
            ));
        };

        logging::log_info("Versuche Verbindung mit Refresh-Token...");
        let response = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.as_str()),
                ("client_id", app_key.as_str()),
                ("client_secret", app_secret.as_str()),
            ])
            .send()
            .await
            .map_err(|e| CloudError::Http(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if status == StatusCode::UNAUTHORIZED || status == StatusCode::BAD_REQUEST {
                logging::log_warn(&format!("Refresh-Token ungültig: {status} {body}"));
                let _ = secrets::delete_secret(&self.keys.refresh_token);
                self.set_token(None);
                self.connection_verified.store(false, Ordering::SeqCst);
            }
            return Err(CloudError::Http(format!(
                "Token-Refresh fehlgeschlagen: {status} {body}"
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
            .ok_or_else(|| CloudError::Http("Token-Antwort ohne access_token".into()))?
            .to_string();
        self.set_token(Some(access.clone()));
        Ok(access)
    }

    async fn ensure_token(&self) -> Result<String, CloudError> {
        if let Some(token) = self.token() {
            return Ok(token);
        }
        self.refresh_access_token().await
    }

    #[allow(dead_code)]
    pub fn secret_keys(&self) -> DropboxSecretKeys {
        self.keys.clone()
    }

    /// Credential pool for this client (native vs custom_api).
    pub fn profile_pool(&self) -> DropboxPool {
        DropboxSecretKeys::pool_from_keys(&self.keys)
    }

    /// AMS Dropbox profile id when using namespaced keys; `None` for legacy pool-wide keys.
    pub fn profile_ams_id(&self) -> Option<String> {
        DropboxSecretKeys::ams_id_from_keys(&self.keys)
    }

    fn checkpoint_payload(&self, mut payload: Value) -> Value {
        if let Some(obj) = payload.as_object_mut() {
            merge_checkpoint_binding(
                obj,
                self.profile_ams_id().as_deref(),
                Some(self.profile_pool()),
            );
        }
        payload
    }

    /// Starts the Dropbox OAuth authorize URL (PKCE no-redirect).
    pub fn start_oauth(&self) -> Result<OauthStart, CloudError> {
        oauth::start_oauth_for_keys(&self.keys.app_key, &self.keys.app_secret)
    }

    /// Completes OAuth with an auth code, stores refresh token, verifies account.
    pub async fn finish_oauth(
        &self,
        auth_code: &str,
        code_verifier: &str,
        emit_status: bool,
    ) -> Result<bool, CloudError> {
        logging::log_info("Schließe Dropbox-OAuth ab...");
        let (access, _refresh) = oauth::finish_oauth_for_keys(
            &self.keys.app_key,
            &self.keys.app_secret,
            &self.keys.refresh_token,
            auth_code,
            code_verifier,
        )
        .await?;
        self.set_token(Some(access.clone()));
        match self.users_get_current_account(&access).await {
            Ok(account) => {
                self.connection_verified.store(true, Ordering::SeqCst);
                let name = if account.display_name.is_empty() {
                    "Dropbox".into()
                } else {
                    account.display_name.clone()
                };
                logging::log_info(&format!(
                    "Erfolgreich mit Dropbox verbunden (via OAuth): {name}"
                ));
                if emit_status {
                    events::emit_connection_status("Verbunden");
                }
                Ok(true)
            }
            Err(e) => {
                self.connection_verified.store(false, Ordering::SeqCst);
                if emit_status {
                    events::emit_connection_status(format!("OAuth-Fehler: {e}"));
                }
                Err(e)
            }
        }
    }

    /// Connect via refresh token (and optionally fall through to caller for OAuth).
    pub async fn connect_session(&self, emit_status: bool) -> Result<bool, CloudError> {
        match self.refresh_access_token().await {
            Ok(token) => match self.users_get_current_account(&token).await {
                Ok(account) => {
                    self.connection_verified.store(true, Ordering::SeqCst);
                    let name = if account.display_name.is_empty() {
                        "Dropbox".into()
                    } else {
                        account.display_name.clone()
                    };
                    logging::log_info(&format!(
                        "Erfolgreich mit Dropbox verbunden (via Refresh-Token): {name}"
                    ));
                    if emit_status {
                        events::emit_connection_status("Verbunden");
                    }
                    Ok(true)
                }
                Err(e) => {
                    self.connection_verified.store(false, Ordering::SeqCst);
                    if emit_status {
                        events::emit_connection_status(format!("Verbindungsfehler: {e}"));
                    }
                    Err(e)
                }
            },
            Err(CloudError::NotConnected(msg)) if msg.contains("App Key") => {
                self.connection_verified.store(false, Ordering::SeqCst);
                if emit_status {
                    events::emit_connection_status("Fehler: App Key/Secret fehlt");
                }
                Err(CloudError::NotConnected(msg))
            }
            Err(CloudError::NotConnected(msg)) if msg.contains("Refresh-Token") => {
                self.connection_verified.store(false, Ordering::SeqCst);
                if emit_status {
                    events::emit_connection_status("Nicht verbunden");
                }
                Err(CloudError::NotConnected(msg))
            }
            Err(e) => {
                self.connection_verified.store(false, Ordering::SeqCst);
                if emit_status {
                    events::emit_connection_status(format!("Verbindungsfehler: {e}"));
                }
                Err(e)
            }
        }
    }

    pub async fn disconnect_session(
        &self,
        emit_status: bool,
        stop_monitoring: bool,
    ) -> Result<(), CloudError> {
        logging::log_info("Trenne Verbindung zu Dropbox...");
        let _ = secrets::delete_secret(&self.keys.refresh_token);
        self.set_token(None);
        self.connection_verified.store(false, Ordering::SeqCst);
        if emit_status {
            events::emit_connection_status("Nicht verbunden");
        }
        if stop_monitoring {
            events::emit(events::STOP_MONITORING, ());
        }
        logging::log_info("Verbindung getrennt.");
        Ok(())
    }

    /// Live status check (`verify=true` in legacy).
    /// Does not emit connection-status events and avoids noisy keyring warnings
    /// when credentials are simply absent (e.g. Settings verifying the inactive cloud).
    pub async fn connection_status_verified(&self) -> String {
        let has_app_creds = secrets::get_secret(&self.keys.app_key)
            .ok()
            .flatten()
            .filter(|s| !s.is_empty())
            .is_some()
            && secrets::get_secret(&self.keys.app_secret)
                .ok()
                .flatten()
                .filter(|s| !s.is_empty())
                .is_some();
        if !has_app_creds {
            return "Nicht verbunden".into();
        }

        if self.token().is_none() {
            if let Ok(token) = self.refresh_access_token().await {
                self.set_token(Some(token.clone()));
                return match self.users_get_current_account(&token).await {
                    Ok(_) => {
                        self.connection_verified.store(true, Ordering::SeqCst);
                        "Verbunden".into()
                    }
                    Err(_) => {
                        self.connection_verified.store(false, Ordering::SeqCst);
                        "Verbindungsfehler".into()
                    }
                };
            }
            return "Nicht verbunden".into();
        }
        if let Some(token) = self.token() {
            return match self.users_get_current_account(&token).await {
                Ok(_) => {
                    self.connection_verified.store(true, Ordering::SeqCst);
                    "Verbunden".into()
                }
                Err(_) => {
                    self.connection_verified.store(false, Ordering::SeqCst);
                    "Verbindungsfehler".into()
                }
            };
        }
        "Nicht verbunden".into()
    }

    /// Account name, token health, and storage quota for Settings.
    pub async fn account_info(&self) -> Result<DropboxAccountInfo, CloudError> {
        let token = self.ensure_token().await?;
        let (mut account, root_namespace_id) =
            self.users_get_current_account_with_root(&token).await?;
        account.app_key_hint = app_key_hint(
            secrets::get_secret(&self.keys.app_key)
                .ok()
                .flatten()
                .as_deref()
                .unwrap_or(""),
        );
        match self.resolve_app_folder_name(&token, &root_namespace_id).await {
            Ok(name) => {
                account.app_name = name;
            }
            Err(e) => {
                logging::log_warn(&format!(
                    "Dropbox App-/Root-Ordnername nicht ermittelbar: {e}"
                ));
            }
        }
        match self.users_get_space_usage(&token).await {
            Ok((used, allocated)) => {
                account.used_bytes = used;
                account.allocated_bytes = allocated;
            }
            Err(e) => {
                logging::log_warn(&format!("Dropbox Speicherabfrage fehlgeschlagen: {e}"));
            }
        }
        self.connection_verified.store(true, Ordering::SeqCst);
        Ok(account)
    }

    async fn users_get_current_account(
        &self,
        token: &str,
    ) -> Result<DropboxAccountInfo, CloudError> {
        let (account, _) = self.users_get_current_account_with_root(token).await?;
        Ok(account)
    }

    async fn users_get_current_account_with_root(
        &self,
        token: &str,
    ) -> Result<(DropboxAccountInfo, String), CloudError> {
        let response = self
            .http
            .post(format!("{API_URL}/users/get_current_account"))
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(CONTENT_TYPE, "application/json")
            .body("null")
            .send()
            .await
            .map_err(|e| CloudError::Http(e.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(CloudError::Http(format!(
                "users/get_current_account: {status} {body}"
            )));
        }
        let payload: Value = response
            .json()
            .await
            .map_err(|e| CloudError::Http(e.to_string()))?;
        let root_namespace_id = payload
            .pointer("/root_info/root_namespace_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        Ok((parse_account_info(&payload, ""), root_namespace_id))
    }

    async fn users_get_space_usage(&self, token: &str) -> Result<(u64, Option<u64>), CloudError> {
        let response = self
            .http
            .post(format!("{API_URL}/users/get_space_usage"))
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(CONTENT_TYPE, "application/json")
            .body("null")
            .send()
            .await
            .map_err(|e| CloudError::Http(e.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(CloudError::Http(format!(
                "users/get_space_usage: {status} {body}"
            )));
        }
        let payload: Value = response
            .json()
            .await
            .map_err(|e| CloudError::Http(e.to_string()))?;
        Ok(parse_space_usage(&payload))
    }

    /// Resolve app-folder display name.
    ///
    /// `files/get_metadata` on `""` is rejected by Dropbox (no root metadata).
    /// Strategy: try `ns:{root_namespace_id}`, then list `/Apps` (Full Dropbox).
    async fn resolve_app_folder_name(
        &self,
        token: &str,
        root_namespace_id: &str,
    ) -> Result<String, CloudError> {
        if !root_namespace_id.is_empty() {
            match self
                .files_get_metadata_name(token, &format!("ns:{root_namespace_id}"))
                .await
            {
                Ok(name) if !name.is_empty() => return Ok(name),
                Ok(_) => {}
                Err(e) => logging::log_info(&format!(
                    "Dropbox ns: root metadata nicht nutzbar: {e}"
                )),
            }
        }

        match self.list_apps_folder_names(token).await {
            Ok(names) => Ok(pick_apps_folder_name(&names)),
            Err(e) => {
                // App-Folder permission cannot see `/Apps` — expected, not fatal.
                logging::log_info(&format!("Dropbox /Apps nicht lesbar: {e}"));
                Ok(String::new())
            }
        }
    }

    async fn files_get_metadata_name(
        &self,
        token: &str,
        path: &str,
    ) -> Result<String, CloudError> {
        let response = self
            .http
            .post(format!("{API_URL}/files/get_metadata"))
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(CONTENT_TYPE, "application/json")
            .json(&json!({ "path": path }))
            .send()
            .await
            .map_err(|e| CloudError::Http(e.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(CloudError::Http(format!(
                "files/get_metadata: {status} {body}"
            )));
        }
        let payload: Value = response
            .json()
            .await
            .map_err(|e| CloudError::Http(e.to_string()))?;
        Ok(parse_root_folder_name(&payload))
    }

    async fn list_apps_folder_names(&self, token: &str) -> Result<Vec<String>, CloudError> {
        let response = self
            .http
            .post(format!("{API_URL}/files/list_folder"))
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(CONTENT_TYPE, "application/json")
            .json(&json!({
                "path": "/Apps",
                "recursive": false,
                "include_mounted_folders": true,
            }))
            .send()
            .await
            .map_err(|e| CloudError::Http(e.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(CloudError::Http(format!(
                "files/list_folder /Apps: {status} {body}"
            )));
        }
        let payload: Value = response
            .json()
            .await
            .map_err(|e| CloudError::Http(e.to_string()))?;
        Ok(parse_apps_folder_names(&payload))
    }

    fn auth_headers(token: &str, api_arg: Option<&Value>) -> Result<HeaderMap, CloudError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| CloudError::Message(e.to_string()))?,
        );
        if let Some(arg) = api_arg {
            let encoded =
                serde_json::to_string(arg).map_err(|e| CloudError::Message(e.to_string()))?;
            headers.insert(
                "Dropbox-API-Arg",
                HeaderValue::from_str(&encoded).map_err(|e| CloudError::Message(e.to_string()))?,
            );
        }
        Ok(headers)
    }

    async fn with_retry<F, Fut>(
        &self,
        tag: &str,
        mut operation: F,
    ) -> Result<reqwest::Response, CloudError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<reqwest::Response, CloudError>>,
    {
        let mut last_err = CloudError::Message(format!("{tag}: keine Versuche"));
        for attempt in 1..=MAX_RETRY_ATTEMPTS {
            match operation().await {
                Ok(response) => {
                    let status = response.status();
                    if status == StatusCode::UNAUTHORIZED {
                        return Err(CloudError::Http(format!("{tag}: 401 unauthorized")));
                    }
                    if should_retry_status(status) && attempt < MAX_RETRY_ATTEMPTS {
                        let body = response.text().await.unwrap_or_default();
                        last_err = CloudError::Http(format!("{tag}: {status} {body}"));
                    } else if status.is_success() {
                        return Ok(response);
                    } else {
                        let body = response.text().await.unwrap_or_default();
                        return Err(CloudError::Http(format!("{tag}: {status} {body}")));
                    }
                }
                Err(e) => {
                    if attempt >= MAX_RETRY_ATTEMPTS || !should_retry_error(&e) {
                        return Err(e);
                    }
                    last_err = e;
                }
            }
            let delay = retry_delay_secs(attempt);
            logging::log_warn(&format!(
                "{tag}: Versuch {attempt}/{MAX_RETRY_ATTEMPTS} fehlgeschlagen, warte {delay:.1}s — {last_err}"
            ));
            tokio::time::sleep(Duration::from_secs_f64(delay)).await;
        }
        Err(last_err)
    }

    pub(crate) async fn content_upload(
        &self,
        path: &str,
        api_arg: &Value,
        body: Bytes,
        control: &UploadControl,
    ) -> Result<Value, CloudError> {
        self.content_upload_with_progress(path, api_arg, body, control, None)
            .await
    }

    /// Like [`content_upload`], but reports bytes as they are handed to the HTTP stack.
    pub(crate) async fn content_upload_with_progress(
        &self,
        path: &str,
        api_arg: &Value,
        body: Bytes,
        control: &UploadControl,
        on_send: Option<std::sync::Arc<dyn Fn(u64) + Send + Sync>>,
    ) -> Result<Value, CloudError> {
        control.wait_if_paused().await?;
        let mut token = self.ensure_token().await?;
        for auth_try in 0..2 {
            match self
                .send_content(path, &token, api_arg, body.clone(), on_send.clone())
                .await
            {
                Ok(response) => {
                    let text = response.text().await.unwrap_or_default();
                    if text.is_empty() {
                        return Ok(Value::Null);
                    }
                    return serde_json::from_str(&text).or_else(|_| Ok(json!({ "raw": text })));
                }
                Err(e) if auth_try == 0 && e.to_string().contains("401") => {
                    logging::log_warn("Dropbox-Token abgelaufen, erneuere...");
                    token = self.refresh_access_token().await?;
                }
                Err(e) => return Err(e),
            }
        }
        Err(CloudError::Http(format!("{path}: Auth-Retry erschöpft")))
    }

    async fn send_content(
        &self,
        path: &str,
        token: &str,
        api_arg: &Value,
        body: Bytes,
        on_send: Option<std::sync::Arc<dyn Fn(u64) + Send + Sync>>,
    ) -> Result<reqwest::Response, CloudError> {
        let headers = Self::auth_headers(token, Some(api_arg))?;
        let url = format!("{CONTENT_URL}{path}");
        let mut last_err = CloudError::Message(format!("{path}: keine Versuche"));
        for attempt in 1..=MAX_RETRY_ATTEMPTS {
            let request_body = if let Some(ref cb) = on_send {
                crate::upload::progress::body_with_send_progress(body.clone(), cb.clone())
            } else {
                body.clone().into()
            };
            let request = self
                .http
                .post(&url)
                .headers(headers.clone())
                .header(CONTENT_TYPE, "application/octet-stream")
                .body(request_body);
            match request.send().await {
                Ok(response) => {
                    let status = response.status();
                    if status == StatusCode::UNAUTHORIZED {
                        return Err(CloudError::Http(format!("{path}: 401 unauthorized")));
                    }
                    if should_retry_status(status) && attempt < MAX_RETRY_ATTEMPTS {
                        let body_text = response.text().await.unwrap_or_default();
                        last_err = CloudError::Http(format!("{path}: {status} {body_text}"));
                    } else if status.is_success() {
                        return Ok(response);
                    } else {
                        let body_text = response.text().await.unwrap_or_default();
                        return Err(CloudError::Http(format!("{path}: {status} {body_text}")));
                    }
                }
                Err(e) => {
                    let err = CloudError::Http(e.to_string());
                    if attempt >= MAX_RETRY_ATTEMPTS || !should_retry_error(&err) {
                        return Err(err);
                    }
                    last_err = err;
                }
            }
            let delay = retry_delay_secs(attempt);
            logging::log_warn(&format!(
                "{path}: Versuch {attempt}/{MAX_RETRY_ATTEMPTS} fehlgeschlagen, warte {delay:.1}s — {last_err}"
            ));
            tokio::time::sleep(Duration::from_secs_f64(delay)).await;
        }
        Err(last_err)
    }

    pub(crate) async fn rpc(&self, path: &str, body: Value) -> Result<Value, CloudError> {
        let mut token = self.ensure_token().await?;
        for auth_try in 0..2 {
            let token_for_req = token.clone();
            let payload = body.clone();
            let result = self
                .with_retry(path, || {
                    let request = self
                        .http
                        .post(format!("{API_URL}{path}"))
                        .header(AUTHORIZATION, format!("Bearer {token_for_req}"))
                        .header(CONTENT_TYPE, "application/json")
                        .json(&payload);
                    async move {
                        request
                            .send()
                            .await
                            .map_err(|e| CloudError::Http(e.to_string()))
                    }
                })
                .await;
            match result {
                Ok(response) => {
                    let text = response.text().await.unwrap_or_default();
                    if text.is_empty() {
                        return Ok(Value::Null);
                    }
                    return serde_json::from_str(&text).map_err(|e| {
                        CloudError::Http(format!("{path}: ungültiges JSON ({e}) {text}"))
                    });
                }
                Err(e) if auth_try == 0 && e.to_string().contains("401") => {
                    token = self.refresh_access_token().await?;
                }
                Err(e) => return Err(e),
            }
        }
        Err(CloudError::Http(format!("{path}: Auth-Retry erschöpft")))
    }

    pub(crate) async fn upload_small_file(
        &self,
        local_path: &Path,
        dropbox_path: &str,
        file_size: u64,
        control: &UploadControl,
    ) -> Result<Option<String>, CloudError> {
        self.upload_small_file_with_progress(local_path, dropbox_path, file_size, control, None)
            .await
    }

    pub(crate) async fn upload_small_file_with_progress(
        &self,
        local_path: &Path,
        dropbox_path: &str,
        file_size: u64,
        control: &UploadControl,
        on_send: Option<std::sync::Arc<dyn Fn(u64) + Send + Sync>>,
    ) -> Result<Option<String>, CloudError> {
        control.wait_if_paused().await?;
        let data = Bytes::from(fs::read(local_path)?);
        let arg = files_upload_arg(dropbox_path);
        let result = self
            .content_upload_with_progress("/files/upload", &arg, data, control, on_send)
            .await?;
        events::emit_progress_file(100, file_size, file_size);
        Ok(result.get("id").and_then(Value::as_str).map(str::to_string))
    }

    pub(crate) async fn upload_large_file<F>(
        &self,
        local_path: &Path,
        dropbox_path: &str,
        file_size: u64,
        base_bytes_uploaded: u64,
        total_job_size: u64,
        control: &UploadControl,
        resume: Option<DropboxSessionResume>,
        mut on_progress_save: Option<F>,
        on_bytes_sent: Option<std::sync::Arc<dyn Fn(u64) + Send + Sync>>,
    ) -> Result<Option<String>, CloudError>
    where
        F: FnMut(Option<DropboxCursor>, bool),
    {
        let flush_err = |cb: &mut Option<F>, cursor: Option<DropboxCursor>, err: CloudError| {
            if let Some(cb) = cb.as_mut() {
                cb(cursor, true);
            }
            err
        };

        if let Err(e) = control.wait_if_paused().await {
            return Err(flush_err(&mut on_progress_save, None, e.into()));
        }
        let mut file = tokio::fs::File::open(local_path).await?;
        let mut buf = vec![0u8; CHUNK_SIZE];
        let (session_id, mut offset) = if let Some(resume) = resume.filter(|r| r.offset > 0) {
            if resume.offset > file_size {
                return Err(CloudError::Message(format!(
                    "{}: Resume-Offset {} > Dateigröße {file_size}",
                    resume.rel_path, resume.offset
                )));
            }
            use tokio::io::AsyncSeekExt;
            file.seek(std::io::SeekFrom::Start(resume.offset)).await?;
            logging::log_info(&format!(
                "Dropbox-Session wird bei Byte {} fortgesetzt (session_id={:?}).",
                resume.offset, resume.session_id
            ));
            let cursor = DropboxCursor {
                session_id: resume.session_id.clone(),
                offset: resume.offset,
            };
            if let Some(cb) = on_progress_save.as_mut() {
                cb(Some(cursor.clone()), false);
            }
            (resume.session_id, resume.offset)
        } else {
            let n = read_chunk(&mut file, &mut buf).await?;
            let start_arg = json!({ "close": false });
            let chunk_n = n as u64;
            let on_bytes = on_bytes_sent.clone();
            let on_send = std::sync::Arc::new(move |sent: u64| {
                let abs = sent.min(chunk_n);
                emit_chunk_progress(abs, file_size, base_bytes_uploaded, total_job_size);
                if let Some(ref cb) = on_bytes {
                    cb(abs);
                }
            }) as std::sync::Arc<dyn Fn(u64) + Send + Sync>;
            let start = match self
                .content_upload_with_progress(
                    "/files/upload_session/start",
                    &start_arg,
                    Bytes::copy_from_slice(&buf[..n]),
                    control,
                    Some(on_send),
                )
                .await
            {
                Ok(v) => v,
                Err(e) => return Err(flush_err(&mut on_progress_save, None, e)),
            };
            let session_id = start
                .get("session_id")
                .and_then(Value::as_str)
                .ok_or_else(|| CloudError::Message("upload_session/start ohne session_id".into()))?
                .to_string();
            let offset = n as u64;
            let cursor = DropboxCursor {
                session_id: session_id.clone(),
                offset,
            };
            if let Some(cb) = on_progress_save.as_mut() {
                cb(Some(cursor.clone()), false);
            }
            (session_id, offset)
        };
        emit_chunk_progress(offset, file_size, base_bytes_uploaded, total_job_size);

        while file_size.saturating_sub(offset) > CHUNK_SIZE as u64 {
            let cursor_now = DropboxCursor {
                session_id: session_id.clone(),
                offset,
            };
            if let Err(e) = control.wait_if_paused().await {
                return Err(flush_err(&mut on_progress_save, Some(cursor_now.clone()), e.into()));
            }
            let n = read_chunk(&mut file, &mut buf).await?;
            if n == 0 {
                break;
            }
            let arg = session_append_arg(&session_id, offset, false);
            let base_off = offset;
            let chunk_n = n as u64;
            let on_bytes = on_bytes_sent.clone();
            let on_send = std::sync::Arc::new(move |sent: u64| {
                let abs = base_off.saturating_add(sent.min(chunk_n));
                emit_chunk_progress(abs, file_size, base_bytes_uploaded, total_job_size);
                if let Some(ref cb) = on_bytes {
                    cb(abs);
                }
            }) as std::sync::Arc<dyn Fn(u64) + Send + Sync>;
            if let Err(e) = self
                .content_upload_with_progress(
                    "/files/upload_session/append_v2",
                    &arg,
                    Bytes::copy_from_slice(&buf[..n]),
                    control,
                    Some(on_send),
                )
                .await
            {
                return Err(flush_err(
                    &mut on_progress_save,
                    Some(DropboxCursor {
                        session_id: session_id.clone(),
                        offset,
                    }),
                    e,
                ));
            }
            offset += n as u64;
            emit_chunk_progress(offset, file_size, base_bytes_uploaded, total_job_size);
            if let Some(cb) = on_progress_save.as_mut() {
                cb(Some(DropboxCursor {
                        session_id: session_id.clone(),
                        offset,
                    }),
                    false,
                );
            }
        }

        let cursor_now = DropboxCursor {
            session_id: session_id.clone(),
            offset,
        };
        if let Err(e) = control.wait_if_paused().await {
            return Err(flush_err(&mut on_progress_save, Some(cursor_now.clone()), e.into()));
        }
        let n = read_chunk(&mut file, &mut buf).await?;
        let finish_arg = session_finish_arg(&session_id, offset, dropbox_path);
        let base_off = offset;
        let chunk_n = n as u64;
        let on_bytes = on_bytes_sent.clone();
        let on_send = std::sync::Arc::new(move |sent: u64| {
            let abs = base_off.saturating_add(sent.min(chunk_n)).min(file_size);
            emit_chunk_progress(abs, file_size, base_bytes_uploaded, total_job_size);
            if let Some(ref cb) = on_bytes {
                cb(abs);
            }
        }) as std::sync::Arc<dyn Fn(u64) + Send + Sync>;
        let finished = match self
            .content_upload_with_progress(
                "/files/upload_session/finish",
                &finish_arg,
                Bytes::copy_from_slice(&buf[..n]),
                control,
                Some(on_send),
            )
            .await
        {
            Ok(v) => v,
            Err(e) => {
                return Err(flush_err(
                    &mut on_progress_save,
                    Some(DropboxCursor {
                        session_id: session_id.clone(),
                        offset,
                    }),
                    e,
                ));
            }
        };
        if let Some(cb) = on_progress_save.as_mut() {
            cb(None, true);
        }
        events::emit_progress_file(100, file_size, file_size);
        let current_total = base_bytes_uploaded + file_size;
        let total_progress = percent(current_total, total_job_size);
        events::emit_progress_total(total_progress, current_total, total_job_size);
        Ok(finished
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string))
    }

    pub(crate) async fn get_shareable_link_raw(
        &self,
        remote_path: &str,
    ) -> Result<Option<String>, CloudError> {
        self.shareable_link_url(remote_path).await
    }

    async fn shareable_link_url(&self, remote_path: &str) -> Result<Option<String>, CloudError> {
        if self.token().is_none() {
            logging::log_error("Link-Erstellung fehlgeschlagen: Nicht verbunden.");
            return Ok(None);
        }
        logging::log_info(&format!("Erstelle Freigabelink für: {remote_path}"));
        match self
            .rpc(
                "/sharing/list_shared_links",
                json!({ "path": remote_path, "direct_only": true }),
            )
            .await
        {
            Ok(result) => {
                if let Some(url) = first_shared_link_url(&result) {
                    logging::log_debug("Link existiert bereits, verwende existierenden Link.");
                    return Ok(Some(url));
                }
            }
            Err(e) => {
                logging::log_warn(&format!("sharing/list_shared_links: {e}"));
            }
        }

        match self
            .rpc(
                "/sharing/create_shared_link_with_settings",
                create_shared_link_body(remote_path),
            )
            .await
        {
            Ok(result) => {
                if let Some(url) = result.get("url").and_then(Value::as_str) {
                    logging::log_info(&format!("Link erfolgreich erstellt: {url}"));
                    return Ok(Some(url.to_string()));
                }
                Ok(None)
            }
            Err(e) => {
                let message = e.to_string();
                if message.contains("shared_link_already_exists") {
                    logging::log_warn("API-Fehler 'Link existiert bereits', versuche Abruf...");
                    match self
                        .rpc(
                            "/sharing/list_shared_links",
                            json!({ "path": remote_path, "direct_only": true }),
                        )
                        .await
                    {
                        Ok(result) => return Ok(first_shared_link_url(&result)),
                        Err(e2) => {
                            logging::log_error(&format!(
                                "Fehler beim Abrufen des existierenden Links: {e2}"
                            ));
                            return Ok(None);
                        }
                    }
                }
                logging::log_error(&format!("Dropbox API Fehler bei Link-Erstellung: {e}"));
                Ok(None)
            }
        }
    }
}

#[async_trait]
impl CloudClient for DropboxClient {
    async fn connect(&self) -> Result<bool, CloudError> {
        self.connect_session(true).await
    }

    async fn disconnect(&self) -> Result<(), CloudError> {
        self.disconnect_session(true, true).await
    }

    fn connection_status(&self) -> String {
        if self.token().is_none() {
            "Nicht verbunden".into()
        } else if self.connection_verified.load(Ordering::SeqCst) {
            "Verbunden".into()
        } else {
            "Verbindungsfehler".into()
        }
    }

    async fn upload_directory(
        &self,
        local_dir_path: &Path,
        remote_base_path: &str,
        control: &UploadControl,
        _kunde: &Kunde,
    ) -> Result<bool, CloudError> {
        if self.token().is_none() && self.connect().await.is_err() {
            logging::log_error("Upload fehlgeschlagen: Nicht mit Dropbox verbunden.");
            return Ok(false);
        }

        logging::log_info(&format!(
            "Beginne Upload von '{}' nach '{remote_base_path}'",
            local_dir_path.display()
        ));

        let files = collect_upload_files(local_dir_path, remote_base_path);
        let total_size: u64 = files.iter().map(|f| f.size).sum();
        if total_size == 0 {
            logging::log_error("Keine Dateien (oder nur leere Dateien) zum Hochladen gefunden.");
            events::emit_progress_total(100, 0, 0);
            return Ok(false);
        }

        let manifest: Vec<Value> = files
            .iter()
            .map(|f| json!({"name": f.rel_norm, "size": f.size}))
            .collect();
        let manifest_fp = manifest_fingerprint(&manifest);
        let raw_ck = load_checkpoint(local_dir_path);
        let mut resume_ck = None;
        if let Some(raw) = raw_ck {
            if raw.get("kind").and_then(Value::as_str) == Some("dropbox_native")
                && raw.get("manifest_fp").and_then(Value::as_str) == Some(manifest_fp.as_str())
                && raw.get("remote_base_path").and_then(Value::as_str) == Some(remote_base_path)
            {
                if let Err(msg) = assert_checkpoint_binding_matches(
                    &raw,
                    self.profile_ams_id().as_deref(),
                    self.profile_pool(),
                ) {
                    logging::log_warn(&msg);
                    return Err(CloudError::Message(msg));
                }
                resume_ck = Some(raw);
            } else {
                logging::log_warn("Dropbox-Native-Checkpoint verworfen.");
                clear_checkpoint(local_dir_path);
            }
        }

        let mut start_idx = 0usize;
        let mut resume_db = None;
        if let Some(ck) = resume_ck.as_ref() {
            start_idx = ck
                .get("next_file_index")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize;
            start_idx = start_idx.min(files.len());
            let da = ck.get("db_active").cloned().unwrap_or(Value::Null);
            if start_idx < files.len()
                && da.get("rel_path").and_then(Value::as_str)
                    == Some(files[start_idx].rel_norm.as_str())
            {
                let offset = da.get("offset").and_then(Value::as_u64).unwrap_or(0);
                if offset > 0 {
                    if let Some(sid) = da.get("session_id").and_then(Value::as_str) {
                        resume_db = Some(DropboxSessionResume {
                            session_id: sid.to_string(),
                            offset,
                            rel_path: files[start_idx].rel_norm.clone(),
                        });
                    }
                }
            }
            logging::log_info(&format!(
                "Dropbox-Upload fortsetzen (next_file_index={start_idx})."
            ));
        }

        let bytes_uploaded = if start_idx > 0 {
            files.iter().take(start_idx).map(|f| f.size).sum()
        } else {
            0
        };

        if resume_ck.is_none() {
            let _ = save_checkpoint(
                local_dir_path,
                &self.checkpoint_payload(json!({
                    "kind": "dropbox_native",
                    "manifest_fp": manifest_fp,
                    "remote_base_path": remote_base_path,
                    "total_size": total_size,
                    "phase": "uploading",
                    "next_file_index": 0,
                    "db_active": Value::Null,
                })),
            );
        }

        events::emit_started_at(files.len() as i32, start_idx as u32);

        let dir = local_dir_path.to_path_buf();
        let fp = manifest_fp.clone();
        let remote = remote_base_path.to_string();
        let ams = self.profile_ams_id();
        let pool = self.profile_pool();
        let files_for_ck = files.clone();
        let ck_saver = std::sync::Mutex::new(ThrottledCheckpointSaver::new(
            CK_MIN_INTERVAL_SECS,
            CHUNK_SIZE as u64,
        ));

        let result = dropbox_batch::upload_files_hybrid(
            self,
            &files,
            start_idx,
            total_size,
            bytes_uploaded,
            control,
            resume_db,
            |file_idx, cursor, force| {
                let i = file_idx.min(files_for_ck.len().saturating_sub(1));
                let file = &files_for_ck[i];
                let offset = cursor.as_ref().map(|c| c.offset).unwrap_or(0);
                let clear_active = cursor.is_none();
                let mut payload = if let Some(cursor) = cursor {
                    json!({
                        "kind": "dropbox_native",
                        "manifest_fp": fp,
                        "remote_base_path": remote,
                        "total_size": total_size,
                        "phase": "uploading",
                        "next_file_index": i,
                        "db_active": {
                            "rel_path": file.rel_norm,
                            "session_id": cursor.session_id,
                            "offset": cursor.offset,
                            "dropbox_path": file.dropbox_path,
                        },
                    })
                } else {
                    json!({
                        "kind": "dropbox_native",
                        "manifest_fp": fp,
                        "remote_base_path": remote,
                        "total_size": total_size,
                        "phase": "uploading",
                        "next_file_index": i,
                        "db_active": Value::Null,
                    })
                };
                if let Some(obj) = payload.as_object_mut() {
                    merge_checkpoint_binding(obj, ams.as_deref(), Some(pool));
                }
                if let Ok(mut saver) = ck_saver.lock() {
                    let _ = saver.update(&dir, &payload, offset, force);
                    if clear_active {
                        let _ = saver.flush();
                    }
                }
            },
            |next_idx, _bytes, _uploaded: &[HybridUploaded]| {
                if let Ok(mut saver) = ck_saver.lock() {
                    let _ = saver.flush();
                }
                let mut payload = json!({
                    "kind": "dropbox_native",
                    "manifest_fp": fp,
                    "remote_base_path": remote,
                    "total_size": total_size,
                    "phase": "uploading",
                    "next_file_index": next_idx,
                    "db_active": Value::Null,
                });
                if let Some(obj) = payload.as_object_mut() {
                    merge_checkpoint_binding(obj, ams.as_deref(), Some(pool));
                }
                let _ = save_checkpoint(&dir, &payload);
                Ok(())
            },
        )
        .await;

        match result {
            Ok(()) => {
                clear_checkpoint(local_dir_path);
                events::emit_status(format!("Upload für '{remote_base_path}' abgeschlossen."));
                logging::log_info(&format!("Upload für '{remote_base_path}' abgeschlossen."));
                Ok(true)
            }
            Err(e) if e.is_cancelled() => Err(e),
            Err(e) => {
                logging::log_error(&format!("Fehler beim Dropbox-Upload: {e}"));
                events::emit_status(format!("Fehler: {e}"));
                Ok(false)
            }
        }
    }

    async fn get_shareable_link(&self, remote_path: &str) -> Result<Option<String>, CloudError> {
        match self.shareable_link_url(remote_path).await? {
            Some(url) => Ok(Some(link_shortener::shorten(&url).await)),
            None => Ok(None),
        }
    }
}

pub fn app_key_hint(app_key: &str) -> String {
    let trimmed = app_key.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 8 {
        return trimmed.to_string();
    }
    let prefix: String = chars.iter().take(4).collect();
    let suffix: String = chars.iter().rev().take(4).rev().collect();
    format!("{prefix}…{suffix}")
}

fn json_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| {
            value
                .as_f64()
                .and_then(|n| if n.is_finite() && n >= 0.0 { Some(n as u64) } else { None })
        })
}

pub fn parse_account_info(payload: &Value, app_key_hint_value: &str) -> DropboxAccountInfo {
    let account_id = payload
        .get("account_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let display_name = payload
        .pointer("/name/display_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let email = payload
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let profile_photo_url = payload
        .get("profile_photo_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    DropboxAccountInfo {
        account_id,
        display_name,
        email,
        profile_photo_url,
        app_name: String::new(),
        app_key_hint: app_key_hint_value.to_string(),
        token_valid: true,
        used_bytes: 0,
        allocated_bytes: None,
    }
}

/// Root / app-folder display name from `files/get_metadata`.
pub fn parse_root_folder_name(payload: &Value) -> String {
    let name = payload
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !name.is_empty() {
        return name.to_string();
    }
    let path = payload
        .get("path_display")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .trim_matches('/');
    if path.is_empty() {
        return String::new();
    }
    path.rsplit('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Folder names under `/Apps` from `files/list_folder`.
pub fn parse_apps_folder_names(payload: &Value) -> Vec<String> {
    let Some(entries) = payload.get("entries").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter(|entry| {
            entry
                .get(".tag")
                .and_then(Value::as_str)
                .map(|t| t == "folder")
                .unwrap_or(false)
        })
        .filter_map(|entry| {
            entry
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .collect()
}

/// Prefer a single `/Apps` child; if several, prefer names that look like AMS.
pub fn pick_apps_folder_name(names: &[String]) -> String {
    if names.is_empty() {
        return String::new();
    }
    if names.len() == 1 {
        return names[0].clone();
    }
    let preferred = ["aeromediaservice", "aero media", "dropboxuploader", "dropbox uploader"];
    for name in names {
        let lower = name.to_ascii_lowercase();
        if preferred.iter().any(|p| lower.contains(p)) {
            return name.clone();
        }
    }
    String::new()
}

pub fn parse_space_usage(payload: &Value) -> (u64, Option<u64>) {
    let used = payload.get("used").and_then(json_u64).unwrap_or(0);
    let allocated = payload
        .pointer("/allocation/allocated")
        .and_then(json_u64)
        .filter(|&n| n > 0);
    (used, allocated)
}

pub fn collect_upload_files(local_dir_path: &Path, remote_base_path: &str) -> Vec<UploadFile> {
    let mut files = Vec::new();
    walk_collect(local_dir_path, local_dir_path, remote_base_path, &mut files);
    files.sort_by(|a, b| a.rel_norm.cmp(&b.rel_norm));
    files
}

fn walk_collect(root: &Path, current: &Path, remote_base: &str, out: &mut Vec<UploadFile>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_collect(root, &path, remote_base, out);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if should_skip_upload_file(name) {
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let rel_norm = relative.to_string_lossy().replace('\\', "/");
        let dropbox_path = join_dropbox_path(remote_base, &rel_norm);
        match fs::metadata(&path) {
            Ok(meta) => out.push(UploadFile {
                local_path: path,
                dropbox_path,
                size: meta.len(),
                rel_norm,
            }),
            Err(_) => logging::log_warn(&format!(
                "Datei nicht gefunden, überspringe: {}",
                path.display()
            )),
        }
    }
}

pub fn join_dropbox_path(remote_base: &str, relative: &str) -> String {
    let base = remote_base.replace('\\', "/");
    let base = base.trim_end_matches('/');
    let rel = relative
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string();
    if rel.is_empty() {
        base.to_string()
    } else if base.is_empty() {
        format!("/{rel}")
    } else {
        format!("{base}/{rel}")
    }
}

pub fn remote_dir_name(remote_base: &str) -> String {
    remote_base
        .replace('\\', "/")
        .trim_matches('/')
        .to_string()
}

pub fn files_upload_arg(path: &str) -> Value {
    json!({
        "path": path,
        "mode": { ".tag": "overwrite" },
        "autorename": false,
        "mute": false,
    })
}

pub fn session_append_arg(session_id: &str, offset: u64, close: bool) -> Value {
    json!({
        "cursor": { "session_id": session_id, "offset": offset },
        "close": close,
    })
}

pub fn session_finish_arg(session_id: &str, offset: u64, path: &str) -> Value {
    json!({
        "cursor": { "session_id": session_id, "offset": offset },
        "commit": {
            "path": path,
            "mode": { ".tag": "overwrite" },
        }
    })
}

pub fn create_shared_link_body(path: &str) -> Value {
    json!({
        "path": path,
        "settings": {
            "requested_visibility": { ".tag": "public" }
        }
    })
}

pub fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::BAD_GATEWAY
        || status == StatusCode::SERVICE_UNAVAILABLE
        || status == StatusCode::GATEWAY_TIMEOUT
        || status.as_u16() == 429
}

fn should_retry_error(err: &CloudError) -> bool {
    let lowered = err.to_string().to_lowercase();
    lowered.contains("timeout")
        || lowered.contains("connection")
        || lowered.contains("timed out")
        || lowered.contains("too_many_requests")
        || lowered.contains("rate_limit")
        || lowered.contains("internal_server")
        || lowered.contains("503")
        || lowered.contains("502")
        || lowered.contains("504")
}

pub fn retry_delay_secs(attempt: u32) -> f64 {
    (2.0_f64.powi(attempt as i32)).min(60.0)
}

pub(crate) fn percent(current: u64, total: u64) -> i32 {
    if total == 0 {
        0
    } else {
        ((current as f64 / total as f64) * 100.0) as i32
    }
}

pub(crate) fn emit_chunk_progress(bytes_sent: u64, file_size: u64, base: u64, total_job: u64) {
    let file_progress = percent(bytes_sent, file_size);
    events::emit_progress_file(file_progress, bytes_sent, file_size);
    let current_total = base + bytes_sent;
    events::emit_progress_total(percent(current_total, total_job), current_total, total_job);
}

/// Read up to `buf.len()` bytes, looping past short OS `read`s (like Python `f.read(n)`).
/// Returns 0 only at EOF; otherwise fills the buffer completely unless the file ends first.
pub(crate) async fn read_chunk(
    file: &mut tokio::fs::File,
    buf: &mut [u8],
) -> Result<usize, CloudError> {
    read_chunk_into(file, buf).await
}

pub(crate) async fn read_chunk_into<R: tokio::io::AsyncRead + Unpin>(
    file: &mut R,
    buf: &mut [u8],
) -> Result<usize, CloudError> {
    use tokio::io::AsyncReadExt;
    if buf.is_empty() {
        return Ok(0);
    }
    let mut filled = 0usize;
    while filled < buf.len() {
        let n = file.read(&mut buf[filled..]).await?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

fn first_shared_link_url(result: &Value) -> Option<String> {
    result
        .get("links")
        .and_then(Value::as_array)
        .and_then(|links| links.first())
        .and_then(|link| link.get("url"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::marker::{MARKER_FERTIG, MARKER_PROCESSING};
    use crate::upload::checkpoint::CHECKPOINT_FILENAME;
    use tempfile::tempdir;

    #[test]
    fn chunk_and_small_file_thresholds() {
        assert_eq!(CHUNK_SIZE, 32 * 1024 * 1024);
        assert_eq!(SMALL_FILE_BYTES, 4 * 1024 * 1024);
        assert!(SMALL_FILE_BYTES < CHUNK_SIZE);
        assert_eq!(BATCH_PARALLEL_WORKERS, 4);
        assert_eq!(BATCH_MAX_FILES, 1000);
        assert_eq!(CK_MIN_INTERVAL_SECS, 10.0);
    }

    /// `AsyncRead` that yields at most `max_per_read` bytes per call (simulates OS short reads).
    struct PartialReader {
        data: Vec<u8>,
        pos: usize,
        max_per_read: usize,
    }

    impl tokio::io::AsyncRead for PartialReader {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if self.pos >= self.data.len() {
                return std::task::Poll::Ready(Ok(()));
            }
            let want = buf.remaining().min(self.max_per_read);
            let end = (self.pos + want).min(self.data.len());
            let slice = &self.data[self.pos..end];
            buf.put_slice(slice);
            self.pos = end;
            std::task::Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn read_chunk_fills_buffer_despite_short_os_reads() {
        let payload = vec![0xABu8; 50_000];
        let mut reader = PartialReader {
            data: payload.clone(),
            pos: 0,
            max_per_read: 2048,
        };
        let mut buf = vec![0u8; 50_000];
        let n = read_chunk_into(&mut reader, &mut buf).await.unwrap();
        assert_eq!(n, 50_000);
        assert_eq!(buf, payload);
        let n_eof = read_chunk_into(&mut reader, &mut buf).await.unwrap();
        assert_eq!(n_eof, 0);
    }

    #[tokio::test]
    async fn read_chunk_returns_short_at_eof() {
        let mut reader = PartialReader {
            data: vec![1, 2, 3, 4, 5],
            pos: 0,
            max_per_read: 2,
        };
        let mut buf = [0u8; 16];
        let n = read_chunk_into(&mut reader, &mut buf).await.unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf[..5], &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn join_dropbox_path_normalizes_separators() {
        assert_eq!(join_dropbox_path("/Job", "a\\b.jpg"), "/Job/a/b.jpg");
        assert_eq!(join_dropbox_path("/Job/", "clip.mp4"), "/Job/clip.mp4");
        assert_eq!(join_dropbox_path("Job", "x"), "Job/x");
        assert_eq!(remote_dir_name("/Job-1/"), "Job-1");
        assert_eq!(remote_dir_name("\\Job-1\\sub\\"), "Job-1/sub");
    }

    #[test]
    fn upload_payloads_use_overwrite_tag() {
        let arg = files_upload_arg("/folder/a.jpg");
        assert_eq!(arg["path"], "/folder/a.jpg");
        assert_eq!(arg["mode"][".tag"], "overwrite");
        let finish = session_finish_arg("sid", 8192, "/folder/big.bin");
        assert_eq!(finish["cursor"]["session_id"], "sid");
        assert_eq!(finish["cursor"]["offset"], 8192);
        assert_eq!(finish["commit"]["mode"][".tag"], "overwrite");
        let append = session_append_arg("sid", 8, false);
        assert_eq!(append["close"], false);
        let share = create_shared_link_body("/Job");
        assert_eq!(share["settings"]["requested_visibility"][".tag"], "public");
    }

    #[test]
    fn retry_policy_matches_legacy() {
        assert!(should_retry_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(should_retry_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(!should_retry_status(StatusCode::BAD_REQUEST));
        assert_eq!(retry_delay_secs(1), 2.0);
        assert_eq!(retry_delay_secs(2), 4.0);
        assert_eq!(retry_delay_secs(10), 60.0);
    }

    #[test]
    fn collect_upload_files_skips_markers_and_sorts() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("b.jpg"), b"bb").unwrap();
        fs::write(dir.path().join("a.jpg"), b"a").unwrap();
        fs::write(dir.path().join(MARKER_FERTIG), b"{}").unwrap();
        fs::write(dir.path().join(MARKER_PROCESSING), b"{}").unwrap();
        fs::write(dir.path().join(CHECKPOINT_FILENAME), b"{}").unwrap();
        fs::write(dir.path().join(".DS_Store"), b"x").unwrap();
        let nested = dir.path().join("sub");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("c.bin"), b"ccc").unwrap();

        let files = collect_upload_files(dir.path(), "/Job");
        let names: Vec<_> = files.iter().map(|f| f.rel_norm.as_str()).collect();
        assert_eq!(names, ["a.jpg", "b.jpg", "sub/c.bin"]);
        assert_eq!(files[0].dropbox_path, "/Job/a.jpg");
        assert_eq!(files[2].dropbox_path, "/Job/sub/c.bin");
        assert_eq!(files[0].size, 1);
    }

    #[test]
    fn first_shared_link_reads_url() {
        let payload = json!({"links": [{"url": "https://www.dropbox.com/s/abc?dl=0"}]});
        assert_eq!(
            first_shared_link_url(&payload).as_deref(),
            Some("https://www.dropbox.com/s/abc?dl=0")
        );
        assert_eq!(first_shared_link_url(&json!({"links": []})), None);
    }

    #[test]
    fn parse_account_info_reads_name_and_email() {
        let payload = json!({
            "account_id": "dbid:AAH4f99T0taONIb-OurWxbNQ6ywGRopQngc",
            "name": { "display_name": "Ada Lovelace" },
            "email": "ada@example.com",
            "profile_photo_url": "https://example.com/ada.jpg"
        });
        let info = parse_account_info(&payload, "abcd…wxyz");
        assert_eq!(
            info.account_id,
            "dbid:AAH4f99T0taONIb-OurWxbNQ6ywGRopQngc"
        );
        assert_eq!(info.display_name, "Ada Lovelace");
        assert_eq!(info.email, "ada@example.com");
        assert!(is_connected_status("Verbunden"));
        assert!(is_connected_status("Verbunden (Ada Lovelace)"));
        assert!(!is_connected_status("Nicht verbunden"));
        assert!(!is_connected_status("Verbindungsfehler"));
        assert_eq!(info.profile_photo_url, "https://example.com/ada.jpg");
        assert!(info.token_valid);
        assert_eq!(info.app_key_hint, "abcd…wxyz");
    }

    #[test]
    fn secret_keys_for_account_are_pool_namespaced() {
        let native = DropboxSecretKeys::for_account(DropboxPool::Native, "ams-1");
        assert_eq!(native.app_key, "db_app_key_ams-1");
        assert_eq!(native.app_secret, "db_app_secret_ams-1");
        assert_eq!(native.refresh_token, "db_refresh_token_ams-1");

        let custom = DropboxSecretKeys::for_account(DropboxPool::CustomApi, "ams-1");
        assert_eq!(custom.app_key, "custom_db_app_key_ams-1");
        assert_eq!(custom.app_secret, "custom_db_app_secret_ams-1");
        assert_eq!(custom.refresh_token, "custom_db_refresh_token_ams-1");

        assert_ne!(native.app_key, custom.app_key);
        assert_ne!(native.refresh_token, custom.refresh_token);
        assert_ne!(native, DropboxSecretKeys::native());
        assert_ne!(custom, DropboxSecretKeys::custom_api());
        assert_eq!(
            DropboxSecretKeys::ams_id_from_keys(&native).as_deref(),
            Some("ams-1")
        );
        assert_eq!(
            DropboxSecretKeys::ams_id_from_keys(&custom).as_deref(),
            Some("ams-1")
        );
        assert_eq!(
            DropboxSecretKeys::pool_from_keys(&native),
            DropboxPool::Native
        );
        assert_eq!(
            DropboxSecretKeys::pool_from_keys(&custom),
            DropboxPool::CustomApi
        );
        assert!(DropboxSecretKeys::ams_id_from_keys(&DropboxSecretKeys::native()).is_none());
    }

    #[test]
    fn parse_space_usage_individual_and_team() {
        let individual = json!({
            "used": 1500,
            "allocation": { ".tag": "individual", "allocated": 2000 }
        });
        assert_eq!(parse_space_usage(&individual), (1500, Some(2000)));

        let team = json!({
            "used": 10,
            "allocation": { ".tag": "team", "allocated": 100 }
        });
        assert_eq!(parse_space_usage(&team), (10, Some(100)));

        let missing = json!({ "used": 42 });
        assert_eq!(parse_space_usage(&missing), (42, None));
    }

    #[test]
    fn app_key_hint_masks_middle() {
        assert_eq!(app_key_hint(""), "");
        assert_eq!(app_key_hint("short"), "short");
        assert_eq!(app_key_hint("abcdefghijklmnop"), "abcd…mnop");
    }

    #[test]
    fn parse_root_folder_name_from_metadata() {
        assert_eq!(
            parse_root_folder_name(&json!({
                ".tag": "folder",
                "name": "AeroMediaService",
                "path_display": ""
            })),
            "AeroMediaService"
        );
        assert_eq!(
            parse_root_folder_name(&json!({
                ".tag": "folder",
                "name": "",
                "path_display": "/Apps/My Dropbox App"
            })),
            "My Dropbox App"
        );
        assert_eq!(parse_root_folder_name(&json!({})), "");
    }

    #[test]
    fn parse_and_pick_apps_folder_names() {
        let payload = json!({
            "entries": [
                { ".tag": "folder", "name": "OtherApp" },
                { ".tag": "file", "name": "readme.txt" },
                { ".tag": "folder", "name": "AeroMediaService" }
            ]
        });
        let names = parse_apps_folder_names(&payload);
        assert_eq!(names, vec!["OtherApp", "AeroMediaService"]);
        assert_eq!(pick_apps_folder_name(&names), "AeroMediaService");
        assert_eq!(
            pick_apps_folder_name(&[String::from("OnlyOne")]),
            "OnlyOne"
        );
        assert_eq!(
            pick_apps_folder_name(&[String::from("Foo"), String::from("Bar")]),
            ""
        );
    }
}
