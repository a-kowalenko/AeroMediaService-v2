//! Custom Cloud API client (auth, orders/customer lookup, proxied + direct upload).
//! Port of the upload kernel from legacy `services/custom_api_client.py` (no notify/history).

pub mod auth;
pub mod orders;
pub mod upload;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::cloud::dropbox::DropboxClient;
use crate::cloud::traits::{CloudClient, CloudError};
use crate::events;
use crate::model::kunde::Kunde;
use crate::storage::logging;
use crate::upload::control::UploadControl;

pub use orders::{fetch_customer_as_kunde, lookup_customer_url};

pub const CHUNK_BYTES: usize = 4 * 1024 * 1024;
pub const ORDERS_CREATE_TIMEOUT_SECS: u64 = 120;
pub const MANIFEST_STATUS_POLL_MAX_SECS: u64 = 300;
pub const MANIFEST_STATUS_POLL_INTERVAL_SECS: u64 = 3;

pub struct CustomApiClient {
    http: reqwest::Client,
    api_base_url: Mutex<Option<String>>,
    api_key: Mutex<Option<String>>,
    connected: AtomicBool,
    last_customer_url: Mutex<Option<String>>,
    last_session_id: Mutex<Option<String>>,
    last_order_id: Mutex<Option<String>>,
    last_kunde: Mutex<Option<Kunde>>,
    dropbox: Arc<DropboxClient>,
}

impl Default for CustomApiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl CustomApiClient {
    pub fn new() -> Self {
        Self::with_dropbox(Arc::new(DropboxClient::for_custom_api()))
    }

    pub fn with_dropbox(dropbox: Arc<DropboxClient>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            api_base_url: Mutex::new(None),
            api_key: Mutex::new(None),
            connected: AtomicBool::new(false),
            last_customer_url: Mutex::new(None),
            last_session_id: Mutex::new(None),
            last_order_id: Mutex::new(None),
            last_kunde: Mutex::new(None),
            dropbox,
        }
    }

    fn api_base(&self) -> Option<String> {
        self.api_base_url.lock().ok().and_then(|g| g.clone())
    }

    fn api_key(&self) -> Option<String> {
        self.api_key.lock().ok().and_then(|g| g.clone())
    }

    fn set_credentials(&self, base: Option<String>, key: Option<String>) {
        if let Ok(mut guard) = self.api_base_url.lock() {
            *guard = base;
        }
        if let Ok(mut guard) = self.api_key.lock() {
            *guard = key;
        }
    }

    fn set_last_customer_url(&self, url: Option<String>) {
        if let Ok(mut guard) = self.last_customer_url.lock() {
            *guard = url;
        }
    }

    fn last_customer_url(&self) -> Option<String> {
        self.last_customer_url.lock().ok().and_then(|g| g.clone())
    }

    fn set_last_session_id(&self, id: Option<String>) {
        if let Ok(mut guard) = self.last_session_id.lock() {
            *guard = id;
        }
    }

    fn last_session_id(&self) -> Option<String> {
        self.last_session_id.lock().ok().and_then(|g| g.clone())
    }

    fn set_last_order_id(&self, id: Option<String>) {
        if let Ok(mut guard) = self.last_order_id.lock() {
            *guard = id.filter(|s| !s.is_empty());
        }
    }

    fn last_order_id(&self) -> Option<String> {
        self.last_order_id.lock().ok().and_then(|g| g.clone())
    }

    fn set_last_kunde(&self, kunde: Option<Kunde>) {
        if let Ok(mut guard) = self.last_kunde.lock() {
            *guard = kunde;
        }
    }

    #[allow(dead_code)]
    fn last_kunde(&self) -> Option<Kunde> {
        self.last_kunde.lock().ok().and_then(|g| g.clone())
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    #[allow(dead_code)]
    pub fn dropbox(&self) -> &DropboxClient {
        self.dropbox.as_ref()
    }

    pub async fn connect_dropbox(&self) -> Result<bool, CloudError> {
        self.dropbox.connect_session(false).await
    }

    pub fn start_dropbox_oauth(&self) -> Result<crate::cloud::OauthStart, CloudError> {
        self.dropbox.start_oauth()
    }

    pub async fn finish_dropbox_oauth(
        &self,
        auth_code: &str,
        code_verifier: &str,
    ) -> Result<bool, CloudError> {
        self.dropbox
            .finish_oauth(auth_code, code_verifier, false)
            .await
    }

    pub async fn disconnect_dropbox(&self) -> Result<(), CloudError> {
        self.dropbox.disconnect_session(false, false).await
    }

    #[allow(dead_code)]
    pub fn dropbox_connection_status(&self) -> String {
        self.dropbox.connection_status()
    }

    pub async fn dropbox_connection_status_verified(&self) -> String {
        self.dropbox.connection_status_verified().await
    }
}

#[async_trait]
impl CloudClient for CustomApiClient {
    async fn connect(&self) -> Result<bool, CloudError> {
        self.connect_api().await
    }

    async fn disconnect(&self) -> Result<(), CloudError> {
        logging::log_info("Trenne Verbindung zur Custom API...");
        self.set_credentials(None, None);
        self.connected.store(false, Ordering::SeqCst);
        events::emit_connection_status("Nicht verbunden");
        events::emit(events::STOP_MONITORING, ());
        Ok(())
    }

    fn connection_status(&self) -> String {
        if self.connected.load(Ordering::SeqCst) {
            "Verbunden".into()
        } else {
            "Nicht verbunden".into()
        }
    }

    async fn upload_directory(
        &self,
        local_dir_path: &std::path::Path,
        remote_base_path: &str,
        control: &UploadControl,
        kunde: &Kunde,
    ) -> Result<bool, CloudError> {
        self.upload_directory_inner(local_dir_path, remote_base_path, control, kunde)
            .await
    }

    async fn get_shareable_link(&self, _remote_path: &str) -> Result<Option<String>, CloudError> {
        self.shareable_link().await
    }
}

pub fn api_origin(base_url: &str) -> String {
    let b = base_url.trim().trim_end_matches('/');
    if let Some(stripped) = b.strip_suffix("/api") {
        stripped.to_string()
    } else {
        b.to_string()
    }
}

pub fn upload_api_root(base_url: &str) -> String {
    format!("{}/api/upload", api_origin(base_url))
}

pub fn http_is_transient(status: u16, body: &str) -> bool {
    matches!(status, 408 | 429 | 502 | 503 | 504) || body_suggests_invocation_timeout(body)
}

pub fn body_suggests_invocation_timeout(text: &str) -> bool {
    !text.is_empty() && text.to_ascii_uppercase().contains("FUNCTION_INVOCATION_TIMEOUT")
}

pub fn backoff_delay_secs(attempt: u32) -> f64 {
    (2.0_f64.powi(attempt.saturating_sub(1) as i32)).min(30.0)
}

pub fn summarize_api_error_body(text: &str, max_len: usize) -> String {
    let snippet = text.trim();
    if snippet.is_empty() {
        return "(leer)".into();
    }
    if let Ok(data) = serde_json::from_str::<Value>(snippet) {
        if let Some(summary) = data.get("error_summary").and_then(Value::as_str) {
            return truncate(summary, max_len);
        }
        if let Some(err) = data.get("error") {
            if err.is_object() && err.get(".tag").is_some() {
                return truncate(&err.to_string(), max_len);
            }
            if let Some(s) = err.as_str() {
                return truncate(s, max_len);
            }
        }
    }
    truncate(snippet, max_len)
}

fn truncate(text: &str, max_len: usize) -> String {
    if text.len() > max_len {
        format!("{}...", &text[..max_len])
    } else {
        text.to_string()
    }
}

pub fn extract_customer_url(result: &Value) -> Option<String> {
    if !result.is_object() {
        return None;
    }
    const DIRECT_KEYS: [&str; 9] = [
        "final_url",
        "customer_url",
        "customerUrl",
        "share_url",
        "shareUrl",
        "public_url",
        "publicUrl",
        "url",
        "archive_url",
    ];
    for key in DIRECT_KEYS {
        if let Some(value) = result.get(key).and_then(Value::as_str) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    for nested_key in ["data", "result", "upload", "session"] {
        if let Some(candidate) = result.get(nested_key) {
            if let Some(found) = extract_customer_url(candidate) {
                return Some(found);
            }
        }
    }
    None
}

/// Link extraction from `aero-media-customer` payloads (Resend / share-link lookup).
pub fn extract_link_from_customer_payload(payload: &Value) -> Option<String> {
    const LINK_KEYS: [&str; 6] = [
        "link",
        "customer_url",
        "customerUrl",
        "url",
        "short_order_id",
        "shortOrderId",
    ];
    fn from_obj(obj: &Value) -> Option<String> {
        for key in LINK_KEYS {
            if let Some(value) = obj.get(key).and_then(Value::as_str) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
        None
    }
    if let Some(found) = from_obj(payload) {
        return Some(found);
    }
    let customer = payload.get("customer")?;
    if let Some(found) = from_obj(customer) {
        return Some(found);
    }
    for media_key in ["media", "handycam", "handcam", "files"] {
        if let Some(found) = customer.get(media_key).and_then(from_obj) {
            return Some(found);
        }
    }
    None
}

pub fn parse_next_offset(payload: Option<&Value>, expected_next: u64) -> u64 {
    let Some(j) = payload else {
        return expected_next;
    };
    if let Some(no) = j.get("next_offset") {
        let parsed = no
            .as_u64()
            .or_else(|| no.as_i64().map(|v| v as u64))
            .or_else(|| no.as_str().and_then(|s| s.parse().ok()));
        if let Some(no) = parsed {
            return no;
        }
    }
    expected_next
}

pub fn guess_mime(path: &std::path::Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("heic") | Some("heif") => "image/heic",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("m4v") => "video/x-m4v",
        Some("avi") => "video/x-msvideo",
        Some("mkv") => "video/x-matroska",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("json") => "application/json",
        Some("txt") => "text/plain",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_origin_strips_trailing_api() {
        assert_eq!(api_origin("https://host.example/api"), "https://host.example");
        assert_eq!(api_origin("https://host.example/api/"), "https://host.example");
        assert_eq!(api_origin("https://host.example"), "https://host.example");
        assert_eq!(
            upload_api_root("https://host.example/api"),
            "https://host.example/api/upload"
        );
    }

    #[test]
    fn transient_and_timeout_body() {
        assert!(http_is_transient(503, ""));
        assert!(http_is_transient(200, "FUNCTION_INVOCATION_TIMEOUT"));
        assert!(!http_is_transient(400, "nope"));
        assert_eq!(backoff_delay_secs(1), 1.0);
        assert_eq!(backoff_delay_secs(2), 2.0);
        assert_eq!(backoff_delay_secs(6), 30.0);
    }

    #[test]
    fn parse_next_offset_prefers_server() {
        assert_eq!(parse_next_offset(None, 10), 10);
        assert_eq!(parse_next_offset(Some(&serde_json::json!({})), 10), 10);
        assert_eq!(
            parse_next_offset(Some(&serde_json::json!({"next_offset": 20})), 10),
            20
        );
        assert_eq!(
            parse_next_offset(Some(&serde_json::json!({"next_offset": "30"})), 10),
            30
        );
    }

    #[test]
    fn extract_customer_url_direct_and_nested() {
        assert_eq!(
            extract_customer_url(&serde_json::json!({"customer_url": " https://x "})).as_deref(),
            Some("https://x")
        );
        assert_eq!(
            extract_customer_url(&serde_json::json!({"data": {"final_url": "https://y"}})).as_deref(),
            Some("https://y")
        );
        assert_eq!(extract_customer_url(&serde_json::json!({"url": ""})), None);
    }

    #[test]
    fn extract_link_from_customer_payload_nested() {
        assert_eq!(
            extract_link_from_customer_payload(&serde_json::json!({"link": " https://x "})).as_deref(),
            Some("https://x")
        );
        assert_eq!(
            extract_link_from_customer_payload(&serde_json::json!({
                "customer": {"media": {"url": "https://y"}}
            }))
            .as_deref(),
            Some("https://y")
        );
        assert_eq!(
            extract_link_from_customer_payload(&serde_json::json!({"customer": {}})),
            None
        );
    }

    #[test]
    fn guess_mime_common_media() {
        assert_eq!(guess_mime(std::path::Path::new("a.JPG")), "image/jpeg");
        assert_eq!(guess_mime(std::path::Path::new("clip.mp4")), "video/mp4");
        assert_eq!(guess_mime(std::path::Path::new("x.bin")), "application/octet-stream");
    }
}
