//! Shortens share URLs via POST `/api/shorten`.
//! Port of legacy `utils/link_shortener.py`. Secrets come only from the OS keyring.

use std::time::Duration;

use chrono::{Datelike, Duration as ChronoDuration, TimeZone, Timelike, Utc};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};

use crate::storage::config::runtime_setting;
use crate::storage::logging;
use crate::storage::secrets;

pub const EXPIRES_PRESET_PERMANENT: &str = "permanent";
pub const EXPIRES_PRESET_14D: &str = "14d";
pub const EXPIRES_PRESET_1M: &str = "1m";
pub const EXPIRES_PRESET_3M: &str = "3m";
pub const EXPIRES_PRESET_6M: &str = "6m";
pub const EXPIRES_PRESET_1Y: &str = "1y";

pub const EXPIRES_PRESET_KEYS: [&str; 6] = [
    EXPIRES_PRESET_PERMANENT,
    EXPIRES_PRESET_14D,
    EXPIRES_PRESET_1M,
    EXPIRES_PRESET_3M,
    EXPIRES_PRESET_6M,
    EXPIRES_PRESET_1Y,
];

/// ISO-8601 UTC expiry, or `None` for permanent / unknown presets.
pub fn expires_at_from_preset(preset: &str) -> Option<String> {
    expires_at_from_preset_at(preset, Utc::now())
}

pub fn expires_at_from_preset_at(preset: &str, now: chrono::DateTime<Utc>) -> Option<String> {
    let key = preset.trim().to_ascii_lowercase();
    if key == EXPIRES_PRESET_PERMANENT || !EXPIRES_PRESET_KEYS.contains(&key.as_str()) {
        return None;
    }
    let exp = if key == EXPIRES_PRESET_14D {
        now + ChronoDuration::days(14)
    } else {
        let months = match key.as_str() {
            EXPIRES_PRESET_1M => 1,
            EXPIRES_PRESET_3M => 3,
            EXPIRES_PRESET_6M => 6,
            EXPIRES_PRESET_1Y => 12,
            _ => return None,
        };
        add_calendar_months(now, months)
    };
    Some(exp.format("%Y-%m-%dT%H:%M:%S.000Z").to_string())
}

fn add_calendar_months(dt: chrono::DateTime<Utc>, months: i32) -> chrono::DateTime<Utc> {
    let mut month0 = dt.month() as i32 - 1 + months;
    let year = dt.year() + month0.div_euclid(12);
    month0 = month0.rem_euclid(12);
    let month = (month0 + 1) as u32;
    let last_day = days_in_month(year, month);
    let day = dt.day().min(last_day);
    Utc.with_ymd_and_hms(year, month, day, dt.hour(), dt.minute(), dt.second())
        .single()
        .unwrap_or(dt)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let start = chrono::NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let next = chrono::NaiveDate::from_ymd_opt(ny, nm, 1).unwrap();
    (next - start).num_days() as u32
}

pub fn legacy_url_to_base(api_url: &str) -> String {
    let url = api_url.trim().trim_end_matches('/');
    for suffix in ["/api/shorten", "/api/create"] {
        if let Some(stripped) = url.strip_suffix(suffix) {
            return stripped.to_string();
        }
    }
    url.to_string()
}

fn is_enabled(override_enabled: Option<bool>) -> bool {
    if let Some(enabled) = override_enabled {
        return enabled;
    }
    runtime_setting("link_shortener_enabled")
        .trim()
        .eq_ignore_ascii_case("true")
}

fn resolve_preset(override_preset: Option<&str>) -> String {
    if let Some(preset) = override_preset {
        let trimmed = preset.trim().to_ascii_lowercase();
        if trimmed.is_empty() {
            return EXPIRES_PRESET_PERMANENT.to_string();
        }
        return trimmed;
    }
    let raw = runtime_setting("shortener_expires_preset");
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        EXPIRES_PRESET_PERMANENT.to_string()
    } else {
        trimmed
    }
}

fn resolve_credentials(
    override_base: Option<&str>,
    override_key: Option<&str>,
) -> (String, String) {
    let mut base = override_base
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| secrets::get_secret("shortener_base_url").ok().flatten());
    let mut api_key = override_key
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| secrets::get_secret("shortener_api_key").ok().flatten());

    if base.as_ref().map(|s| s.is_empty()).unwrap_or(true) || api_key.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
        let legacy_url = secrets::get_secret("skylink_api_url").ok().flatten();
        let legacy_key = secrets::get_secret("skylink_api_key").ok().flatten();
        if api_key.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
            if let Some(key) = legacy_key.filter(|s| !s.is_empty()) {
                api_key = Some(key);
            }
        }
        if base.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
            if let Some(url) = legacy_url.filter(|s| !s.is_empty()) {
                base = Some(legacy_url_to_base(&url));
            }
        }
    }

    (
        base.unwrap_or_default().trim().to_string(),
        api_key.unwrap_or_default().trim().to_string(),
    )
}

/// Shortens `long_url`. On error or when disabled, returns the original URL.
pub async fn shorten(long_url: &str) -> String {
    shorten_with(long_url, None, None, None, None).await
}

pub async fn shorten_with(
    long_url: &str,
    override_base: Option<&str>,
    override_key: Option<&str>,
    override_enabled: Option<bool>,
    override_preset: Option<&str>,
) -> String {
    if !is_enabled(override_enabled) {
        logging::log_debug("Link-Shortener deaktiviert, überspringe Kürzen.");
        return long_url.to_string();
    }

    let (base, api_key) = resolve_credentials(override_base, override_key);
    if base.is_empty() || api_key.is_empty() {
        logging::log_debug("Shortener Basis-URL oder API-Key fehlt, überspringe Kürzen.");
        return long_url.to_string();
    }

    let preset = resolve_preset(override_preset);
    let expires_at = expires_at_from_preset(&preset);
    let endpoint = format!("{}/api/shorten", base.trim_end_matches('/'));
    logging::log_info(&format!("Versuche, URL zu kürzen: {long_url}"));

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("Accept", HeaderValue::from_static("application/json"));
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {api_key}")) {
        headers.insert(AUTHORIZATION, value);
    }

    let mut body = json!({ "url": long_url });
    if let Some(exp) = expires_at {
        body["expires_at"] = json!(exp);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    match client.post(&endpoint).headers(headers).json(&body).send().await {
        Ok(response) => {
            let status = response.status();
            if status.as_u16() == 201 {
                match response.json::<Value>().await {
                    Ok(payload) => {
                        if let Some(short_url) = payload.get("short_url").and_then(Value::as_str) {
                            if !short_url.is_empty() {
                                logging::log_info(&format!("Link erfolgreich gekürzt: {short_url}"));
                                return short_url.to_string();
                            }
                        }
                        logging::log_warn("Shortener-Antwort ohne short_url");
                    }
                    Err(_) => logging::log_warn("Shortener-Antwort ohne short_url"),
                }
                return long_url.to_string();
            }
            let error_msg = parse_error_status(status.as_u16(), &response.text().await.unwrap_or_default());
            logging::log_warn(&format!(
                "Kürzen fehlgeschlagen (Status {}): {error_msg}",
                status.as_u16()
            ));
            if status.as_u16() == 401 {
                logging::log_error("API-Key ungültig, abgelaufen oder Rate-Limit überschritten.");
            }
            long_url.to_string()
        }
        Err(e) => {
            if e.is_timeout() {
                logging::log_error("Verbindung zum Link-Shortener: Timeout");
            } else {
                logging::log_error(&format!("Verbindung zum Link-Shortener: {e}"));
            }
            long_url.to_string()
        }
    }
}

fn parse_error_status(status: u16, text: &str) -> String {
    if let Ok(data) = serde_json::from_str::<Value>(text) {
        if let Some(err) = data.get("error").and_then(Value::as_str) {
            return err.to_string();
        }
    }
    let trimmed = text.trim();
    if trimmed.len() > 300 {
        format!("{}...", &trimmed[..300])
    } else if trimmed.is_empty() {
        format!("HTTP {status}")
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn expires_at_permanent_and_unknown_are_none() {
        let now = Utc.with_ymd_and_hms(2024, 1, 31, 12, 0, 0).unwrap();
        assert!(expires_at_from_preset_at("permanent", now).is_none());
        assert!(expires_at_from_preset_at("", now).is_none());
        assert!(expires_at_from_preset_at("nope", now).is_none());
    }

    #[test]
    fn expires_at_14d_and_calendar_months() {
        let now = Utc.with_ymd_and_hms(2024, 1, 31, 12, 0, 0).unwrap();
        assert_eq!(
            expires_at_from_preset_at("14d", now).as_deref(),
            Some("2024-02-14T12:00:00.000Z")
        );
        assert_eq!(
            expires_at_from_preset_at("1m", now).as_deref(),
            Some("2024-02-29T12:00:00.000Z")
        );
        assert_eq!(
            expires_at_from_preset_at("1y", now).as_deref(),
            Some("2025-01-31T12:00:00.000Z")
        );
    }

    #[test]
    fn legacy_skylink_url_strips_endpoint() {
        assert_eq!(
            legacy_url_to_base("https://host.example/api/shorten"),
            "https://host.example"
        );
        assert_eq!(
            legacy_url_to_base("https://host.example/api/create/"),
            "https://host.example"
        );
        assert_eq!(legacy_url_to_base("https://host.example"), "https://host.example");
    }

    #[tokio::test]
    async fn disabled_shortener_returns_original() {
        let original = "https://www.dropbox.com/s/abc";
        let result = shorten_with(original, None, None, Some(false), None).await;
        assert_eq!(result, original);
    }

    #[tokio::test]
    async fn missing_credentials_return_original() {
        let original = "https://www.dropbox.com/s/abc";
        let result = shorten_with(original, Some(""), Some(""), Some(true), None).await;
        assert_eq!(result, original);
    }
}
