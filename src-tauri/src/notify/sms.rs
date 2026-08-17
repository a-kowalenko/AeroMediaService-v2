//! seven.io SMS client (port of legacy `services/sms_client.py`).
//! Journal/history sync lives in `notify/sms_sync.rs`. Secrets come only from the OS keyring.

use std::time::Duration;

use serde_json::Value;

use crate::model::kunde::{normalize_phone, Kunde};
use crate::notify::message;
use crate::notify::{secret_first, setting_flag, setting_or_default};
use crate::storage::logging;

pub const SMS_ENDPOINT: &str = "https://gateway.seven.io/api/sms";
pub const BALANCE_ENDPOINT: &str = "https://gateway.seven.io/api/balance";
pub const JOURNAL_ENDPOINT: &str = "https://gateway.seven.io/api/journal/outbound";

#[derive(Debug, Clone, PartialEq)]
pub struct SmsSendResult {
    pub success: bool,
    pub sms_id: Option<String>,
    pub last_error: String,
}

impl SmsSendResult {
    fn ok(sms_id: Option<String>) -> Self {
        Self {
            success: true,
            sms_id,
            last_error: String::new(),
        }
    }

    fn err(last_error: impl Into<String>, sms_id: Option<String>) -> Self {
        Self {
            success: false,
            sms_id,
            last_error: last_error.into(),
        }
    }
}

pub fn is_sandbox() -> bool {
    setting_flag("seven_sandbox_mode")
}

/// Live: `seven_api_key` then `sms_api_key`. Sandbox: `seven_sandbox_api_key` then `sms_sandbox_api_key`.
pub fn resolve_api_key(sandbox: bool) -> (Option<String>, &'static str) {
    if sandbox {
        (
            secret_first(&["seven_sandbox_api_key", "sms_sandbox_api_key"]),
            "seven_sandbox_api_key",
        )
    } else {
        (
            secret_first(&["seven_api_key", "sms_api_key"]),
            "seven_api_key",
        )
    }
}

pub fn is_falsey_json(value: &Value) -> bool {
    match value {
        Value::Bool(false) => true,
        Value::Number(n) => n.as_i64() == Some(0) || n.as_u64() == Some(0),
        Value::String(s) => matches!(s.as_str(), "false" | "0"),
        _ => false,
    }
}

fn json_id(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Parse a seven.io SMS JSON body. HTTP 200 can still mean the message was rejected.
pub fn parse_sms_response(status: u16, body: &str) -> SmsSendResult {
    if status != 200 {
        return SmsSendResult::err(format!("HTTP {status}: {body}"), None);
    }

    let data: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            return SmsSendResult::err(
                format!("Antwort konnte nicht verarbeitet werden: {e}"),
                None,
            );
        }
    };

    let mut sms_id = None;
    if let Some(messages) = data.get("messages").and_then(Value::as_array) {
        if let Some(message) = messages.first() {
            sms_id = message.get("id").and_then(json_id);
            if message.get("success").is_some_and(is_falsey_json) {
                let error_text = message
                    .get("error_text")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("SMS abgelehnt");
                let mut last_error = error_text.to_string();
                if let Some(code) = message.get("error").filter(|v| !v.is_null()) {
                    last_error.push_str(&format!(" (Code {code})"));
                }
                return SmsSendResult::err(last_error, sms_id);
            }
        }
    }

    SmsSendResult::ok(sms_id)
}

pub fn parse_balance_amount(body: &str) -> Option<f64> {
    let data: Value = serde_json::from_str(body).ok()?;
    let amount = data.get("amount")?;
    amount
        .as_f64()
        .or_else(|| amount.as_i64().map(|n| n as f64))
        .or_else(|| amount.as_str()?.parse().ok())
}

pub fn sms_form_payload<'a>(
    to: &'a str,
    text: &'a str,
    sender: &'a str,
    sandbox: bool,
) -> [(&'static str, String); 5] {
    [
        ("to", to.to_string()),
        ("text", text.to_string()),
        ("from", sender.to_string()),
        ("sandbox", if sandbox { "1" } else { "0" }.to_string()),
        ("json", "1".to_string()),
    ]
}

pub async fn get_balance(api_key: &str) -> Option<f64> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    match client
        .get(BALANCE_ENDPOINT)
        .header("X-Api-Key", api_key)
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(resp) if resp.status().as_u16() == 200 => {
            let text = resp.text().await.ok()?;
            parse_balance_amount(&text)
        }
        Ok(_) => None,
        Err(e) => {
            logging::log_error(&format!("Fehler beim Abrufen der Seven.io Balance: {e}"));
            None
        }
    }
}

/// Human-readable balance string for settings UI (legacy format).
pub async fn get_balance_display(api_key: &str) -> String {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    match client
        .get(BALANCE_ENDPOINT)
        .header("X-Api-Key", api_key)
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(resp) if resp.status().as_u16() == 200 => {
            let text = resp.text().await.unwrap_or_default();
            let data: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let amount = data.get("amount").map(|v| match v {
                Value::Number(n) => n.to_string(),
                Value::String(s) => s.clone(),
                _ => "?".into(),
            });
            let mut currency = data
                .get("currency")
                .and_then(Value::as_str)
                .unwrap_or("€")
                .to_string();
            if currency == "EUR" {
                currency = "€".into();
            }
            match amount {
                Some(a) => format!("{a} {currency}"),
                None => "Unbekannt".into(),
            }
        }
        Ok(resp) => format!("Fehler ({})", resp.status().as_u16()),
        Err(_) => "Netzwerkfehler".into(),
    }
}

pub async fn send_sms(to_recipient: &str, text_body: &str) -> SmsSendResult {
    let sandbox = is_sandbox();
    let mode = if sandbox { "SANDBOX" } else { "LIVE" };
    let (api_key, key_name) = resolve_api_key(sandbox);
    let sender = setting_or_default("seven_sender", "");
    let sender = sender.trim();

    let Some(api_key) = api_key.filter(|k| !k.is_empty()) else {
        logging::log_error(&format!(
            "SMS-Versand fehlgeschlagen: '{key_name}' oder 'seven_sender' unvollständig."
        ));
        return SmsSendResult::err(
            format!("Konfigurationsfehler: {key_name}/seven_sender unvollständig"),
            None,
        );
    };
    if sender.is_empty() {
        logging::log_error(&format!(
            "SMS-Versand fehlgeschlagen: '{key_name}' oder 'seven_sender' unvollständig."
        ));
        return SmsSendResult::err(
            format!("Konfigurationsfehler: {key_name}/seven_sender unvollständig"),
            None,
        );
    }
    if to_recipient.trim().is_empty() {
        logging::log_error("SMS-Versand fehlgeschlagen: Kein Empfänger (to_recipient) angegeben.");
        return SmsSendResult::err("Kein Empfänger angegeben", None);
    }

    logging::log_info(&format!("({mode}) Versende SMS an {to_recipient}..."));

    let payload = sms_form_payload(to_recipient, text_body, sender, sandbox);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let response = client
        .post(SMS_ENDPOINT)
        .header("X-Api-Key", &api_key)
        .form(&payload)
        .send()
        .await;

    match response {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let response_text = resp.text().await.unwrap_or_default();
            if status == 200 {
                let preview = if response_text.len() > 80 {
                    format!("{}...", &response_text[..80])
                } else {
                    response_text.clone()
                };
                logging::log_info(&format!(
                    "({mode}) SMS an {to_recipient} erfolgreich verarbeitet. Response: {preview}"
                ));
            }
            let parsed = parse_sms_response(status, &response_text);
            if !parsed.success {
                if status == 200 {
                    logging::log_error(&format!(
                        "({mode}) SMS abgelehnt für {to_recipient}: {}. Response: {response_text}",
                        parsed.last_error
                    ));
                } else {
                    logging::log_error(&format!(
                        "({mode}) SMS-API-Fehler bei Versand an {to_recipient}. Status: {status}, Response: {response_text}"
                    ));
                }
                return parsed;
            }
            if !sandbox {
                if let Some(balance) = get_balance(&api_key).await {
                    if balance < 1.0 {
                        logging::log_error(&format!(
                            "ACHTUNG: Seven.io Balance ist unter 1€ (Aktueller Stand: {balance}€)"
                        ));
                    }
                }
            }
            parsed
        }
        Err(e) => {
            if e.is_timeout() || e.is_connect() {
                logging::log_error(&format!(
                    "AIOHTTP-Fehler beim Senden der SMS an {to_recipient}: {e}"
                ));
                SmsSendResult::err(format!("Netzwerkfehler: {e}"), None)
            } else {
                logging::log_error(&format!(
                    "Allgemeiner Fehler beim SMS-Versand an {to_recipient}: {e}"
                ));
                SmsSendResult::err(format!("Allgemeiner Fehler: {e}"), None)
            }
        }
    }
}

pub async fn send_upload_success_sms(share_link: &str, kunde: &Kunde) -> SmsSendResult {
    let phone_number = normalize_phone(kunde.phone.as_deref());
    let Some(phone_number) = phone_number else {
        logging::log_warn(&format!(
            "Keine Telefonnummer für Erfolgs-SMS (Gast: {} {}) angegeben. Versand wird übersprungen.",
            kunde.first_name.as_deref().unwrap_or(""),
            kunde.last_name.as_deref().unwrap_or("")
        ));
        return SmsSendResult::err("Keine Telefonnummer angegeben", None);
    };
    let text = message::download_link_text(
        message::display_name(kunde.first_name.as_deref()),
        share_link,
    );
    send_sms(&phone_number, &text).await
}

/// Outbound journal from seven.io. `None` if the API key is missing or the request fails.
pub async fn get_sms_journal(limit: u32) -> Option<Value> {
    let sandbox = is_sandbox();
    let (api_key, _) = resolve_api_key(sandbox);
    let Some(api_key) = api_key.filter(|k| !k.is_empty()) else {
        return None;
    };
    let limit = limit.clamp(1, 1000);
    let url = format!("{JOURNAL_ENDPOINT}?limit={limit}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    match client
        .get(&url)
        .header("X-Api-Key", api_key)
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(resp) if resp.status().as_u16() == 200 => resp.json::<Value>().await.ok(),
        Ok(resp) => {
            logging::log_error(&format!(
                "Fehler beim Abrufen des SMS-Journals: HTTP {}",
                resp.status()
            ));
            None
        }
        Err(e) => {
            logging::log_error(&format!("Ausnahme beim Abrufen des SMS-Journals: {e}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_sets_sandbox_and_json_flags() {
        let live = sms_form_payload("+49170", "hi", "AERO", false);
        assert_eq!(live[3], ("sandbox", "0".into()));
        assert_eq!(live[4], ("json", "1".into()));
        let sand = sms_form_payload("+49170", "hi", "AERO", true);
        assert_eq!(sand[3], ("sandbox", "1".into()));
        assert_eq!(sand[0], ("to", "+49170".into()));
        assert_eq!(sand[2], ("from", "AERO".into()));
    }

    #[test]
    fn parse_accepts_id_and_rejects_false_success() {
        let ok = parse_sms_response(200, r#"{"messages":[{"id":"12345","success":true}]}"#);
        assert!(ok.success);
        assert_eq!(ok.sms_id.as_deref(), Some("12345"));

        let numeric = parse_sms_response(200, r#"{"messages":[{"id":99,"success":1}]}"#);
        assert!(numeric.success);
        assert_eq!(numeric.sms_id.as_deref(), Some("99"));

        let rejected = parse_sms_response(
            200,
            r#"{"messages":[{"id":"x","success":false,"error_text":"invalid to","error":900}]}"#,
        );
        assert!(!rejected.success);
        assert_eq!(rejected.sms_id.as_deref(), Some("x"));
        assert!(rejected.last_error.contains("invalid to"));
        assert!(rejected.last_error.contains("900"));

        let http_err = parse_sms_response(401, "nope");
        assert!(!http_err.success);
        assert!(http_err.last_error.starts_with("HTTP 401"));
    }

    #[test]
    fn falsey_json_matches_legacy() {
        assert!(is_falsey_json(&Value::Bool(false)));
        assert!(is_falsey_json(&Value::from(0)));
        assert!(is_falsey_json(&Value::String("false".into())));
        assert!(is_falsey_json(&Value::String("0".into())));
        assert!(!is_falsey_json(&Value::Bool(true)));
        assert!(!is_falsey_json(&Value::from(1)));
    }

    #[test]
    fn balance_amount_parses_number_and_string() {
        assert_eq!(parse_balance_amount(r#"{"amount":12.5}"#), Some(12.5));
        assert_eq!(parse_balance_amount(r#"{"amount":"0.4"}"#), Some(0.4));
        assert_eq!(parse_balance_amount(r#"{}"#), None);
    }
}
