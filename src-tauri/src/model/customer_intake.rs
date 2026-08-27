//! Kundenaufnahme ID-Lookup helpers (Phase 19b).
//! Dual Handcam/Outside lookup + Diff-Felder; Assign-Umbau bleibt 19c.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::model::kunde::Kunde;
use crate::model::marker::normalize_marker_type;

pub const LOOKUP_MIN_ID_DIGITS: usize = 4;

pub const INTAKE_LOOKUP_TYPES: &[&str] = &["Handcam", "Outside"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IntakeLookupHit {
    pub vorname: String,
    pub nachname: String,
    pub email: String,
    pub telefon: String,
    pub kunden_id: String,
    pub booking_id: String,
    pub booking_date: String,
    pub typ: String,
    pub handcam_foto: bool,
    pub handcam_video: bool,
    pub outside_foto: bool,
    pub outside_video: bool,
    pub ist_bezahlt_handcam_foto: bool,
    pub ist_bezahlt_handcam_video: bool,
    pub ist_bezahlt_outside_foto: bool,
    pub ist_bezahlt_outside_video: bool,
    pub media_option: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IntakeLookupResult {
    Hit { customer: IntakeLookupHit },
    Choice {
        handcam: IntakeLookupHit,
        outside: IntakeLookupHit,
    },
    NotFound,
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeFieldDiff {
    pub field: String,
    pub label: String,
    pub form_value: String,
    pub api_value: String,
}

pub fn is_lookup_id_ready(id: &str) -> bool {
    let t = id.trim();
    t.len() >= LOOKUP_MIN_ID_DIGITS && t.chars().all(|c| c.is_ascii_digit())
}

pub fn is_lookup_id_pair_ready(kunden_id: &str, booking_id: &str) -> bool {
    is_lookup_id_ready(kunden_id) && is_lookup_id_ready(booking_id)
}

/// Beide IDs oder keine — Speichern nur mit konsistentem Paar.
pub fn normalize_id_pair(kunden_id: &str, booking_id: &str) -> Result<(String, String), String> {
    let k = kunden_id.trim().to_string();
    let b = booking_id.trim().to_string();
    if k.is_empty() && b.is_empty() {
        return Ok((String::new(), String::new()));
    }
    if k.is_empty() || b.is_empty() {
        return Err(
            "Kunden-ID und Buchungs-ID müssen beide gesetzt sein oder beide leer bleiben.".into(),
        );
    }
    Ok((k, b))
}

pub fn has_media_flags(hit: &IntakeLookupHit) -> bool {
    hit.handcam_foto || hit.handcam_video || hit.outside_foto || hit.outside_video
}

pub fn hit_from_kunde(
    kunde: &Kunde,
    kunden_id: &str,
    booking_id: &str,
    booking_date: &str,
    media_option: &str,
) -> IntakeLookupHit {
    let typ = kunde
        .customer_type
        .as_deref()
        .map(|t| normalize_marker_type(Some(t)))
        .filter(|t| !t.is_empty())
        .unwrap_or_default();
    IntakeLookupHit {
        vorname: kunde.first_name.clone().unwrap_or_default(),
        nachname: kunde.last_name.clone().unwrap_or_default(),
        email: kunde.email.clone().unwrap_or_default(),
        telefon: kunde.phone.clone().unwrap_or_default(),
        kunden_id: kunden_id.trim().to_string(),
        booking_id: booking_id.trim().to_string(),
        booking_date: booking_date.trim().to_string(),
        typ,
        handcam_foto: kunde.handcam_foto,
        handcam_video: kunde.handcam_video,
        outside_foto: kunde.outside_foto,
        outside_video: kunde.outside_video,
        ist_bezahlt_handcam_foto: kunde.ist_bezahlt_handcam_foto,
        ist_bezahlt_handcam_video: kunde.ist_bezahlt_handcam_video,
        ist_bezahlt_outside_foto: kunde.ist_bezahlt_outside_foto,
        ist_bezahlt_outside_video: kunde.ist_bezahlt_outside_video,
        media_option: media_option.trim().to_string(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClassifiedIntakeHits {
    None,
    One { customer: IntakeLookupHit },
    Choice {
        handcam: IntakeLookupHit,
        outside: IntakeLookupHit,
    },
}

/// Zwei typisierte API-Antworten: Familien nicht mergen.
pub fn classify_typed_hits(
    handcam: Option<&IntakeLookupHit>,
    outside: Option<&IntakeLookupHit>,
) -> ClassifiedIntakeHits {
    let h = handcam.filter(|c| has_media_flags(c));
    let o = outside.filter(|c| has_media_flags(c));
    match (h, o) {
        (Some(a), Some(b)) => ClassifiedIntakeHits::Choice {
            handcam: a.clone(),
            outside: b.clone(),
        },
        (Some(a), None) => ClassifiedIntakeHits::One {
            customer: a.clone(),
        },
        (None, Some(b)) => ClassifiedIntakeHits::One {
            customer: b.clone(),
        },
        (None, None) => {
            // Kontakt ohne Medienflags: ersten Treffer nutzen (Outside bevorzugt).
            if let Some(b) = outside {
                return ClassifiedIntakeHits::One {
                    customer: b.clone(),
                };
            }
            if let Some(a) = handcam {
                return ClassifiedIntakeHits::One {
                    customer: a.clone(),
                };
            }
            ClassifiedIntakeHits::None
        }
    }
}

fn norm_cmp(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn contact_nonempty(value: &str) -> bool {
    !value.trim().is_empty()
}

/// Diff nur für bereits befüllte Formular-Kontaktfelder, die von der API abweichen.
pub fn contact_field_diffs(form: &IntakeLookupHit, api: &IntakeLookupHit) -> Vec<IntakeFieldDiff> {
    let mut out = Vec::new();
    let pairs = [
        ("vorname", "Vorname", &form.vorname, &api.vorname),
        ("nachname", "Nachname", &form.nachname, &api.nachname),
        ("email", "E-Mail", &form.email, &api.email),
        ("telefon", "Telefon", &form.telefon, &api.telefon),
    ];
    for (field, label, form_value, api_value) in pairs {
        if !contact_nonempty(form_value) {
            continue;
        }
        if !contact_nonempty(api_value) {
            continue;
        }
        if norm_cmp(form_value, api_value) {
            continue;
        }
        out.push(IntakeFieldDiff {
            field: field.into(),
            label: label.into(),
            form_value: form_value.trim().to_string(),
            api_value: api_value.trim().to_string(),
        });
    }
    out
}

/// Leere Kontaktfelder aus API füllen; Medienflags/IDs/Typ immer von API.
pub fn merge_lookup_into_form(form: &IntakeLookupHit, api: &IntakeLookupHit) -> IntakeLookupHit {
    IntakeLookupHit {
        vorname: if contact_nonempty(&form.vorname) {
            form.vorname.clone()
        } else {
            api.vorname.clone()
        },
        nachname: if contact_nonempty(&form.nachname) {
            form.nachname.clone()
        } else {
            api.nachname.clone()
        },
        email: if contact_nonempty(&form.email) {
            form.email.clone()
        } else {
            api.email.clone()
        },
        telefon: if contact_nonempty(&form.telefon) {
            form.telefon.clone()
        } else {
            api.telefon.clone()
        },
        kunden_id: api.kunden_id.clone(),
        booking_id: api.booking_id.clone(),
        booking_date: if contact_nonempty(&api.booking_date) {
            api.booking_date.clone()
        } else {
            form.booking_date.clone()
        },
        typ: api.typ.clone(),
        handcam_foto: api.handcam_foto,
        handcam_video: api.handcam_video,
        outside_foto: api.outside_foto,
        outside_video: api.outside_video,
        ist_bezahlt_handcam_foto: api.ist_bezahlt_handcam_foto,
        ist_bezahlt_handcam_video: api.ist_bezahlt_handcam_video,
        ist_bezahlt_outside_foto: api.ist_bezahlt_outside_foto,
        ist_bezahlt_outside_video: api.ist_bezahlt_outside_video,
        media_option: api.media_option.clone(),
    }
}

fn map_get_ci<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    if let Some(value) = obj.get(key) {
        return Some(value);
    }
    obj.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

fn value_as_trimmed_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Prefer calendar date (`YYYY-MM-DD` / `DD.MM.YYYY`); strip ISO timestamps to date part.
fn normalize_booking_date_value(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("null") {
        return None;
    }
    // ISO-8601: 2026-08-16T06:17:24... → 2026-08-16
    if t.len() >= 10 && t.as_bytes().get(4) == Some(&b'-') && t.as_bytes().get(7) == Some(&b'-') {
        let date = &t[..10];
        if date.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return Some(date.to_string());
        }
    }
    Some(t.to_string())
}

fn media_option_object<'a>(source: &'a Value) -> Option<&'a Map<String, Value>> {
    let obj = source.as_object()?;
    let mo = map_get_ci(obj, "media_option").or_else(|| map_get_ci(obj, "mediaOption"))?;
    mo.as_object()
}

/// Roh-`media_option` / Art-Code aus Customer-Objekt oder Payload-Root.
/// API liefert oft `{ "key": "ou_fv", "created_at": "...", ... }` statt eines Strings.
pub fn extract_media_option(customer: &Value, payload: &Value) -> String {
    for source in [customer, payload] {
        let Some(obj) = source.as_object() else {
            continue;
        };
        for key in [
            "media_option",
            "mediaOption",
            "media_code",
            "mediaCode",
            "art",
            "code",
        ] {
            let Some(v) = map_get_ci(obj, key) else {
                continue;
            };
            if let Some(s) = value_as_trimmed_string(v) {
                return s;
            }
            if let Some(mo) = v.as_object() {
                if let Some(code) = map_get_ci(mo, "key")
                    .or_else(|| map_get_ci(mo, "code"))
                    .or_else(|| map_get_ci(mo, "art"))
                    .and_then(value_as_trimmed_string)
                {
                    return code;
                }
            }
        }
    }
    String::new()
}

/// Buchungsdatum aus Customer-Objekt oder Payload-Root (optional).
/// Live-API: oft kein Top-Level-Datum, sondern `media_option.created_at` (ISO).
pub fn extract_booking_date(customer: &Value, payload: &Value) -> String {
    for source in [customer, payload] {
        let Some(obj) = source.as_object() else {
            continue;
        };
        for key in [
            "booking_date",
            "buchungsdatum",
            "flugdatum",
            "jump_date",
            "datum",
            "date",
        ] {
            if let Some(v) = map_get_ci(obj, key)
                .and_then(value_as_trimmed_string)
                .and_then(|s| normalize_booking_date_value(&s))
            {
                return v;
            }
        }
        // Nested under media_option object (Supabase aero-media-customer-fallback).
        if let Some(mo) = media_option_object(source) {
            for key in ["created_at", "updated_at", "datum", "date", "booking_date"] {
                if let Some(v) = map_get_ci(mo, key)
                    .and_then(value_as_trimmed_string)
                    .and_then(|s| normalize_booking_date_value(&s))
                {
                    return v;
                }
            }
        }
        // Customer-level Created_at / created_at (may be null).
        for key in ["Created_at", "created_at", "updated_at"] {
            if let Some(v) = map_get_ci(obj, key)
                .and_then(value_as_trimmed_string)
                .and_then(|s| normalize_booking_date_value(&s))
            {
                return v;
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hit_flags(handcam: bool, outside: bool) -> IntakeLookupHit {
        IntakeLookupHit {
            vorname: "Max".into(),
            nachname: "Muster".into(),
            handcam_video: handcam,
            outside_video: outside,
            ..IntakeLookupHit::default()
        }
    }

    #[test]
    fn lookup_id_ready_requires_four_digits() {
        assert!(!is_lookup_id_ready(""));
        assert!(!is_lookup_id_ready("123"));
        assert!(!is_lookup_id_ready("12ab"));
        assert!(is_lookup_id_ready("1234"));
        assert!(is_lookup_id_ready("012345"));
    }

    #[test]
    fn normalize_id_pair_both_or_neither() {
        assert_eq!(
            normalize_id_pair("", "").unwrap(),
            ("".into(), "".into())
        );
        assert_eq!(
            normalize_id_pair(" 1111 ", " 2222 ").unwrap(),
            ("1111".into(), "2222".into())
        );
        assert!(normalize_id_pair("1111", "").is_err());
        assert!(normalize_id_pair("", "2222").is_err());
    }

    #[test]
    fn classify_asks_when_both_families_have_media() {
        let h = hit_flags(true, false);
        let o = hit_flags(false, true);
        match classify_typed_hits(Some(&h), Some(&o)) {
            ClassifiedIntakeHits::Choice { .. } => {}
            other => panic!("expected choice, got {other:?}"),
        }
    }

    #[test]
    fn classify_prefers_outside_contact_without_media() {
        let h = IntakeLookupHit {
            vorname: "A".into(),
            ..IntakeLookupHit::default()
        };
        let o = IntakeLookupHit {
            vorname: "B".into(),
            ..IntakeLookupHit::default()
        };
        match classify_typed_hits(Some(&h), Some(&o)) {
            ClassifiedIntakeHits::One { customer } => assert_eq!(customer.vorname, "B"),
            other => panic!("expected one, got {other:?}"),
        }
    }

    #[test]
    fn contact_diffs_skip_empty_form_fields() {
        let form = IntakeLookupHit {
            vorname: "Anna".into(),
            email: "".into(),
            ..IntakeLookupHit::default()
        };
        let api = IntakeLookupHit {
            vorname: "Anne".into(),
            email: "a@example.com".into(),
            ..IntakeLookupHit::default()
        };
        let diffs = contact_field_diffs(&form, &api);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].field, "vorname");
    }

    #[test]
    fn merge_fills_empty_and_overwrites_flags() {
        let form = IntakeLookupHit {
            vorname: "Anna".into(),
            email: "".into(),
            handcam_foto: false,
            ..IntakeLookupHit::default()
        };
        let api = IntakeLookupHit {
            vorname: "Anne".into(),
            email: "a@example.com".into(),
            outside_video: true,
            ist_bezahlt_outside_video: true,
            typ: "Outside".into(),
            kunden_id: "1111".into(),
            booking_id: "2222".into(),
            media_option: "ou_v".into(),
            ..IntakeLookupHit::default()
        };
        let merged = merge_lookup_into_form(&form, &api);
        assert_eq!(merged.vorname, "Anna");
        assert_eq!(merged.email, "a@example.com");
        assert!(merged.outside_video);
        assert!(merged.ist_bezahlt_outside_video);
        assert_eq!(merged.typ, "Outside");
        assert_eq!(merged.media_option, "ou_v");
    }

    #[test]
    fn extract_media_option_and_date_from_payload() {
        let customer = json!({ "vorname": "A" });
        let payload = json!({
            "customer": { "vorname": "A" },
            "media_option": "ou_fv",
            "datum": "2026-08-27",
        });
        assert_eq!(extract_media_option(&customer, &payload), "ou_fv");
        assert_eq!(extract_booking_date(&customer, &payload), "2026-08-27");
    }

    #[test]
    fn extract_from_live_api_media_option_object() {
        let customer = json!({
            "vorname": "Andreas",
            "nachname": "Kowalenko",
            "email": "gsr.andy@hotmail.de",
            "telefon": "015224575366",
            "typ": "outside",
            "paid": "Video",
            "foto_paid": true,
            "video_paid": true,
            "media_option": {
                "key": "ou_fv",
                "created_at": "2026-08-16T06:17:24.370799+00:00",
                "updated_at": "2026-08-26T14:44:49.790522+00:00",
                "typ": "outside",
                "foto": false,
                "video": true
            },
            "Created_at": null
        });
        let payload = json!({ "customer": customer.clone() });
        assert_eq!(extract_media_option(&customer, &payload), "ou_fv");
        assert_eq!(extract_booking_date(&customer, &payload), "2026-08-16");

        let kunde = crate::model::marker::build_kunde_from_customer(&customer).unwrap();
        assert!(kunde.outside_video, "expected outside_video from foto_paid/video_paid+typ");
        assert!(kunde.outside_foto, "expected outside_foto from foto_paid");
        let hit = hit_from_kunde(
            &kunde,
            "3971",
            "2405",
            &extract_booking_date(&customer, &payload),
            &extract_media_option(&customer, &payload),
        );
        assert_eq!(hit.booking_date, "2026-08-16");
        assert_eq!(hit.media_option, "ou_fv");
        assert!(hit.outside_video);
        assert!(hit.outside_foto);
    }
}
