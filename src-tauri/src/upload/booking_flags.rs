//! Booking-option lookup: last-known history flags, with optional customer-API refresh.
//!
//! Policies:
//! - `cache` — never network
//! - `auto` — API when lookup is possible and TTL elapsed
//! - `force` — API regardless of TTL

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use tokio::sync::Mutex as AsyncMutex;

use crate::cloud::custom_api::fetch_customer_as_kunde;
use crate::events;
use crate::model::kunde::{normalize_phone, Kunde};
use crate::model::marker::{
    apply_media_flags_from_json, apply_media_flags_if_present, has_api_lookup_fields,
    load_marker_data, merge_kunde_media_flags, normalize_marker_type, parse_api_marker_data,
    resolve_kunde_from_marker, ApiMarkerQuery, LookupMode, MarkerError,
};
use crate::storage::logging;

pub const BOOKING_FLAGS_TTL_SECS: i64 = 10 * 60;
pub const BOOKING_FLAGS_UPDATED_AT: &str = "booking_flags_updated_at";

static INFLIGHT: Lazy<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookingFlagsPolicy {
    Cache,
    Auto,
    Force,
}

impl BookingFlagsPolicy {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("cache") => Self::Cache,
            Some("force") => Self::Force,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BookingFlagsResolve {
    pub kunde: Kunde,
    pub lookup: &'static str,
    pub updated_at: Option<String>,
    pub can_refresh: bool,
    pub persisted: bool,
}

fn json_str<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry.get(key).and_then(Value::as_str).unwrap_or("")
}

fn nonempty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn inflight_key(entry: &Value) -> String {
    let id = json_str(entry, "id").trim();
    if !id.is_empty() {
        return id.to_string();
    }
    json_str(entry, "dir_name").trim().to_string()
}

async fn with_inflight<T, F, Fut>(key: &str, f: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    if key.is_empty() {
        return f().await;
    }
    let lock = {
        let mut map = match INFLIGHT.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        map.entry(key.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    };
    let _guard = lock.lock().await;
    f().await
}

pub fn kunde_from_history_fields(entry: &Value) -> Option<Kunde> {
    let first = json_str(entry, "first_name").trim();
    let last = json_str(entry, "last_name").trim();
    let email = json_str(entry, "email").trim();
    if first.is_empty() || last.is_empty() || email.is_empty() {
        return None;
    }
    let mut kunde = Kunde {
        first_name: Some(first.to_string()),
        last_name: Some(last.to_string()),
        email: Some(email.to_string()),
        phone: normalize_phone(Some(json_str(entry, "phone"))),
        customer_number: nonempty(json_str(entry, "customer_number")),
        booking_number: nonempty(json_str(entry, "booking_number")),
        customer_type: nonempty(json_str(entry, "type")),
        ..Kunde::default()
    };
    apply_media_flags_from_json(&mut kunde, entry);
    Some(kunde)
}

pub fn cached_kunde(entry: &Value) -> Kunde {
    kunde_from_history_fields(entry).unwrap_or_else(|| {
        let mut kunde = Kunde::default();
        apply_media_flags_from_json(&mut kunde, entry);
        kunde.customer_number = nonempty(json_str(entry, "customer_number"));
        kunde.booking_number = nonempty(json_str(entry, "booking_number"));
        kunde.customer_type = nonempty(json_str(entry, "type"));
        kunde
    })
}

fn usable_lookup_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower == "none" || lower == "null" {
        return None;
    }
    Some(trimmed.to_string())
}

fn lookup_query_from_history_ids(entry: &Value) -> Option<(ApiMarkerQuery, LookupMode)> {
    let customer_id = usable_lookup_id(json_str(entry, "customer_number"))?;
    let booking_id = usable_lookup_id(json_str(entry, "booking_number"))?;
    let marker_type = normalize_marker_type(Some(json_str(entry, "type")));
    if marker_type.is_empty() {
        return None;
    }
    Some((
        ApiMarkerQuery {
            customer_id,
            booking_id,
            marker_type,
        },
        LookupMode::Id,
    ))
}

fn lookup_query_from_marker(entry: &Value) -> Option<(ApiMarkerQuery, LookupMode)> {
    let marker_raw = json_str(entry, "marker_raw").trim();
    if marker_raw.is_empty() {
        return None;
    }
    let data = load_marker_data(marker_raw).ok()?;
    if !has_api_lookup_fields(&data) {
        return None;
    }
    let (query, mode) = parse_api_marker_data(&data).ok()?;
    let customer_id = usable_lookup_id(&query.customer_id)?;
    let booking_id = usable_lookup_id(&query.booking_id)?;
    Some((
        ApiMarkerQuery {
            customer_id,
            booking_id,
            marker_type: query.marker_type,
        },
        mode,
    ))
}

pub fn lookup_query_from_entry(entry: &Value) -> Option<(ApiMarkerQuery, LookupMode)> {
    lookup_query_from_marker(entry).or_else(|| lookup_query_from_history_ids(entry))
}

pub fn can_refresh_booking_flags(entry: &Value) -> bool {
    lookup_query_from_entry(entry).is_some()
}

pub fn parse_flags_updated_at(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw.trim())
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

pub fn booking_flags_ttl_fresh(entry: &Value, now: DateTime<Utc>) -> bool {
    let raw = json_str(entry, BOOKING_FLAGS_UPDATED_AT).trim();
    let Some(ts) = parse_flags_updated_at(raw) else {
        return false;
    };
    let age = now.signed_duration_since(ts);
    age >= Duration::zero() && age < Duration::seconds(BOOKING_FLAGS_TTL_SECS)
}

fn fill_contact_if_empty(target: &mut Kunde, cached: &Kunde) {
    if target
        .first_name
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        target.first_name = cached.first_name.clone();
    }
    if target
        .last_name
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        target.last_name = cached.last_name.clone();
    }
    if target
        .email
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        target.email = cached.email.clone();
    }
    if target
        .phone
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        target.phone = cached.phone.clone();
    }
    if target
        .customer_number
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        target.customer_number = cached.customer_number.clone();
    }
    if target
        .booking_number
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        target.booking_number = cached.booking_number.clone();
    }
    if target
        .customer_type
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        target.customer_type = cached.customer_type.clone();
    }
}

/// API flags replace cache/marker. Contact fields fall back to history if the API omits them.
pub fn apply_api_over_cache(cached: Option<&Kunde>, mut api: Kunde) -> Kunde {
    if let Some(cached) = cached {
        fill_contact_if_empty(&mut api, cached);
    }
    api
}

fn cache_with_marker_overlay(entry: &Value) -> Kunde {
    let cached = cached_kunde(entry);
    let marker_raw = json_str(entry, "marker_raw").trim();
    if marker_raw.is_empty() {
        return cached;
    }
    match resolve_kunde_from_marker(marker_raw) {
        Ok(from_marker) => apply_api_over_cache(Some(&cached), from_marker),
        Err(MarkerError::ApiLookupRequired) => {
            let mut overlay = cached.clone();
            if let Ok(data) = load_marker_data(marker_raw) {
                apply_media_flags_if_present(&mut overlay, &data);
            }
            overlay
        }
        Err(_) => cached,
    }
}

fn lookup_error_message(exc: &str) -> String {
    if exc.contains("Customer-Lookup fehlgeschlagen") {
        format!(
            "Kundendaten konnten nicht von der API geladen werden:\n{exc}\n\n\
             Bitte Marker-IDs prüfen oder den API-Fehler beheben."
        )
    } else {
        format!("Buchungsoptionen konnten nicht aktualisiert werden: {exc}")
    }
}

fn persist_history_media_flags(entry: &Value, kunde: &Kunde, stamp: &str) -> bool {
    let dir_name = json_str(entry, "dir_name").trim();
    if dir_name.is_empty() {
        return false;
    }
    let mut payload = json!({ "dir_name": dir_name });
    merge_kunde_media_flags(&mut payload, kunde);
    payload[BOOKING_FLAGS_UPDATED_AT] = Value::String(stamp.to_string());
    events::emit(events::UPLOAD_HISTORY_UPDATE, payload);
    true
}

async fn fetch_api_kunde(entry: &Value) -> Result<Kunde, String> {
    let marker_query = lookup_query_from_marker(entry);
    let id_query = lookup_query_from_history_ids(entry);
    let primary = marker_query.clone().or_else(|| id_query.clone());
    let Some((query, mode)) = primary else {
        return Err("Kein API-Lookup möglich.".into());
    };
    match fetch_customer_as_kunde(&query, mode).await {
        Ok(kunde) => Ok(kunde),
        Err(err) => {
            if mode == LookupMode::Hash {
                if let Some((fallback, LookupMode::Id)) = id_query {
                    if fallback.customer_id != query.customer_id
                        || fallback.booking_id != query.booking_id
                    {
                        logging::log_warn(&format!(
                            "Hash-Lookup fehlgeschlagen, versuche ID-Lookup: {err}"
                        ));
                        return fetch_customer_as_kunde(&fallback, LookupMode::Id).await;
                    }
                }
            }
            Err(err)
        }
    }
}

async fn resolve_booking_flags_inner(
    entry: &Value,
    policy: BookingFlagsPolicy,
) -> Result<BookingFlagsResolve, String> {
    let can_refresh = can_refresh_booking_flags(entry);
    let cached = cache_with_marker_overlay(entry);
    let updated_at = nonempty(json_str(entry, BOOKING_FLAGS_UPDATED_AT));

    let use_network = match policy {
        BookingFlagsPolicy::Cache => false,
        BookingFlagsPolicy::Auto | BookingFlagsPolicy::Force => can_refresh,
    };

    if !use_network {
        return Ok(BookingFlagsResolve {
            kunde: cached,
            lookup: if can_refresh { "cache" } else { "skipped" },
            updated_at,
            can_refresh,
            persisted: false,
        });
    }

    match fetch_api_kunde(entry).await {
        Ok(api) => {
            let kunde = apply_api_over_cache(Some(&cached), api);
            let stamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            logging::log_info(&format!(
                "Buchungsoptionen von API: HV={} paid={} HF={} paid={} OV={} paid={} OF={} paid={}",
                kunde.handcam_video,
                kunde.ist_bezahlt_handcam_video,
                kunde.handcam_foto,
                kunde.ist_bezahlt_handcam_foto,
                kunde.outside_video,
                kunde.ist_bezahlt_outside_video,
                kunde.outside_foto,
                kunde.ist_bezahlt_outside_foto,
            ));
            let persisted = persist_history_media_flags(entry, &kunde, &stamp);
            Ok(BookingFlagsResolve {
                kunde,
                lookup: "api",
                updated_at: Some(stamp),
                can_refresh,
                persisted,
            })
        }
        Err(err) => {
            if policy == BookingFlagsPolicy::Force {
                return Err(lookup_error_message(&err));
            }
            logging::log_warn(&format!(
                "Buchungsstatus nicht nachgeladen, nutze Historiendaten: {err}"
            ));
            Ok(BookingFlagsResolve {
                kunde: cached,
                lookup: "cache",
                updated_at,
                can_refresh,
                persisted: false,
            })
        }
    }
}

pub async fn resolve_booking_flags(
    entry: &Value,
    policy: BookingFlagsPolicy,
) -> Result<BookingFlagsResolve, String> {
    let key = inflight_key(entry);
    with_inflight(&key, || resolve_booking_flags_inner(entry, policy)).await
}

/// Like `resolve_booking_flags`, but the caller already holds `with_booking_flags_lock`.
pub async fn resolve_booking_flags_unlocked(
    entry: &Value,
    policy: BookingFlagsPolicy,
) -> Result<BookingFlagsResolve, String> {
    resolve_booking_flags_inner(entry, policy).await
}

/// Used by the Tauri command so a waiting Force/Auto reloads after the previous persist.
pub async fn with_booking_flags_lock<T, F, Fut>(key: &str, f: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    with_inflight(key, f).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn policy_parse_defaults_auto() {
        assert_eq!(BookingFlagsPolicy::parse(None), BookingFlagsPolicy::Auto);
        assert_eq!(
            BookingFlagsPolicy::parse(Some("FORCE")),
            BookingFlagsPolicy::Force
        );
        assert_eq!(
            BookingFlagsPolicy::parse(Some("cache")),
            BookingFlagsPolicy::Cache
        );
    }

    #[test]
    fn api_flags_replace_cache() {
        let cached = Kunde {
            handcam_video: true,
            ist_bezahlt_handcam_video: false,
            outside_foto: true,
            first_name: Some("Ada".into()),
            ..Kunde::default()
        };
        let api = Kunde {
            handcam_video: true,
            ist_bezahlt_handcam_video: true,
            outside_foto: false,
            ..Kunde::default()
        };
        let merged = apply_api_over_cache(Some(&cached), api);
        assert!(merged.ist_bezahlt_handcam_video);
        assert!(!merged.outside_foto);
        assert_eq!(merged.first_name.as_deref(), Some("Ada"));
    }

    #[test]
    fn ttl_fresh_within_window() {
        let now = Utc::now();
        let stamp = (now - Duration::seconds(60)).to_rfc3339_opts(SecondsFormat::Secs, true);
        let entry = json!({ BOOKING_FLAGS_UPDATED_AT: stamp });
        assert!(booking_flags_ttl_fresh(&entry, now));
        let old = (now - Duration::seconds(BOOKING_FLAGS_TTL_SECS + 1))
            .to_rfc3339_opts(SecondsFormat::Secs, true);
        let stale = json!({ BOOKING_FLAGS_UPDATED_AT: old });
        assert!(!booking_flags_ttl_fresh(&stale, now));
        assert!(!booking_flags_ttl_fresh(&json!({}), now));
    }

    #[test]
    fn lookup_from_marker_hash() {
        let q = lookup_query_from_entry(&json!({
            "marker_raw": "{\"type\":\"Handcam\",\"kunden_id_hash\":\"a\",\"booking_id_hash\":\"b\"}",
        }))
        .unwrap();
        assert_eq!(q.1, LookupMode::Hash);
        assert_eq!(q.0.customer_id, "a");
    }

    #[test]
    fn lookup_from_history_ids() {
        let q = lookup_query_from_entry(&json!({
            "customer_number": "12",
            "booking_number": "34",
            "type": "Outside",
        }))
        .unwrap();
        assert_eq!(q.1, LookupMode::Id);
        assert_eq!(q.0.customer_id, "12");
        assert_eq!(q.0.booking_id, "34");
    }

    #[test]
    fn no_lookup_for_pure_contact() {
        assert!(lookup_query_from_entry(&json!({
            "marker_raw": "{\"vorname\":\"Ada\",\"nachname\":\"Lovelace\",\"email\":\"ada@example.de\"}",
        }))
        .is_none());
        assert!(!can_refresh_booking_flags(&json!({
            "first_name": "Ada",
            "last_name": "Lovelace",
            "email": "ada@example.de",
        })));
    }

    #[test]
    fn empty_marker_hashes_fall_back_to_history_ids() {
        let q = lookup_query_from_entry(&json!({
            "customer_number": "12",
            "booking_number": "34",
            "type": "Outside",
            "marker_raw": "{\"type\":\"Outside\",\"kunden_id_hash\":null,\"booking_id_hash\":null}",
        }))
        .unwrap();
        assert_eq!(q.1, LookupMode::Id);
        assert_eq!(q.0.customer_id, "12");
    }

    #[tokio::test]
    async fn cache_policy_keeps_persisted_unpaid() {
        let resolved = resolve_booking_flags(
            &json!({
                "first_name": "Ada",
                "last_name": "Lovelace",
                "email": "ada@example.de",
                "handcam_video": true,
                "ist_bezahlt_handcam_video": false,
                "marker_raw": "{\"type\":\"Handcam\",\"kunden_id_hash\":\"a\",\"booking_id_hash\":\"b\"}",
            }),
            BookingFlagsPolicy::Cache,
        )
        .await
        .unwrap();
        assert!(resolved.kunde.handcam_video);
        assert!(!resolved.kunde.ist_bezahlt_handcam_video);
        assert_eq!(resolved.lookup, "cache");
        assert!(resolved.can_refresh);
        assert!(!resolved.persisted);
    }

    #[tokio::test]
    async fn auto_falls_back_to_cache_when_api_unavailable() {
        let stamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let resolved = resolve_booking_flags(
            &json!({
                "first_name": "Ada",
                "last_name": "Lovelace",
                "email": "ada@example.de",
                "handcam_video": true,
                "ist_bezahlt_handcam_video": false,
                "booking_flags_updated_at": stamp,
                "marker_raw": "{\"type\":\"Handcam\",\"kunden_id_hash\":\"a\",\"booking_id_hash\":\"b\"}",
            }),
            BookingFlagsPolicy::Auto,
        )
        .await
        .unwrap();
        assert!(!resolved.kunde.ist_bezahlt_handcam_video);
        assert_eq!(resolved.lookup, "cache");
        assert!(!resolved.persisted);
    }

    #[tokio::test]
    async fn contact_marker_reads_flags_without_api() {
        let resolved = resolve_booking_flags(
            &json!({
                "dir_name": "Flug_1",
                "first_name": "Ada",
                "last_name": "Lovelace",
                "email": "ada@example.de",
                "marker_raw": "{\"vorname\":\"Ada\",\"nachname\":\"Lovelace\",\"email\":\"ada@example.de\",\"outside_foto\":true,\"ist_bezahlt_outside_foto\":false}",
            }),
            BookingFlagsPolicy::Auto,
        )
        .await
        .unwrap();
        assert!(resolved.kunde.outside_foto);
        assert!(!resolved.kunde.ist_bezahlt_outside_foto);
        assert_eq!(resolved.lookup, "skipped");
        assert!(!resolved.can_refresh);
    }
}
