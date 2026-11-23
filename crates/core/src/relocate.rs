//! Finding a game again after it moved, e.g. when a new emulator / recomp
//! version was extracted into a new folder (`fefw-recomp-0.2.6` → `0.2.7`).

use crate::adapters;
use crate::config::GameEntry;
use crate::util;
use globset::GlobBuilder;
use std::path::{Path, PathBuf};

struct Rule {
    exe: String,
    dir_pattern: Option<String>,
    sibling: Option<String>,
}

fn rule_for(game: &GameEntry) -> Option<Rule> {
    if let Some(r) = game
        .adapter
        .as_deref()
        .and_then(adapters::find)
        .and_then(|a| a.relocate.clone())
    {
        return Some(Rule {
            exe: r.exe,
            dir_pattern: r.dir_pattern,
            sibling: r.sibling,
        });
    }
    let exe = game.exe.as_ref()?;
    let name = exe.file_name()?.to_string_lossy().into_owned();
    let dir_name = exe.parent()?.file_name()?.to_string_lossy().into_owned();
    Some(Rule {
        exe: name,
        dir_pattern: derive_pattern(&dir_name),
        sibling: None,
    })
}

/// `fefw-recomp-0.2.6` → `fefw-recomp-*`; names without a version → `None`
/// (then only the exe name has to match).
pub fn derive_pattern(dir_name: &str) -> Option<String> {
    // The version starts at the first digit that follows a separator or "v".
    let chars: Vec<(usize, char)> = dir_name.char_indices().collect();
    let idx = chars.iter().enumerate().find_map(|(i, &(byte, c))| {
        let prev_ok = i > 0 && matches!(chars[i - 1].1, '-' | '_' | ' ' | '.' | 'v' | 'V');
        (c.is_ascii_digit() && prev_ok).then_some(byte)
    })?;
    let prefix = dir_name[..idx].trim_end_matches(['-', '_', ' ', '.', 'v', 'V']);
    if prefix.is_empty() {
        return None;
    }
    Some(format!("{prefix}*"))
}

/// Numbers in a folder name, for "highest version wins".
fn version_key(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// Search near the old location for a folder containing the game.
pub fn find_candidate(game: &GameEntry) -> Option<PathBuf> {
    let rule = rule_for(game)?;
    let old_dir = game
        .exe
        .as_ref()
        .and_then(|e| e.parent().map(Path::to_path_buf))
        .or_else(|| game.install.clone())?;
    let pattern = rule.dir_pattern.as_ref().and_then(|p| {
        GlobBuilder::new(p)
            .case_insensitive(true)
            .build()
            .ok()
            .map(|g| g.compile_matcher())
    });

    let mut search_roots = Vec::new();
    if let Some(parent) = old_dir.parent() {
        search_roots.push(parent.to_path_buf());
        if let Some(gp) = parent.parent() {
            search_roots.push(gp.to_path_buf());
        }
    }

    let mut found: Vec<(Vec<u64>, std::time::SystemTime, PathBuf)> = Vec::new();
    for root in search_roots {
        for entry in walkdir::WalkDir::new(&root)
            .min_depth(1)
            .max_depth(2)
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_dir() {
                continue;
            }
            let dir = entry.path();
            if util::same_path(&dir.to_string_lossy(), &old_dir.to_string_lossy()) {
                continue;
            }
            let dir_name = entry.file_name().to_string_lossy().into_owned();
            if let Some(m) = &pattern {
                if !m.is_match(&dir_name) {
                    continue;
                }
            }
            let Some(exe) = util::find_file_ci(dir, &rule.exe) else {
                continue;
            };
            if let Some(sib) = &rule.sibling {
                if util::find_file_ci(dir, sib).is_none() {
                    continue;
                }
            }
            let mtime = std::fs::metadata(&exe)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            found.push((version_key(&dir_name), mtime, exe));
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    found.into_iter().next().map(|(_, _, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_patterns() {
        assert_eq!(derive_pattern("fefw-recomp-0.2.6").as_deref(), Some("fefw-recomp*"));
        assert_eq!(derive_pattern("shadPS4-0.7.0").as_deref(), Some("shadPS4*"));
        assert_eq!(derive_pattern("BloodbornePC"), None);
    }

    #[test]
    fn finds_newer_version() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("fefw-recomp-0.2.6");
        let new = tmp.path().join("fefw-recomp-0.2.7");
        let newer = tmp.path().join("fefw-recomp-0.10.0");
        for d in [&new, &newer] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("fefw.exe"), b"x").unwrap();
        }
        let game = GameEntry {
            key: "fe".into(),
            exe: Some(old.join("fefw.exe")),
            ..Default::default()
        };
        assert_eq!(find_candidate(&game), Some(newer.join("fefw.exe")));
    }
}
