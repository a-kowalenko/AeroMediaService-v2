//! App-wide constants (storage paths, keyring, setting defaults).

/// Directory name under the platform app-data folder.
/// Windows: `%LOCALAPPDATA%\AeroMediaService\`
/// macOS: `~/Library/Application Support/AeroMediaService/`
/// Linux: `~/.local/share/AeroMediaService/`
pub const APP_DIR_NAME: &str = "AeroMediaService";

/// SQLite file for non-secret settings.
pub const CONFIG_DB_FILE: &str = "config.db";

/// SQLite file for upload history (not JSON as primary store).
pub const HISTORY_DB_FILE: &str = "history.db";

/// SQLite file for ATS bridge presence / activity observability.
pub const ATS_PRESENCE_DB_FILE: &str = "ats_presence.db";

/// SQLite file for customer intake queue (Fertig-App replacement).
pub const CUSTOMERS_DB_FILE: &str = "customers.db";

/// Legacy JSON history filename (optional one-shot import).
pub const LEGACY_HISTORY_JSON: &str = "upload_history.json";

/// OS-Keyring service name (v2). Legacy secrets lived under `LEGACY_KEYRING_SERVICE_NAME`.
pub const KEYRING_SERVICE_NAME: &str = "AeroMediaService-v2";

/// Legacy PySide keyring service (`core/config.py`).
pub const LEGACY_KEYRING_SERVICE_NAME: &str = "DropboxUploaderApp";

/// Legacy QSettings organization / application (`main.py`).
pub const LEGACY_QSETTINGS_ORG: &str = "AKSoftware";
pub const LEGACY_QSETTINGS_APP: &str = "AeroMediaService";

/// Debug log filename inside `log_file_path` (or the app-data dir).
pub const DEBUG_LOG_FILE: &str = "debug.log";

/// Default scan interval in seconds (legacy QSettings default).
pub const DEFAULT_SCAN_INTERVAL: &str = "10";

/// Default folder-stability wait in seconds.
pub const DEFAULT_FOLDER_STABILITY_SECONDS: &str = "15";

/// Known non-secret setting keys and their defaults.
pub fn setting_default(key: &str) -> Option<&'static str> {
    match key {
        "monitor_path" | "archive_path" | "log_file_path" => Some(""),
        "scan_interval" => Some(DEFAULT_SCAN_INTERVAL),
        "folder_stability_enabled" => Some("true"),
        "folder_stability_seconds" => Some(DEFAULT_FOLDER_STABILITY_SECONDS),
        "manifest_required" => Some("false"),
        "bridge_enabled" => Some("false"),
        "bridge_bind" => Some("0.0.0.0:8787"),
        "bridge_display_name" => Some(""),
        "bridge_instance_id" => Some(""),
        "ats_primary_smb_url" => Some(""),
        "ats_backup_smb_url" => Some(""),
        "selected_cloud_service" => Some("dropbox"),
        "active_dropbox_account_id" => Some(""),
        "active_custom_dropbox_account_id" => Some(""),
        "dropbox_multi_account_migrated" => Some("false"),
        "custom_api_upload_mode" => Some("proxied_session"),
        "custom_api_upload_endpoint" => Some("/upload"),
        "custom_api_share_endpoint" => Some("/share"),
        "custom_api_health_endpoint" => Some("/health"),
        "link_shortener_enabled" => Some("false"),
        "shortener_expires_preset" => Some("permanent"),
        "smtp_host"
        | "smtp_sender_addr"
        | "smtp_fallback_recipient"
        | "imap_host"
        | "imap_sent_folder"
        | "seven_sender"
        | "twilio_whatsapp_from" => Some(""),
        "smtp_port" => Some("587"),
        "smtp_sender_name" => Some("Dropbox Uploader"),
        "smtp_sandbox_mode" => Some("false"),
        "imap_save_sent_enabled" => Some("true"),
        "imap_port" => Some("993"),
        "imap_same_credentials" => Some("true"),
        "seven_sandbox_mode" => Some("false"),
        "updater_ignore_version" => Some(""),
        "beta_updates_enabled" => Some("false"),
        "setup_completed" => Some("false"),
        "legacy_migration_done" => Some("false"),
        "ui_theme" => Some("dark"),
        "brochure_enabled" => Some("false"),
        "brochure_export_name" => Some("Infobroschuere.pdf"),
        "brochure_subdir" => Some(""),
        // Empty → Rust/TS load ATS default roster (Phase 19a).
        "crew_list" => Some(""),
        _ => None,
    }
}

/// Canonical Custom-API upload modes (legacy-compatible).
pub const CUSTOM_API_UPLOAD_MODE_PROXIED: &str = "proxied_session";
pub const CUSTOM_API_UPLOAD_MODE_DIRECT_DROPBOX: &str = "direct_dropbox_complete";

/// Normalize stored/UI upload-mode values (alias `direct` → Manifest v1.1 path).
pub fn normalize_custom_api_upload_mode(raw: &str) -> &'static str {
    match raw.trim() {
        "direct_dropbox_complete" | "direct" => CUSTOM_API_UPLOAD_MODE_DIRECT_DROPBOX,
        _ => CUSTOM_API_UPLOAD_MODE_PROXIED,
    }
}

/// Whether the mode uploads via Dropbox + Manifest v1.1.
pub fn is_direct_dropbox_upload_mode(raw: &str) -> bool {
    normalize_custom_api_upload_mode(raw) == CUSTOM_API_UPLOAD_MODE_DIRECT_DROPBOX
}

/// Non-secret keys imported from legacy QSettings (Phase 11).
pub const LEGACY_SETTING_KEYS: &[&str] = &[
    "monitor_path",
    "archive_path",
    "log_file_path",
    "scan_interval",
    "folder_stability_enabled",
    "folder_stability_seconds",
    "selected_cloud_service",
    "custom_api_upload_mode",
    "custom_api_upload_endpoint",
    "custom_api_share_endpoint",
    "custom_api_health_endpoint",
    "link_shortener_enabled",
    "shortener_expires_preset",
    "smtp_host",
    "smtp_port",
    "smtp_sender_addr",
    "smtp_sender_name",
    "smtp_fallback_recipient",
    "smtp_sandbox_mode",
    "imap_host",
    "imap_port",
    "imap_sent_folder",
    "imap_save_sent_enabled",
    "imap_same_credentials",
    "seven_sender",
    "seven_sandbox_mode",
    "twilio_whatsapp_from",
];

/// Secret keys copied from legacy keyring → v2 keyring (never into SQLite).
pub const LEGACY_SECRET_KEYS: &[&str] = &[
    "db_app_key",
    "db_app_secret",
    "db_refresh_token",
    "custom_db_app_key",
    "custom_db_app_secret",
    "custom_db_refresh_token",
    "custom_api_url",
    "custom_api_bearer_token",
    "aero_customer_base_url",
    "aero_customer_api_token",
    "smtp_user",
    "smtp_pass",
    "imap_user",
    "imap_pass",
    "seven_api_key",
    "seven_sandbox_api_key",
    "sms_api_key",
    "sms_sandbox_api_key",
    "twilio_account_sid",
    "twilio_auth_token",
    "shortener_base_url",
    "shortener_api_key",
    "skylink_api_url",
    "skylink_api_key",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_dir_and_keyring_names_are_stable() {
        assert_eq!(APP_DIR_NAME, "AeroMediaService");
        assert_eq!(KEYRING_SERVICE_NAME, "AeroMediaService-v2");
        assert_eq!(CONFIG_DB_FILE, "config.db");
        assert_eq!(HISTORY_DB_FILE, "history.db");
        assert_eq!(ATS_PRESENCE_DB_FILE, "ats_presence.db");
        assert_eq!(CUSTOMERS_DB_FILE, "customers.db");
        assert_eq!(LEGACY_HISTORY_JSON, "upload_history.json");
        assert_ne!(KEYRING_SERVICE_NAME, "DropboxUploaderApp");
    }

    #[test]
    fn phase1_setting_defaults() {
        assert_eq!(setting_default("monitor_path"), Some(""));
        assert_eq!(setting_default("archive_path"), Some(""));
        assert_eq!(setting_default("log_file_path"), Some(""));
        assert_eq!(setting_default("scan_interval"), Some("10"));
        assert_eq!(setting_default("folder_stability_enabled"), Some("true"));
        assert_eq!(setting_default("folder_stability_seconds"), Some("15"));
        assert_eq!(setting_default("manifest_required"), Some("false"));
        assert_eq!(setting_default("bridge_enabled"), Some("false"));
        assert_eq!(setting_default("bridge_bind"), Some("0.0.0.0:8787"));
        assert_eq!(setting_default("ats_primary_smb_url"), Some(""));
        assert_eq!(setting_default("ats_backup_smb_url"), Some(""));
        assert_eq!(setting_default("selected_cloud_service"), Some("dropbox"));
        assert_eq!(setting_default("active_dropbox_account_id"), Some(""));
        assert_eq!(setting_default("active_custom_dropbox_account_id"), Some(""));
        assert_eq!(setting_default("dropbox_multi_account_migrated"), Some("false"));
        assert_eq!(
            setting_default("custom_api_upload_mode"),
            Some("proxied_session")
        );
        assert_eq!(
            normalize_custom_api_upload_mode("direct_dropbox_complete"),
            CUSTOM_API_UPLOAD_MODE_DIRECT_DROPBOX
        );
        assert_eq!(
            normalize_custom_api_upload_mode("direct"),
            CUSTOM_API_UPLOAD_MODE_DIRECT_DROPBOX
        );
        assert_eq!(
            normalize_custom_api_upload_mode("proxied_session"),
            CUSTOM_API_UPLOAD_MODE_PROXIED
        );
        assert_eq!(
            normalize_custom_api_upload_mode("unknown"),
            CUSTOM_API_UPLOAD_MODE_PROXIED
        );
        assert!(is_direct_dropbox_upload_mode("direct"));
        assert!(!is_direct_dropbox_upload_mode("proxied_session"));
        assert_eq!(setting_default("link_shortener_enabled"), Some("false"));
        assert_eq!(
            setting_default("shortener_expires_preset"),
            Some("permanent")
        );
        assert_eq!(setting_default("smtp_port"), Some("587"));
        assert_eq!(
            setting_default("smtp_sender_name"),
            Some("Dropbox Uploader")
        );
        assert_eq!(setting_default("smtp_sandbox_mode"), Some("false"));
        assert_eq!(setting_default("imap_save_sent_enabled"), Some("true"));
        assert_eq!(setting_default("imap_port"), Some("993"));
        assert_eq!(setting_default("imap_same_credentials"), Some("true"));
        assert_eq!(setting_default("seven_sandbox_mode"), Some("false"));
        assert_eq!(setting_default("updater_ignore_version"), Some(""));
        assert_eq!(setting_default("beta_updates_enabled"), Some("false"));
        assert_eq!(setting_default("setup_completed"), Some("false"));
        assert_eq!(setting_default("legacy_migration_done"), Some("false"));
        assert_eq!(setting_default("ui_theme"), Some("dark"));
        assert_eq!(setting_default("brochure_enabled"), Some("false"));
        assert_eq!(
            setting_default("brochure_export_name"),
            Some("Infobroschuere.pdf")
        );
        assert_eq!(setting_default("brochure_subdir"), Some(""));
        assert_eq!(setting_default("crew_list"), Some(""));
        assert_eq!(setting_default("unknown_key"), None);
    }

    #[test]
    fn platform_app_data_layout_docs_match_constants() {
        // Documented in ARCHITECTURE.md / IMPLEMENTATION_PLAN — keep stable across Win/Mac/Linux.
        assert_eq!(APP_DIR_NAME, "AeroMediaService");
        assert_eq!(KEYRING_SERVICE_NAME, "AeroMediaService-v2");
        assert_eq!(LEGACY_KEYRING_SERVICE_NAME, "DropboxUploaderApp");
        assert_eq!(LEGACY_QSETTINGS_ORG, "AKSoftware");
        assert_eq!(LEGACY_QSETTINGS_APP, "AeroMediaService");
    }

    #[test]
    fn legacy_migration_key_lists_are_non_empty() {
        assert!(!LEGACY_SETTING_KEYS.is_empty());
        assert!(!LEGACY_SECRET_KEYS.is_empty());
        for key in LEGACY_SECRET_KEYS {
            assert!(
                crate::storage::secrets::is_secret_key(key),
                "legacy secret key {key} must be classified as secret"
            );
        }
        for key in LEGACY_SETTING_KEYS {
            assert!(
                !crate::storage::secrets::is_secret_key(key),
                "legacy setting key {key} must not be a secret"
            );
        }
    }
}
