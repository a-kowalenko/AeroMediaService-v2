//! Manual overall-status actions for upload history.
//! Port of legacy `core/manual_status.py`.

use chrono::Local;
use serde_json::{json, Map, Value};

use crate::model::history_status::{build_overall_status, is_problem_status};

pub const ACTION_MARK_COMPLETE: &str = "Komplett";
pub const ACTION_MARK_SENT: &str = "Versendet";
pub const ACTION_RESOLVE_PROBLEM: &str = "Problem auflösen";

pub const MANUAL_STATUS_ACTIONS: [&str; 3] = [
    ACTION_MARK_COMPLETE,
    ACTION_MARK_SENT,
    ACTION_RESOLVE_PROBLEM,
];

fn json_str<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry.get(key).and_then(Value::as_str).unwrap_or("")
}

fn has_email(entry: &Value) -> bool {
    !json_str(entry, "email").trim().is_empty()
}

fn has_phone(entry: &Value) -> bool {
    !json_str(entry, "phone").trim().is_empty()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChannelSnapshot {
    status: String,
    email_status: String,
    sms_status: String,
    error_msg: String,
}

fn snapshot_channels(entry: &Value) -> ChannelSnapshot {
    ChannelSnapshot {
        status: json_str(entry, "status").trim().to_string(),
        email_status: json_str(entry, "email_status").trim().to_string(),
        sms_status: json_str(entry, "sms_status").trim().to_string(),
        error_msg: json_str(entry, "error_msg").trim().to_string(),
    }
}

fn snapshot_to_json(snap: &ChannelSnapshot) -> Value {
    json!({
        "status": snap.status,
        "email_status": snap.email_status,
        "sms_status": snap.sms_status,
        "error_msg": snap.error_msg,
    })
}

/// Hints shown before applying a manual status (non-blocking).
pub fn collect_manual_status_warnings(entry: &Value, action: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    let upload_status = json_str(entry, "status").trim();

    if action == ACTION_MARK_COMPLETE || action == ACTION_MARK_SENT {
        if !matches!(upload_status, "" | "Erfolgreich" | "Gestartet") {
            warnings.push(format!(
                "Upload-Status ist „{upload_status}“ — wird auf „Erfolgreich“ gesetzt."
            ));
        }
        if json_str(entry, "share_link").trim().is_empty() {
            warnings.push(
                "Kein Download-Link gespeichert — erneuter Versand ist ggf. nicht möglich.".into(),
            );
        }
    }

    if action == ACTION_RESOLVE_PROBLEM && build_overall_status(entry) != "Problem" {
        warnings.push("Der aktuelle Gesamtstatus ist nicht „Problem“.".into());
    }

    if action == ACTION_MARK_COMPLETE && !has_email(entry) && !has_phone(entry) {
        warnings.push("Weder E-Mail noch Telefon hinterlegt.".into());
    }

    warnings
}

fn target_email_status(entry: &Value, delivered: bool) -> Option<&'static str> {
    if !has_email(entry) {
        return None;
    }
    let current = json_str(entry, "email_status").trim();
    if delivered || is_problem_status(Some(current)) || current.is_empty() {
        Some("Gesendet")
    } else {
        None
    }
}

fn target_sms_status(entry: &Value, delivered: bool) -> Option<&'static str> {
    if !has_phone(entry) {
        return None;
    }
    let current = json_str(entry, "sms_status").trim();
    if delivered {
        if current == "Zugestellt" {
            return None;
        }
        return Some("Zugestellt");
    }
    if is_problem_status(Some(current)) || current.is_empty() {
        return Some("Gesendet");
    }
    let lower = current.to_lowercase();
    if [
        "gesendet",
        "zugestellt",
        "übertragen",
        "gepuffert",
        "akzeptiert",
        "übersprungen",
    ]
    .iter()
    .any(|token| lower.contains(token))
    {
        None
    } else {
        Some("Gesendet")
    }
}

fn apply_resolve_problem(entry: &Value, updates: &mut Map<String, Value>) {
    let upload_status = json_str(entry, "status").trim();
    if is_problem_status(Some(upload_status)) || matches!(upload_status, "Fehler" | "Abgebrochen") {
        updates.insert("status".into(), Value::String("Erfolgreich".into()));
        updates.insert("error_msg".into(), Value::String(String::new()));
    }

    let email_status = json_str(entry, "email_status").trim();
    if has_email(entry) && is_problem_status(Some(email_status)) {
        updates.insert("email_status".into(), Value::String("Gesendet".into()));
    }

    let sms_status = json_str(entry, "sms_status").trim();
    if has_phone(entry) && is_problem_status(Some(sms_status)) {
        updates.insert("sms_status".into(), Value::String("Zugestellt".into()));
    }
}

fn merge_preview(entry: &Value, updates: &Map<String, Value>) -> Value {
    let mut preview = entry.clone();
    if let Some(obj) = preview.as_object_mut() {
        for (key, value) in updates {
            obj.insert(key.clone(), value.clone());
        }
    }
    preview
}

/// Builds the History `add_or_update` payload for a manual status action.
pub fn build_manual_status_update(
    entry: &Value,
    action: &str,
    reason: &str,
) -> Result<Value, String> {
    if !MANUAL_STATUS_ACTIONS.contains(&action) {
        return Err(format!("Unbekannte Aktion: {action}"));
    }

    let dir_name = json_str(entry, "dir_name").trim();
    if dir_name.is_empty() {
        return Err("Historieneintrag ohne dir_name.".into());
    }

    let before = snapshot_channels(entry);
    let mut updates = Map::new();
    updates.insert("dir_name".into(), Value::String(dir_name.to_string()));

    match action {
        ACTION_MARK_COMPLETE => {
            updates.insert("status".into(), Value::String("Erfolgreich".into()));
            if let Some(email_target) = target_email_status(entry, true) {
                updates.insert(
                    "email_status".into(),
                    Value::String(email_target.to_string()),
                );
            }
            if let Some(sms_target) = target_sms_status(entry, true) {
                updates.insert("sms_status".into(), Value::String(sms_target.to_string()));
            }
            if is_problem_status(Some(&before.status))
                || matches!(before.status.as_str(), "Fehler" | "Abgebrochen")
            {
                updates.insert("error_msg".into(), Value::String(String::new()));
            }
        }
        ACTION_MARK_SENT => {
            updates.insert("status".into(), Value::String("Erfolgreich".into()));
            if let Some(email_target) = target_email_status(entry, false) {
                updates.insert(
                    "email_status".into(),
                    Value::String(email_target.to_string()),
                );
            }
            if let Some(sms_target) = target_sms_status(entry, false) {
                updates.insert("sms_status".into(), Value::String(sms_target.to_string()));
            }
            if is_problem_status(Some(&before.status))
                || matches!(before.status.as_str(), "Fehler" | "Abgebrochen")
            {
                updates.insert("error_msg".into(), Value::String(String::new()));
            }
        }
        ACTION_RESOLVE_PROBLEM => {
            apply_resolve_problem(entry, &mut updates);
            if !["status", "email_status", "sms_status", "error_msg"]
                .iter()
                .any(|k| updates.contains_key(*k))
            {
                return Err("Kein Problem-Status zum Auflösen vorhanden.".into());
            }
        }
        _ => unreachable!(),
    }

    let after_preview = merge_preview(entry, &updates);
    let after = snapshot_channels(&after_preview);
    if before == after && action != ACTION_RESOLVE_PROBLEM {
        return Err("Status ist bereits auf dem Zielzustand — keine Änderung nötig.".into());
    }

    let now = Local::now().format("%Y-%m-%dT%H:%M:%S%.f").to_string();
    let log_entry = json!({
        "at": now,
        "action": action,
        "from": snapshot_to_json(&before),
        "to": snapshot_to_json(&after),
        "reason": reason.trim(),
        "triggered_by": "manual",
    });

    let mut status_change_log = entry
        .get("status_change_log")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    status_change_log.insert(0, log_entry);

    updates.insert("manual_status_override".into(), Value::Bool(true));
    updates.insert("manual_status_at".into(), Value::String(now.clone()));
    updates.insert(
        "manual_status_action".into(),
        Value::String(action.to_string()),
    );
    if !reason.trim().is_empty() {
        updates.insert(
            "manual_status_note".into(),
            Value::String(reason.trim().to_string()),
        );
    }

    if has_phone(entry) && updates.contains_key("sms_status") {
        updates.insert("sms_status_locked".into(), Value::Bool(true));
    }

    updates.insert("status_change_log".into(), Value::Array(status_change_log));
    Ok(Value::Object(updates))
}

#[allow(dead_code)]
pub fn format_manual_status_summary(entry: &Value) -> String {
    let override_flag = match entry.get("manual_status_override") {
        Some(Value::Bool(true)) => true,
        Some(Value::String(s)) => matches!(s.to_lowercase().as_str(), "1" | "true" | "yes"),
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        _ => false,
    };
    if !override_flag {
        return "—".into();
    }
    let action = json_str(entry, "manual_status_action").trim();
    let action = if action.is_empty() { "Manuell" } else { action };
    let at_raw = json_str(entry, "manual_status_at").trim();
    let at_display = if at_raw.is_empty() {
        "—".to_string()
    } else {
        at_raw.replace('T', " ").chars().take(16).collect()
    };
    let note = json_str(entry, "manual_status_note").trim();
    if note.is_empty() {
        format!("{action} ({at_display})")
    } else {
        format!("{action} ({at_display}) — {note}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use serde_json::json;

    fn merge(entry: &Value, payload: Value) -> Value {
        let mut merged = entry.clone();
        if let (Some(obj), Some(patch)) = (merged.as_object_mut(), payload.as_object()) {
            for (k, v) in patch {
                obj.insert(k.clone(), v.clone());
            }
        }
        merged
    }

    #[test]
    fn test_mark_complete_sets_komplett() {
        let entry = json!({
            "dir_name": "test_dir",
            "status": "Erfolgreich",
            "email_status": "Gesendet",
            "email": "a@b.de",
            "phone": "016099501966",
            "sms_status": "Gesendet",
        });
        let payload =
            build_manual_status_update(&entry, ACTION_MARK_COMPLETE, "Kunde bestätigt").unwrap();
        let merged = merge(&entry, payload);
        assert_eq!(json_str(&merged, "sms_status"), "Zugestellt");
        assert_eq!(merged.get("manual_status_override"), Some(&json!(true)));
        assert_eq!(merged.get("sms_status_locked"), Some(&json!(true)));
        assert_eq!(
            merged
                .get("status_change_log")
                .and_then(Value::as_array)
                .map(|a| a.len()),
            Some(1)
        );
        assert_eq!(build_overall_status(&merged), "Komplett");
    }

    #[test]
    fn test_mark_sent_sets_versendet() {
        let entry = json!({
            "dir_name": "test_dir",
            "status": "Erfolgreich",
            "email_status": "",
            "email": "a@b.de",
            "phone": "016099501966",
            "sms_status": "",
            "last_updated": Local::now().format("%Y-%m-%dT%H:%M:%S%.f").to_string(),
        });
        let payload = build_manual_status_update(&entry, ACTION_MARK_SENT, "").unwrap();
        let merged = merge(&entry, payload);
        assert_eq!(json_str(&merged, "email_status"), "Gesendet");
        assert_eq!(json_str(&merged, "sms_status"), "Gesendet");
        assert_eq!(build_overall_status(&merged), "Versendet");
    }

    #[test]
    fn test_resolve_problem_fixes_failed_channels() {
        let entry = json!({
            "dir_name": "test_dir",
            "status": "Fehler",
            "error_msg": "Timeout",
            "email_status": "Fehler: Versand fehlgeschlagen",
            "email": "a@b.de",
            "phone": "016099501966",
            "sms_status": "Fehlgeschlagen",
        });
        let payload = build_manual_status_update(&entry, ACTION_RESOLVE_PROBLEM, "").unwrap();
        let merged = merge(&entry, payload);
        assert_eq!(json_str(&merged, "status"), "Erfolgreich");
        assert_eq!(json_str(&merged, "error_msg"), "");
        assert_eq!(json_str(&merged, "email_status"), "Gesendet");
        assert_eq!(json_str(&merged, "sms_status"), "Zugestellt");
        assert_eq!(build_overall_status(&merged), "Komplett");
    }

    #[test]
    fn test_collect_warnings_for_missing_share_link() {
        let entry = json!({
            "dir_name": "x",
            "status": "Erfolgreich",
            "email": "a@b.de",
        });
        let warnings = collect_manual_status_warnings(&entry, ACTION_MARK_COMPLETE);
        assert!(warnings.iter().any(|w| w.contains("Download-Link")));
    }

    #[test]
    fn rejects_unknown_action_and_noop() {
        let entry = json!({"dir_name": "x", "status": "Erfolgreich"});
        assert!(build_manual_status_update(&entry, "Nope", "").is_err());
        let complete = json!({
            "dir_name": "x",
            "status": "Erfolgreich",
            "email": "a@b.de",
            "email_status": "Gesendet",
        });
        let err = build_manual_status_update(&complete, ACTION_MARK_COMPLETE, "").unwrap_err();
        assert!(err.contains("bereits"));
    }
}
