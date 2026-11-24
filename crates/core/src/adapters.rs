//! Built-in save rules for games that no database knows about
//! (emulators, recompilations, fan ports). Three files, read in this order
//! (the first rule that matches best wins):
//!
//! - `adapters/builtin.yaml`: hand-written rules for recomps and ports
//! - `adapters/emulators.yaml`: emulators; per-game saves when the game a
//!   shortcut starts can be identified (see [`crate::emulators`])
//! - `adapters/recomplauncher.yaml`: generated from RecompLauncher's catalog
//!   by `scripts/import-recomplauncher.py`

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// One game.
    #[default]
    Game,
    /// An emulator: many games share it.
    Emulator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Adapter {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub kind: Kind,
    /// Emulators: the library icon, a generic shape of the device it
    /// emulates (`cube`, `hybrid`, `disc`…; see `app/src/source.ts`).
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(rename = "match", default)]
    pub matcher: Option<Matcher>,
    /// RecompLauncher catalog ids: its Steam shortcuts start
    /// `recomplauncher.exe --play <id>`.
    #[serde(default)]
    pub recomplauncher: Vec<String>,
    #[serde(default)]
    pub roots: BTreeMap<String, String>,
    #[serde(default)]
    pub saves: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub wait_for: Vec<String>,
    #[serde(default)]
    pub quiesce: Quiesce,
    #[serde(default)]
    pub relocate: Option<Relocate>,
    /// Upload as soon as the game process (`waitFor`) exits, without waiting
    /// for a launcher that stays open.
    #[serde(default)]
    pub upload_on_game_exit: bool,
    /// Emulators: how to tell which game a shortcut starts (see
    /// [`crate::emulators`]).
    #[serde(default)]
    pub game_id: Option<String>,
    /// Emulators: the saves of one game, by kind of game (`default`, or e.g.
    /// `gc` / `wii`). `<name>` placeholders are filled from the game id.
    #[serde(default)]
    pub game_saves: BTreeMap<String, Vec<String>>,
    /// Also leave out files that are never saves (programs, disc images,
    /// caches, logs). Set on generated rules, whose save folders are not
    /// checked by hand.
    #[serde(default)]
    pub skip_junk: bool,
}

impl Adapter {
    /// `platform` in the interface language. The rule files are written in
    /// Chinese; the few words they use are translated here.
    pub fn platform_label(&self) -> Option<String> {
        let p = self.platform.as_deref()?;
        if !crate::i18n::is_en() {
            return Some(p.to_string());
        }
        let mut s = p.to_string();
        for (zh, en) in [
            ("含 torzu 等分支", "incl. forks such as torzu"),
            ("多平台模拟器", "Multi-platform emulator"),
            ("模拟器", "emulator"),
            ("原生重制", "native remake"),
            ("同人重制", "fan remake"),
            ("移植", "port"),
            ("（", " ("),
            ("）", ")"),
        ] {
            s = s.replace(zh, en);
        }
        Some(s)
    }

    pub fn is_emulator(&self) -> bool {
        self.kind == Kind::Emulator
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Matcher {
    /// Exe file names, or paths relative to the folder `{install}` stands for
    /// (`game/xc2.exe`). `*` matches any characters within one name.
    #[serde(deserialize_with = "one_or_many")]
    pub exe: Vec<String>,
    /// A file that must be next to the exe.
    #[serde(default)]
    pub sibling: Option<String>,
    /// Only when the launch options contain this (case-insensitive).
    #[serde(default)]
    pub args: Option<String>,
}

fn one_or_many<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    })
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

const FILES: [(&str, &str); 3] = [
    ("builtin.yaml", include_str!("../../../adapters/builtin.yaml")),
    ("emulators.yaml", include_str!("../../../adapters/emulators.yaml")),
    ("recomplauncher.yaml", include_str!("../../../adapters/recomplauncher.yaml")),
];

pub fn builtin() -> &'static [Adapter] {
    static CELL: OnceLock<Vec<Adapter>> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut all: Vec<Adapter> = Vec::new();
        for (file, text) in FILES {
            let list: Vec<Adapter> =
                serde_yaml::from_str(text).unwrap_or_else(|e| panic!("adapters/{file} must be valid: {e}"));
            for a in list {
                // A generated rule never replaces a hand-written one.
                if !all.iter().any(|x| x.key == a.key) {
                    all.push(a);
                }
            }
        }
        all
    })
}

pub fn find(key: &str) -> Option<&'static Adapter> {
    builtin().iter().find(|a| a.key == key)
}

/// The rule for a RecompLauncher catalog id (`recomp--game` ids fall back
/// to the recomp).
pub fn find_recomplauncher(id: &str) -> Option<&'static Adapter> {
    let id = id.trim().to_lowercase();
    let base = id.split("--").next().unwrap_or(&id).to_string();
    for want in [&id, &base] {
        if let Some(a) = builtin().iter().find(|a| a.recomplauncher.iter().any(|r| r.eq_ignore_ascii_case(want))) {
            return Some(a);
        }
    }
    None
}

/// A rule matched to a game executable.
#[derive(Debug, Clone)]
pub struct Detected {
    pub adapter: &'static Adapter,
    /// The folder `{install}` stands for, when it is not the exe's own folder
    /// (the exe sits deeper in the release, or a launcher starts the game).
    pub install: Option<PathBuf>,
    /// Started through RecompLauncher (`recomplauncher.exe --play <id>`).
    pub via_recomplauncher: bool,
}

/// Guess the rule for a game executable.
pub fn detect(exe: &Path) -> Option<&'static Adapter> {
    detect_with(exe, "").map(|d| d.adapter)
}

/// Guess the rule for a game executable started with `args`.
pub fn detect_with(exe: &Path, args: &str) -> Option<Detected> {
    if let Some(d) = crate::recomplauncher::detect(exe, args) {
        return Some(d);
    }
    let comps: Vec<String> = exe
        .to_string_lossy()
        .split(['\\', '/'])
        .filter(|c| !c.is_empty())
        .map(str::to_lowercase)
        .collect();
    let args_l = args.to_lowercase();
    let mut best: Option<(usize, &'static Adapter, usize)> = None;
    for a in builtin() {
        let Some(m) = &a.matcher else { continue };
        if m.args.as_ref().is_some_and(|need| !args_l.contains(&need.to_lowercase())) {
            continue;
        }
        for pat in &m.exe {
            let parts: Vec<String> = pat.split(['\\', '/']).filter(|c| !c.is_empty()).map(str::to_lowercase).collect();
            if parts.is_empty() || parts.len() > comps.len() {
                continue;
            }
            let tail = &comps[comps.len() - parts.len()..];
            if !parts.iter().zip(tail).all(|(p, c)| wildcard(p, c)) {
                continue;
            }
            if let Some(sib) = &m.sibling {
                if !exe.parent().is_some_and(|d| crate::util::find_file_ci(d, sib).is_some()) {
                    continue;
                }
            }
            // Deeper paths, exact names, and extra conditions are more specific.
            let literal: usize = parts.iter().map(|p| p.chars().filter(|&c| c != '*').count()).sum();
            let score = parts.len() * 10_000
                + literal
                + if m.sibling.is_some() { 5_000 } else { 0 }
                + if m.args.is_some() { 5_000 } else { 0 };
            if best.as_ref().is_none_or(|(s, _, _)| score > *s) {
                best = Some((score, a, parts.len()));
            }
        }
    }
    best.map(|(_, adapter, depth)| Detected {
        adapter,
        install: (depth > 1).then(|| exe.ancestors().nth(depth).map(Path::to_path_buf)).flatten(),
        via_recomplauncher: false,
    })
}

/// `*` matches any run of characters; everything else literally.
fn wildcard(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !text.starts_with(first) || !text[first.len()..].ends_with(last) || text.len() < first.len() + last.len() {
        return false;
    }
    let mut rest = &text[first.len()..text.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

/// What to show for a detected rule: the name, and for emulators which game
/// (or that the whole emulator's saves go together).
pub fn label(adapter: &Adapter, game: Option<&crate::emulators::EmuGame>) -> String {
    if !adapter.is_emulator() {
        return adapter.name.clone();
    }
    match game {
        Some(g) => crate::tr!("{}（{}）", "{} ({})", adapter.name, g.id),
        None => crate::tr!("{}（整个模拟器的存档）", "{} (whole emulator)", adapter.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every emulator has a library icon the interface draws
    /// (`SOURCE_ICONS` in app/src/source.ts).
    #[test]
    fn emulator_icons() {
        const KNOWN: &[&str] = &["cube", "tablet", "dual-screen", "hybrid", "disc", "handheld", "layers"];
        for a in builtin().iter().filter(|a| a.is_emulator()) {
            let icon = a.icon.as_deref().unwrap_or_else(|| panic!("{}: no icon", a.key));
            assert!(KNOWN.contains(&icon), "{}: unknown icon {icon}", a.key);
        }
        assert!(builtin().iter().filter(|a| !a.is_emulator()).all(|a| a.icon.is_none()));
    }

    #[test]
    fn rules_parse() {
        let all = builtin();
        assert!(all.len() >= 100, "{}", all.len());
        assert!(find("fe-fortunes-weave").is_some());
        assert!(find("dolphin").is_some_and(Adapter::is_emulator));
        let mut keys = std::collections::HashSet::new();
        for a in all {
            assert!(keys.insert(a.key.as_str()), "duplicate key {}", a.key);
            assert!(!a.saves.is_empty() || !a.game_saves.is_empty(), "{} has no saves", a.key);
            for p in a.saves.iter().chain(a.game_saves.values().flatten()).chain(&a.exclude) {
                let (root, _) = crate::spec::split_pattern(p).unwrap_or_else(|e| panic!("{}: {e}", a.key));
                let known = ["install", "home", "appdata", "localappdata", "documents"].contains(&root.as_str())
                    || a.roots.contains_key(&root)
                    || (a.game_id.as_deref() == Some("switch") && crate::switch::is_root_template(&root));
                assert!(known, "{}: unknown root {{{root}}} in {p}", a.key);
            }
            if a.is_emulator() {
                assert!(a.matcher.is_some(), "{}", a.key);
            }
        }
    }

    #[test]
    fn wildcards() {
        assert!(wildcard("duckstation*.exe", "duckstation-qt-x64-releaseltcg.exe"));
        assert!(wildcard("*.exe", "a.exe"));
        assert!(wildcard("a*b*c", "aXXbYYc"));
        assert!(!wildcard("a*b*c", "aXXcYYb"));
        assert!(!wildcard("ab*ba", "aba"));
        assert!(wildcard("dolphin.exe", "dolphin.exe"));
    }

    #[test]
    fn detects_by_exe_name_and_path() {
        let a = detect(Path::new(r"D:\Games\fefw-recomp-1.2\fefw.exe")).unwrap();
        assert_eq!(a.key, "fe-fortunes-weave");
        let d = detect_with(Path::new(r"D:\Emu\Dolphin-x64\Dolphin.exe"), "-e \"D:\\ROMs\\x.iso\"").unwrap();
        assert_eq!(d.adapter.key, "dolphin");
        assert!(d.install.is_none());
        assert!(detect(Path::new(r"C:\Games\Unknown\game.exe")).is_none());
        // A nested exe: `{install}` is the release folder, not the exe's.
        let nested = builtin()
            .iter()
            .find_map(|a| {
                let m = a.matcher.as_ref()?;
                m.exe.iter().find(|e| e.contains('/') && !e.contains('*')).map(|e| (a, e.clone()))
            })
            .expect("some generated rule has a nested exe");
        let exe = PathBuf::from(r"E:\Recomps\Release").join(nested.1.replace('/', "\\"));
        let d = detect_with(&exe, "").unwrap();
        assert_eq!(d.install.as_deref(), Some(Path::new(r"E:\Recomps\Release")), "{}", nested.0.key);
    }

    #[test]
    fn launch_options_pick_the_rule() {
        let jak2 = detect_with(Path::new(r"C:\OpenGOAL\versions\official\v0.2\gk.exe"), "--game jak2 -boot").unwrap();
        assert_eq!(jak2.adapter.key, "opengoal-jak2");
        let jak1 = detect_with(Path::new(r"C:\OpenGOAL\versions\official\v0.2\gk.exe"), "-boot").unwrap();
        assert_eq!(jak1.adapter.key, "opengoal-jak1");
    }
}
