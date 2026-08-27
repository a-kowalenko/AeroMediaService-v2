//! Ordnername → TM/VS/Dropzone predictor (Phase 19a / 19e).
//!
//! Tokenisiert Original-Ordnernamen, droppt Noise, matched Crew inkl. Aliases,
//! liefert Confidence + `needs_review` für den Assign-Dialog (19d).
//! Phase 19e: Crew nur aus Post-`TA`/`TD`-Zone; Gast-Namen in der Gast-Zone
//! werden unterdrückt (kein globales Namens-Kill nach `TA`).

use serde::{Deserialize, Serialize};

use crate::model::crew::CrewMember;
use crate::storage::folder_match::{crew_segment, fold_key, guest_segment};

/// Options that affect review rules and guest-name suppression (Phase 19e).
#[derive(Debug, Clone, Default)]
pub struct PredictOptions {
    /// When true, missing VS forces `needs_review` (Outside-Video needs `_V_{VS}`).
    pub outside_video: bool,
    /// Customer first name — tokens matching this in the guest zone are not crew.
    pub guest_vorname: Option<String>,
    /// Customer last name — tokens matching this in the guest zone are not crew.
    pub guest_nachname: Option<String>,
}

/// Result of predicting TM/VS (and optional dropzone) from a folder name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FolderRenamePrediction {
    pub tandemmaster: Option<String>,
    pub videospringer: Option<String>,
    /// Dropzone letter without underscore, e.g. `"G"` for Gera → `_G` in target name.
    pub dropzone_suffix: Option<String>,
    pub tm_confidence: f32,
    pub vs_confidence: f32,
    pub needs_review: bool,
    pub review_reasons: Vec<String>,
    /// Tokens skipped because they belong to the guest / were guest-suppressed (19e).
    #[serde(default)]
    pub skipped_guest_tokens: Vec<String>,
    /// Crew hits taken only from tokens after `TA`/`TD`.
    #[serde(default)]
    pub structured_crew_zone: bool,
}

#[derive(Debug, Clone)]
struct CrewHit {
    name: String,
    tandemmaster: bool,
    videospringer: bool,
    /// 1.0 exact name, 0.9 alias.
    confidence: f32,
}

/// Predict TM / VS / dropzone from an original folder name + crew roster.
pub fn predict_from_folder_name(
    folder_name: &str,
    crew: &[CrewMember],
    options: PredictOptions,
) -> FolderRenamePrediction {
    let tokens = tokenize(folder_name);
    let structure_idx = tokens.iter().position(|t| is_structure_ta(t));
    // Underscore form (`_TA_` / `_TD_`) or CamelCase (`TACorni`) both count.
    let structured_crew_zone =
        structure_idx.is_some() || crew_segment(folder_name).is_some();

    let mut dropzone_suffix: Option<String> = None;
    let mut saw_v_marker = false;

    // Dropzone / V may appear anywhere (guest zone, crew zone, trailing).
    for token in &tokens {
        if let Some(dz) = dropzone_from_token(token) {
            dropzone_suffix = Some(dz);
        }
        if is_v_marker(token) {
            saw_v_marker = true;
        }
    }

    let mut skipped_guest_tokens: Vec<String> = Vec::new();
    let mut hits: Vec<CrewHit> = Vec::new();
    let mut ambiguous_roles = false;

    let match_range: std::ops::Range<usize> = if let Some(idx) = structure_idx {
        // Record guest-zone tokens that would have matched crew (transparency).
        for token in &tokens[..idx] {
            if is_noise_token(token)
                || dropzone_from_token(token).is_some()
                || is_structure_ta(token)
                || is_v_marker(token)
            {
                continue;
            }
            if crew.iter().any(|c| c.matches_token(token))
                || token_matches_guest(token, &options)
            {
                push_unique_token(&mut skipped_guest_tokens, token);
            }
        }
        (idx + 1)..tokens.len()
    } else {
        0..tokens.len()
    };

    for token in &tokens[match_range] {
        if dropzone_from_token(token).is_some() {
            continue;
        }
        if is_noise_token(token) {
            continue;
        }
        if is_structure_ta(token) {
            continue;
        }
        if is_v_marker(token) {
            continue;
        }

        // Unstructured: suppress guest vor/nachname so e.g. Andreas≠Andy.
        // Structured crew zone: never suppress (Gast und VS können gleich heißen).
        if !structured_crew_zone && token_matches_guest(token, &options) {
            push_unique_token(&mut skipped_guest_tokens, token);
            continue;
        }

        let matches: Vec<CrewHit> = crew
            .iter()
            .filter(|c| c.matches_token(token))
            .map(|c| {
                let exact = c.name.eq_ignore_ascii_case(token.trim());
                let base: f32 = if exact { 1.0 } else { 0.9 };
                // Stricter post-TA hits → slightly higher confidence (19e).
                let confidence = if structured_crew_zone {
                    (base + 0.05_f32).min(1.0)
                } else {
                    base
                };
                CrewHit {
                    name: c.name.clone(),
                    tandemmaster: c.tandemmaster,
                    videospringer: c.videospringer,
                    confidence,
                }
            })
            .collect();

        if matches.is_empty() {
            continue;
        }
        if matches.len() > 1 {
            ambiguous_roles = true;
        }
        let mut seen = std::collections::HashSet::new();
        for m in matches {
            if seen.insert(m.name.to_lowercase()) {
                hits.push(m);
            }
        }
    }

    // When underscore structure exists, prefer guest_segment tokens for skip list
    // so UI/tests stay aligned with folder_match (reuse, little duplication).
    if structured_crew_zone {
        for token in tokenize(guest_segment(folder_name)) {
            if is_noise_token(&token) || dropzone_from_token(&token).is_some() {
                continue;
            }
            if crew.iter().any(|c| c.matches_token(&token))
                || token_matches_guest(&token, &options)
            {
                push_unique_token(&mut skipped_guest_tokens, &token);
            }
        }
    }

    let (tandemmaster, tm_confidence, tm_ambiguous) =
        pick_tandemmaster(&hits, structured_crew_zone);
    let (videospringer, vs_confidence, vs_ambiguous) =
        pick_videospringer(&hits, &tandemmaster, saw_v_marker || structured_crew_zone);

    let mut review_reasons = Vec::new();
    if tandemmaster.is_none() {
        review_reasons.push("Tandemmaster fehlt oder unsicher".into());
    } else if tm_confidence < 0.7 || tm_ambiguous {
        review_reasons.push("Tandemmaster unsicher".into());
    }
    if ambiguous_roles {
        review_reasons.push("Mehrere Crew-Treffer für dasselbe Token".into());
    }
    if tm_ambiguous || vs_ambiguous {
        review_reasons.push("Mehrfachkandidaten für Rolle".into());
    }
    if options.outside_video && videospringer.is_none() {
        review_reasons.push("Outside-Video ohne Videospringer".into());
    }

    let needs_review = !review_reasons.is_empty();

    FolderRenamePrediction {
        tandemmaster,
        videospringer,
        dropzone_suffix,
        tm_confidence,
        vs_confidence,
        needs_review,
        review_reasons,
        skipped_guest_tokens,
        structured_crew_zone,
    }
}

fn push_unique_token(out: &mut Vec<String>, token: &str) {
    let t = token.trim();
    if t.is_empty() {
        return;
    }
    if out.iter().any(|x| x.eq_ignore_ascii_case(t)) {
        return;
    }
    out.push(t.to_string());
}

fn token_matches_guest(token: &str, options: &PredictOptions) -> bool {
    let tf = fold_key(token);
    if tf.len() < 2 {
        return false;
    }
    for name in [&options.guest_vorname, &options.guest_nachname]
        .into_iter()
        .flatten()
    {
        let n = name.trim();
        if n.is_empty() {
            continue;
        }
        if fold_key(n) == tf {
            return true;
        }
    }
    false
}

fn pick_tandemmaster(
    hits: &[CrewHit],
    _structure_ta: bool,
) -> (Option<String>, f32, bool) {
    let tms: Vec<&CrewHit> = hits.iter().filter(|h| h.tandemmaster).collect();
    if tms.is_empty() {
        return (None, 0.0, false);
    }
    // First TM-capable hit in folder order is the tandemmaster.
    let first = tms[0];
    let ambiguous = tms
        .iter()
        .skip(1)
        .any(|h| h.name.eq_ignore_ascii_case(&first.name) == false && h.tandemmaster);
    // Multiple distinct TM hits: still take the first, but flag ambiguity when
    // a second TM appears *before* any VS-only person would clearly be VS.
    // Practical rule: if ≥2 distinct TM-capable people appear and the second
    // is also only-TM (not VS), that's ambiguous TM. If second is VS-capable,
    // treat second as VS candidate instead (handled in pick_videospringer).
    let second_is_vs_candidate = tms.get(1).map(|h| h.videospringer).unwrap_or(false);
    let ambiguous_tm = ambiguous && !second_is_vs_candidate && tms.len() > 1;
    (Some(first.name.clone()), first.confidence, ambiguous_tm)
}

fn pick_videospringer(
    hits: &[CrewHit],
    tandemmaster: &Option<String>,
    prefer_after_tm: bool,
) -> (Option<String>, f32, bool) {
    let tm_name = tandemmaster.as_deref().unwrap_or("");
    let vs_hits: Vec<&CrewHit> = hits
        .iter()
        .filter(|h| h.videospringer)
        .filter(|h| !h.name.eq_ignore_ascii_case(tm_name))
        .collect();

    if vs_hits.is_empty() {
        return (None, 0.0, false);
    }

    // Prefer the first VS after the TM in token order when structure suggests it.
    let chosen = if prefer_after_tm {
        let tm_idx = hits.iter().position(|h| h.name.eq_ignore_ascii_case(tm_name));
        vs_hits
            .iter()
            .copied()
            .find(|h| {
                tm_idx
                    .map(|ti| {
                        hits.iter()
                            .position(|x| x.name.eq_ignore_ascii_case(&h.name))
                            .map(|vi| vi > ti)
                            .unwrap_or(true)
                    })
                    .unwrap_or(true)
            })
            .unwrap_or(vs_hits[0])
    } else {
        vs_hits[0]
    };

    let ambiguous = vs_hits
        .iter()
        .filter(|h| !h.name.eq_ignore_ascii_case(&chosen.name))
        .count()
        > 0
        && vs_hits.len() > 1
        && prefer_after_tm == false;

    (Some(chosen.name.clone()), chosen.confidence, ambiguous)
}

/// Split folder name into tokens (Whitespace / `_` / `-`, CamelCase, digit edges).
pub fn tokenize(folder_name: &str) -> Vec<String> {
    let base = folder_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(folder_name)
        .trim();

    let mut coarse = Vec::new();
    let mut buf = String::new();
    for ch in base.chars() {
        if ch.is_whitespace() || ch == '_' || ch == '-' {
            if !buf.is_empty() {
                coarse.push(std::mem::take(&mut buf));
            }
        } else {
            buf.push(ch);
        }
    }
    if !buf.is_empty() {
        coarse.push(buf);
    }

    let mut out = Vec::new();
    for part in coarse {
        out.extend(split_camel_and_digits(&part));
    }
    out
}

fn split_camel_and_digits(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }

    let mut parts = Vec::new();
    let mut start = 0;
    for i in 1..chars.len() {
        let prev = chars[i - 1];
        let cur = chars[i];
        let boundary = (prev.is_lowercase() && cur.is_uppercase())
            || (prev.is_ascii_digit() != cur.is_ascii_digit()
                && (prev.is_alphanumeric() && cur.is_alphanumeric()));
        // Also split ALL-CAPS acronym + Capitalized name: TACorni → TA / Corni
        // when a run of ≥2 uppercase is followed by uppercase+lowercase.
        let acronym_break = i + 1 < chars.len()
            && prev.is_uppercase()
            && cur.is_uppercase()
            && chars[i + 1].is_lowercase()
            && chars[start..i].iter().all(|c| c.is_uppercase());
        if boundary || acronym_break {
            let piece: String = chars[start..i].iter().collect();
            if !piece.is_empty() {
                parts.push(piece);
            }
            start = i;
        }
    }
    let rest: String = chars[start..].iter().collect();
    if !rest.is_empty() {
        parts.push(rest);
    }
    if parts.is_empty() {
        parts.push(s.to_string());
    }
    parts
}

fn dropzone_from_token(token: &str) -> Option<String> {
    let t = token.trim();
    if t.eq_ignore_ascii_case("G") || t.eq_ignore_ascii_case("Gera") {
        return Some("G".into());
    }
    if t.eq_ignore_ascii_case("C") || t.eq_ignore_ascii_case("Calden") {
        return Some("C".into());
    }
    None
}

fn is_structure_ta(token: &str) -> bool {
    let t = token.trim();
    t.eq_ignore_ascii_case("TA") || t.eq_ignore_ascii_case("TD")
}

fn is_v_marker(token: &str) -> bool {
    token.trim().eq_ignore_ascii_case("V")
}

fn is_noise_token(token: &str) -> bool {
    let t = token.trim();
    if t.is_empty() {
        return true;
    }
    let lower = t.to_ascii_lowercase();

    // Load / L# (L1, L12, …)
    if lower == "load" {
        return true;
    }
    if lower.starts_with('l')
        && lower.len() > 1
        && lower.chars().skip(1).all(|c| c.is_ascii_digit())
    {
        return true;
    }
    // Pure digit chunks from Load3 splits etc.
    if t.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }

    // Media / booking codes commonly left in camera folder names.
    const MEDIA_CODES: &[&str] = &[
        "f", "foto", "photo", "photos", "pic", "pics",
        "video", "videos", "vid", "vids", "handcam", "hc",
        "outside", "ov", "of", "hv", "hf", "media", "bilder",
        "film", "filme",
    ];
    if MEDIA_CODES.iter().any(|c| lower == *c) {
        return true;
    }

    // Date-like YYYYMMDD prefix noise when left as a token.
    if t.len() == 8 && t.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::crew::default_crew_list;

    fn predict(name: &str, outside_video: bool) -> FolderRenamePrediction {
        predict_from_folder_name(
            name,
            &default_crew_list(),
            PredictOptions {
                outside_video,
                ..Default::default()
            },
        )
    }

    fn predict_guest(
        name: &str,
        outside_video: bool,
        vorname: &str,
        nachname: &str,
    ) -> FolderRenamePrediction {
        predict_from_folder_name(
            name,
            &default_crew_list(),
            PredictOptions {
                outside_video,
                guest_vorname: Some(vorname.into()),
                guest_nachname: Some(nachname.into()),
            },
        )
    }

    #[test]
    fn tokenize_whitespace_underscore_camel_and_tacorni() {
        assert_eq!(
            tokenize("Roman Stefan Robin"),
            vec!["Roman", "Stefan", "Robin"]
        );
        assert_eq!(
            tokenize("Niels_TACorni"),
            vec!["Niels", "TA", "Corni"]
        );
        assert_eq!(tokenize("Emilia-TD-Ralph"), vec!["Emilia", "TD", "Ralph"]);
        assert_eq!(tokenize("Load3"), vec!["Load", "3"]);
    }

    #[test]
    fn gold_roman_stefan_robin() {
        let p = predict("Roman_Stefan_Robin", true);
        assert_eq!(p.tandemmaster.as_deref(), Some("Stefan"));
        assert_eq!(p.videospringer.as_deref(), Some("Robin"));
        assert!(!p.needs_review, "{:?}", p.review_reasons);
    }

    #[test]
    fn gold_roman_with_load_and_gera_dropzone() {
        let p = predict("Roman Stefan Robin L3 Gera", true);
        assert_eq!(p.tandemmaster.as_deref(), Some("Stefan"));
        assert_eq!(p.videospringer.as_deref(), Some("Robin"));
        assert_eq!(p.dropzone_suffix.as_deref(), Some("G"));
        assert!(!p.needs_review, "{:?}", p.review_reasons);
    }

    #[test]
    fn gold_niels_tacorni_alias_to_cornelius() {
        let p = predict("Niels_TACorni", false);
        assert_eq!(p.tandemmaster.as_deref(), Some("Cornelius"));
        assert!(p.videospringer.is_none());
        assert!(p.structured_crew_zone);
        assert!(!p.needs_review, "{:?}", p.review_reasons);
        // Outside without VS → review
        let p2 = predict("Niels_TACorni", true);
        assert_eq!(p2.tandemmaster.as_deref(), Some("Cornelius"));
        assert!(p2.needs_review);
        assert!(p2
            .review_reasons
            .iter()
            .any(|r| r.contains("Outside-Video")));
    }

    #[test]
    fn gold_christin_futti() {
        let p = predict("Christin_Futti", false);
        assert_eq!(p.tandemmaster.as_deref(), Some("Futti"));
        assert!(!p.needs_review, "{:?}", p.review_reasons);
    }

    #[test]
    fn gold_emilia_td_ralph() {
        let p = predict("Emilia_TD_Ralph", false);
        assert_eq!(p.tandemmaster.as_deref(), Some("Ralph"));
        assert!(p.structured_crew_zone);
        assert!(!p.needs_review, "{:?}", p.review_reasons);
    }

    #[test]
    fn gold_sabine_f_ralph_media_code_dropped() {
        let p = predict("Sabine_F_Ralph", false);
        assert_eq!(p.tandemmaster.as_deref(), Some("Ralph"));
        assert!(p.videospringer.is_none());
        assert!(!p.needs_review, "{:?}", p.review_reasons);
    }

    #[test]
    fn missing_tm_needs_review() {
        let p = predict("NurGast_Load2", false);
        assert!(p.tandemmaster.is_none());
        assert!(p.needs_review);
    }

    #[test]
    fn dropzone_g_from_single_letter() {
        let p = predict("Gast_Andy_G", false);
        assert_eq!(p.tandemmaster.as_deref(), Some("Andy"));
        assert_eq!(p.dropzone_suffix.as_deref(), Some("G"));
    }

    /// Bug-Repro 19e: Gast Andreas matchte Alias Andy vor TA.
    #[test]
    fn guest_andreas_after_ta_futti_henni_not_andy() {
        let p = predict_guest(
            "20260827_Andreas_Kowalenko_TA_Futti_V_Henni_C",
            true,
            "Andreas",
            "Kowalenko",
        );
        assert_eq!(p.tandemmaster.as_deref(), Some("Futti"));
        assert_eq!(p.videospringer.as_deref(), Some("Henrik"));
        assert_eq!(p.dropzone_suffix.as_deref(), Some("C"));
        assert!(p.structured_crew_zone);
        assert!(
            p.skipped_guest_tokens
                .iter()
                .any(|t| t.eq_ignore_ascii_case("Andreas")),
            "{:?}",
            p.skipped_guest_tokens
        );
        assert!(!p.needs_review, "{:?}", p.review_reasons);

        // Struktur-Zone allein (ohne Gast-Optionen) reicht ebenfalls.
        let p2 = predict(
            "20260827_Andreas_Kowalenko_TA_Futti_V_Henni_C",
            true,
        );
        assert_eq!(p2.tandemmaster.as_deref(), Some("Futti"));
        assert_eq!(p2.videospringer.as_deref(), Some("Henrik"));
    }

    #[test]
    fn unstructured_guest_andreas_futti_not_andy() {
        let p = predict_guest("Andreas_Futti", false, "Andreas", "Mustermann");
        assert_eq!(p.tandemmaster.as_deref(), Some("Futti"));
        assert_ne!(p.tandemmaster.as_deref(), Some("Andy"));
        assert!(
            p.skipped_guest_tokens
                .iter()
                .any(|t| t.eq_ignore_ascii_case("Andreas")),
            "{:?}",
            p.skipped_guest_tokens
        );
        assert!(!p.needs_review, "{:?}", p.review_reasons);
    }

    #[test]
    fn real_crew_andreas_after_ta_still_andy() {
        let p = predict_guest(
            "20260827_Max_Muster_TA_Andreas_V_Robin",
            true,
            "Max",
            "Muster",
        );
        assert_eq!(p.tandemmaster.as_deref(), Some("Andy"));
        assert_eq!(p.videospringer.as_deref(), Some("Robin"));
        assert!(p.structured_crew_zone);
        assert!(!p.needs_review, "{:?}", p.review_reasons);

        let p2 = predict_guest(
            "20260827_Max_Muster_TA_Andy_V_Robin",
            true,
            "Max",
            "Muster",
        );
        assert_eq!(p2.tandemmaster.as_deref(), Some("Andy"));
    }

    #[test]
    fn guest_robin_does_not_kill_vs_robin_after_ta() {
        let p = predict_guest(
            "20260827_Robin_Guest_TA_Stefan_V_Robin",
            true,
            "Robin",
            "Guest",
        );
        assert_eq!(p.tandemmaster.as_deref(), Some("Stefan"));
        assert_eq!(p.videospringer.as_deref(), Some("Robin"));
        assert!(p.structured_crew_zone);
        assert!(!p.needs_review, "{:?}", p.review_reasons);
    }

    #[test]
    fn andreas_alias_still_matches_andy_token() {
        let andy = default_crew_list()
            .into_iter()
            .find(|c| c.name == "Andy")
            .expect("Andy");
        assert!(andy.matches_token("Andreas"));
        assert!(andy.matches_token("Andy"));
    }
}
