//! Resend email/SMS from upload history.
//! Port of legacy `core/resend_notifications.py`.

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::cloud::custom_api::lookup_customer_url;
use crate::cloud::{CloudClient, DropboxClient};
use crate::model::kunde::{normalize_phone, Kunde};
use crate::model::validation::{is_valid_email, is_valid_share_link};
use crate::notify::{email, setting_flag, setting_or_default, sms};
use crate::storage::logging;
use crate::util::link_shortener;

pub const RESENDABLE_UPLOAD_STATUS: &str = "Erfolgreich";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelResult {
    pub channel: String,
    pub status: String,
    pub success: bool,
    pub sms_id: Option<String>,
}

impl ChannelResult {
    pub fn new(
        channel: impl Into<String>,
        status: impl Into<String>,
        success: bool,
        sms_id: Option<String>,
    ) -> Self {
        Self {
            channel: channel.into(),
            status: status.into(),
            success,
            sms_id,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ResendResult {
    pub email_result: Option<ChannelResult>,
    pub sms_result: Option<ChannelResult>,
    pub share_link: String,
    pub history_updates: Value,
}

fn json_str<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry.get(key).and_then(Value::as_str).unwrap_or("")
}

fn json_int(entry: &Value, key: &str) -> i64 {
    match entry.get(key) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        Some(Value::Bool(true)) => 1,
        _ => 0,
    }
}

pub fn is_sandbox_email() -> bool {
    setting_flag("smtp_sandbox_mode")
}

pub fn is_sandbox_sms() -> bool {
    setting_flag("seven_sandbox_mode")
}

pub fn sandbox_warnings(email_sandbox: bool, fallback: &str, sms_sandbox: bool) -> Vec<String> {
    let mut warnings = Vec::new();
    if email_sandbox {
        let fallback = fallback.trim();
        if fallback.is_empty() {
            warnings.push("E-Mail-Sandbox aktiv — kein Fallback-Empfänger konfiguriert.".into());
        } else {
            warnings.push(format!(
                "E-Mail-Sandbox aktiv — Versand geht an {fallback}."
            ));
        }
    }
    if sms_sandbox {
        warnings.push("SMS-Sandbox aktiv — keine echte Zustellung.".into());
    }
    warnings
}

pub fn get_sandbox_warnings() -> Vec<String> {
    sandbox_warnings(
        is_sandbox_email(),
        &setting_or_default("smtp_fallback_recipient", ""),
        is_sandbox_sms(),
    )
}

pub fn normalize_contact(email: &str, phone: &str) -> (String, Option<String>) {
    (email.trim().to_string(), normalize_phone(Some(phone)))
}

pub fn validate_contact_for_channels(
    email: &str,
    phone: Option<&str>,
    send_email: bool,
    send_sms: bool,
) -> Result<(), String> {
    if !send_email && !send_sms {
        return Err("Bitte mindestens einen Kanal auswählen.".into());
    }
    if send_email {
        if email.trim().is_empty() {
            return Err("E-Mail-Adresse fehlt.".into());
        }
        if !is_valid_email(email) {
            return Err("E-Mail-Adresse ist ungültig.".into());
        }
    }
    if send_sms && phone.map(str::trim).unwrap_or("").is_empty() {
        return Err("Telefonnummer fehlt.".into());
    }
    Ok(())
}

fn is_delivered_email_status(status: &str) -> bool {
    let s = status.trim().to_lowercase();
    s.contains("gesendet") || s.contains("zugestellt") || s.contains("erfolgreich")
}

fn is_delivered_sms_status(status: &str) -> bool {
    let s = status.trim().to_lowercase();
    s.contains("zugestellt") || s.contains("erfolgreich")
}

pub fn channels_already_delivered(entry: &Value, send_email: bool, send_sms: bool) -> Vec<String> {
    let mut delivered = Vec::new();
    if send_email && is_delivered_email_status(json_str(entry, "email_status")) {
        delivered.push("email".into());
    }
    if send_sms && is_delivered_sms_status(json_str(entry, "sms_status")) {
        delivered.push("sms".into());
    }
    delivered
}

pub fn can_resend_notifications(entry: &Value) -> bool {
    json_str(entry, "status").trim() == RESENDABLE_UPLOAD_STATUS
}

pub fn remote_path_for_entry(entry: &Value) -> String {
    let remote = json_str(entry, "remote_path").trim();
    if !remote.is_empty() {
        return remote.to_string();
    }
    let dir_name = json_str(entry, "dir_name").trim();
    if dir_name.is_empty() {
        String::new()
    } else {
        format!("/{dir_name}")
    }
}

async fn lookup_link_from_cloud(entry: &Value, selected_cloud: &str) -> Option<String> {
    if selected_cloud.trim() == "custom_api" {
        let link = lookup_customer_url(
            json_str(entry, "customer_number"),
            json_str(entry, "booking_number"),
            json_str(entry, "type"),
        )
        .await?;
        let shortened = link_shortener::shorten(&link).await;
        return Some(if shortened.is_empty() {
            link
        } else {
            shortened
        });
    }

    let remote_path = remote_path_for_entry(entry);
    if remote_path.is_empty() {
        return None;
    }
    let client = match resolve_dropbox_client_for_entry(entry) {
        Ok(c) => c,
        Err(e) => {
            logging::log_warn(&format!("Share-Link Lookup: {e}"));
            return None;
        }
    };
    if !client.connect().await.ok()? {
        return None;
    }
    if client.connection_status() != "Verbunden" {
        return None;
    }
    client.get_shareable_link(&remote_path).await.ok().flatten()
}

fn resolve_dropbox_client_for_entry(entry: &Value) -> Result<DropboxClient, String> {
    use crate::cloud::binding::resolve_binding_for_history;
    use crate::cloud::dropbox::{DropboxPool, DropboxSecretKeys};
    use crate::storage::dropbox_accounts::DropboxAccountStore;

    let accounts = DropboxAccountStore::open_default().map_err(|e| e.to_string())?;
    let rows = accounts
        .list(DropboxPool::Native)
        .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Ok(DropboxClient::new());
    }
    let binding = resolve_binding_for_history(entry, DropboxPool::Native, &accounts)?;
    Ok(DropboxClient::with_keys(DropboxSecretKeys::for_account(
        binding.pool,
        &binding.ams_id,
    )))
}

pub async fn lookup_share_link_from_cloud(
    entry: &Value,
    selected_cloud: &str,
) -> Result<String, String> {
    lookup_link_from_cloud(entry, selected_cloud)
        .await
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Download-Link konnte nicht aus der Cloud geladen werden.".into())
}

#[allow(dead_code)]
pub async fn resolve_share_link(
    entry: &Value,
    selected_cloud: &str,
    manual_link: Option<&str>,
    allow_cloud: bool,
) -> Result<String, String> {
    let stored = json_str(entry, "share_link").trim();
    if !stored.is_empty() {
        return Ok(stored.to_string());
    }

    let manual = manual_link.unwrap_or("").trim();
    if !manual.is_empty() {
        if !is_valid_share_link(manual) {
            return Err("Download-Link muss mit http:// oder https:// beginnen.".into());
        }
        return Ok(manual.to_string());
    }

    if allow_cloud {
        if let Some(link) = lookup_link_from_cloud(entry, selected_cloud).await {
            let trimmed = link.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
    }

    Err("Kein Download-Link verfügbar. Bitte Link manuell eingeben oder Cloud verbinden.".into())
}

/// Stored/manual resolution without a cloud round-trip (unit-testable).
#[allow(dead_code)]
pub fn resolve_share_link_offline(
    entry: &Value,
    manual_link: Option<&str>,
) -> Result<String, String> {
    let stored = json_str(entry, "share_link").trim();
    if !stored.is_empty() {
        return Ok(stored.to_string());
    }
    let manual = manual_link.unwrap_or("").trim();
    if !manual.is_empty() {
        if !is_valid_share_link(manual) {
            return Err("Download-Link muss mit http:// oder https:// beginnen.".into());
        }
        return Ok(manual.to_string());
    }
    Err("Kein Download-Link verfügbar. Bitte Link manuell eingeben oder Cloud verbinden.".into())
}

pub fn build_contact_update_payload(entry: &Value, email: &str, phone: Option<&str>) -> Value {
    json!({
        "dir_name": json_str(entry, "dir_name"),
        "email": email,
        "phone": phone.unwrap_or(""),
    })
}

pub fn build_resend_history_updates(
    entry: &Value,
    email: &str,
    phone: Option<&str>,
    share_link: &str,
    email_result: Option<&ChannelResult>,
    sms_result: Option<&ChannelResult>,
    channels: &[String],
    sandbox_email: bool,
    sandbox_sms: bool,
) -> Value {
    let now = Local::now().format("%Y-%m-%dT%H:%M:%S%.f").to_string();
    let log_entry = json!({
        "at": now,
        "channels": channels,
        "email": email,
        "phone": phone.unwrap_or(""),
        "share_link": share_link,
        "email_status": email_result.map(|r| r.status.clone()),
        "sms_status": sms_result.map(|r| r.status.clone()),
        "sms_id": sms_result.and_then(|r| r.sms_id.clone()),
        "sandbox_email": sandbox_email,
        "sandbox_sms": sandbox_sms,
        "triggered_by": "manual_resend",
    });

    let mut resend_log = entry
        .get("resend_log")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    resend_log.insert(0, log_entry);

    let mut updates = Map::new();
    updates.insert(
        "dir_name".into(),
        Value::String(json_str(entry, "dir_name").to_string()),
    );
    updates.insert("email".into(), Value::String(email.to_string()));
    updates.insert(
        "phone".into(),
        Value::String(phone.unwrap_or("").to_string()),
    );
    updates.insert("share_link".into(), Value::String(share_link.to_string()));
    updates.insert("resend_log".into(), Value::Array(resend_log));
    updates.insert("last_updated".into(), Value::String(now.clone()));

    if let Some(email_result) = email_result {
        updates.insert(
            "email_status".into(),
            Value::String(email_result.status.clone()),
        );
        if email_result.success {
            updates.insert(
                "email_resend_count".into(),
                json!(json_int(entry, "email_resend_count") + 1),
            );
            updates.insert("last_email_resent_at".into(), Value::String(now.clone()));
        }
    }

    if let Some(sms_result) = sms_result {
        updates.insert(
            "sms_status".into(),
            Value::String(sms_result.status.clone()),
        );
        if let Some(sms_id) = &sms_result.sms_id {
            updates.insert("sms_id".into(), Value::String(sms_id.clone()));
        }
        if sms_result.success {
            updates.insert(
                "sms_resend_count".into(),
                json!(json_int(entry, "sms_resend_count") + 1),
            );
            updates.insert("last_sms_resent_at".into(), Value::String(now));
        }
    }

    Value::Object(updates)
}

pub fn format_resend_result_message(result: &ResendResult) -> String {
    let mut lines = Vec::new();
    let email_to = json_str(&result.history_updates, "email").trim();
    let phone_to = json_str(&result.history_updates, "phone").trim();

    if let Some(email_result) = &result.email_result {
        let prefix = if email_result.success { "✓" } else { "✗" };
        let target = if email_to.is_empty() {
            String::new()
        } else {
            format!(" an {email_to}")
        };
        lines.push(format!("{prefix} E-Mail{target}: {}", email_result.status));
    }
    if let Some(sms_result) = &result.sms_result {
        let prefix = if sms_result.success { "✓" } else { "✗" };
        let target = if phone_to.is_empty() {
            String::new()
        } else {
            format!(" an {phone_to}")
        };
        lines.push(format!("{prefix} SMS{target}: {}", sms_result.status));
    }
    if lines.is_empty() {
        "Kein Versand durchgeführt.".into()
    } else {
        lines.join("\n")
    }
}

pub fn resend_had_failures(result: &ResendResult) -> bool {
    result.email_result.as_ref().is_some_and(|r| !r.success)
        || result.sms_result.as_ref().is_some_and(|r| !r.success)
}

#[allow(dead_code)]
pub fn format_resend_history_summary(entry: &Value) -> String {
    let email_count = json_int(entry, "email_resend_count");
    let sms_count = json_int(entry, "sms_resend_count");
    let mut parts = Vec::new();
    if email_count > 0 {
        parts.push(format!("E-Mail {email_count}× erneut"));
    }
    if sms_count > 0 {
        parts.push(format!("SMS {sms_count}× erneut"));
    }
    if parts.is_empty() {
        "Keine Wiederversände".into()
    } else {
        parts.join(" | ")
    }
}

pub async fn resend_notifications(
    entry: &Value,
    email_addr: &str,
    phone: Option<&str>,
    share_link: &str,
    send_email: bool,
    send_sms: bool,
) -> Result<ResendResult, String> {
    if !can_resend_notifications(entry) {
        return Err("Nur erfolgreiche Uploads unterstützen einen erneuten Versand.".into());
    }

    let (email_addr, phone) = normalize_contact(email_addr, phone.unwrap_or(""));
    validate_contact_for_channels(&email_addr, phone.as_deref(), send_email, send_sms)?;

    if !is_valid_share_link(share_link) {
        return Err("Ungültiger Download-Link.".into());
    }

    let dir_name = json_str(entry, "dir_name").trim();
    let first_name = {
        let s = json_str(entry, "first_name").trim();
        if s.is_empty() {
            "Gast"
        } else {
            s
        }
    };

    let kunde = Kunde {
        first_name: Some(first_name.to_string()),
        last_name: {
            let s = json_str(entry, "last_name").trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        },
        email: Some(email_addr.clone()),
        phone: phone.clone(),
        customer_number: nonempty(json_str(entry, "customer_number")),
        booking_number: nonempty(json_str(entry, "booking_number")),
        customer_type: nonempty(json_str(entry, "type")),
        ..Kunde::default()
    };

    let mut email_result = None;
    let mut sms_result = None;
    let mut channels = Vec::new();

    if send_email {
        channels.push("email".into());
        let success = email::send_upload_success_email(
            dir_name,
            share_link,
            Some(&email_addr),
            Some(first_name),
        )
        .await;
        email_result = Some(if success {
            ChannelResult::new("email", "Gesendet", true, None)
        } else {
            ChannelResult::new("email", "Fehler: Versand fehlgeschlagen", false, None)
        });
    }

    if send_sms {
        channels.push("sms".into());
        let result = sms::send_upload_success_sms(share_link, &kunde).await;
        sms_result = Some(if result.success {
            ChannelResult::new("sms", "Gesendet", true, result.sms_id)
        } else {
            let err_text = if result.last_error.trim().is_empty() {
                "Fehler beim Senden".to_string()
            } else {
                result.last_error
            };
            ChannelResult::new("sms", format!("Fehler: {err_text}"), false, result.sms_id)
        });
    }

    let history_updates = build_resend_history_updates(
        entry,
        &email_addr,
        phone.as_deref(),
        share_link,
        email_result.as_ref(),
        sms_result.as_ref(),
        &channels,
        is_sandbox_email(),
        is_sandbox_sms(),
    );

    Ok(ResendResult {
        email_result,
        sms_result,
        share_link: share_link.to_string(),
        history_updates,
    })
}

fn nonempty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Backfill missing share links for successful history entries.
#[allow(dead_code)]
pub async fn migrate_share_links_for_history(
    history: &mut [Value],
    selected_cloud: &str,
    cloud_connected: bool,
) -> usize {
    if !cloud_connected {
        return 0;
    }
    let mut updated = 0usize;
    let now = Local::now().format("%Y-%m-%dT%H:%M:%S%.f").to_string();
    for entry in history.iter_mut() {
        if json_str(entry, "status").trim() != RESENDABLE_UPLOAD_STATUS {
            continue;
        }
        if !json_str(entry, "share_link").trim().is_empty() {
            continue;
        }
        let link = match lookup_link_from_cloud(entry, selected_cloud).await {
            Some(link) => link,
            None => continue,
        };
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("share_link".into(), Value::String(link.trim().to_string()));
            obj.insert("last_updated".into(), Value::String(now.clone()));
        }
        let remote = remote_path_for_entry(entry);
        if !remote.is_empty() {
            if let Some(obj) = entry.as_object_mut() {
                obj.insert("remote_path".into(), Value::String(remote));
            }
        }
        updated += 1;
    }
    updated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::validation::{is_valid_email, is_valid_share_link};

    #[test]
    fn validation_matches_legacy() {
        assert!(is_valid_email("user@example.com"));
        assert!(is_valid_email("a@b.co"));
        assert!(!is_valid_email(""));
        assert!(!is_valid_email("invalid"));
        assert!(!is_valid_email("a@b"));
        assert!(!is_valid_email("a @b.de"));
        assert!(is_valid_share_link("https://example.com/x"));
        assert!(is_valid_share_link("http://example.com"));
        assert!(!is_valid_share_link("ftp://example.com"));
        assert!(!is_valid_share_link(""));
    }

    #[test]
    fn resend_eligibility() {
        let entry_ok = json!({
            "status": "Erfolgreich",
            "dir_name": "test",
            "share_link": "https://x.y/z",
        });
        assert!(can_resend_notifications(&entry_ok));
        assert!(!can_resend_notifications(&json!({"status": "Fehler"})));
    }

    #[test]
    fn contact_validation_and_normalize() {
        validate_contact_for_channels("a@b.de", Some("01601234567"), true, false).unwrap();
        assert!(validate_contact_for_channels("", Some("0160"), true, false).is_err());
        let (email, phone) = normalize_contact("  a@b.de ", "0160-99501966");
        assert_eq!(email, "a@b.de");
        assert_eq!(phone.as_deref(), Some("0160-99501966"));
    }

    #[test]
    fn delivered_channels() {
        let delivered = channels_already_delivered(
            &json!({"email_status": "Gesendet", "sms_status": "Zugestellt"}),
            true,
            true,
        );
        assert_eq!(delivered, vec!["email", "sms"]);
    }

    #[test]
    fn share_link_resolution_offline() {
        assert_eq!(
            resolve_share_link_offline(&json!({"share_link": "https://saved.link"}), None).unwrap(),
            "https://saved.link"
        );
        assert_eq!(
            resolve_share_link_offline(&json!({}), Some("https://manual.link")).unwrap(),
            "https://manual.link"
        );
        assert!(resolve_share_link_offline(&json!({}), None).is_err());
    }

    #[test]
    fn history_updates_and_messages() {
        let entry_ok = json!({
            "status": "Erfolgreich",
            "dir_name": "test",
            "share_link": "https://x.y/z",
        });
        let email_result = ChannelResult::new("email", "Gesendet", true, None);
        let sms_result = ChannelResult::new("sms", "Gesendet", true, Some("99".into()));
        let updates = build_resend_history_updates(
            &entry_ok,
            "a@b.de",
            Some("01601234567"),
            "https://x.y/z",
            Some(&email_result),
            Some(&sms_result),
            &["email".into(), "sms".into()],
            false,
            false,
        );
        assert_eq!(json_int(&updates, "email_resend_count"), 1);
        assert_eq!(json_int(&updates, "sms_resend_count"), 1);
        assert_eq!(
            updates
                .get("resend_log")
                .and_then(Value::as_array)
                .map(|a| a.len()),
            Some(1)
        );
        assert_eq!(json_str(&updates, "sms_id"), "99");

        let summary = format_resend_history_summary(&json!({
            "email_resend_count": 2,
            "sms_resend_count": 1,
        }));
        assert!(summary.contains("E-Mail 2×"));
        assert!(summary.contains("SMS 1×"));

        let message = format_resend_result_message(&ResendResult {
            email_result: Some(ChannelResult::new("email", "Gesendet", true, None)),
            sms_result: Some(ChannelResult::new("sms", "Fehler: x", false, None)),
            share_link: "https://x.y/z".into(),
            history_updates: json!({"email": "a@b.de", "phone": "0160"}),
        });
        assert!(message.contains("✓ E-Mail an a@b.de: Gesendet"));
        assert!(message.contains("✗ SMS an 0160: Fehler: x"));
        assert!(resend_had_failures(&ResendResult {
            email_result: Some(ChannelResult::new("email", "Gesendet", true, None)),
            sms_result: Some(ChannelResult::new("sms", "Fehler: x", false, None)),
            share_link: "https://x.y/z".into(),
            history_updates: json!({}),
        }));
    }

    #[test]
    fn sandbox_warning_texts() {
        let both = sandbox_warnings(true, "ops@example.de", true);
        assert!(both[0].contains("ops@example.de"));
        assert!(both[1].contains("SMS-Sandbox"));
        let no_fallback = sandbox_warnings(true, "", false);
        assert!(no_fallback[0].contains("kein Fallback"));
    }

    #[test]
    fn remote_path_falls_back_to_dir_name() {
        assert_eq!(
            remote_path_for_entry(&json!({"remote_path": "/saved"})),
            "/saved"
        );
        assert_eq!(
            remote_path_for_entry(&json!({"dir_name": "Flug_1"})),
            "/Flug_1"
        );
        assert_eq!(remote_path_for_entry(&json!({})), "");
    }

    #[test]
    fn contact_update_payload() {
        let payload =
            build_contact_update_payload(&json!({"dir_name": "d1"}), "a@b.de", Some("0160"));
        assert_eq!(json_str(&payload, "dir_name"), "d1");
        assert_eq!(json_str(&payload, "email"), "a@b.de");
        assert_eq!(json_str(&payload, "phone"), "0160");
    }
}
