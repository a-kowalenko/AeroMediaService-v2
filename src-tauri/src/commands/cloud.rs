//! Tauri IPC for cloud connect/disconnect, Dropbox OAuth, SMS balance, shortener test.

use serde::Serialize;
use tauri::State;

use crate::cloud::{oauth::OauthStart, CloudClient, CloudState, DropboxSecretKeys};
use crate::commands::ConfigState;
use crate::notify::sms;
use crate::storage::logging;
use crate::storage::secrets;
use crate::util::link_shortener;

#[derive(Debug, Clone, Serialize)]
pub struct ConnectResult {
    pub success: bool,
    pub status: String,
    pub message: String,
    pub needs_oauth: bool,
    pub authorize_url: Option<String>,
    pub code_verifier: Option<String>,
}

impl ConnectResult {
    fn ok(status: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            success: true,
            status: status.into(),
            message: message.into(),
            needs_oauth: false,
            authorize_url: None,
            code_verifier: None,
        }
    }

    fn fail(status: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            success: false,
            status: status.into(),
            message: message.into(),
            needs_oauth: false,
            authorize_url: None,
            code_verifier: None,
        }
    }

    fn needs_oauth(start: OauthStart) -> Self {
        Self {
            success: false,
            status: "Warte auf OAuth...".into(),
            message: "Browser-Autorisierung erforderlich.".into(),
            needs_oauth: true,
            authorize_url: Some(start.authorize_url),
            code_verifier: Some(start.code_verifier),
        }
    }
}

fn selected_cloud(config: &ConfigState) -> String {
    config
        .get("selected_cloud_service", Some("dropbox"))
        .unwrap_or_else(|_| "dropbox".into())
        .trim()
        .to_ascii_lowercase()
}

fn parse_which(which: &str) -> Result<&'static str, String> {
    match which.trim().to_ascii_lowercase().as_str() {
        "native" | "dropbox" => Ok("native"),
        "custom" | "custom_dropbox" | "custom_api" => Ok("custom"),
        other => Err(format!("Unbekanntes Dropbox-Ziel: {other}")),
    }
}

#[tauri::command]
pub fn get_cloud_connection_status(
    cloud: State<'_, CloudState>,
    config: State<'_, ConfigState>,
) -> String {
    if selected_cloud(&config) == "custom_api" {
        cloud.custom_api.connection_status()
    } else {
        cloud.dropbox.connection_status()
    }
}

#[tauri::command]
pub async fn verify_dropbox_status(
    cloud: State<'_, CloudState>,
    which: String,
) -> Result<String, String> {
    match parse_which(&which)? {
        "custom" => Ok(cloud.custom_api.dropbox_connection_status_verified().await),
        _ => Ok(cloud.dropbox.connection_status_verified().await),
    }
}

#[tauri::command]
pub fn start_dropbox_oauth(
    cloud: State<'_, CloudState>,
    which: String,
) -> Result<OauthStart, String> {
    match parse_which(&which)? {
        "custom" => cloud
            .custom_api
            .start_dropbox_oauth()
            .map_err(|e| e.to_string()),
        _ => cloud.dropbox.start_oauth().map_err(|e| e.to_string()),
    }
}

#[tauri::command]
pub async fn finish_dropbox_oauth(
    cloud: State<'_, CloudState>,
    which: String,
    auth_code: String,
    code_verifier: String,
) -> Result<ConnectResult, String> {
    let which = parse_which(&which)?;
    let result = match which {
        "custom" => {
            cloud
                .custom_api
                .finish_dropbox_oauth(&auth_code, &code_verifier)
                .await
        }
        _ => {
            cloud
                .dropbox
                .finish_oauth(&auth_code, &code_verifier, true)
                .await
        }
    };
    match result {
        Ok(true) => Ok(ConnectResult::ok("Verbunden", "Dropbox-Verbindung hergestellt.")),
        Ok(false) => Ok(ConnectResult::fail("Nicht verbunden", "OAuth fehlgeschlagen.")),
        Err(e) => Ok(ConnectResult::fail("OAuth-Fehler", e.to_string())),
    }
}

#[tauri::command]
pub async fn connect_dropbox(
    cloud: State<'_, CloudState>,
    which: String,
) -> Result<ConnectResult, String> {
    let which = parse_which(&which)?;
    let connect_result = match which {
        "custom" => cloud.custom_api.connect_dropbox().await,
        _ => cloud.dropbox.connect_session(true).await,
    };

    match connect_result {
        Ok(true) => Ok(ConnectResult::ok("Verbunden", "Dropbox verbunden.")),
        Ok(false) => Ok(ConnectResult::fail(
            "Nicht verbunden",
            "Verbindung fehlgeschlagen.",
        )),
        Err(e) => {
            let text = e.to_string();
            if text.contains("Refresh-Token") {
                let oauth = match which {
                    "custom" => cloud.custom_api.start_dropbox_oauth(),
                    _ => cloud.dropbox.start_oauth(),
                };
                match oauth {
                    Ok(start) => Ok(ConnectResult::needs_oauth(start)),
                    Err(e2) => Ok(ConnectResult::fail("Nicht verbunden", e2.to_string())),
                }
            } else {
                Ok(ConnectResult::fail("Verbindungsfehler", text))
            }
        }
    }
}

#[tauri::command]
pub async fn disconnect_dropbox(
    cloud: State<'_, CloudState>,
    which: String,
) -> Result<ConnectResult, String> {
    match parse_which(&which)? {
        "custom" => {
            cloud
                .custom_api
                .disconnect_dropbox()
                .await
                .map_err(|e| e.to_string())?;
            Ok(ConnectResult::ok(
                "Nicht verbunden",
                "Custom-Dropbox getrennt.",
            ))
        }
        _ => {
            cloud
                .dropbox
                .disconnect_session(true, true)
                .await
                .map_err(|e| e.to_string())?;
            Ok(ConnectResult::ok("Nicht verbunden", "Dropbox getrennt."))
        }
    }
}

#[tauri::command]
pub async fn connect_custom_api(cloud: State<'_, CloudState>) -> Result<ConnectResult, String> {
    connect_custom_api_inner(&cloud).await
}

#[tauri::command]
pub async fn disconnect_custom_api(cloud: State<'_, CloudState>) -> Result<ConnectResult, String> {
    cloud
        .custom_api
        .disconnect()
        .await
        .map_err(|e| e.to_string())?;
    Ok(ConnectResult::ok("Nicht verbunden", "Custom API getrennt."))
}

#[tauri::command]
pub async fn connect_active_cloud(
    cloud: State<'_, CloudState>,
    config: State<'_, ConfigState>,
) -> Result<ConnectResult, String> {
    if selected_cloud(&config) == "custom_api" {
        connect_custom_api_inner(&cloud).await
    } else {
        match cloud.dropbox.connect_session(true).await {
            Ok(true) => Ok(ConnectResult::ok("Verbunden", "Dropbox verbunden.")),
            Ok(false) => Ok(ConnectResult::fail(
                "Nicht verbunden",
                "Dropbox-Verbindung fehlgeschlagen.",
            )),
            Err(e) => Ok(ConnectResult::fail("Verbindungsfehler", e.to_string())),
        }
    }
}

async fn connect_custom_api_inner(cloud: &CloudState) -> Result<ConnectResult, String> {
    match cloud.custom_api.connect().await {
        Ok(true) => {
            connect_dropbox_for_pure_contact_markers(cloud).await;
            Ok(ConnectResult::ok("Verbunden", "Custom API verbunden."))
        }
        Ok(false) => Ok(ConnectResult::fail(
            cloud.custom_api.connection_status(),
            "Custom-API-Verbindung fehlgeschlagen.",
        )),
        Err(e) => Ok(ConnectResult::fail("Verbindungsfehler", e.to_string())),
    }
}

fn has_dropbox_refresh_token(keys: DropboxSecretKeys) -> bool {
    secrets::get_secret(keys.refresh_token)
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .is_some()
}

fn has_dropbox_app_credentials(keys: DropboxSecretKeys) -> bool {
    let app_key = secrets::get_secret(keys.app_key)
        .ok()
        .flatten()
        .filter(|s| !s.is_empty());
    let app_secret = secrets::get_secret(keys.app_secret)
        .ok()
        .flatten()
        .filter(|s| !s.is_empty());
    app_key.is_some() && app_secret.is_some()
}

/// Legacy `StartupConnectWorker._connect_dropbox_for_contact_markers`: when Custom API
/// is active, also connect Dropbox upload accounts used for pure-contact markers.
async fn connect_dropbox_for_pure_contact_markers(cloud: &CloudState) {
    let custom_keys = DropboxSecretKeys::custom_api();
    if has_dropbox_app_credentials(custom_keys) && has_dropbox_refresh_token(custom_keys) {
        match cloud.custom_api.connect_dropbox().await {
            Ok(true) => {
                logging::log_info(
                    "Custom-Dropbox parallel verbunden (für reine Kontakt-Marker / Manifest-Upload).",
                );
            }
            Ok(false) => {
                logging::log_warn(
                    "Custom-Dropbox Auto-Verbindung fehlgeschlagen — reine Kontakt-Marker können scheitern.",
                );
            }
            Err(e) => {
                logging::log_warn(&format!(
                    "Custom-Dropbox Auto-Verbindung fehlgeschlagen: {e}"
                ));
            }
        }
    }

    let native_keys = DropboxSecretKeys::native();
    if !has_dropbox_refresh_token(native_keys) {
        logging::log_info(
            "Kein natives Dropbox Refresh-Token — optionaler Fallback für reine Kontakt-Marker entfällt.",
        );
        return;
    }
    if cloud.dropbox.connection_status() == "Verbunden" {
        return;
    }
    match cloud.dropbox.connect_session(false).await {
        Ok(true) => {
            logging::log_info("Natives Dropbox parallel verbunden (Legacy-Fallback für reine Kontakt-Marker).");
        }
        Ok(false) | Err(_) => {
            logging::log_warn(
                "Native Dropbox Auto-Verbindung fehlgeschlagen — Fallback für reine Kontakt-Marker nicht verfügbar.",
            );
        }
    }
}

#[tauri::command]
pub async fn disconnect_active_cloud(
    cloud: State<'_, CloudState>,
    config: State<'_, ConfigState>,
) -> Result<ConnectResult, String> {
    if selected_cloud(&config) == "custom_api" {
        cloud
            .custom_api
            .disconnect()
            .await
            .map_err(|e| e.to_string())?;
        Ok(ConnectResult::ok("Nicht verbunden", "Custom API getrennt."))
    } else {
        cloud
            .dropbox
            .disconnect_session(true, true)
            .await
            .map_err(|e| e.to_string())?;
        Ok(ConnectResult::ok("Nicht verbunden", "Dropbox getrennt."))
    }
}

#[tauri::command]
pub async fn auto_connect_cloud(
    cloud: State<'_, CloudState>,
    config: State<'_, ConfigState>,
) -> Result<ConnectResult, String> {
    let selected = selected_cloud(&config);
    let should = if selected == "custom_api" {
        let url = secrets::get_secret("custom_api_url")
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty());
        let token = secrets::get_secret("custom_api_bearer_token")
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty());
        url.is_some() && token.is_some()
    } else {
        secrets::get_secret("db_refresh_token")
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .is_some()
    };

    if !should {
        logging::log_info("Keine Auto-Verbindung (Credentials fehlen).");
        return Ok(ConnectResult::fail(
            "Nicht verbunden",
            "Keine gespeicherten Cloud-Credentials für Auto-Connect.",
        ));
    }

    logging::log_info("Prüfe auf Auto-Verbindung...");
    connect_active_cloud(cloud, config).await
}

#[tauri::command]
pub async fn get_sms_balance(api_key: Option<String>, sandbox: Option<bool>) -> Result<String, String> {
    let key = if let Some(k) = api_key.filter(|s| !s.trim().is_empty()) {
        k
    } else {
        let sandbox = sandbox.unwrap_or(false);
        let name = if sandbox {
            "seven_sandbox_api_key"
        } else {
            "seven_api_key"
        };
        secrets::get_secret(name)
            .map_err(|e| e.to_string())?
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "Fehlender API-Key".to_string())?
    };
    Ok(sms::get_balance_display(&key).await)
}

#[tauri::command]
pub async fn test_link_shortener(
    base_url: String,
    api_key: String,
    expires_preset: Option<String>,
) -> Result<String, String> {
    let base = base_url.trim();
    let key = api_key.trim();
    if base.is_empty() || key.is_empty() {
        return Err("Bitte Basis-URL und API-Key eintragen.".into());
    }
    let test_url = "https://example.com/aero-media-shortener-test";
    let result = link_shortener::shorten_with(
        test_url,
        Some(base),
        Some(key),
        Some(true),
        expires_preset.as_deref(),
    )
    .await;
    if result != test_url {
        Ok(result)
    } else {
        Err("Kürzen fehlgeschlagen. Details stehen im Log.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_which_accepts_aliases() {
        assert_eq!(parse_which("native").unwrap(), "native");
        assert_eq!(parse_which("dropbox").unwrap(), "native");
        assert_eq!(parse_which("custom").unwrap(), "custom");
        assert_eq!(parse_which("custom_dropbox").unwrap(), "custom");
        assert!(parse_which("other").is_err());
    }

    #[test]
    fn connect_result_needs_oauth_flags() {
        let r = ConnectResult::needs_oauth(OauthStart {
            authorize_url: "https://example".into(),
            code_verifier: "abc".into(),
        });
        assert!(r.needs_oauth);
        assert_eq!(r.authorize_url.as_deref(), Some("https://example"));
    }
}
