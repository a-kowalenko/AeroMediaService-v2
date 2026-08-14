//! Customer domain model (port of legacy `models/kunde.py`).
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

const INVALID_PHONE_VALUES: [&str; 3] = ["none", "null", "nan"];

/// Returns a trimmed phone number, or `None` for missing / placeholder values.
///
/// Matches legacy `normalize_phone`: empty, `none`, `null`, and `nan`
/// (case-insensitive) are treated as absent.
pub fn normalize_phone(value: Option<&str>) -> Option<String> {
    let s = value?.trim();
    if s.is_empty() {
        return None;
    }
    if INVALID_PHONE_VALUES.contains(&s.to_lowercase().as_str()) {
        return None;
    }
    Some(s.to_string())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Kunde {
    pub customer_number: Option<String>,
    pub booking_number: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    #[serde(rename = "type")]
    pub customer_type: Option<String>,
    pub handcam_foto: bool,
    pub handcam_video: bool,
    pub outside_foto: bool,
    pub outside_video: bool,
    pub ist_bezahlt_handcam_foto: bool,
    pub ist_bezahlt_handcam_video: bool,
    pub ist_bezahlt_outside_foto: bool,
    pub ist_bezahlt_outside_video: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_phone_rejects_placeholders() {
        assert_eq!(normalize_phone(None), None);
        assert_eq!(normalize_phone(Some("")), None);
        assert_eq!(normalize_phone(Some("   ")), None);
        assert_eq!(normalize_phone(Some("None")), None);
        assert_eq!(normalize_phone(Some("null")), None);
        assert_eq!(normalize_phone(Some("NAN")), None);
        assert_eq!(normalize_phone(Some(" None ")), None);
    }

    #[test]
    fn normalize_phone_keeps_real_numbers() {
        assert_eq!(
            normalize_phone(Some("016099501966")),
            Some("016099501966".into())
        );
        assert_eq!(
            normalize_phone(Some("  +49 160 99501966  ")),
            Some("+49 160 99501966".into())
        );
    }

    #[test]
    fn kunde_serializes_type_field() {
        let k = Kunde {
            customer_type: Some("Outside".into()),
            ..Kunde::default()
        };
        let v = serde_json::to_value(&k).unwrap();
        assert_eq!(v["type"], "Outside");
        assert!(v.get("customer_type").is_none());
    }
}
