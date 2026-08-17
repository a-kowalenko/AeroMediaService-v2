//! Gesamtstatus der Upload-Historie (Versendet / Komplett / …).
//! Port of legacy `core/history_status.py`.

#![allow(dead_code)]

use chrono::{Local, NaiveDateTime, TimeZone};
use serde_json::Value;

/// Ohne Zustellbestätigung nach dieser Zeit gilt SMS als zugestellt (Seven-DLR fehlt oft).
pub const SMS_DELIVERY_STALE_HOURS: f64 = 72.0;

pub fn translate_sms_dlr_status(status: Option<&str>) -> String {
    let lower_status = status.unwrap_or("").to_lowercase();
    if lower_status.contains("notdelivered")
        || matches!(lower_status.as_str(), "undeliv" | "rejectd" | "expired")
    {
        return "Fehlgeschlagen".into();
    }
    if lower_status.contains("failed") {
        return "Fehlgeschlagen".into();
    }
    if lower_status.contains("delivered") || lower_status == "delivrd" {
        return "Zugestellt".into();
    }
    if lower_status.contains("buffered") {
        return "Gepuffert".into();
    }
    if lower_status.contains("transmitted") {
        return "Übertragen".into();
    }
    if lower_status.contains("accepted") || lower_status == "acceptd" {
        return "Akzeptiert".into();
    }
    if lower_status.contains("rejected") {
        return "Abgelehnt".into();
    }
    status.unwrap_or("").to_string()
}

fn json_str<'a>(item: &'a Value, key: &str) -> &'a str {
    item.get(key).and_then(Value::as_str).unwrap_or("")
}

pub fn parse_iso_timestamp(ts_str: Option<&str>) -> Option<f64> {
    let raw = ts_str?;
    let mut text = raw.trim().replace('Z', "");
    if text.is_empty() {
        return None;
    }
    if let Some(pos) = text.find('+') {
        text.truncate(pos);
    }
    if let Some((main, frac)) = text.split_once('.') {
        let frac: String = frac.chars().take(6).collect();
        text = format!("{main}.{frac}");
    }

    let naive = parse_naive(&text)?;
    let local = Local.from_local_datetime(&naive).single()?;
    Some(local.timestamp() as f64 + f64::from(local.timestamp_subsec_nanos()) / 1_000_000_000.0)
}

fn parse_naive(text: &str) -> Option<NaiveDateTime> {
    let candidates: &[&str] = if text.contains('T') {
        &["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S"]
    } else {
        &["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d %H:%M:%S"]
    };
    for fmt in candidates {
        let slice = if fmt.contains("%.f") {
            text
        } else {
            text.get(..19.min(text.len())).unwrap_or(text)
        };
        if let Ok(dt) = NaiveDateTime::parse_from_str(slice, fmt) {
            return Some(dt);
        }
    }
    None
}

/// Zeitpunkt des letzten SMS-Versands für Stale-Logik.
pub fn sms_sent_reference_timestamp(item: &Value) -> Option<f64> {
    for key in ["last_sms_resent_at", "last_updated", "created_at"] {
        if let Some(ts) = parse_iso_timestamp(item.get(key).and_then(Value::as_str)) {
            return Some(ts);
        }
    }
    None
}

pub fn hours_since_sms_sent(item: &Value) -> Option<f64> {
    let reference = sms_sent_reference_timestamp(item)?;
    Some(((Local::now().timestamp() as f64) - reference).max(0.0) / 3600.0)
}

pub fn is_sms_sent_status(status_value: Option<&str>) -> bool {
    let s = status_value.unwrap_or("").trim().to_lowercase();
    if s.is_empty() {
        return false;
    }
    if s == "übersprungen" {
        return true;
    }
    [
        "gesendet",
        "zugestellt",
        "erfolgreich",
        "übertragen",
        "gepuffert",
        "akzeptiert",
    ]
    .iter()
    .any(|token| s.contains(token))
}

pub fn is_sms_delivered_status(status_value: Option<&str>, item: Option<&Value>) -> bool {
    let s = status_value.unwrap_or("").trim().to_lowercase();
    if s.is_empty() {
        return false;
    }
    if s == "übersprungen" {
        return true;
    }
    if s.contains("zugestellt") || s.contains("erfolgreich") {
        return true;
    }
    if let Some(item) = item {
        if is_sms_sent_status(status_value) {
            if let Some(age) = hours_since_sms_sent(item) {
                if age >= SMS_DELIVERY_STALE_HOURS {
                    return true;
                }
            }
        }
    }
    false
}

fn is_problem(status_value: Option<&str>) -> bool {
    is_problem_status(status_value)
}

fn is_in_progress(status_value: Option<&str>) -> bool {
    let s = status_value.unwrap_or("").trim().to_lowercase();
    if s.is_empty() {
        return false;
    }
    s.contains("gestartet")
        || s.contains("übertragen")
        || s.contains("gepuffert")
        || s.contains("akzeptiert")
}

fn is_best_upload(status_value: Option<&str>) -> bool {
    status_value
        .unwrap_or("")
        .trim()
        .to_lowercase()
        .contains("erfolgreich")
}

fn is_best_email(status_value: Option<&str>) -> bool {
    let s = status_value.unwrap_or("").trim().to_lowercase();
    s.contains("gesendet") || s.contains("zugestellt") || s.contains("erfolgreich")
}

/// Erstellt den Gesamtstatus für das Main Grid.
pub fn build_overall_status(item: &Value) -> String {
    let upload_status = json_str(item, "status").trim();
    let email_status = json_str(item, "email_status").trim();
    let sms_status = json_str(item, "sms_status").trim();
    let email_value = json_str(item, "email").trim();
    let phone_value = json_str(item, "phone").trim();

    let upload_problem = is_problem(Some(upload_status));
    let email_problem = !email_value.is_empty() && is_problem(Some(email_status));
    let sms_problem = !phone_value.is_empty() && is_problem(Some(sms_status));
    if upload_problem || email_problem || sms_problem {
        return "Problem".into();
    }

    // Nur Upload/E-Mail-Laufstatus blockiert; SMS-Zwischenstände (Übertragen …) = Versendet.
    if is_in_progress(Some(upload_status)) || is_in_progress(Some(email_status)) {
        return "In Bearbeitung".into();
    }

    let upload_is_best = is_best_upload(Some(upload_status));
    let email_is_best = email_value.is_empty() || is_best_email(Some(email_status));
    let sms_is_sent = phone_value.is_empty() || is_sms_sent_status(Some(sms_status));
    let sms_is_delivered =
        phone_value.is_empty() || is_sms_delivered_status(Some(sms_status), Some(item));

    if upload_is_best && email_is_best && sms_is_sent {
        if sms_is_delivered {
            return "Komplett".into();
        }
        return "Versendet".into();
    }

    if !upload_status.is_empty() || !email_status.is_empty() || !sms_status.is_empty() {
        return "Teilweise".into();
    }

    "Unbekannt".into()
}

/// True, wenn ein Journal-Abgleich den SMS-Status noch verbessern könnte.
pub fn history_entry_needs_sms_journal_check(item: &Value) -> bool {
    if item
        .get("sms_status_locked")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return false;
    }
    let phone = json_str(item, "phone").trim();
    if phone.is_empty() {
        return false;
    }
    let sms_status = json_str(item, "sms_status").trim();
    if sms_status.is_empty() || sms_status == "Übersprungen" {
        return false;
    }
    if sms_status == "Zugestellt" || sms_status == "Fehlgeschlagen" {
        return false;
    }
    if is_problem_status(Some(sms_status)) {
        return false;
    }
    if is_sms_delivered_status(Some(sms_status), Some(item)) {
        return false;
    }
    true
}

pub fn is_problem_status(status_value: Option<&str>) -> bool {
    let s = status_value.unwrap_or("").trim().to_lowercase();
    if s.is_empty() {
        return false;
    }
    s.contains("fehler") || s.contains("fehlgeschlagen") || s.contains("abgelehnt")
}

pub fn normalize_phone_digits(phone: Option<&str>) -> String {
    let mut digits: String = phone
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return String::new();
    }
    if digits.starts_with("00") {
        digits = digits[2..].to_string();
    }
    if digits.starts_with('0') {
        digits = format!("49{}", &digits[1..]);
    } else if digits.len() <= 11 && !digits.starts_with("49") {
        let stripped = digits.trim_start_matches('0');
        digits = format!("49{stripped}");
    }
    digits
}

pub fn phones_match(phone_a: Option<&str>, phone_b: Option<&str>) -> bool {
    let a = normalize_phone_digits(phone_a);
    let b = normalize_phone_digits(phone_b);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    let tail_len = a.len().min(b.len()).min(10);
    tail_len >= 8 && a[a.len() - tail_len..] == b[b.len() - tail_len..]
}

/// Sammelt aktuelle Upload-/E-Mail-/SMS-Fehler in einem Text (Legacy `build_combined_error_text`).
/// Eine `error_msg` nach erfolgreichem Retry zählt nicht mehr als aktueller Upload-Fehler.
pub fn build_combined_error_text(item: &Value) -> String {
    let mut errors = Vec::new();
    let upload_status = json_str(item, "status").trim();
    let upload_error = json_str(item, "error_msg").trim();
    if !upload_error.is_empty() && is_problem_status(Some(upload_status)) {
        errors.push(format!("Upload: {upload_error}"));
    }
    let email_status = json_str(item, "email_status").trim();
    if !email_status.is_empty() && is_problem_status(Some(email_status)) {
        errors.push(format!("E-Mail: {email_status}"));
    }
    let sms_status = json_str(item, "sms_status").trim();
    if !sms_status.is_empty() && is_problem_status(Some(sms_status)) {
        errors.push(format!("SMS: {sms_status}"));
    }
    errors.join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Local};
    use serde_json::json;

    fn iso_now() -> String {
        Local::now().format("%Y-%m-%dT%H:%M:%S%.f").to_string()
    }

    fn iso_hours_ago(hours: i64) -> String {
        (Local::now() - Duration::hours(hours))
            .format("%Y-%m-%dT%H:%M:%S%.f")
            .to_string()
    }

    #[test]
    fn test_translate_sms_dlr_status() {
        assert_eq!(translate_sms_dlr_status(Some("DELIVERED")), "Zugestellt");
        assert_eq!(translate_sms_dlr_status(Some("TRANSMITTED")), "Übertragen");
        assert_eq!(
            translate_sms_dlr_status(Some("NOTDELIVERED")),
            "Fehlgeschlagen"
        );
    }

    #[test]
    fn test_build_overall_status_email_only_complete() {
        let item = json!({
            "status": "Erfolgreich",
            "email_status": "Gesendet",
            "email": "a@b.de",
        });
        assert_eq!(build_overall_status(&item), "Komplett");
    }

    #[test]
    fn test_build_overall_status_sms_sent_is_versendet() {
        let item = json!({
            "status": "Erfolgreich",
            "email_status": "Gesendet",
            "email": "a@b.de",
            "phone": "016099501966",
            "sms_status": "Gesendet",
            "last_updated": iso_now(),
        });
        assert_eq!(build_overall_status(&item), "Versendet");
    }

    #[test]
    fn test_build_overall_status_sms_transmitted_is_versendet_not_in_progress() {
        let item = json!({
            "status": "Erfolgreich",
            "email_status": "Gesendet",
            "email": "a@b.de",
            "phone": "016099501966",
            "sms_status": "Übertragen",
            "last_updated": iso_now(),
        });
        assert_eq!(build_overall_status(&item), "Versendet");
    }

    #[test]
    fn test_build_overall_status_sms_delivered_complete() {
        let item = json!({
            "status": "Erfolgreich",
            "email_status": "Gesendet",
            "email": "a@b.de",
            "phone": "016099501966",
            "sms_status": "Zugestellt",
        });
        assert_eq!(build_overall_status(&item), "Komplett");
    }

    #[test]
    fn test_build_overall_status_stale_sms_becomes_complete() {
        let item = json!({
            "status": "Erfolgreich",
            "email_status": "Gesendet",
            "email": "a@b.de",
            "phone": "016099501966",
            "sms_status": "Gesendet",
            "last_updated": iso_hours_ago(80),
        });
        assert!(is_sms_delivered_status(Some("Gesendet"), Some(&item)));
        assert_eq!(build_overall_status(&item), "Komplett");
    }

    #[test]
    fn test_phones_match_german_formats() {
        assert!(phones_match(Some("016099501966"), Some("4916099501966")));
        assert!(phones_match(
            Some("+49 160 99501966"),
            Some("4916099501966")
        ));
    }

    #[test]
    fn test_history_entry_needs_check_skips_delivered() {
        assert!(!history_entry_needs_sms_journal_check(&json!({
            "phone": "123",
            "sms_status": "Zugestellt",
        })));
        assert!(history_entry_needs_sms_journal_check(&json!({
            "phone": "123",
            "sms_status": "Gesendet",
            "last_updated": iso_now(),
        })));
    }

    #[test]
    fn test_problem_and_in_progress() {
        assert_eq!(
            build_overall_status(&json!({
                "status": "Fehler",
                "error_msg": "timeout",
            })),
            "Problem"
        );
        assert_eq!(
            build_overall_status(&json!({
                "status": "Gestartet",
            })),
            "In Bearbeitung"
        );
        assert_eq!(build_overall_status(&json!({})), "Unbekannt");
    }

    #[test]
    fn test_combined_error_text() {
        let item = json!({
            "status": "Fehler",
            "error_msg": "HTTP 500",
            "email_status": "Fehler: SMTP",
            "sms_status": "Fehlgeschlagen",
        });
        let text = build_combined_error_text(&item);
        assert!(text.contains("Upload: HTTP 500"));
        assert!(text.contains("E-Mail: Fehler: SMTP"));
        assert!(text.contains("SMS: Fehlgeschlagen"));
    }

    #[test]
    fn test_combined_error_ignores_stale_upload_error_after_success() {
        let item = json!({
            "status": "Erfolgreich",
            "error_msg": "timeout",
            "email_status": "Gesendet",
        });
        assert_eq!(build_combined_error_text(&item), "");
    }

    #[test]
    fn test_combined_error_ignores_stale_upload_error_while_retry_running() {
        let item = json!({
            "status": "Gestartet",
            "error_msg": "timeout",
        });
        assert_eq!(build_combined_error_text(&item), "");
    }
}
