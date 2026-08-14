//! OS keyring for secrets (port of legacy `core/config.py` keyring half).
//!
//! Never persist these values in SQLite or config files.

use thiserror::Error;

use crate::constants::KEYRING_SERVICE_NAME;

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

#[cfg(not(test))]
use keyring::Entry;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("empty secret key")]
    EmptyKey,
    #[error("keyring error: {0}")]
    #[allow(dead_code)]
    Keyring(String),
}

/// Keys that must never be written to the settings database.
pub fn is_secret_key(key: &str) -> bool {
    matches!(
        key,
        "db_app_key"
            | "db_app_secret"
            | "db_refresh_token"
            | "custom_db_app_key"
            | "custom_db_app_secret"
            | "custom_db_refresh_token"
            | "custom_api_url"
            | "custom_api_bearer_token"
            | "aero_customer_base_url"
            | "aero_customer_api_token"
            | "smtp_user"
            | "smtp_pass"
            | "imap_user"
            | "imap_pass"
            | "seven_api_key"
            | "seven_sandbox_api_key"
            | "sms_api_key"
            | "sms_sandbox_api_key"
            | "twilio_account_sid"
            | "twilio_auth_token"
            | "shortener_base_url"
            | "shortener_api_key"
            | "skylink_api_url"
            | "skylink_api_key"
    )
}

#[cfg(test)]
static TEST_SECRETS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

#[cfg(test)]
fn with_test_map<T>(f: impl FnOnce(&mut HashMap<String, String>) -> T) -> T {
    let mut guard = TEST_SECRETS.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    f(guard.as_mut().expect("test secret map"))
}

pub fn save_secret(key: &str, value: &str) -> Result<(), SecretError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(SecretError::EmptyKey);
    }
    if value.is_empty() {
        return delete_secret(key);
    }
    backend_set(KEYRING_SERVICE_NAME, key, value)
}

pub fn get_secret(key: &str) -> Result<Option<String>, SecretError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(SecretError::EmptyKey);
    }
    backend_get(KEYRING_SERVICE_NAME, key)
}

pub fn delete_secret(key: &str) -> Result<(), SecretError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(SecretError::EmptyKey);
    }
    backend_delete(KEYRING_SERVICE_NAME, key)
}

/// Read a secret from an arbitrary keyring service (legacy migration).
pub fn get_secret_from_service(service: &str, key: &str) -> Result<Option<String>, SecretError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(SecretError::EmptyKey);
    }
    let service = service.trim();
    if service.is_empty() {
        return Err(SecretError::EmptyKey);
    }
    backend_get(service, key)
}

#[cfg(test)]
fn backend_set(service: &str, key: &str, value: &str) -> Result<(), SecretError> {
    let compound = format!("{service}::{key}");
    with_test_map(|map| {
        map.insert(compound, value.to_string());
    });
    Ok(())
}

#[cfg(test)]
fn backend_get(service: &str, key: &str) -> Result<Option<String>, SecretError> {
    let compound = format!("{service}::{key}");
    Ok(with_test_map(|map| map.get(&compound).cloned()))
}

#[cfg(test)]
fn backend_delete(service: &str, key: &str) -> Result<(), SecretError> {
    let compound = format!("{service}::{key}");
    with_test_map(|map| {
        map.remove(&compound);
    });
    Ok(())
}

#[cfg(not(test))]
fn backend_set(service: &str, key: &str, value: &str) -> Result<(), SecretError> {
    let entry = Entry::new(service, key).map_err(|e| SecretError::Keyring(e.to_string()))?;
    entry
        .set_password(value)
        .map_err(|e| SecretError::Keyring(e.to_string()))
}

#[cfg(not(test))]
fn backend_get(service: &str, key: &str) -> Result<Option<String>, SecretError> {
    let entry = Entry::new(service, key).map_err(|e| SecretError::Keyring(e.to_string()))?;
    match entry.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(SecretError::Keyring(e.to_string())),
    }
}

#[cfg(not(test))]
fn backend_delete(service: &str, key: &str) -> Result<(), SecretError> {
    let entry = Entry::new(service, key).map_err(|e| SecretError::Keyring(e.to_string()))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(SecretError::Keyring(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_secret_keys_are_classified() {
        assert!(is_secret_key("db_app_secret"));
        assert!(is_secret_key("db_refresh_token"));
        assert!(is_secret_key("smtp_pass"));
        assert!(is_secret_key("seven_api_key"));
        assert!(is_secret_key("twilio_auth_token"));
        assert!(!is_secret_key("monitor_path"));
        assert!(!is_secret_key("scan_interval"));
        assert!(!is_secret_key("log_file_path"));
    }

    #[test]
    fn memory_backend_roundtrip_and_delete_on_empty() {
        save_secret("db_app_key", "test-key-value").unwrap();
        assert_eq!(get_secret("db_app_key").unwrap().as_deref(), Some("test-key-value"));
        save_secret("db_app_key", "").unwrap();
        assert_eq!(get_secret("db_app_key").unwrap(), None);
    }

    #[test]
    fn empty_key_is_rejected() {
        assert!(matches!(save_secret("  ", "x"), Err(SecretError::EmptyKey)));
        assert!(matches!(get_secret(""), Err(SecretError::EmptyKey)));
    }

    #[test]
    fn keyring_service_name_is_v2() {
        assert_eq!(KEYRING_SERVICE_NAME, "AeroMediaService-v2");
    }
}
