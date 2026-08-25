//! Tauri IPC for cloud connect/disconnect, Dropbox OAuth, SMS balance, shortener test.

use serde::Serialize;
use tauri::State;

use crate::cloud::{
    guards::{self, OauthIdentityOutcome},
    oauth::OauthStart, CloudClient, CloudState, DropboxAccountInfo, DropboxPool, DropboxSecretKeys,
};
use crate::commands::ConfigState;
use crate::notify::sms;
use crate::storage::dropbox_accounts::{
    self, DropboxAccountRow, DropboxAccountState,
};
use crate::storage::logging;
use crate::storage::secrets;
use crate::upload::UploadState;
use crate::util::link_shortener;
use std::sync::Arc;
use crate::cloud::DropboxClient;

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

fn parse_pool(which: &str) -> Result<DropboxPool, String> {
    DropboxPool::parse(which).map_err(|e| e.to_string())
}

fn active_client(cloud: &CloudState, pool: DropboxPool) -> Arc<DropboxClient> {
    match pool {
        DropboxPool::Native => cloud.dropbox(),
        DropboxPool::CustomApi => cloud.custom_dropbox(),
    }
}

fn resolve_client(
    cloud: &CloudState,
    pool: DropboxPool,
    account_id: Option<&str>,
) -> Arc<DropboxClient> {
    match account_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => cloud.client_for(pool, id),
        None => active_client(cloud, pool),
    }
}

/// Apply Dropbox account_id semantics after OAuth/connect (16c D5).
fn apply_identity_after_connect(
    accounts: &DropboxAccountState,
    config: &ConfigState,
    pool: DropboxPool,
    ams_id: &str,
    info: &DropboxAccountInfo,
) -> Result<OauthIdentityOutcome, String> {
    let outcome = accounts.with_store(|store| {
        guards::apply_oauth_account_identity(store, pool, ams_id, info)
            .map_err(crate::storage::dropbox_accounts::DropboxAccountError::Message)
    })?;
    if outcome.is_ok() {
        let _ = config.with_store_mut(|store| {
            dropbox_accounts::sync_active_secrets_with_legacy(store, pool).map_err(|e| e.to_string())
        });
    }
    Ok(outcome)
}

fn connect_result_from_identity(
    outcome: OauthIdentityOutcome,
    default_ok_message: &str,
) -> ConnectResult {
    match &outcome {
        OauthIdentityOutcome::Updated { .. } => {
            ConnectResult::ok("Verbunden", default_ok_message)
        }
        OauthIdentityOutcome::AppliedToExisting { .. } => {
            ConnectResult::ok("Verbunden", outcome.message())
        }
        OauthIdentityOutcome::RejectedMismatch { .. } => {
            ConnectResult::fail("Konto-Konflikt", outcome.message())
        }
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
        cloud.dropbox().connection_status()
    }
}

#[tauri::command]
pub async fn verify_dropbox_status(
    cloud: State<'_, CloudState>,
    which: String,
    account_id: Option<String>,
) -> Result<String, String> {
    let pool = parse_pool(&which)?;
    let client = resolve_client(&cloud, pool, account_id.as_deref());
    Ok(client.connection_status_verified().await)
}

#[tauri::command]
pub async fn get_dropbox_account_info(
    cloud: State<'_, CloudState>,
    which: String,
    account_id: Option<String>,
) -> Result<DropboxAccountInfo, String> {
    let pool = parse_pool(&which)?;
    let client = resolve_client(&cloud, pool, account_id.as_deref());
    client.account_info().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_dropbox_oauth(
    cloud: State<'_, CloudState>,
    which: String,
    account_id: Option<String>,
) -> Result<OauthStart, String> {
    let pool = parse_pool(&which)?;
    let client = resolve_client(&cloud, pool, account_id.as_deref());
    client.start_oauth().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn finish_dropbox_oauth(
    cloud: State<'_, CloudState>,
    accounts: State<'_, DropboxAccountState>,
    config: State<'_, ConfigState>,
    which: String,
    auth_code: String,
    code_verifier: String,
    account_id: Option<String>,
) -> Result<ConnectResult, String> {
    let pool = parse_pool(&which)?;
    let ams_id = account_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| cloud.active_account_id(pool));
    let client = resolve_client(&cloud, pool, ams_id.as_deref());
    let emit = pool == DropboxPool::Native;
    match client
        .finish_oauth(&auth_code, &code_verifier, emit)
        .await
    {
        Ok(true) => {
            if let Some(id) = ams_id.as_deref() {
                match client.account_info().await {
                    Ok(info) => {
                        let outcome =
                            apply_identity_after_connect(&accounts, &config, pool, id, &info)?;
                        Ok(connect_result_from_identity(
                            outcome,
                            "Dropbox-Verbindung hergestellt.",
                        ))
                    }
                    Err(e) => {
                        logging::log_warn(&format!(
                            "OAuth ok, account_info fehlgeschlagen: {e}"
                        ));
                        Ok(ConnectResult::ok(
                            "Verbunden",
                            "Dropbox-Verbindung hergestellt.",
                        ))
                    }
                }
            } else {
                Ok(ConnectResult::ok(
                    "Verbunden",
                    "Dropbox-Verbindung hergestellt.",
                ))
            }
        }
        Ok(false) => Ok(ConnectResult::fail(
            "Nicht verbunden",
            "OAuth fehlgeschlagen.",
        )),
        Err(e) => Ok(ConnectResult::fail("OAuth-Fehler", e.to_string())),
    }
}

#[tauri::command]
pub async fn connect_dropbox(
    cloud: State<'_, CloudState>,
    accounts: State<'_, DropboxAccountState>,
    config: State<'_, ConfigState>,
    which: String,
    account_id: Option<String>,
) -> Result<ConnectResult, String> {
    let pool = parse_pool(&which)?;
    let ams_id = account_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| cloud.active_account_id(pool));
    let client = resolve_client(&cloud, pool, ams_id.as_deref());
    let emit = pool == DropboxPool::Native;
    match client.connect_session(emit).await {
        Ok(true) => {
            if let Some(id) = ams_id.as_deref() {
                match client.account_info().await {
                    Ok(info) => {
                        let outcome =
                            apply_identity_after_connect(&accounts, &config, pool, id, &info)?;
                        Ok(connect_result_from_identity(outcome, "Dropbox verbunden."))
                    }
                    Err(e) => {
                        logging::log_warn(&format!(
                            "Connect ok, account_info fehlgeschlagen: {e}"
                        ));
                        Ok(ConnectResult::ok("Verbunden", "Dropbox verbunden."))
                    }
                }
            } else {
                Ok(ConnectResult::ok("Verbunden", "Dropbox verbunden."))
            }
        }
        Ok(false) => Ok(ConnectResult::fail(
            "Nicht verbunden",
            "Verbindung fehlgeschlagen.",
        )),
        Err(e) => {
            let text = e.to_string();
            if text.contains("Refresh-Token") {
                match client.start_oauth() {
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
    config: State<'_, ConfigState>,
    upload: State<'_, UploadState>,
    which: String,
    account_id: Option<String>,
) -> Result<ConnectResult, String> {
    let pool = parse_pool(&which)?;
    let ams_id = account_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| cloud.active_account_id(pool));
    if let Some(id) = ams_id.as_deref() {
        guards::assert_can_delete_or_disconnect(&upload.registry, id)?;
    }
    let client = resolve_client(&cloud, pool, ams_id.as_deref());
    let emit = pool == DropboxPool::Native;
    client
        .disconnect_session(emit, emit)
        .await
        .map_err(|e| e.to_string())?;
    let _ = config.with_store_mut(|store| {
        dropbox_accounts::sync_active_secrets_with_legacy(store, pool).map_err(|e| e.to_string())
    });
    Ok(ConnectResult::ok(
        "Nicht verbunden",
        if pool == DropboxPool::CustomApi {
            "Custom-Dropbox getrennt."
        } else {
            "Dropbox getrennt."
        },
    ))
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
        match cloud.dropbox().connect_session(true).await {
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

fn has_dropbox_refresh_token(keys: &DropboxSecretKeys) -> bool {
    secrets::get_secret(&keys.refresh_token)
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .is_some()
}

fn has_dropbox_app_credentials(keys: &DropboxSecretKeys) -> bool {
    let app_key = secrets::get_secret(&keys.app_key)
        .ok()
        .flatten()
        .filter(|s| !s.is_empty());
    let app_secret = secrets::get_secret(&keys.app_secret)
        .ok()
        .flatten()
        .filter(|s| !s.is_empty());
    app_key.is_some() && app_secret.is_some()
}

/// Legacy `StartupConnectWorker._connect_dropbox_for_contact_markers`: when Custom API
/// is active, also connect Dropbox upload accounts used for pure-contact markers.
async fn connect_dropbox_for_pure_contact_markers(cloud: &CloudState) {
    let custom_keys = match cloud.active_account_id(DropboxPool::CustomApi) {
        Some(id) => DropboxSecretKeys::for_account(DropboxPool::CustomApi, &id),
        None => DropboxSecretKeys::custom_api(),
    };
    if has_dropbox_app_credentials(&custom_keys) && has_dropbox_refresh_token(&custom_keys) {
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

    let native_keys = match cloud.active_account_id(DropboxPool::Native) {
        Some(id) => DropboxSecretKeys::for_account(DropboxPool::Native, &id),
        None => DropboxSecretKeys::native(),
    };
    if !has_dropbox_refresh_token(&native_keys) {
        logging::log_info(
            "Kein natives Dropbox Refresh-Token — optionaler Fallback für reine Kontakt-Marker entfällt.",
        );
        return;
    }
    if cloud.dropbox().connection_status() == "Verbunden" {
        return;
    }
    match cloud.dropbox().connect_session(false).await {
        Ok(true) => {
            logging::log_info(
                "Natives Dropbox parallel verbunden (Legacy-Fallback für reine Kontakt-Marker).",
            );
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
    upload: State<'_, UploadState>,
) -> Result<ConnectResult, String> {
    if selected_cloud(&config) == "custom_api" {
        cloud
            .custom_api
            .disconnect()
            .await
            .map_err(|e| e.to_string())?;
        Ok(ConnectResult::ok("Nicht verbunden", "Custom API getrennt."))
    } else {
        if let Some(id) = cloud.active_account_id(DropboxPool::Native) {
            guards::assert_can_delete_or_disconnect(&upload.registry, &id)?;
        }
        cloud
            .dropbox()
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
        let keys = match cloud.active_account_id(DropboxPool::Native) {
            Some(id) => DropboxSecretKeys::for_account(DropboxPool::Native, &id),
            None => DropboxSecretKeys::native(),
        };
        has_dropbox_refresh_token(&keys)
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
pub fn list_dropbox_accounts(
    accounts: State<'_, DropboxAccountState>,
    config: State<'_, ConfigState>,
    pool: String,
) -> Result<Vec<DropboxAccountRow>, String> {
    let pool = parse_pool(&pool)?;
    let mut rows = accounts.with_store(|store| store.list(pool))?;
    let active = config
        .get(pool.active_setting_key(), Some(""))
        .unwrap_or_default();
    // Active first for stable UI ordering (16d will use badge; keep deterministic here).
    rows.sort_by(|a, b| {
        let a_active = a.id == active;
        let b_active = b.id == active;
        b_active.cmp(&a_active).then(a.created_at.cmp(&b.created_at))
    });
    Ok(rows)
}

#[tauri::command]
pub fn create_dropbox_account(
    cloud: State<'_, CloudState>,
    accounts: State<'_, DropboxAccountState>,
    config: State<'_, ConfigState>,
    pool: String,
    label: Option<String>,
) -> Result<DropboxAccountRow, String> {
    let pool = parse_pool(&pool)?;
    let row = accounts.with_store(|store| store.create(pool, label.as_deref().unwrap_or("")))?;
    // Seed app key/secret from active/legacy so OAuth can start without re-entry.
    let seed_from = cloud
        .active_account_id(pool)
        .map(|id| DropboxSecretKeys::for_account(pool, &id))
        .unwrap_or_else(|| pool.legacy_keys());
    let target = DropboxSecretKeys::for_account(pool, &row.id);
    for (from, to) in [
        (seed_from.app_key.as_str(), target.app_key.as_str()),
        (seed_from.app_secret.as_str(), target.app_secret.as_str()),
    ] {
        if let Some(v) = secrets::get_secret(from)
            .ok()
            .flatten()
            .filter(|s| !s.is_empty())
        {
            let _ = secrets::save_secret(to, &v);
        }
    }
    cloud.client_for(pool, &row.id);
    let active = config
        .get(pool.active_setting_key(), Some(""))
        .unwrap_or_default();
    if active.trim().is_empty() {
        config.with_store_mut(|store| {
            store
                .save(pool.active_setting_key(), &row.id)
                .map_err(|e| e.to_string())
        })?;
        cloud.set_active_account(pool, Some(&row.id));
    }
    Ok(row)
}

#[tauri::command]
pub fn set_active_dropbox_account(
    cloud: State<'_, CloudState>,
    accounts: State<'_, DropboxAccountState>,
    config: State<'_, ConfigState>,
    pool: String,
    account_id: String,
) -> Result<DropboxAccountRow, String> {
    // Soft-Active (16c): only changes the default for *new* jobs. Queued / active jobs
    // keep their frozen dropbox_binding and continue with that client.
    let _ = guards::soft_active_switch_is_safe();
    let pool = parse_pool(&pool)?;
    let id = account_id.trim();
    if id.is_empty() {
        return Err("account_id fehlt.".into());
    }
    let row = accounts
        .with_store(|store| store.get(id))?
        .ok_or_else(|| format!("Dropbox-Profil nicht gefunden: {id}"))?;
    if row.pool != pool.as_str() {
        return Err(format!(
            "Profil {id} gehört zu Pool '{}', nicht '{}'.",
            row.pool,
            pool.as_str()
        ));
    }
    config.with_store_mut(|store| {
        store
            .save(pool.active_setting_key(), id)
            .map_err(|e| e.to_string())?;
        dropbox_accounts::sync_active_secrets_with_legacy(store, pool).map_err(|e| e.to_string())
    })?;
    cloud.set_active_account(pool, Some(id));
    Ok(row)
}

#[tauri::command]
pub fn rename_dropbox_account(
    accounts: State<'_, DropboxAccountState>,
    account_id: String,
    label: String,
) -> Result<DropboxAccountRow, String> {
    accounts.with_store(|store| store.rename(&account_id, &label))
}

#[tauri::command]
pub fn delete_dropbox_account(
    cloud: State<'_, CloudState>,
    accounts: State<'_, DropboxAccountState>,
    config: State<'_, ConfigState>,
    upload: State<'_, UploadState>,
    account_id: String,
) -> Result<(), String> {
    let id = account_id.trim();
    let row = accounts
        .with_store(|store| store.get(id))?
        .ok_or_else(|| format!("Dropbox-Profil nicht gefunden: {id}"))?;
    let pool = parse_pool(&row.pool)?;
    guards::assert_can_delete_or_disconnect(&upload.registry, id)?;
    let was_active = cloud.active_account_id(pool).as_deref() == Some(id);
    accounts.with_store(|store| store.delete(id))?;
    cloud.forget_account(pool, id);
    if was_active {
        let remaining = accounts.with_store(|store| store.list(pool))?;
        let next = remaining.first().map(|r| r.id.clone());
        config.with_store_mut(|store| {
            store
                .save(pool.active_setting_key(), next.as_deref().unwrap_or(""))
                .map_err(|e| e.to_string())?;
            dropbox_accounts::sync_active_secrets_with_legacy(store, pool).map_err(|e| e.to_string())
        })?;
        cloud.set_active_account(pool, next.as_deref());
    }
    Ok(())
}

#[tauri::command]
pub async fn get_sms_balance(
    api_key: Option<String>,
    sandbox: Option<bool>,
) -> Result<String, String> {
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
    fn parse_pool_accepts_aliases() {
        assert_eq!(parse_pool("native").unwrap(), DropboxPool::Native);
        assert_eq!(parse_pool("dropbox").unwrap(), DropboxPool::Native);
        assert_eq!(parse_pool("custom").unwrap(), DropboxPool::CustomApi);
        assert_eq!(parse_pool("custom_dropbox").unwrap(), DropboxPool::CustomApi);
        assert!(parse_pool("other").is_err());
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
