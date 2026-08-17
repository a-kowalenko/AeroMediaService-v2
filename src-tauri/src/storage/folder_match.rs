//! Rank media folders against a customer name.
//!
//! ATS job folders follow `{YYYYMMDD}_{Gast}_TA_{Tandemmaster}[_V_{Videospringer}]`.
//! Matching uses only the guest segment so tandemmaster / videospringer names
//! cannot produce false recommendations.

use std::cmp::Ordering;

pub const SCORE_NACHNAME: u32 = 100;
pub const SCORE_VORNAME: u32 = 80;
pub const SCORE_TODAY: u32 = 20;
pub const SCORE_READY: u32 = 10;
const NEAR_BEST: u32 = 40;
const MIN_TOKEN_LEN: usize = 2;
const MIN_NACHNAME_SUBSTRING: usize = 4;
const MIN_FUZZY_LEN: usize = 4;
/// `ey`/`ay` spelling variants only for longer names (`Viehmeyer` ↔ `Viehmeier`).
/// Short names like `Meyer`/`Meier` stay distinct to avoid unique-last-name auto-assign.
const MIN_DIGRAPH_LEN: usize = 6;

/// Guest portion of an ATS (or legacy) folder name.
pub fn guest_segment(folder_name: &str) -> &str {
    let rest = strip_date_prefix(folder_name);
    let lower = rest.to_ascii_lowercase();
    if let Some(idx) = lower.find("_ta_") {
        rest.get(..idx).unwrap_or(rest)
    } else {
        rest
    }
}

fn strip_date_prefix(name: &str) -> &str {
    let bytes = name.as_bytes();
    if bytes.len() >= 9 && bytes[..8].iter().all(|b| b.is_ascii_digit()) && bytes[8] == b'_' {
        &name[9..]
    } else {
        name
    }
}

fn fold_key(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        let lower = ch.to_lowercase().next().unwrap_or(ch);
        match lower {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            'á' | 'à' | 'â' | 'ã' => out.push('a'),
            'é' | 'è' | 'ê' | 'ë' => out.push('e'),
            'í' | 'ì' | 'î' | 'ï' => out.push('i'),
            'ó' | 'ò' | 'ô' | 'õ' => out.push('o'),
            'ú' | 'ù' | 'û' => out.push('u'),
            'ç' => out.push('c'),
            'ñ' => out.push('n'),
            c if c.is_alphanumeric() => out.push(c),
            _ => out.push(' '),
        }
    }
    out
}

fn tokens(value: &str) -> Vec<String> {
    fold_key(value)
        .split_whitespace()
        .filter(|part| part.chars().count() >= MIN_TOKEN_LEN)
        .map(|part| part.to_string())
        .collect()
}

fn all_tokens_present(needles: &[String], haystack: &[String]) -> bool {
    !needles.is_empty() && needles.iter().all(|n| haystack.iter().any(|h| h == n))
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

/// Strip common German diminutive endings (`Mausi` → `maus`).
fn nickname_stem(token: &str) -> &str {
    let bytes = token.as_bytes();
    if bytes.len() >= 6 && token.ends_with("ie") {
        return &token[..token.len() - 2];
    }
    if bytes.len() >= 5 && (token.ends_with('i') || token.ends_with('y')) {
        return &token[..token.len() - 1];
    }
    token
}

/// `henn` → `hen` so `Henni` can match `Henrik`.
fn collapse_doubled_final(stem: &str) -> Option<&str> {
    let bytes = stem.as_bytes();
    if bytes.len() < 4 {
        return None;
    }
    let last = bytes[bytes.len() - 1];
    let prev = bytes[bytes.len() - 2];
    if last.is_ascii_alphabetic() && last == prev {
        Some(&stem[..stem.len() - 1])
    } else {
        None
    }
}

fn diminutive_prefix_match(original: &str, stem: &str, other: &str, other_stem: &str) -> bool {
    if stem.len() >= MIN_FUZZY_LEN && (other.starts_with(stem) || other_stem.starts_with(stem)) {
        return true;
    }
    // Collapse doubled consonants only for stripped diminutives (`Henni` → `henn` → `hen`).
    if stem.len() == original.len() {
        return false;
    }
    let Some(collapsed) = collapse_doubled_final(stem) else {
        return false;
    };
    if collapsed.len() < 3 {
        return false;
    }
    [other, other_stem].into_iter().any(|candidate| {
        candidate.len() >= collapsed.len() + 2 && candidate.starts_with(collapsed)
    })
}

/// German spelling variants: `Viehmeyer` → `Viehmeier`, `Haymann` → `Haimann`.
fn normalize_german_digraphs(token: &str) -> String {
    token.replace("ey", "ei").replace("ay", "ai")
}

fn tokens_similar_raw(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (shorter, longer) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if shorter.len() >= MIN_FUZZY_LEN && longer.starts_with(shorter) {
        return true;
    }
    let stem_a = nickname_stem(a);
    let stem_b = nickname_stem(b);
    if diminutive_prefix_match(a, stem_a, b, stem_b)
        || diminutive_prefix_match(b, stem_b, a, stem_a)
    {
        return true;
    }
    let lcp = common_prefix_len(a, b);
    let min_len = shorter.len();
    lcp >= MIN_FUZZY_LEN && min_len > 0 && lcp * 10 >= min_len * 7
}

/// Nickname / truncation match: `mausi` ↔ `maushake`, `alex` ↔ `alexander`.
/// Longer names also accept `ei`/`ey` and `ai`/`ay` spelling variants.
fn tokens_similar(a: &str, b: &str) -> bool {
    if tokens_similar_raw(a, b) {
        return true;
    }
    if a.len() < MIN_DIGRAPH_LEN || b.len() < MIN_DIGRAPH_LEN {
        return false;
    }
    if !a.contains('y') && !b.contains('y') {
        return false;
    }
    let na = normalize_german_digraphs(a);
    let nb = normalize_german_digraphs(b);
    if na == a && nb == b {
        return false;
    }
    tokens_similar_raw(&na, &nb)
}

fn any_token_similar(needles: &[String], haystack: &[String]) -> bool {
    needles
        .iter()
        .any(|n| haystack.iter().any(|h| tokens_similar(n, h)))
}

fn nachname_hits(nach_tokens: &[String], guest_tokens: &[String], guest_joined: &str) -> bool {
    if nach_tokens.is_empty() {
        return false;
    }
    if all_tokens_present(nach_tokens, guest_tokens) {
        return true;
    }
    if any_token_similar(nach_tokens, guest_tokens) {
        return true;
    }
    let concat = nach_tokens.join("");
    if concat.len() >= MIN_NACHNAME_SUBSTRING && guest_joined.contains(&concat) {
        return true;
    }
    if concat.len() >= MIN_DIGRAPH_LEN {
        let concat_norm = normalize_german_digraphs(&concat);
        let guest_norm = normalize_german_digraphs(guest_joined);
        if guest_norm.contains(&concat_norm) {
            return true;
        }
    }
    false
}

fn vorname_hits(vor_tokens: &[String], guest_tokens: &[String]) -> bool {
    !vor_tokens.is_empty()
        && vor_tokens
            .iter()
            .all(|n| guest_tokens.iter().any(|h| tokens_similar(n, h)))
}

/// Score a folder name against customer first/last name.
///
/// `assignable` is true only for ready (green) folders. Occupied/busy folders
/// can still score on the name so they sort near the top, but they never get
/// the ready bonus.
pub fn score_folder_name(
    folder_name: &str,
    vorname: &str,
    nachname: &str,
    assignable: bool,
    today_yyyymmdd: &str,
) -> u32 {
    let nach_tokens = tokens(nachname);
    if nach_tokens.is_empty() {
        return 0;
    }

    let guest = guest_segment(folder_name);
    let guest_tokens = tokens(guest);
    if guest_tokens.is_empty() {
        return 0;
    }
    let guest_joined = guest_tokens.join("");

    if !nachname_hits(&nach_tokens, &guest_tokens, &guest_joined) {
        return 0;
    }

    let mut score = SCORE_NACHNAME;
    let vor_tokens = tokens(vorname);
    if vorname_hits(&vor_tokens, &guest_tokens) {
        score += SCORE_VORNAME;
    }
    if today_yyyymmdd.len() == 8
        && today_yyyymmdd.bytes().all(|b| b.is_ascii_digit())
        && folder_name.starts_with(today_yyyymmdd)
    {
        score += SCORE_TODAY;
    }
    if assignable {
        score += SCORE_READY;
    }
    score
}

/// Strong recommendation: last+first name, or a unique last-name hit.
/// Always requires an assignable (ready) folder close to the best score.
pub fn is_recommended(score: u32, assignable: bool, best: u32, last_name_hits: usize) -> bool {
    if !assignable || score < SCORE_NACHNAME || best < SCORE_NACHNAME {
        return false;
    }
    if score + NEAR_BEST < best {
        return false;
    }
    let has_first = score >= SCORE_NACHNAME + SCORE_VORNAME;
    has_first || last_name_hits == 1
}

pub fn cmp_rank(
    a_recommended: bool,
    a_score: u32,
    a_name: &str,
    b_recommended: bool,
    b_score: u32,
    b_name: &str,
) -> Ordering {
    b_recommended
        .cmp(&a_recommended)
        .then(b_score.cmp(&a_score))
        .then_with(|| a_name.to_lowercase().cmp(&b_name.to_lowercase()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueAssignment {
    pub customer_index: usize,
    pub folder_index: usize,
    pub score: u32,
}

/// Greedy 1:1 matching: highest recommended score wins a folder.
/// Ambiguous last-name-only hits are not auto-assigned (`is_recommended` is false).
pub fn propose_unique_assignments(
    customers: &[(&str, &str)],
    folder_names: &[&str],
    today_yyyymmdd: &str,
) -> Vec<UniqueAssignment> {
    let mut candidates: Vec<(u32, usize, usize)> = Vec::new();
    for (ci, (vorname, nachname)) in customers.iter().enumerate() {
        let scores: Vec<u32> = folder_names
            .iter()
            .map(|name| score_folder_name(name, vorname, nachname, true, today_yyyymmdd))
            .collect();
        let best = scores.iter().copied().max().unwrap_or(0);
        let last_name_hits = scores
            .iter()
            .filter(|score| **score >= SCORE_NACHNAME)
            .count();
        for (fi, score) in scores.into_iter().enumerate() {
            if is_recommended(score, true, best, last_name_hits) {
                candidates.push((score, ci, fi));
            }
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    let mut used_customers = std::collections::HashSet::new();
    let mut used_folders = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (score, ci, fi) in candidates {
        if used_customers.contains(&ci) || used_folders.contains(&fi) {
            continue;
        }
        used_customers.insert(ci);
        used_folders.insert(fi);
        out.push(UniqueAssignment {
            customer_index: ci,
            folder_index: fi,
            score,
        });
    }
    out.sort_by_key(|item| item.customer_index);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_segment_strips_date_and_crew() {
        assert_eq!(
            guest_segment("20260815_Max_Mustermann_TA_Schmidt"),
            "Max_Mustermann"
        );
        assert_eq!(
            guest_segment("20260815_Max_Mustermann_TA_Schmidt_V_Bob"),
            "Max_Mustermann"
        );
        assert_eq!(guest_segment("20260815_Max_TA_Anna"), "Max");
        assert_eq!(guest_segment("Job-1"), "Job-1");
        assert_eq!(guest_segment("Max_Mustermann"), "Max_Mustermann");
    }

    #[test]
    fn tandemmaster_does_not_match() {
        let score = score_folder_name(
            "20260815_Max_Mustermann_TA_Schmidt",
            "Anna",
            "Schmidt",
            true,
            "20260815",
        );
        assert_eq!(score, 0);
    }

    #[test]
    fn full_name_today_ready_scores_highest() {
        let score = score_folder_name(
            "20260815_Max_Mustermann_TA_Anna",
            "Max",
            "Mustermann",
            true,
            "20260815",
        );
        assert_eq!(
            score,
            SCORE_NACHNAME + SCORE_VORNAME + SCORE_TODAY + SCORE_READY
        );
    }

    #[test]
    fn umlaut_mueller_matches_müller() {
        let score = score_folder_name(
            "20260815_Lisa_Mueller_TA_X",
            "Lisa",
            "Müller",
            true,
            "20260815",
        );
        assert!(score >= SCORE_NACHNAME + SCORE_VORNAME);
        let reverse = score_folder_name(
            "20260815_Lisa_Müller_TA_X",
            "Lisa",
            "Mueller",
            true,
            "20260815",
        );
        assert!(reverse >= SCORE_NACHNAME + SCORE_VORNAME);
    }

    #[test]
    fn first_name_only_does_not_score() {
        let score = score_folder_name(
            "20260815_Max_Mustermann_TA_X",
            "Max",
            "Schmidt",
            true,
            "20260815",
        );
        assert_eq!(score, 0);
    }

    #[test]
    fn short_first_name_does_not_substring_match() {
        // "Max" must not match guest token "Maximilian"
        let score = score_folder_name(
            "20260815_Maximilian_Schmidt_TA_X",
            "Max",
            "Schmidt",
            true,
            "20260815",
        );
        assert_eq!(score, SCORE_NACHNAME + SCORE_TODAY + SCORE_READY);
    }

    #[test]
    fn doubled_consonant_does_not_match_unrelated_last_name() {
        assert_eq!(
            score_folder_name(
                "20260817_Paul_Kampmann_TA_X",
                "Paul",
                "Kamm",
                true,
                "20260817",
            ),
            0
        );
    }

    #[test]
    fn concatenated_last_name_still_hits() {
        let score = score_folder_name(
            "20260815_MaxMustermann_TA_X",
            "Max",
            "Mustermann",
            true,
            "20260815",
        );
        assert!(score >= SCORE_NACHNAME);
        assert!(score < SCORE_NACHNAME + SCORE_VORNAME);
    }

    #[test]
    fn recommend_unique_last_name() {
        assert!(is_recommended(
            SCORE_NACHNAME + SCORE_READY,
            true,
            SCORE_NACHNAME + SCORE_READY,
            1
        ));
    }

    #[test]
    fn do_not_recommend_ambiguous_last_name_only() {
        assert!(!is_recommended(
            SCORE_NACHNAME + SCORE_READY,
            true,
            SCORE_NACHNAME + SCORE_READY,
            2
        ));
    }

    #[test]
    fn recommend_first_and_last_even_if_another_last_name_exists() {
        let best = SCORE_NACHNAME + SCORE_VORNAME + SCORE_READY;
        assert!(is_recommended(best, true, best, 2));
        assert!(!is_recommended(SCORE_NACHNAME + SCORE_READY, true, best, 2));
    }

    #[test]
    fn occupied_folder_is_never_recommended() {
        assert!(!is_recommended(
            SCORE_NACHNAME + SCORE_VORNAME + SCORE_TODAY,
            false,
            SCORE_NACHNAME + SCORE_VORNAME + SCORE_TODAY,
            1
        ));
    }

    #[test]
    fn sort_puts_recommended_first() {
        let mut rows = vec![
            (false, 210u32, "20260815_Other"),
            (true, 180u32, "20260815_Match"),
            (false, 0u32, "aaa"),
        ];
        rows.sort_by(|a, b| cmp_rank(a.0, a.1, a.2, b.0, b.1, b.2));
        assert_eq!(rows[0].2, "20260815_Match");
    }

    #[test]
    fn unique_assignments_pair_distinct_customers() {
        let customers = [("Max", "Mustermann"), ("Lisa", "Müller")];
        let folders = [
            "20260815_Max_Mustermann_TA_X",
            "20260815_Lisa_Mueller_TA_Y",
            "other",
        ];
        let hits = propose_unique_assignments(&customers, &folders, "20260815");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].customer_index, 0);
        assert_eq!(hits[0].folder_index, 0);
        assert_eq!(hits[1].customer_index, 1);
        assert_eq!(hits[1].folder_index, 1);
    }

    #[test]
    fn unique_assignments_conflict_goes_to_better_score() {
        let customers = [("Max", "Mustermann"), ("Maximilian", "Mustermann")];
        let folders = ["20260815_Max_Mustermann_TA_X"];
        let hits = propose_unique_assignments(&customers, &folders, "20260815");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].customer_index, 0);
        assert_eq!(hits[0].folder_index, 0);
    }

    #[test]
    fn unique_assignments_skips_ambiguous_last_name_only() {
        let customers = [("Anna", "Müller"), ("Bernd", "Müller")];
        let folders = ["20260815_Müller_TA_X", "20260815_Mueller_TA_Y"];
        let hits = propose_unique_assignments(&customers, &folders, "20260815");
        assert!(hits.is_empty());
    }

    #[test]
    fn nickname_last_name_mausi_matches_maushake() {
        let score = score_folder_name(
            "20260817_benno_mausi_TA_Alberto",
            "Benno",
            "Maushake",
            true,
            "20260817",
        );
        assert!(
            score >= SCORE_NACHNAME + SCORE_VORNAME,
            "expected Benno Maushake to match benno_mausi, got {score}"
        );
        assert!(is_recommended(score, true, score, 1));
    }

    #[test]
    fn first_name_prefix_alex_matches_alexander() {
        let score = score_folder_name(
            "20260817_Alexander_Mustermann_TA_X",
            "Alex",
            "Mustermann",
            true,
            "20260817",
        );
        assert!(score >= SCORE_NACHNAME + SCORE_VORNAME);
    }

    #[test]
    fn similar_last_names_do_not_cross_match() {
        assert_eq!(
            score_folder_name(
                "20260817_Anna_Schneider_TA_X",
                "Anna",
                "Schmidt",
                true,
                "20260817",
            ),
            0
        );
        assert_eq!(
            score_folder_name(
                "20260817_Paul_Bergmann_TA_X",
                "Paul",
                "Berger",
                true,
                "20260817",
            ),
            0
        );
    }

    #[test]
    fn unique_assignment_picks_nickname_folder() {
        let customers = [("Benno", "Maushake")];
        let folders = [
            "20260817_benno_mausi_TA_Alberto",
            "20260817_Lisa_Mueller_TA_Y",
        ];
        let hits = propose_unique_assignments(&customers, &folders, "20260817");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].folder_index, 0);
    }

    #[test]
    fn nickname_henni_matches_henrik() {
        assert_eq!(nickname_stem("henni"), "henn");
        assert_eq!(collapse_doubled_final("henn"), Some("hen"));
        assert!(tokens_similar("henni", "henrik"));
        let score = score_folder_name(
            "20260817_Henrik_Mustermann_TA_X",
            "henni",
            "Mustermann",
            true,
            "20260817",
        );
        assert!(
            score >= SCORE_NACHNAME + SCORE_VORNAME,
            "expected henni to match Henrik, got {score}"
        );
    }

    #[test]
    fn ey_ei_last_name_and_nickname_first_name_match() {
        let score = score_folder_name(
            "20260817_Henrik_Viehmeyer_TA_X",
            "henni",
            "viehmeier",
            true,
            "20260817",
        );
        assert!(
            score >= SCORE_NACHNAME + SCORE_VORNAME,
            "expected henni viehmeier to match Henrik Viehmeyer, got {score}"
        );
        assert!(is_recommended(score, true, score, 1));
    }

    #[test]
    fn concatenated_ey_ei_last_name_still_hits() {
        let score = score_folder_name(
            "20260817_HenrikViehmeyer_TA_X",
            "henni",
            "viehmeier",
            true,
            "20260817",
        );
        assert!(score >= SCORE_NACHNAME);
    }

    #[test]
    fn short_meyer_does_not_match_meier() {
        assert_eq!(
            score_folder_name(
                "20260817_Paul_Meyer_TA_X",
                "Anna",
                "Meier",
                true,
                "20260817",
            ),
            0
        );
        assert_eq!(
            score_folder_name(
                "20260817_Paul_Mayer_TA_X",
                "Anna",
                "Maier",
                true,
                "20260817",
            ),
            0
        );
    }

    #[test]
    fn ay_ai_compound_last_name_matches() {
        let score = score_folder_name(
            "20260817_Anna_Haymann_TA_X",
            "Anna",
            "Haimann",
            true,
            "20260817",
        );
        assert!(score >= SCORE_NACHNAME + SCORE_VORNAME);
    }

    #[test]
    fn unique_assignment_picks_ey_ei_nickname_folder() {
        let customers = [("henni", "viehmeier")];
        let folders = [
            "20260817_Henrik_Viehmeyer_TA_X",
            "20260817_Lisa_Mueller_TA_Y",
        ];
        let hits = propose_unique_assignments(&customers, &folders, "20260817");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].folder_index, 0);
    }
}
