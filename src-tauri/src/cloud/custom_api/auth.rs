//! Auth, origin helpers, and HTTP retry for the Custom API.

use std::sync::atomic::Ordering;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::multipart::{Form, Part};
use reqwest::StatusCode;
use serde_json::Value;

use super::{
    api_origin, backoff_delay_secs, http_is_transient, summarize_api_error_body, upload_api_root,
    CustomApiClient,
};
use crate::cloud::traits::CloudError;
use crate::events;
use crate::storage::logging;
use crate::storage::secrets;
use crate::upload::control::UploadControl;

impl CustomApiClient {
    pub(super) async fn connect_api(&self) -> Result<bool, CloudError> {
        let api_base_url = secrets::get_secret("custom_api_url")
            .map_err(|e| CloudError::Message(e.to_string()))?
            .filter(|s| !s.trim().is_empty());
        let api_key = secrets::get_secret("custom_api_bearer_token")
            .map_err(|e| CloudError::Message(e.to_string()))?
            .filter(|s| !s.trim().is_empty());

        let (Some(api_base_url), Some(api_key)) = (api_base_url, api_key) else {
            logging::log_warn("API Base URL oder API Key fehlen.");
            events::emit_connection_status("Fehler: API Credentials fehlen");
            return Ok(false);
        };

        self.set_credentials(Some(api_base_url.clone()), Some(api_key.clone()));

        let health_path = crate::storage::config::runtime_setting("custom_api_health_endpoint");
        let health_path = {
            let trimmed = health_path.trim();
            if trimmed.is_empty() {
                "/health".to_string()
            } else if trimmed.starts_with('/') {
                trimmed.to_string()
            } else {
                format!("/{trimmed}")
            }
        };
        let url = format!(
            "{}{}",
            api_base_url.trim_end_matches('/'),
            health_path
        );
        let response = match self
            .http
            .get(&url)
            .header(AUTHORIZATION, format!("Bearer {api_key}"))
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                logging::log_error(&format!("Verbindungsfehler zur API: {e}"));
                events::emit_connection_status(format!("Verbindungsfehler: {e}"));
                self.connected.store(false, Ordering::SeqCst);
                return Ok(false);
            }
        };

        let status = response.status();
        if status == StatusCode::OK {
            self.connected.store(true, Ordering::SeqCst);
            logging::log_info("Erfolgreich mit Custom API verbunden (Session-Chunk-Upload).");
            events::emit_connection_status("Verbunden");
            return Ok(true);
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            let snippet: String = response.text().await.unwrap_or_default().chars().take(200).collect();
            let msg = "Bearer-Token ungueltig oder ohne 'upload'-Permission";
            logging::log_error(&format!("API Connection: {msg} — {snippet}"));
            events::emit_connection_status(format!("Fehler: {msg}"));
            self.connected.store(false, Ordering::SeqCst);
            return Ok(false);
        }
        logging::log_error(&format!("API Connection fehlgeschlagen: {status}"));
        events::emit_connection_status(format!("Fehler: HTTP {status}"));
        self.connected.store(false, Ordering::SeqCst);
        Ok(false)
    }

    pub(super) fn origin(&self) -> Result<String, CloudError> {
        let base = self
            .api_base()
            .ok_or_else(|| CloudError::NotConnected("Custom API nicht verbunden.".into()))?;
        Ok(api_origin(&base))
    }

    pub(super) fn upload_root(&self) -> Result<String, CloudError> {
        let base = self
            .api_base()
            .ok_or_else(|| CloudError::NotConnected("Custom API nicht verbunden.".into()))?;
        Ok(upload_api_root(&base))
    }

    fn auth_header(&self) -> Result<HeaderMap, CloudError> {
        let key = self
            .api_key()
            .ok_or_else(|| CloudError::NotConnected("Custom API nicht verbunden.".into()))?;
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {key}"))
                .map_err(|e| CloudError::Message(e.to_string()))?,
        );
        Ok(headers)
    }

    pub(super) async fn post_json_upload(
        &self,
        path_suffix: &str,
        json_body: &Value,
        timeout: Duration,
        tag: &str,
        soft_fail_statuses: &[u16],
        control: &UploadControl,
    ) -> Result<reqwest::Response, CloudError> {
        let url = format!("{}{path_suffix}", self.upload_root()?);
        self.post_json_url(&url, json_body, timeout, tag, soft_fail_statuses, control, false)
            .await
    }

    pub(super) async fn post_json_url(
        &self,
        url: &str,
        json_body: &Value,
        timeout: Duration,
        tag: &str,
        soft_fail_statuses: &[u16],
        control: &UploadControl,
        no_retry_if_known_order: bool,
    ) -> Result<reqwest::Response, CloudError> {
        let max_attempts = 6u32;
        let mut last_err = CloudError::Message(format!("{tag}: keine Versuche"));
        for attempt in 1..=max_attempts {
            control.wait_if_paused().await?;
            let headers = self.auth_header()?;
            let result = self
                .http
                .post(url)
                .headers(headers)
                .header(CONTENT_TYPE, "application/json")
                .timeout(timeout)
                .json(json_body)
                .send()
                .await;
            match result {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if soft_fail_statuses.contains(&status) {
                        return Ok(response);
                    }
                    if status == 401 || status == 403 {
                        let body = response.text().await.unwrap_or_default();
                        let snippet: String = body.chars().take(200).collect();
                        return Err(CloudError::Message(format!(
                            "API-Key fehlt oder hat keine 'upload'-Permission (HTTP {status}). {snippet}"
                        )));
                    }
                    if (200..300).contains(&status) {
                        if attempt > 1 {
                            logging::log_info(&format!("{tag}: HTTP {status} nach Versuch {attempt}"));
                        }
                        return Ok(response);
                    }
                    let body = response.text().await.unwrap_or_default();
                    if http_is_transient(status, &body) {
                        if no_retry_if_known_order {
                            return Err(CloudError::Message(format!(
                                "{tag}: HTTP {status} — Order existiert, kein POST-Retry"
                            )));
                        }
                        last_err = CloudError::Http(format!(
                            "{tag}: HTTP {status} — {}",
                            summarize_api_error_body(&body, 800)
                        ));
                    } else {
                        return Err(CloudError::Http(format!(
                            "{tag}: HTTP {status} — {}",
                            summarize_api_error_body(&body, 800)
                        )));
                    }
                }
                Err(e) => {
                    if e.is_timeout() && no_retry_if_known_order {
                        return Err(CloudError::Message(format!(
                            "{tag}: Timeout nach {}s — kein POST-Retry (Order ggf. bereits angelegt).",
                            timeout.as_secs()
                        )));
                    }
                    last_err = CloudError::Http(e.to_string());
                    if attempt >= max_attempts {
                        return Err(last_err);
                    }
                }
            }
            if attempt >= max_attempts {
                break;
            }
            let delay = backoff_delay_secs(attempt);
            logging::log_warn(&format!(
                "{tag}: Versuch {attempt}/{max_attempts} fehlgeschlagen, warte {delay:.1}s — {last_err}"
            ));
            tokio::time::sleep(Duration::from_secs_f64(delay)).await;
        }
        Err(last_err)
    }

    pub(super) async fn get_json(
        &self,
        url: &str,
        timeout: Duration,
        control: &UploadControl,
    ) -> Result<reqwest::Response, CloudError> {
        control.wait_if_paused().await?;
        let headers = self.auth_header()?;
        self.http
            .get(url)
            .headers(headers)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| CloudError::Http(e.to_string()))
    }

    pub(super) async fn post_session_multipart(
        &self,
        subpath: &str,
        session_id: &str,
        file_name: &str,
        extra_text: &[(&str, String)],
        chunk: Vec<u8>,
        control: &UploadControl,
    ) -> Result<reqwest::Response, CloudError> {
        let url = format!("{}{subpath}", self.upload_root()?);
        let max_attempts = 6u32;
        let mut last_err = CloudError::Message(format!("Session {subpath}: keine Versuche"));
        for attempt in 1..=max_attempts {
            control.wait_if_paused().await?;
            let mut form = Form::new()
                .text("session_id", session_id.to_string())
                .text("file_name", file_name.to_string());
            for (key, value) in extra_text {
                form = form.text((*key).to_string(), value.clone());
            }
            let part = Part::bytes(chunk.clone())
                .file_name("chunk")
                .mime_str("application/octet-stream")
                .map_err(|e| CloudError::Message(e.to_string()))?;
            form = form.part("chunk", part);

            let headers = self.auth_header()?;
            let result = self
                .http
                .post(&url)
                .headers(headers)
                .timeout(Duration::from_secs(600))
                .multipart(form)
                .send()
                .await;
            match result {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if status == 401 || status == 403 {
                        let body = response.text().await.unwrap_or_default();
                        let snippet: String = body.chars().take(200).collect();
                        return Err(CloudError::Message(format!(
                            "API-Key fehlt oder hat keine 'upload'-Permission (HTTP {status}). {snippet}"
                        )));
                    }
                    if (200..300).contains(&status) {
                        logging::log_info(&format!(
                            "Session {subpath} [session={session_id:?} file={file_name:?}]: HTTP {status}{}",
                            if attempt > 1 {
                                format!(" (Versuch {attempt})")
                            } else {
                                String::new()
                            }
                        ));
                        return Ok(response);
                    }
                    let body = response.text().await.unwrap_or_default();
                    if http_is_transient(status, &body) {
                        last_err = CloudError::Http(format!(
                            "Session {subpath}: HTTP {status} — {}",
                            summarize_api_error_body(&body, 800)
                        ));
                    } else {
                        return Err(CloudError::Http(format!(
                            "Session upload {subpath}: HTTP {status} — {}",
                            summarize_api_error_body(&body, 800)
                        )));
                    }
                }
                Err(e) => {
                    last_err = CloudError::Http(e.to_string());
                    if attempt >= max_attempts {
                        return Err(last_err);
                    }
                }
            }
            if attempt >= max_attempts {
                break;
            }
            let delay = backoff_delay_secs(attempt);
            logging::log_warn(&format!(
                "Session {subpath}: Versuch {attempt}/{max_attempts} fehlgeschlagen, warte {delay:.1}s — {last_err}"
            ));
            tokio::time::sleep(Duration::from_secs_f64(delay)).await;
        }
        Err(last_err)
    }
}
