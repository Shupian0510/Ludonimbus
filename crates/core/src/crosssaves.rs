//! Games whose saves are the same files on several platforms
//! (`adapters/cross-saves.yaml`, docs/plan-phase5.md §5.6).
//!
//! Without a rule, only versions on the same platform are one game in the
//! save library ([`savelib::save_scope`]). A rule says that a game's
//! versions on several platforms write the same files: those versions get
//! the library id `fmt:<rule>` ([`savelib::ids`]) and so are one game, saves
//! move between them file for file into the rule's folder on each platform
//! ([`crate::transfer`]), and the game version that wrote a save is read
//! from the save itself and kept with each save version
//! ([`crate::snapshot::SnapshotMeta::game_version`]), so a save isn't
//! applied to a version too old to load it unnoticed.

use crate::config::GameEntry;
use crate::snapshot::LocalFile;
use crate::switch::{self, SaveKind};
use crate::{discover, savelib};
use globset::{GlobBuilder, GlobMatcher};
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The `folders` value for a Switch game's save data itself.
pub const NXSAVE: &str = "{nxsave}";

/// Decompressed saves larger than this aren't read for their version.
const MAX_READ: u64 = 64 << 20;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub name: String,
    /// The versions it covers, by library id (`nx:…`, `ludusavi:…`, `name:…`).
    pub ids: Vec<String>,
    /// Save scope → the ending of the saves' folder, or [`NXSAVE`].
    pub folders: BTreeMap<String, String>,
    /// Which files in the folder are saves (wildcard, relative to it).
    #[serde(default = "every_file")]
    pub files: String,
    #[serde(default)]
    pub version: Option<VersionRule>,
    /// An older game can't load a save a newer one wrote.
    #[serde(default)]
    pub newer_only: bool,
    #[serde(default)]
    pub checked: String,
}

fn every_file() -> String {
    "**".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionRule {
    /// The file with the version (wildcard, relative to the folder).
    pub file: String,
    /// The XML element whose text is the version.
    pub xml: String,
}

pub fn rules() -> &'static [Rule] {
    static R: OnceLock<Vec<Rule>> = OnceLock::new();
    R.get_or_init(|| serde_yaml::from_str(include_str!("../../../adapters/cross-saves.yaml")).expect("adapters/cross-saves.yaml must be valid"))
}

pub fn by_id(id: &str) -> Option<&'static Rule> {
    rules().iter().find(|r| r.id == id)
}

/// Whether library id `id` is the rule's `rule_id`. `name:<name>` in a rule
/// stands for that name on any platform (`name:<scope>:<name>`).
fn covers(rule_id: &str, id: &str) -> bool {
    let (r, i) = (rule_id.to_lowercase(), id.to_lowercase());
    match r.strip_prefix("name:") {
        Some(name) => i
            .strip_prefix("name:")
            .and_then(|rest| rest.split_once(':'))
            .is_some_and(|(_, n)| n == discover::match_key(name)),
        None => r == i,
    }
}

/// The rule for a version with library ids `ids` on platform `scope`
/// ([`savelib::save_scope`]).
pub fn rule_for_ids(ids: &[String], scope: &str) -> Option<&'static Rule> {
    rules()
        .iter()
        .find(|r| r.folders.contains_key(scope) && r.ids.iter().any(|rid| ids.iter().any(|i| covers(rid, i))))
}

/// The rule a version's saves follow.
pub fn rule_for(g: &GameEntry) -> Option<&'static Rule> {
    savelib::ids(g).iter().find_map(|i| i.strip_prefix("fmt:")).and_then(by_id)
}

/// The rule two versions share, so saves move between them file for file.
pub fn shared(a: &GameEntry, b: &GameEntry) -> Option<&'static Rule> {
    let r = rule_for(a)?;
    (rule_for(b)?.id == r.id).then_some(r)
}

fn glob(pattern: &str) -> Option<GlobMatcher> {
    GlobBuilder::new(pattern).case_insensitive(true).literal_separator(true).build().ok().map(|g| g.compile_matcher())
}

fn lower_parts(folder: &str) -> Vec<String> {
    folder.split(['/', '\\']).filter(|c| !c.is_empty()).map(str::to_lowercase).collect()
}

/// What follows the last `folder` (whole path parts, any case) in `path`.
fn after_folder(path: &str, folder: &str) -> Option<String> {
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|c| !c.is_empty()).collect();
    let want = lower_parts(folder);
    if want.is_empty() || parts.len() <= want.len() {
        return None;
    }
    (0..parts.len() - want.len())
        .rev()
        .find(|&i| parts[i..i + want.len()].iter().zip(&want).all(|(p, w)| p.to_lowercase() == *w))
        .map(|i| parts[i + want.len()..].join("/"))
}

/// Where a save file is relative to the rule's folder on `scope`, when it is
/// one of the saves (`files`). `logical` is its path in a snapshot
/// (`root/rest`); `root_abs` is where that root was on the device that saved
/// it (unknown for the Switch's own uploads).
pub fn relative(rule: &Rule, scope: &str, logical: &str, root_abs: Option<&str>) -> Option<String> {
    let folder = rule.folders.get(scope)?;
    let (root, rest) = logical.split_once('/')?;
    let rel = if folder == NXSAVE {
        let account = matches!(switch::parse_root(&root.to_lowercase()), Some((SaveKind::Account, _)));
        account.then(|| rest.to_string())?
    } else {
        let full = match root_abs {
            Some(abs) => format!("{}/{rest}", abs.replace('\\', "/").trim_end_matches('/')),
            None => logical.to_string(),
        };
        after_folder(&full, folder)?
    };
    glob(&rule.files)?.is_match(&rel).then_some(rel)
}

/// The saves among a snapshot's files: `(path relative to the rule's folder,
/// the item)`. `roots`: the snapshot's roots.
pub fn saves_of<'a, T>(rule: &Rule, scope: &str, files: impl IntoIterator<Item = (&'a str, T)>, roots: &BTreeMap<String, String>) -> Vec<(String, T)> {
    files
        .into_iter()
        .filter_map(|(logical, item)| {
            let root = logical.split('/').next().unwrap_or_default();
            relative(rule, scope, logical, roots.get(root).map(String::as_str)).map(|rel| (rel, item))
        })
        .collect()
}

fn ends_with(p: &Path, want: &[String]) -> bool {
    let parts: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
    parts.len() >= want.len() && parts[parts.len() - want.len()..] == *want
}

/// The rule's folder for a version on this device: where its saves are now
/// (`files`), or else below one of its save locations (`roots`) where it
/// exists (`…/StardewValley` + `Saves`).
pub fn folder_on_device(rule: &Rule, scope: &str, files: &[LocalFile], roots: &BTreeMap<String, PathBuf>) -> Option<PathBuf> {
    let folder = rule.folders.get(scope)?;
    if folder == NXSAVE {
        return roots
            .iter()
            .find(|(name, p)| matches!(switch::parse_root(name), Some((SaveKind::Account, _))) && p.is_dir())
            .map(|(_, p)| p.clone());
    }
    let want = lower_parts(folder);
    let parts: Vec<&str> = folder.split(['/', '\\']).filter(|c| !c.is_empty()).collect();
    if let Some(dir) = files.iter().find_map(|f| f.abs.ancestors().skip(1).find(|a| ends_with(a, &want))) {
        return Some(dir.to_path_buf());
    }
    let mut best: Option<PathBuf> = None;
    for root in roots.values() {
        // The most of the folder's leading parts the root already ends with.
        let k = (0..=want.len()).rev().find(|&k| ends_with(root, &want[..k])).unwrap_or(0);
        let dir = parts[k..].iter().fold(root.clone(), |p, c| p.join(c));
        if dir.is_dir() && best.as_ref().is_none_or(|b| dir.components().count() > b.components().count()) {
            best = Some(dir);
        }
    }
    best
}

/// Contents of a save file, decompressed when it is gzip or zlib data.
fn inflate(data: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    let read = match data.as_slice() {
        [0x1f, 0x8b, ..] => flate2::read::GzDecoder::new(data.as_slice()).take(MAX_READ).read_to_end(&mut out),
        [0x78, b, ..] if (0x7800u16 | u16::from(*b)).is_multiple_of(31) => flate2::read::ZlibDecoder::new(data.as_slice()).take(MAX_READ).read_to_end(&mut out),
        _ => return data,
    };
    if read.is_ok() {
        out
    } else {
        data
    }
}

/// The text of the first `<tag>` element.
fn xml_text(data: &[u8], tag: &str) -> Option<String> {
    let text = String::from_utf8_lossy(data);
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(i) = text[from..].find(&open) {
        let after = from + i + open.len();
        match text[after..].chars().next() {
            Some('>') | Some(' ') | Some('\t') | Some('\r') | Some('\n') => {
                let start = after + text[after..].find('>')? + 1;
                if text[..start].ends_with("/>") {
                    return None;
                }
                let end = start + text[start..].find('<')?;
                let v = text[start..end].trim();
                return (!v.is_empty()).then(|| v.to_string());
            }
            _ => from = after,
        }
    }
    None
}

/// Compare dotted versions (`1.6.15` > `1.6.9`); `None` when one isn't
/// made of numbers.
pub fn compare(a: &str, b: &str) -> Option<Ordering> {
    let parse = |s: &str| -> Option<Vec<u64>> { s.trim().trim_start_matches(['v', 'V']).split('.').map(|p| p.parse().ok()).collect() };
    let (mut x, mut y) = (parse(a)?, parse(b)?);
    let n = x.len().max(y.len());
    x.resize(n, 0);
    y.resize(n, 0);
    Some(x.cmp(&y))
}

/// The game version that wrote the saves among `files` (`(path relative to
/// the rule's folder, item)`, `read` gives an item's contents): the newest
/// when there are several (one per farm…).
pub fn newest_version<T>(rule: &Rule, files: &[(String, T)], read: impl Fn(&T) -> Option<Vec<u8>>) -> Option<String> {
    let v = rule.version.as_ref()?;
    let m = glob(&v.file)?;
    files
        .iter()
        .filter(|(rel, _)| m.is_match(rel))
        .filter_map(|(_, item)| read(item))
        .filter_map(|data| xml_text(&inflate(data), &v.xml))
        .max_by(|a, b| compare(a, b).unwrap_or(Ordering::Equal))
}

/// The game version that wrote a version's saves on this device (`files`
/// and `roots` as [`crate::snapshot::scan`] gives them).
pub fn local_version(g: &GameEntry, files: &[LocalFile], roots: &BTreeMap<String, PathBuf>) -> Option<String> {
    let rule = rule_for(g)?;
    rule.version.as_ref()?;
    let roots: BTreeMap<String, String> = roots.iter().map(|(k, v)| (k.clone(), v.to_string_lossy().into_owned())).collect();
    let saves = saves_of(rule, &savelib::save_scope(g), files.iter().map(|f| (f.logical.as_str(), f.abs.as_path())), &roots);
    newest_version(rule, &saves, |p| std::fs::read(p).ok())
}

/// The game version that wrote saves given in memory (`(logical path,
/// contents)`), for a version with library ids `ids` on platform `scope`.
pub fn files_version(ids: &[String], scope: &str, files: &[(String, Vec<u8>)], roots: &BTreeMap<String, String>) -> Option<String> {
    let rule = rule_for_ids(ids, scope)?;
    let saves = saves_of(rule, scope, files.iter().map(|(p, d)| (p.as_str(), d)), roots);
    newest_version(rule, &saves, |d| Some((*d).clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn stardew() -> &'static Rule {
        by_id("stardew-valley").expect("the Stardew Valley rule")
    }

    fn entry(name: &str, saves: &[&str], ludusavi: Option<&str>) -> GameEntry {
        GameEntry {
            key: discover::match_key(name),
            name: name.into(),
            saves: saves.iter().map(|s| s.to_string()).collect(),
            ludusavi_title: ludusavi.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn rules_load() {
        assert!(!rules().is_empty());
        for r in rules() {
            assert!(!r.folders.is_empty() && !r.ids.is_empty(), "{}", r.id);
            assert!(glob(&r.files).is_some(), "{}", r.id);
            if let Some(v) = &r.version {
                assert!(glob(&v.file).is_some(), "{}", r.id);
            }
        }
    }

    #[test]
    fn versions_on_both_platforms_follow_the_rule() {
        let steam = entry("Stardew Valley", &[], Some("Stardew Valley"));
        let switch = entry("星露谷物语", &["{nxsave-0100e65002bb8000}/**"], None);
        let folder = entry("Stardew Valley", &["{home}/SDV/StardewValley/Saves/**"], None);
        for g in [&steam, &switch, &folder] {
            assert_eq!(rule_for(g).map(|r| r.id.as_str()), Some("stardew-valley"), "{}", g.name);
        }
        assert!(savelib::ids(&switch).contains(&"fmt:stardew-valley".to_string()));
        assert!(shared(&steam, &switch).is_some());
        // Another game, and Stardew in an emulator the rule has no folder for.
        assert!(rule_for(&entry("Celeste", &[], Some("Celeste"))).is_none());
        assert!(rule_for_ids(&["name:wiiu:stardewvalley".into()], "wiiu").is_none());
    }

    #[test]
    fn saves_relative_to_the_folder() {
        let r = stardew();
        // The Switch's save data holds the farm folders.
        assert_eq!(relative(r, "switch", "nxsave-0100e65002bb8000/Farm_1/Farm_1", None).as_deref(), Some("Farm_1/Farm_1"));
        assert_eq!(relative(r, "switch", "nxsave-0100e65002bb8000/startup_preferences", None), None);
        assert_eq!(relative(r, "switch", "nxdevice-0100e65002bb8000/Farm_1/Farm_1", None), None);
        // PC: wherever the root was, the part after StardewValley/Saves.
        let roots = Some(r"C:\Users\me\AppData\Roaming");
        assert_eq!(relative(r, "pc", "winappdata/StardewValley/Saves/Farm_1/SaveGameInfo", roots).as_deref(), Some("Farm_1/SaveGameInfo"));
        assert_eq!(relative(r, "pc", "data/Saves/Farm_1/SaveGameInfo", Some("/home/me/.config/StardewValley")).as_deref(), Some("Farm_1/SaveGameInfo"));
        assert_eq!(relative(r, "pc", "winappdata/StardewValley/default_options", roots), None);
        assert_eq!(relative(r, "pc", "winappdata/StardewValley/Saves/Farm_1/ErrorLogs/x/y", roots), None);
    }

    #[test]
    fn finds_the_folder_on_this_device() {
        let tmp = tempfile::tempdir().unwrap();
        let r = stardew();
        let appdata = tmp.path().join("Roaming");
        let saves = appdata.join("StardewValley").join("Saves");
        std::fs::create_dir_all(saves.join("Farm_1")).unwrap();
        let roots = BTreeMap::from([("home".to_string(), tmp.path().to_path_buf()), ("winappdata".to_string(), appdata.clone())]);
        // No saves yet: below the save location where the folder exists.
        assert_eq!(folder_on_device(r, "pc", &[], &roots), Some(saves.clone()));
        let roots = BTreeMap::from([("game".to_string(), appdata.join("StardewValley"))]);
        assert_eq!(folder_on_device(r, "pc", &[], &roots), Some(saves.clone()));
        // Where its saves are.
        let f = LocalFile { logical: "x/Farm_1/Farm_1".into(), abs: saves.join("Farm_1").join("Farm_1"), size: 1, mtime: 0 };
        assert_eq!(folder_on_device(r, "pc", &[f], &BTreeMap::new()), Some(saves.clone()));
        // A Switch game's save data, once the game made it.
        let nx = tmp.path().join("bis").join("user").join("save").join("0000000000000003").join("0");
        let roots = BTreeMap::from([("nxsave-0100e65002bb8000".to_string(), nx.clone())]);
        assert_eq!(folder_on_device(r, "switch", &[], &roots), None);
        std::fs::create_dir_all(&nx).unwrap();
        assert_eq!(folder_on_device(r, "switch", &[], &roots), Some(nx));
    }

    #[test]
    fn reads_the_game_version() {
        let info = |v: &str| format!("<?xml version=\"1.0\"?><Farmer><name>A</name><gameVersion>{v}</gameVersion></Farmer>").into_bytes();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&info("1.6.15")).unwrap();
        let files = vec![
            ("Farm_1/SaveGameInfo".to_string(), info("1.6.9")),
            ("Farm_1/Farm_1".to_string(), info("9.9.9")),
            ("Farm_2/SaveGameInfo".to_string(), gz.finish().unwrap()),
        ];
        assert_eq!(newest_version(stardew(), &files, |d| Some(d.clone())).as_deref(), Some("1.6.15"));
        let logical: Vec<(String, Vec<u8>)> = files.iter().map(|(p, d)| (format!("nxsave-0100e65002bb8000/{p}"), d.clone())).collect();
        assert_eq!(files_version(&["nx:0100E65002BB8000".into()], "switch", &logical, &BTreeMap::new()).as_deref(), Some("1.6.15"));
        assert_eq!(xml_text(b"<a><gameVersionX>1</gameVersionX><gameVersion >2</gameVersion></a>", "gameVersion").as_deref(), Some("2"));
        assert_eq!(xml_text(b"<gameVersion/>", "gameVersion"), None);
    }

    #[test]
    fn compares_versions() {
        assert_eq!(compare("1.6.15", "1.6.9"), Some(Ordering::Greater));
        assert_eq!(compare("1.6", "1.6.0"), Some(Ordering::Equal));
        assert_eq!(compare("v1.5.6", "1.6"), Some(Ordering::Less));
        assert_eq!(compare("1.6-beta", "1.6"), None);
    }
}
