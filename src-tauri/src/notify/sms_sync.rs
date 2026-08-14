//! Match upload history against the seven.io outbound journal.
//! Port of legacy `core/sms_history_sync.py`.

use chrono::Local;
use serde_json::Value;

use crate::events;
use crate::model::history_status::{
    history_entry_needs_sms_journal_check, parse_iso_timestamp, phones_match,
    translate_sms_dlr_status,
};
use crate::notify::sms;
use crate::storage::history::HistoryState;
use crate::storage::logging;

pub const JOURNAL_MATCH_WINDOW_SEC: f64 = 86_400.0;

fn json_str<'a>(item: &'a Value, key: &str) -> &'a str {
    item.get(key).and_then(Value::as_str).unwrap_or("")
}

fn journal_message_timestamp(msg: &Value) -> Option<f64> {
    for key in ["timestamp", "dlr_timestamp", "time", "status_time"] {
        if let Some(ts) = parse_iso_timestamp(msg.get(key).and_then(Value::as_str)) {
            return Some(ts);
        }
    }
    None
}

fn history_sms_reference_timestamp(item: &Value) -> Option<f64> {
    for key in ["last_sms_resent_at", "last_updated", "created_at"] {
        if let Some(ts) = parse_iso_timestamp(item.get(key).and_then(Value::as_str)) {
            return Some(ts);
        }
    }
    None
}

fn journal_recipient(msg: &Value) -> String {
    for key in ["to", "recipient", "system"] {
        if let Some(value) = msg.get(key) {
            if let Some(s) = value.as_str() {
                if !s.is_empty() {
                    return s.to_string();
                }
            } else if !value.is_null() {
                return value.to_string().trim_matches('"').to_string();
            }
        }
    }
    String::new()
}

fn json_id(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub fn extract_journal_entries(journal_data: &Value) -> Vec<Value> {
    match journal_data {
        Value::Array(items) => items.clone(),
        Value::Object(obj) => {
            for key in ["messages", "items", "data", "entries"] {
                if let Some(Value::Array(items)) = obj.get(key) {
                    return items.clone();
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

pub fn match_history_entry_to_journal<'a>(
    item: &Value,
    journal_data: &'a [Value],
) -> Option<&'a Value> {
    let mut sms_id = json_str(item, "sms_id").trim().to_string();
    if matches!(sms_id.to_lowercase().as_str(), "none" | "null" | "nan") {
        sms_id.clear();
    }

    if !sms_id.is_empty() {
        for msg in journal_data {
            if let Some(id) = msg.get("id").and_then(json_id) {
                if id == sms_id {
                    return Some(msg);
                }
            }
        }
    }

    let phone = json_str(item, "phone").trim();
    if phone.is_empty() {
        return None;
    }

    let ref_ts = history_sms_reference_timestamp(item);
    let mut candidates: Vec<(f64, &Value)> = Vec::new();
    for msg in journal_data {
        if !phones_match(Some(phone), Some(&journal_recipient(msg))) {
            continue;
        }
        let msg_ts = journal_message_timestamp(msg);
        if let (Some(ref_ts), Some(msg_ts)) = (ref_ts, msg_ts) {
            let delta = (msg_ts - ref_ts).abs();
            if delta > JOURNAL_MATCH_WINDOW_SEC {
                continue;
            }
            candidates.push((delta, msg));
        } else {
            candidates.push((0.0, msg));
        }
    }

    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    Some(candidates[0].1)
}

pub fn apply_journal_message_to_item(item: &mut Value, matched_msg: &Value) -> bool {
    if item
        .get("sms_status_locked")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return false;
    }

    let status_raw = matched_msg
        .get("dlr")
        .or_else(|| matched_msg.get("state"))
        .or_else(|| matched_msg.get("status"))
        .and_then(Value::as_str);
    let translated_status = translate_sms_dlr_status(status_raw);
    let price = matched_msg.get("price");

    let mut changed = false;
    if !translated_status.is_empty() && json_str(item, "sms_status") != translated_status {
        if let Some(obj) = item.as_object_mut() {
            obj.insert("sms_status".into(), Value::String(translated_status));
        }
        changed = true;
    }

    if let Some(price) = price {
        let meaningful = match price {
            Value::Null => false,
            Value::Bool(false) => false,
            Value::String(s) if s.is_empty() => false,
            Value::Number(n) if n.as_f64() == Some(0.0) => false,
            _ => true,
        };
        if meaningful && item.get("sms_price") != Some(price) {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("sms_price".into(), price.clone());
            }
            changed = true;
        }
    }

    if let Some(msg_id) = matched_msg.get("id").and_then(json_id) {
        if json_str(item, "sms_id").is_empty() {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("sms_id".into(), Value::String(msg_id));
            }
            changed = true;
        }
    }

    if changed {
        if let Some(obj) = item.as_object_mut() {
            obj.insert(
                "last_updated".into(),
                Value::String(Local::now().format("%Y-%m-%dT%H:%M:%S%.f").to_string()),
            );
        }
    }
    changed
}

pub fn update_history_from_journal(history: &mut [Value], journal_data: &[Value]) -> Vec<Value> {
    let mut updated_items = Vec::new();
    for item in history.iter_mut() {
        if !history_entry_needs_sms_journal_check(item) {
            continue;
        }
        let Some(matched) = match_history_entry_to_journal(item, journal_data) else {
            continue;
        };
        let matched = matched.clone();
        if apply_journal_message_to_item(item, &matched) {
            updated_items.push(item.clone());
        }
    }
    updated_items
}

/// Fetch the journal and persist matching DLR updates. Returns the number of changed entries.
pub async fn sync_history_with_journal(history: &HistoryState) -> Result<usize, String> {
    let mut entries: Vec<Value> = history
        .all_entries()?
        .into_iter()
        .map(|e| e.to_json())
        .collect();
    if !entries
        .iter()
        .any(history_entry_needs_sms_journal_check)
    {
        return Ok(0);
    }

    let Some(journal_data) = sms::get_sms_journal(200).await else {
        return Ok(0);
    };
    let journal_entries = extract_journal_entries(&journal_data);
    if journal_entries.is_empty() {
        return Ok(0);
    }

    let updated = update_history_from_journal(&mut entries, &journal_entries);
    for item in &updated {
        events::emit(events::UPLOAD_HISTORY_UPDATE, item);
    }
    if !updated.is_empty() {
        logging::log_info(&format!(
            "SMS-Journal: {} Historieneinträge aktualisiert.",
            updated.len()
        ));
    }
    Ok(updated.len())
}

pub fn history_needs_sms_journal_check(history: &HistoryState) -> bool {
    history
        .all_entries()
        .ok()
        .map(|entries| {
            entries
                .iter()
                .any(|e| history_entry_needs_sms_journal_check(&e.to_json()))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use serde_json::json;

    fn iso_now() -> String {
        Local::now().format("%Y-%m-%dT%H:%M:%S%.f").to_string()
    }

    #[test]
    fn test_match_history_entry_by_phone_and_time() {
        let ref_ts = iso_now();
        let item = json!({
            "phone": "016099501966",
            "sms_status": "Gesendet",
            "last_updated": ref_ts,
        });
        let journal_ts = ref_ts.replace('T', " ").chars().take(19).collect::<String>();
        let journal = vec![json!({
            "id": "77123456789",
            "to": "4916099501966",
            "timestamp": journal_ts,
            "dlr": "DELIVERED",
            "price": "0.0750",
        })];
        let matched = match_history_entry_to_journal(&item, &journal);
        assert!(matched.is_some());
        assert_eq!(
            matched.unwrap().get("id").and_then(Value::as_str),
            Some("77123456789")
        );
    }

    #[test]
    fn test_update_history_from_journal_sets_zugestellt() {
        let ref_ts = iso_now();
        let mut item = json!({
            "dir_name": "test_dir",
            "phone": "016099501966",
            "sms_status": "Gesendet",
            "last_updated": ref_ts,
        });
        let journal_ts = ref_ts.replace('T', " ").chars().take(19).collect::<String>();
        let journal = vec![json!({
            "id": "77123456789",
            "to": "4916099501966",
            "timestamp": journal_ts,
            "dlr": "DELIVERED",
        })];
        let updated = update_history_from_journal(std::slice::from_mut(&mut item), &journal);
        assert_eq!(updated.len(), 1);
        assert_eq!(json_str(&item, "sms_status"), "Zugestellt");
        assert_eq!(json_str(&item, "sms_id"), "77123456789");
    }

    #[test]
    fn test_sms_journal_respects_locked_status() {
        let mut item = json!({
            "phone": "016099501966",
            "sms_status": "Zugestellt",
            "sms_status_locked": true,
            "last_updated": iso_now(),
        });
        assert!(!history_entry_needs_sms_journal_check(&item));
        let changed = apply_journal_message_to_item(
            &mut item,
            &json!({"dlr": "TRANSMITTED", "id": "123"}),
        );
        assert!(!changed);
        assert_eq!(json_str(&item, "sms_status"), "Zugestellt");
    }

    #[test]
    fn extract_journal_unwraps_nested_lists() {
        let data = json!({"messages": [{"id": "1"}]});
        assert_eq!(extract_journal_entries(&data).len(), 1);
        assert!(extract_journal_entries(&json!({"ok": true})).is_empty());
    }

    #[test]
    fn match_prefers_sms_id() {
        let item = json!({"sms_id": "abc", "phone": "0160"});
        let journal = vec![
            json!({"id": "other", "to": "0160"}),
            json!({"id": "abc", "to": "000"}),
        ];
        let matched = match_history_entry_to_journal(&item, &journal).unwrap();
        assert_eq!(matched.get("id").and_then(Value::as_str), Some("abc"));
    }
}
