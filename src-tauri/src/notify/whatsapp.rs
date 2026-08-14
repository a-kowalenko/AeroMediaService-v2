//! Twilio WhatsApp client (port of legacy `services/whatsapp_client.py`).
//! Secrets (`twilio_account_sid` / `twilio_auth_token`) come only from the OS keyring.

use std::time::Duration;

use serde_json::Value;

use crate::notify::{secret_first, setting_or_default};
use crate::storage::logging;

pub const TWILIO_API_BASE: &str = "https://api.twilio.com/2010-04-01/Accounts";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwilioConfig {
    pub account_sid: String,
    pub auth_token: String,
    pub from_number: String,
}

pub fn twilio_messages_url(account_sid: &str) -> String {
    format!("{TWILIO_API_BASE}/{account_sid}/Messages.json")
}

pub fn whatsapp_address(number: &str) -> String {
    let trimmed = number.trim();
    if trimmed.starts_with("whatsapp:") {
        trimmed.to_string()
    } else {
        format!("whatsapp:{trimmed}")
    }
}

pub fn load_twilio_config() -> TwilioConfig {
    TwilioConfig {
        account_sid: secret_first(&["twilio_account_sid"]).unwrap_or_default(),
        auth_token: secret_first(&["twilio_auth_token"]).unwrap_or_default(),
        from_number: setting_or_default("twilio_whatsapp_from", "").trim().to_string(),
    }
}

pub fn config_complete(cfg: &TwilioConfig) -> bool {
    !(cfg.account_sid.is_empty() || cfg.auth_token.is_empty() || cfg.from_number.is_empty())
}

/// Legacy dummy: always `true` until a Lookup API is added.
#[allow(dead_code)]
pub async fn has_whatsapp(_to_number: &str) -> bool {
    true
}

pub async fn send_whatsapp(to_number: &str, body: &str) -> bool {
    let cfg = load_twilio_config();
    if !config_complete(&cfg) {
        logging::log_error("WhatsApp-Konfiguration unvollständig.");
        return false;
    }

    let endpoint = twilio_messages_url(&cfg.account_sid);
    let from = whatsapp_address(&cfg.from_number);
    let to = whatsapp_address(to_number);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let response = client
        .post(&endpoint)
        .basic_auth(&cfg.account_sid, Some(&cfg.auth_token))
        .form(&[("From", from.as_str()), ("To", to.as_str()), ("Body", body)])
        .send()
        .await;

    match response {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if status == 200 || status == 201 {
                let sid = resp
                    .json::<Value>()
                    .await
                    .ok()
                    .and_then(|v| v.get("sid").and_then(Value::as_str).map(str::to_string))
                    .unwrap_or_else(|| "?".into());
                logging::log_info(&format!("WhatsApp accepted for {to_number}. SID: {sid}"));
                true
            } else {
                let text = resp.text().await.unwrap_or_default();
                logging::log_warn(&format!(
                    "WhatsApp send failed ({status}) for {to_number}: {text}"
                ));
                false
            }
        }
        Err(e) => {
            if e.is_timeout() || e.is_connect() {
                logging::log_error(&format!(
                    "Netzwerkfehler bei WhatsApp-Versand an {to_number}: {e}"
                ));
            } else {
                logging::log_error(&format!(
                    "Unerwarteter Fehler beim WhatsApp-Versand an {to_number}: {e}"
                ));
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_url_and_whatsapp_prefix() {
        assert_eq!(
            twilio_messages_url("ACabc"),
            "https://api.twilio.com/2010-04-01/Accounts/ACabc/Messages.json"
        );
        assert_eq!(whatsapp_address("+49170"), "whatsapp:+49170");
        assert_eq!(whatsapp_address("whatsapp:+49170"), "whatsapp:+49170");
    }

    #[test]
    fn incomplete_config_is_detected() {
        let mut cfg = TwilioConfig {
            account_sid: "AC".into(),
            auth_token: "tok".into(),
            from_number: "+1415".into(),
        };
        assert!(config_complete(&cfg));
        cfg.from_number.clear();
        assert!(!config_complete(&cfg));
    }
}
