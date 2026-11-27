//! Games whose saves are the same files on several platforms
//! (`rules/games/<id>.yaml`, one file per game; docs/plan-v2.md section 11).
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
//!
//! The files are built into the program (`build.rs`). [`check_file`] is what
//! `ludonimbus rules check` runs on one: its fields, and the sample saves
//! next to it (`rules/games/<id>/<platform>/`).

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

/// A game's rule, as the program uses it ([`parse`] makes it from a file).
#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    pub name: String,
    /// The name in other languages, by language tag (`zh-CN`).
    pub names: BTreeMap<String, String>,
    /// The versions it covers, by library id (`nx:…`, `ludusavi:…`, `name:…`).
    pub ids: Vec<String>,
    /// Save scope → the ending of the saves' folder, or [`NXSAVE`].
    pub folders: BTreeMap<String, String>,
    /// Which files in the folder are saves (wildcard, relative to it).
    pub files: String,
    pub version: Option<VersionRule>,
    /// An older game can't load a save a newer one wrote.
    pub newer_only: bool,
    /// What has been tried for real, by pair of platforms (`pc-switch`).
    pub checked: BTreeMap<String, String>,
}

impl Rule {
    /// The game's name in the language the app shows.
    pub fn display_name(&self) -> &str {
        let lang = if crate::i18n::is_en() { "en" } else { "zh-CN" };
        self.names.get(lang).map(String::as_str).unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionRule {
    /// The file with the version (wildcard, relative to the folder).
    pub file: String,
    /// The XML element whose text is the version.
    pub xml: String,
}

/// A game's rule as written in `rules/games/<id>.yaml` (docs/plan-v2.md,
/// section 11). Unknown fields are refused, so a misspelt one is noticed.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleFile {
    id: String,
    name: String,
    #[serde(default)]
    names: BTreeMap<String, String>,
    ids: Vec<String>,
    lines: Vec<LineFile>,
}

/// One save line of a game: its versions on these platforms keep the same
/// files. One per game for now.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LineFile {
    id: String,
    #[serde(default = "every_file")]
    files: String,
    platforms: BTreeMap<String, PlatformFile>,
    #[serde(default)]
    version: Option<VersionFile>,
    #[serde(default)]
    checked: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlatformFile {
    /// The ending of the saves' folder's path, or [`NXSAVE`].
    saves: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct VersionFile {
    file: String,
    xml: String,
    #[serde(default)]
    newer_only: bool,
}

fn every_file() -> String {
    "**".into()
}

include!(concat!(env!("OUT_DIR"), "/game_rules.rs"));

/// A game's rule from the text of its file (`name`: the file's name, for
/// messages). Only what the program can't do without is required here;
/// [`check_file`] says the rest.
pub fn parse(name: &str, text: &str) -> anyhow::Result<Rule> {
    let file: RuleFile = serde_yaml::from_str(text).map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
    let mut lines = file.lines.into_iter();
    let (Some(line), None) = (lines.next(), lines.next()) else {
        anyhow::bail!("{name}: a rule has exactly one line for now");
    };
    if line.id != "main" {
        anyhow::bail!("{name}: the line's id is \"main\"");
    }
    let newer_only = line.version.as_ref().is_some_and(|v| v.newer_only);
    Ok(Rule {
        id: file.id,
        name: file.name,
        names: file.names,
        ids: file.ids,
        folders: line.platforms.into_iter().map(|(scope, p)| (scope, p.saves)).collect(),
        files: line.files,
        version: line.version.map(|v| VersionRule { file: v.file, xml: v.xml }),
        newer_only,
        checked: line.checked,
    })
}

pub fn rules() -> &'static [Rule] {
    static R: OnceLock<Vec<Rule>> = OnceLock::new();
    R.get_or_init(|| GAME_RULES.iter().map(|(name, text)| parse(name, text).unwrap_or_else(|e| panic!("rules/games: {e:#}"))).collect())
}

/// What [`check_file`] found: problems to fix, and what it saw.
#[derive(Debug, Default)]
pub struct Check {
    pub problems: Vec<String>,
    pub notes: Vec<String>,
}

/// Look over a game's rule file before it goes in (`ludonimbus rules check`):
/// its fields, that no other rule next to it claims its game, and its sample
/// saves (`<id>/<platform>/` beside the file: a save folder as the game
/// leaves it on that platform). At least two platforms need a sample; each
/// must have saves the rule names, the game version must be readable from
/// them when the rule says how, and all samples must hold the same saves.
pub fn check_file(path: &Path) -> anyhow::Result<Check> {
    let text = std::fs::read_to_string(path)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = Check::default();
    let rule = match parse(&name, &text) {
        Ok(r) => r,
        Err(e) => {
            out.problems.push(format!("{e:#}"));
            return Ok(out);
        }
    };
    let p = &mut out.problems;
    let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let kebab = !rule.id.is_empty() && rule.id.split('-').all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    if !kebab {
        p.push(format!("id \"{}\": lowercase letters and digits joined by \"-\"", rule.id));
    }
    if rule.id != stem {
        p.push(format!("id \"{}\" must be the file's name ({stem}.yaml)", rule.id));
    }
    if rule.name.trim().is_empty() {
        p.push("name is empty".into());
    }
    if rule.ids.is_empty() {
        p.push("ids: which versions it covers (nx:…, ludusavi:…, name:…)".into());
    }
    for id in &rule.ids {
        let ok = match id.split_once(':') {
            Some(("nx", t)) => t.len() == 16 && t.chars().all(|c| c.is_ascii_hexdigit()),
            Some(("ludusavi", t)) => !t.trim().is_empty(),
            Some(("name", t)) => !t.is_empty() && discover::match_key(t) == t,
            _ => false,
        };
        if !ok {
            p.push(format!("ids: \"{id}\" is none of nx:<16 hex digits>, ludusavi:<title>, name:<lowercase letters and digits>"));
        }
    }
    if rule.folders.len() < 2 {
        p.push("platforms: at least two (the platforms whose versions keep the same files)".into());
    }
    for (scope, folder) in &rule.folders {
        if scope != "pc" && scope != "switch" && crate::adapters::find(scope).is_none() {
            p.push(format!("platforms: \"{scope}\" is none of pc, switch or a program's key in adapters/"));
        }
        if folder == NXSAVE {
            if scope != "switch" {
                p.push(format!("platforms.{scope}: {NXSAVE} is a Switch game's save data, only for switch"));
            }
        } else if folder.trim().is_empty() || folder.contains(['{', '}', '\\', ':']) || folder.starts_with('/') || folder.split('/').any(|c| c == "..") {
            p.push(format!("platforms.{scope}.saves \"{folder}\": the ending of the saves' folder's path, such as Game/Saves"));
        }
    }
    if glob(&rule.files).is_none() {
        p.push(format!("files \"{}\" is not a wildcard", rule.files));
    }
    if let Some(v) = &rule.version {
        if glob(&v.file).is_none() {
            p.push(format!("version.file \"{}\" is not a wildcard", v.file));
        }
        if v.xml.trim().is_empty() {
            p.push("version.xml: the XML element with the game version".into());
        }
    }
    for pair in rule.checked.keys() {
        let ok = pair.split_once('-').is_some_and(|(a, b)| a != b && rule.folders.contains_key(a) && rule.folders.contains_key(b));
        if !ok {
            p.push(format!("checked: \"{pair}\" is not two of its platforms, as pc-switch"));
        }
    }
    // Another rule next to it for the same game.
    let dir = path.parent().unwrap_or(Path::new("."));
    for other in std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()) {
        if other == path || other.extension().is_none_or(|x| x != "yaml") {
            continue;
        }
        let Ok(o) = std::fs::read_to_string(&other).map_err(anyhow::Error::from).and_then(|t| parse(&other.display().to_string(), &t)) else { continue };
        if o.id == rule.id {
            p.push(format!("{} has the id \"{}\" too", other.display(), rule.id));
        }
        for id in rule.ids.iter().filter(|i| o.ids.iter().any(|x| x.eq_ignore_ascii_case(i))) {
            p.push(format!("{} covers \"{id}\" too", other.display()));
        }
    }
    check_samples(&rule, &dir.join(&rule.id), &mut out);
    Ok(out)
}

/// The sample saves of `rule` under `dir` (one folder per platform).
fn check_samples(rule: &Rule, dir: &Path, out: &mut Check) {
    let mut seen: Vec<(String, Vec<String>)> = Vec::new();
    for scope in rule.folders.keys() {
        let root = dir.join(scope);
        if !root.is_dir() {
            continue;
        }
        let mut files: Vec<(String, PathBuf)> = walkdir::WalkDir::new(&root)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| Some((e.path().strip_prefix(&root).ok()?.to_string_lossy().replace('\\', "/"), e.path().to_path_buf())))
            .collect();
        files.sort();
        let saves: Vec<(String, PathBuf)> = files.into_iter().filter(|(rel, _)| matches(&rule.files, rel)).collect();
        if saves.is_empty() {
            out.problems.push(format!("sample {scope}: no file the rule's files (\"{}\") names", rule.files));
            continue;
        }
        let version = newest_version(rule, &saves, |p| std::fs::read(p).ok());
        if rule.version.is_some() && version.is_none() {
            out.problems.push(format!("sample {scope}: no game version could be read from the saves"));
        }
        out.notes.push(format!("{scope}: {} save file(s){}", saves.len(), version.map(|v| format!(", game version {v}")).unwrap_or_default()));
        seen.push((scope.clone(), saves.into_iter().map(|(rel, _)| rel.to_lowercase()).collect()));
    }
    if seen.len() < 2 {
        out.problems.push(format!("samples: a save folder for at least two platforms, as {}/<platform>/", dir.display()));
    }
    if let Some((first, files)) = seen.first() {
        for (scope, other) in &seen[1..] {
            if other != files {
                out.problems.push(format!("samples: {first} and {scope} hold different saves ({} and {})", files.join(", "), other.join(", ")));
            }
        }
    }
}

pub fn by_id(id: &str) -> Option<&'static Rule> {
    rules().iter().find(|r| r.id == id)
}

/// Whether library id `id` is the rule's `rule_id`. `name:<name>` in a rule
/// stands for that name on any platform (`name:<scope>:<name>`).
fn covers(rule_id: &str, id: &str) -> bool {
    let (r, i) = (rule_id.to_lowercase(), id.to_lowercase());
    match r.strip_prefix("name:") {
        Some(name) => i.strip_prefix("name:").and_then(|rest| rest.split_once(':')).is_some_and(|(_, n)| n == discover::match_key(name)),
        None => r == i,
    }
}

/// The rule for a version with library ids `ids` on platform `scope`
/// ([`savelib::save_scope`]).
pub fn rule_for_ids(ids: &[String], scope: &str) -> Option<&'static Rule> {
    rules().iter().find(|r| r.folders.contains_key(scope) && r.ids.iter().any(|rid| ids.iter().any(|i| covers(rid, i))))
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

/// Whether the wildcard `pattern` names the path `rel`, the way a rule's
/// `files` do: any letter case, `*` doesn't cross `/`. A pattern that isn't
/// one names nothing. What [`crate::bind::Bind::files`] goes by; compiled
/// once per pattern and thread.
pub fn matches(pattern: &str, rel: &str) -> bool {
    thread_local! {
        static SEEN: std::cell::RefCell<std::collections::HashMap<String, Option<GlobMatcher>>> = Default::default();
    }
    SEEN.with(|seen| seen.borrow_mut().entry(pattern.to_string()).or_insert_with(|| glob(pattern)).as_ref().is_some_and(|m| m.is_match(rel)))
}

/// Whether `dir` (a folder among a snapshot's paths, `root/rest`) is the
/// rule's folder for `scope`, as a version of that platform writes it: for
/// a Switch game's save data its `nxsave-…` root itself, else a folder whose
/// path ends with the rule's folder.
pub fn is_folder(rule: &Rule, scope: &str, dir: &str) -> bool {
    let Some(folder) = rule.folders.get(scope) else { return false };
    if folder == NXSAVE {
        return !dir.contains('/') && matches!(switch::parse_root(&dir.to_lowercase()), Some((SaveKind::Account, _)));
    }
    // Not a Switch game's save data, whatever it is called.
    if switch::parse_root(&dir.split('/').next().unwrap_or_default().to_lowercase()).is_some() {
        return false;
    }
    after_folder(&format!("{dir}/x"), folder).as_deref() == Some("x")
}

/// Whether the rule's two platforms `a` and `b` are known to keep the very
/// same files, so that versions on them would share one save line without
/// asking (docs/plan-v2.md, sections 6 and 11: "直接共用").
///
/// No pair is yet: `checked` is free text, not a verdict, and every pair a
/// rule gives is asked once ([`crate::join::suggestion`]). This is the one
/// place where a verified pair would plug in: [`crate::join::auto`] would
/// take the rule's bindings for such a version when no device has
/// confirmed any.
pub fn verified(_rule: &Rule, _a: &str, _b: &str) -> bool {
    false
}

/// `found` is a newer game version than `here` and the rule says an older
/// game can't load what a newer one saved: a save written by `found` may
/// not load here. Unknown or uncomparable versions are never that.
pub fn too_new(rule: &Rule, found: Option<&str>, here: Option<&str>) -> bool {
    match (found, here) {
        (Some(found), Some(here)) => rule.newer_only && compare(found, here) == Some(Ordering::Greater),
        _ => false,
    }
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
        return roots.iter().find(|(name, p)| matches!(switch::parse_root(name), Some((SaveKind::Account, _))) && p.is_dir()).map(|(_, p)| p.clone());
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
/// and `roots` as [`crate::snapshot::scan`] gives them). A version that
/// shares the line of another platform through the rule's bindings
/// ([`crate::join::by_rule`]) has them under that platform's paths there:
/// they are looked at under the version's own.
pub fn local_version(g: &GameEntry, files: &[LocalFile], roots: &BTreeMap<String, PathBuf>) -> Option<String> {
    let rule = rule_for(g)?;
    rule.version.as_ref()?;
    let roots: BTreeMap<String, String> = roots.iter().map(|(k, v)| (k.clone(), v.to_string_lossy().into_owned())).collect();
    let bound = crate::join::by_rule(g).is_some();
    let own: Vec<(String, &Path)> = files
        .iter()
        .filter_map(|f| match bound {
            false => Some((f.logical.clone(), f.abs.as_path())),
            true => crate::bind::to_local(&g.bind, &f.logical).map(|own| (own, f.abs.as_path())),
        })
        .collect();
    let saves = saves_of(rule, &savelib::save_scope(g), own.iter().map(|(p, abs)| (p.as_str(), *abs)), &roots);
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
