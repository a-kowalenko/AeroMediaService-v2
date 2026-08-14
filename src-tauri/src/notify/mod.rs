//! Customer notifications after a successful upload.
//! Orchestration matches legacy `core/uploader.py` (email + SMS statuses);
//! WhatsApp is sent in addition when a phone number is present.

pub mod email;
pub mod message;
pub mod resend;
pub mod sms;
pub mod sms_sync;
pub mod whatsapp;

use crate::model::kunde::{normalize_phone, Kunde};
use crate::storage::config::runtime_setting;
use crate::storage::logging;
use crate::storage::secrets;

pub const STATUS_SKIPPED: &str = "Übersprungen";
pub const STATUS_SENT: &str = "Gesendet";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyOutcome {
    pub email_status: String,
    pub sms_status: String,
    pub sms_id: Option<String>,
}

impl NotifyOutcome {
    pub fn skipped() -> Self {
        Self {
            email_status: STATUS_SKIPPED.to_string(),
            sms_status: STATUS_SKIPPED.to_string(),
            sms_id: None,
        }
    }
}

pub fn secret_first(keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Ok(Some(value)) = secrets::get_secret(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

pub fn setting_or_default(key: &str, default: &str) -> String {
    let value = runtime_setting(key);
    if value.is_empty() {
        default.to_string()
    } else {
        value
    }
}

pub fn setting_flag(key: &str) -> bool {
    runtime_setting(key).trim().eq_ignore_ascii_case("true")
}

pub fn has_email(kunde: &Kunde) -> bool {
    kunde
        .email
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
}

pub fn has_phone(kunde: &Kunde) -> bool {
    normalize_phone(kunde.phone.as_deref()).is_some()
}

pub fn email_status_from_sent(sent: bool) -> String {
    if sent {
        STATUS_SENT.to_string()
    } else {
        "Fehler: Versand fehlgeschlagen".to_string()
    }
}

pub fn sms_status_from_result(success: bool, last_error: &str) -> String {
    if success {
        STATUS_SENT.to_string()
    } else {
        let err = last_error.trim();
        if err.is_empty() {
            "Fehler: Fehler beim Senden".to_string()
        } else {
            format!("Fehler: {err}")
        }
    }
}

/// Send email (if `kunde.email`), SMS (if `kunde.phone`), and WhatsApp (if phone).
/// No-op statuses stay `Übersprungen` when there is no share link or no matching contact.
pub async fn notify_after_upload(
    dir_name: &str,
    share_link: Option<&str>,
    kunde: Option<&Kunde>,
) -> NotifyOutcome {
    let mut outcome = NotifyOutcome::skipped();
    let Some(share_link) = share_link.filter(|s| !s.trim().is_empty()) else {
        return outcome;
    };
    let Some(kunde) = kunde else {
        logging::log_warn(&format!(
            "Keine Kundendaten für {dir_name} gefunden. Benachrichtigungen übersprungen."
        ));
        return outcome;
    };

    if has_email(kunde) {
        let sent = email::send_upload_success_email(
            dir_name,
            share_link,
            kunde.email.as_deref(),
            kunde.first_name.as_deref(),
        )
        .await;
        outcome.email_status = email_status_from_sent(sent);
    }

    if has_phone(kunde) {
        let result = sms::send_upload_success_sms(share_link, kunde).await;
        if result.success {
            outcome.sms_status = STATUS_SENT.to_string();
            outcome.sms_id = result.sms_id;
            if outcome.sms_id.is_none() {
                logging::log_warn(&format!(
                    "SMS für {dir_name} versendet, aber keine sms_id von Seven.io erhalten."
                ));
            }
        } else {
            outcome.sms_status = sms_status_from_result(false, &result.last_error);
        }

        if let Some(phone) = normalize_phone(kunde.phone.as_deref()) {
            if whatsapp::config_complete(&whatsapp::load_twilio_config()) {
                let text = message::download_link_text(
                    message::display_name(kunde.first_name.as_deref()),
                    share_link,
                );
                let _ = whatsapp::send_whatsapp(&phone, &text).await;
            }
        }
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kunde_email() -> Kunde {
        Kunde {
            first_name: Some("Anna".into()),
            email: Some("anna@example.de".into()),
            ..Kunde::default()
        }
    }

    #[test]
    fn contact_helpers() {
        let mut k = kunde_email();
        assert!(has_email(&k));
        assert!(!has_phone(&k));
        k.phone = Some("none".into());
        assert!(!has_phone(&k));
        k.phone = Some("+49170".into());
        assert!(has_phone(&k));
        k.email = Some("  ".into());
        assert!(!has_email(&k));
    }

    #[test]
    fn status_strings() {
        assert_eq!(email_status_from_sent(true), "Gesendet");
        assert_eq!(
            email_status_from_sent(false),
            "Fehler: Versand fehlgeschlagen"
        );
        assert_eq!(sms_status_from_result(true, ""), "Gesendet");
        assert_eq!(
            sms_status_from_result(false, "HTTP 500: x"),
            "Fehler: HTTP 500: x"
        );
        assert_eq!(
            sms_status_from_result(false, ""),
            "Fehler: Fehler beim Senden"
        );
    }

    #[tokio::test]
    async fn skipped_without_link_or_kunde() {
        let none_link = notify_after_upload("dir", None, Some(&kunde_email())).await;
        assert_eq!(none_link, NotifyOutcome::skipped());

        let no_kunde = notify_after_upload("dir", Some("https://x"), None).await;
        assert_eq!(no_kunde, NotifyOutcome::skipped());
    }

    #[tokio::test]
    async fn email_error_when_smtp_incomplete() {
        let outcome =
            notify_after_upload("dir", Some("https://dropbox.com/s/x"), Some(&kunde_email())).await;
        assert_eq!(outcome.email_status, "Fehler: Versand fehlgeschlagen");
        assert_eq!(outcome.sms_status, STATUS_SKIPPED);
        assert!(outcome.sms_id.is_none());
    }
}
