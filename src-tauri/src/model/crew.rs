//! Crew roster for TM/VS prediction (Phase 19a).
//!
//! Mirrors ATS `CrewMember` / `DEFAULT_CREW_LIST`, plus editable `aliases`.

use serde::{Deserialize, Serialize};

/// One person in the local crew roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrewMember {
    pub name: String,
    #[serde(default)]
    pub tandemmaster: bool,
    #[serde(default)]
    pub videospringer: bool,
    /// Alternate spellings / short forms matched in folder names (e.g. Corni → Cornelius).
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl CrewMember {
    pub fn new(name: impl Into<String>, tandemmaster: bool, videospringer: bool) -> Self {
        Self {
            name: name.into(),
            tandemmaster,
            videospringer,
            aliases: Vec::new(),
        }
    }

    pub fn with_aliases(mut self, aliases: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.aliases = aliases.into_iter().map(Into::into).collect();
        self
    }

    /// Case-insensitive equality against canonical name or any alias.
    pub fn matches_token(&self, token: &str) -> bool {
        let t = token.trim();
        if t.is_empty() {
            return false;
        }
        if eq_ci(&self.name, t) {
            return true;
        }
        self.aliases.iter().any(|a| eq_ci(a, t))
    }
}

fn eq_ci(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Production roster from ATS `DEFAULT_CREW_LIST`, plus sensible start aliases.
pub fn default_crew_list() -> Vec<CrewMember> {
    let mut list = vec![
        CrewMember::new("Alberto", true, false),
        CrewMember::new("Ana", true, true),
        CrewMember::new("Andy", true, true).with_aliases(["Andreas"]),
        CrewMember::new("Chris", true, false),
        CrewMember::new("Cornelius", true, false).with_aliases(["Corni"]),
        CrewMember::new("Futti", true, true),
        CrewMember::new("Harry", true, true),
        CrewMember::new("Henrik", true, true).with_aliases(["Henni"]),
        CrewMember::new("Jan", true, false),
        CrewMember::new("Jojo", false, true),
        CrewMember::new("Kai", false, true),
        CrewMember::new("Käthe", false, true),
        CrewMember::new("Mathi", true, true).with_aliases(["Mathias"]),
        CrewMember::new("Max", true, false),
        CrewMember::new("Mayo", true, true),
        CrewMember::new("Pascal", true, false).with_aliases(["Passy"]),
        CrewMember::new("Ralph", true, true),
        CrewMember::new("Rene", true, false),
        CrewMember::new("Robert", false, true),
        CrewMember::new("Robin", false, true),
        CrewMember::new("Sabrina", false, true),
        CrewMember::new("Sahira", true, true),
        CrewMember::new("Samuel", true, true).with_aliases(["Samu"]),
        CrewMember::new("Stefan", true, false),
        CrewMember::new("Steve", true, false),
        CrewMember::new("Tim", true, true),
        CrewMember::new("Tom", true, true),
        CrewMember::new("Torsten", true, true),
    ];
    list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    list
}

/// Parse `crew_list` setting JSON; empty / invalid → defaults.
pub fn load_crew_list(raw: &str) -> Vec<CrewMember> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return default_crew_list();
    }
    match serde_json::from_str::<Vec<CrewMember>>(trimmed) {
        Ok(list) if !list.is_empty() => list,
        _ => default_crew_list(),
    }
}

/// Serialize crew list for the `crew_list` setting key.
pub fn serialize_crew_list(list: &[CrewMember]) -> Result<String, serde_json::Error> {
    serde_json::to_string(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_crew_has_expected_roles_and_aliases() {
        let list = default_crew_list();
        assert!(list.iter().any(|c| c.name == "Andy" && c.tandemmaster && c.videospringer));
        assert!(list.iter().any(|c| c.name == "Stefan" && c.tandemmaster && !c.videospringer));
        assert!(list.iter().any(|c| c.name == "Robin" && !c.tandemmaster && c.videospringer));
        let cornelius = list.iter().find(|c| c.name == "Cornelius").expect("Cornelius");
        assert!(cornelius.tandemmaster);
        assert!(!cornelius.videospringer);
        assert!(cornelius.matches_token("Corni"));
        assert!(cornelius.matches_token("corni"));
        assert!(list.iter().find(|c| c.name == "Samuel").unwrap().matches_token("Samu"));
        assert!(list.iter().find(|c| c.name == "Pascal").unwrap().matches_token("Passy"));
        assert!(list.iter().find(|c| c.name == "Andy").unwrap().matches_token("Andreas"));
        assert!(list.iter().find(|c| c.name == "Henrik").unwrap().matches_token("Henni"));
        assert!(list.iter().find(|c| c.name == "Mathi").unwrap().matches_token("Mathias"));
    }

    #[test]
    fn load_crew_list_falls_back_on_empty_or_invalid() {
        assert_eq!(load_crew_list(""), default_crew_list());
        assert_eq!(load_crew_list("not-json"), default_crew_list());
        assert_eq!(load_crew_list("[]"), default_crew_list());
    }

    #[test]
    fn load_crew_list_roundtrips_custom() {
        let custom = vec![
            CrewMember::new("Ada", true, false).with_aliases(["A"]),
            CrewMember::new("Bea", false, true),
        ];
        let json = serialize_crew_list(&custom).unwrap();
        let loaded = load_crew_list(&json);
        assert_eq!(loaded, custom);
    }

    #[test]
    fn aliases_default_when_missing_in_json() {
        let raw = r#"[{"name":"Tom","tandemmaster":true,"videospringer":true}]"#;
        let loaded = load_crew_list(raw);
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].aliases.is_empty());
    }
}
