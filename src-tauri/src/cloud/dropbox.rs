//! Dropbox HTTP client: refresh-token auth, OAuth/PKCE, chunk upload, share links.
//! Port of legacy `services/dropbox_client.py`.
//!
//! Secrets are read only from the OS keyring.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::cloud::oauth::{self, OauthStart};
use crate::cloud::traits::{should_skip_upload_file, CloudClient, CloudError};
use crate::events;
use crate::model::kunde::Kunde;
use crate::storage::logging;
use crate::storage::secrets;
use crate::upload::checkpoint::{clear_checkpoint, load_checkpoint, manifest_fingerprint, save_checkpoint};
use crate::upload::control::UploadControl;
use crate::util::link_shortener;

pub const CHUNK_SIZE: usize = 8 * 1024 * 1024;

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

#[derive(Debug, Clone, Copy)]
pub struct DropboxSecretKeys {
    pub app_key: &'static str,
    pub app_secret: &'static str,
    pub refresh_token: &'static str,
}

impl DropboxSecretKeys {
    pub fn native() -> Self {
        Self {
            app_key: "db_app_key",
            app_secret: "db_app_secret",
            refresh_token: "db_refresh_token",
        }
    }

    pub fn custom_api() -> Self {
        Self {
            app_key: "custom_db_app_key",
            app_secret: "custom_db_app_secret",
            refresh_token: "custom_db_refresh_token",
        }
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

pub struct DropboxClient {
    http: reqwest::Client,
    access_token: Mutex<Option<String>>,
    connection_verified: AtomicBool,
    keys: DropboxSecretKeys,
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
            access_token: Mutex::new(None),
            connection_verified: AtomicBool::new(false),
            keys,
        }
    }

    fn should_emit_status(&self) -> bool {
        self.keys.app_key == "db_app_key"
    }

    fn token(&self) -> Option<String> {
        self.access_token
            .lock()
            .ok()
            .and_then(|g| g.clone())
    }

    fn set_token(&self, token: Option<String>) {
        if let Ok(mut guard) = self.access_token.lock() {
            *guard = token;
        }
    }

    async fn refresh_access_token(&self) -> Result<String, CloudError> {
        let app_key = secrets::get_secret(self.keys.app_key)
            .map_err(|e| CloudError::Message(e.to_string()))?
            .filter(|s| !s.is_empty());
        let app_secret = secrets::get_secret(self.keys.app_secret)
            .map_err(|e| CloudError::Message(e.to_string()))?
            .filter(|s| !s.is_empty());
        let refresh_token = secrets::get_secret(self.keys.refresh_token)
            .map_err(|e| CloudError::Message(e.to_string()))?
            .filter(|s| !s.is_empty());

        let (Some(app_key), Some(app_secret)) = (app_key, app_secret) else {
            logging::log_warn("App Key oder App Secret für Dropbox fehlen.");
            if self.should_emit_status() {
                events::emit_connection_status("Fehler: App Key/Secret fehlt");
            }
            return Err(CloudError::NotConnected(
                "App Key oder App Secret für Dropbox fehlen.".into(),
            ));
        };
        let Some(refresh_token) = refresh_token else {
            if self.should_emit_status() {
                events::emit_connection_status("Nicht verbunden");
            }
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
                let _ = secrets::delete_secret(self.keys.refresh_token);
                self.set_token(None);
                self.connection_verified.store(false, Ordering::SeqCst);
                if self.should_emit_status() {
                    events::emit_connection_status("Nicht verbunden");
                }
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
        self.keys
    }

    /// Starts the Dropbox OAuth authorize URL (PKCE no-redirect).
    pub fn start_oauth(&self) -> Result<OauthStart, CloudError> {
        oauth::start_oauth_for_keys(self.keys.app_key, self.keys.app_secret)
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
            self.keys.app_key,
            self.keys.app_secret,
            self.keys.refresh_token,
            auth_code,
            code_verifier,
        )
        .await?;
        self.set_token(Some(access.clone()));
        match self.users_get_current_account(&access).await {
            Ok(()) => {
                self.connection_verified.store(true, Ordering::SeqCst);
                logging::log_info("Erfolgreich mit Dropbox verbunden (via OAuth).");
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
                Ok(()) => {
                    self.connection_verified.store(true, Ordering::SeqCst);
                    logging::log_info("Erfolgreich mit Dropbox verbunden (via Refresh-Token).");
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
        let _ = secrets::delete_secret(self.keys.refresh_token);
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
    pub async fn connection_status_verified(&self) -> String {
        if self.token().is_none() {
            if let Ok(token) = self.refresh_access_token().await {
                self.set_token(Some(token.clone()));
                return match self.users_get_current_account(&token).await {
                    Ok(()) => {
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
                Ok(()) => {
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

    async fn users_get_current_account(&self, token: &str) -> Result<(), CloudError> {
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
        Ok(())
    }

    fn auth_headers(token: &str, api_arg: Option<&Value>) -> Result<HeaderMap, CloudError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| CloudError::Message(e.to_string()))?,
        );
        if let Some(arg) = api_arg {
            let encoded = serde_json::to_string(arg).map_err(|e| CloudError::Message(e.to_string()))?;
            headers.insert(
                "Dropbox-API-Arg",
                HeaderValue::from_str(&encoded).map_err(|e| CloudError::Message(e.to_string()))?,
            );
        }
        Ok(headers)
    }

    async fn with_retry<F, Fut>(&self, tag: &str, mut operation: F) -> Result<reqwest::Response, CloudError>
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

    async fn content_upload(
        &self,
        path: &str,
        api_arg: &Value,
        body: Vec<u8>,
        control: &UploadControl,
    ) -> Result<Value, CloudError> {
        control.wait_if_paused().await?;
        let mut token = self.ensure_token().await?;
        for auth_try in 0..2 {
            match self.send_content(path, &token, api_arg, &body).await {
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
        body: &[u8],
    ) -> Result<reqwest::Response, CloudError> {
        let headers = Self::auth_headers(token, Some(api_arg))?;
        let url = format!("{CONTENT_URL}{path}");
        let bytes = body.to_vec();
        let request = self
            .http
            .post(url)
            .headers(headers)
            .header(CONTENT_TYPE, "application/octet-stream")
            .body(bytes);
        self.with_retry(path, || {
            let request = request.try_clone().ok_or_else(|| {
                CloudError::Message("Dropbox-Request konnte nicht geklont werden.".into())
            });
            async move {
                request?
                    .send()
                    .await
                    .map_err(|e| CloudError::Http(e.to_string()))
            }
        })
        .await
    }

    async fn rpc(&self, path: &str, body: Value) -> Result<Value, CloudError> {
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
                    return serde_json::from_str(&text)
                        .map_err(|e| CloudError::Http(format!("{path}: ungültiges JSON ({e}) {text}")));
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
        control.wait_if_paused().await?;
        let data = fs::read(local_path)?;
        let arg = files_upload_arg(dropbox_path);
        let result = self
            .content_upload("/files/upload", &arg, data, control)
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
        on_progress_save: Option<F>,
    ) -> Result<Option<String>, CloudError>
    where
        F: Fn(Option<&DropboxCursor>),
    {
        control.wait_if_paused().await?;
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
            if let Some(cb) = on_progress_save.as_ref() {
                cb(Some(&cursor));
            }
            (resume.session_id, resume.offset)
        } else {
            let n = read_chunk(&mut file, &mut buf).await?;
            let start_arg = json!({ "close": false });
            let start = self
                .content_upload(
                    "/files/upload_session/start",
                    &start_arg,
                    buf[..n].to_vec(),
                    control,
                )
                .await?;
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
            if let Some(cb) = on_progress_save.as_ref() {
                cb(Some(&cursor));
            }
            (session_id, offset)
        };
        emit_chunk_progress(offset, file_size, base_bytes_uploaded, total_job_size);

        while file_size.saturating_sub(offset) > CHUNK_SIZE as u64 {
            control.wait_if_paused().await?;
            let n = read_chunk(&mut file, &mut buf).await?;
            if n == 0 {
                break;
            }
            let arg = session_append_arg(&session_id, offset, false);
            self.content_upload(
                "/files/upload_session/append_v2",
                &arg,
                buf[..n].to_vec(),
                control,
            )
            .await?;
            offset += n as u64;
            emit_chunk_progress(offset, file_size, base_bytes_uploaded, total_job_size);
            if let Some(cb) = on_progress_save.as_ref() {
                cb(Some(&DropboxCursor {
                    session_id: session_id.clone(),
                    offset,
                }));
            }
        }

        control.wait_if_paused().await?;
        let n = read_chunk(&mut file, &mut buf).await?;
        let finish_arg = session_finish_arg(&session_id, offset, dropbox_path);
        let finished = self
            .content_upload(
                "/files/upload_session/finish",
                &finish_arg,
                buf[..n].to_vec(),
                control,
            )
            .await?;
        if let Some(cb) = on_progress_save.as_ref() {
            cb(None);
        }
        events::emit_progress_file(100, file_size, file_size);
        let current_total = base_bytes_uploaded + file_size;
        let total_progress = percent(current_total, total_job_size);
        events::emit_progress_total(total_progress, current_total, total_job_size);
        Ok(finished.get("id").and_then(Value::as_str).map(str::to_string))
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
                && da.get("rel_path").and_then(Value::as_str) == Some(files[start_idx].rel_norm.as_str())
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

        let mut bytes_uploaded = if start_idx > 0 {
            files.iter().take(start_idx).map(|f| f.size).sum()
        } else {
            0
        };

        if resume_ck.is_none() {
            let _ = save_checkpoint(
                local_dir_path,
                &json!({
                    "kind": "dropbox_native",
                    "manifest_fp": manifest_fp,
                    "remote_base_path": remote_base_path,
                    "total_size": total_size,
                    "phase": "uploading",
                    "next_file_index": 0,
                    "db_active": Value::Null,
                }),
            );
        }

        events::emit_started(files.len() as i32);
        for i in start_idx..files.len() {
            let file = &files[i];
            control.wait_if_paused().await?;
            let mb = file.size as f64 / 1024.0 / 1024.0;
            let status_msg = format!(
                "Lade hoch: {} ({mb:.2} MB)",
                file.local_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
            );
            events::emit_status(&status_msg);
            events::emit_progress_message(format!(
                "Datei {}/{}: {}",
                i + 1,
                files.len(),
                file.rel_norm
            ));
            logging::log_debug(&status_msg);
            events::emit_progress_file(0, 0, file.size);

            let resume = if i == start_idx { resume_db.clone() } else { None };
            let result = if file.size <= CHUNK_SIZE as u64 {
                self.upload_small_file(&file.local_path, &file.dropbox_path, file.size, control)
                    .await
                    .map(|_| ())
            } else {
                let rel = file.rel_norm.clone();
                let dp = file.dropbox_path.clone();
                let dir = local_dir_path.to_path_buf();
                let fp = manifest_fp.clone();
                let remote = remote_base_path.to_string();
                self.upload_large_file(
                    &file.local_path,
                    &file.dropbox_path,
                    file.size,
                    bytes_uploaded,
                    total_size,
                    control,
                    resume,
                    Some(|cursor: Option<&DropboxCursor>| {
                        let payload = if let Some(cursor) = cursor {
                            json!({
                                "kind": "dropbox_native",
                                "manifest_fp": fp,
                                "remote_base_path": remote,
                                "total_size": total_size,
                                "phase": "uploading",
                                "next_file_index": i,
                                "db_active": {
                                    "rel_path": rel,
                                    "session_id": cursor.session_id,
                                    "offset": cursor.offset,
                                    "dropbox_path": dp,
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
                        let _ = save_checkpoint(&dir, &payload);
                    }),
                )
                .await
                .map(|_| ())
            };

            match result {
                Ok(()) => {
                    bytes_uploaded += file.size;
                    let total_progress = percent(bytes_uploaded, total_size);
                    events::emit_progress_total(total_progress, bytes_uploaded, total_size);
                    let _ = save_checkpoint(
                        local_dir_path,
                        &json!({
                            "kind": "dropbox_native",
                            "manifest_fp": manifest_fp,
                            "remote_base_path": remote_base_path,
                            "total_size": total_size,
                            "phase": "uploading",
                            "next_file_index": i + 1,
                            "db_active": Value::Null,
                        }),
                    );
                }
                Err(e) if e.is_cancelled() => return Err(e),
                Err(e) => {
                    logging::log_error(&format!(
                        "Fehler beim Upload von {}: {e}",
                        file.local_path.display()
                    ));
                    events::emit_status(format!("Fehler: {e}"));
                    return Ok(false);
                }
            }
        }

        clear_checkpoint(local_dir_path);
        events::emit_status(format!("Upload für '{remote_base_path}' abgeschlossen."));
        logging::log_info(&format!("Upload für '{remote_base_path}' abgeschlossen."));
        Ok(true)
    }

    async fn get_shareable_link(&self, remote_path: &str) -> Result<Option<String>, CloudError> {
        match self.shareable_link_url(remote_path).await? {
            Some(url) => Ok(Some(link_shortener::shorten(&url).await)),
            None => Ok(None),
        }
    }
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
    let rel = relative.replace('\\', "/").trim_start_matches('/').to_string();
    if rel.is_empty() {
        base.to_string()
    } else if base.is_empty() {
        format!("/{rel}")
    } else {
        format!("{base}/{rel}")
    }
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

fn percent(current: u64, total: u64) -> i32 {
    if total == 0 {
        0
    } else {
        ((current as f64 / total as f64) * 100.0) as i32
    }
}

fn emit_chunk_progress(bytes_sent: u64, file_size: u64, base: u64, total_job: u64) {
    let file_progress = percent(bytes_sent, file_size);
    events::emit_progress_file(file_progress, bytes_sent, file_size);
    let current_total = base + bytes_sent;
    events::emit_progress_total(percent(current_total, total_job), current_total, total_job);
}

async fn read_chunk(file: &mut tokio::fs::File, buf: &mut [u8]) -> Result<usize, CloudError> {
    use tokio::io::AsyncReadExt;
    let n = file.read(buf).await?;
    Ok(n)
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
    fn join_dropbox_path_normalizes_separators() {
        assert_eq!(join_dropbox_path("/Job", "a\\b.jpg"), "/Job/a/b.jpg");
        assert_eq!(join_dropbox_path("/Job/", "clip.mp4"), "/Job/clip.mp4");
        assert_eq!(join_dropbox_path("Job", "x"), "Job/x");
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
}
