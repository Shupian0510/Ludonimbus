//! Built-in save rules for games that no database knows about
//! (emulators, recompilations, fan ports).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Adapter {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(rename = "match")]
    pub matcher: Matcher,
    #[serde(default)]
    pub roots: BTreeMap<String, String>,
    pub saves: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub wait_for: Vec<String>,
    #[serde(default)]
    pub quiesce: Quiesce,
    #[serde(default)]
    pub relocate: Option<Relocate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Matcher {
    pub exe: String,
    #[serde(default)]
    pub sibling: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Quiesce {
    #[serde(default)]
    pub absent: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relocate {
    pub exe: String,
    #[serde(default)]
    pub dir_pattern: Option<String>,
    #[serde(default)]
    pub sibling: Option<String>,
}

const BUILTIN_YAML: &str = include_str!("../../../adapters/builtin.yaml");

pub fn builtin() -> &'static [Adapter] {
    static CELL: OnceLock<Vec<Adapter>> = OnceLock::new();
    CELL.get_or_init(|| {
        serde_yaml::from_str(BUILTIN_YAML).expect("adapters/builtin.yaml must be valid")
    })
}

pub fn find(key: &str) -> Option<&'static Adapter> {
    builtin().iter().find(|a| a.key == key)
}

/// Guess the adapter for a game executable.
pub fn detect(exe: &Path) -> Option<&'static Adapter> {
    let name = crate::util::file_name_lower(exe);
    builtin().iter().find(|a| {
        if a.matcher.exe.to_lowercase() != name {
            return false;
        }
        match (&a.matcher.sibling, exe.parent()) {
            (Some(sib), Some(dir)) => crate::util::find_file_ci(dir, sib).is_some(),
            (Some(_), None) => false,
            (None, _) => true,
        }
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn builtin_parses() {
        let all = super::builtin();
        assert!(all.len() >= 3);
        assert!(super::find("fe-fortunes-weave").is_some());
    }
}
