//! Bridge display name + stable instance id (Phase 13 / P5+).
//! Symmetric to ATS `sd_pc_name` + `ams_bridge_instance_id`.

use uuid::Uuid;

use crate::commands::ConfigState;

pub const SETTING_DISPLAY_NAME: &str = "bridge_display_name";
pub const SETTING_INSTANCE_ID: &str = "bridge_instance_id";
pub const MAX_DISPLAY_NAME_LEN: usize = 64;
pub const DEFAULT_DISPLAY_NAME: &str = "Aero Media Service";

/// Trim and cap user input; empty is allowed (triggers fallback).
pub fn normalize_display_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed.chars().take(MAX_DISPLAY_NAME_LEN).collect()
}

fn hostname_raw() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_default()
}

/// Resolved name for health / mDNS TXT (never empty).
pub fn resolve_display_name(config: &ConfigState) -> String {
    let configured = config
        .get(SETTING_DISPLAY_NAME, Some(""))
        .unwrap_or_default();
    let normalized = normalize_display_name(&configured);
    if !normalized.is_empty() {
        return normalized;
    }
    let host = hostname_raw().trim().to_string();
    if host.is_empty() {
        DEFAULT_DISPLAY_NAME.into()
    } else {
        host
    }
}

/// DNS-SD instance label: `AMS-{sanitized}`.
pub fn instance_dns_label(display_name: &str) -> String {
    let safe = super::mdns::sanitize_dns_label(display_name);
    if safe.is_empty() {
        "AeroMediaService".into()
    } else {
        format!("AMS-{safe}")
    }
}

/// Ensure a stable UUID exists in settings; generate and persist on first use.
pub fn ensure_instance_id(config: &ConfigState) -> Result<String, String> {
    let existing = config
        .get(SETTING_INSTANCE_ID, Some(""))
        .unwrap_or_default();
    let trimmed = existing.trim();
    if !trimmed.is_empty() {
        return Ok(trimmed.to_string());
    }
    let id = Uuid::new_v4().to_string();
    config.with_store_mut(|store| {
        store
            .save(SETTING_INSTANCE_ID, &id)
            .map_err(|e| e.to_string())
    })?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::ConfigStore;

    fn test_config() -> ConfigState {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::open_at(dir.path().join("settings.db")).unwrap();
        std::mem::forget(dir);
        ConfigState::from_store(store)
    }

    #[test]
    fn normalize_display_name_trims_and_caps() {
        assert_eq!(normalize_display_name("  Dropzone 1  "), "Dropzone 1");
        assert_eq!(normalize_display_name(""), "");
        let long = "x".repeat(80);
        assert_eq!(normalize_display_name(&long).len(), MAX_DISPLAY_NAME_LEN);
    }

    #[test]
    fn instance_dns_label_prefixes_ams() {
        assert_eq!(instance_dns_label("Landebahn Nord"), "AMS-landebahn-nord");
    }

    #[test]
    fn resolve_display_name_uses_setting() {
        let config = test_config();
        config
            .with_store_mut(|store| {
                store.save(SETTING_DISPLAY_NAME, "Studio Upload").map_err(|e| e.to_string())
            })
            .unwrap();
        assert_eq!(resolve_display_name(&config), "Studio Upload");
    }

    #[test]
    fn ensure_instance_id_is_stable() {
        let config = test_config();
        let id1 = ensure_instance_id(&config).unwrap();
        let id2 = ensure_instance_id(&config).unwrap();
        assert_eq!(id1, id2);
        assert!(!id1.is_empty());
    }
}
