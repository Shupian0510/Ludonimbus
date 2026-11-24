//! Achievements decided by reading the game's save, for games that have no
//! achievement system of their own (Switch games on Ryujinx and other
//! emulators, Switch and N64 recompilations, native remakes).
//!
//! Lists use TrophyNX's format (github.com/Imp3r1us/TrophyNX,
//! `romfs/games/<TITLE ID>.json`): `reader`, `save_files`, and achievements
//! with one `rule` each. Two kinds:
//! - Ludonimbus's own lists, `adapters/achievements/<adapter key>.json`
//!   (MIT, bundled), for games Ludonimbus knows by adapter. They add
//!   `name_zh` / `desc_zh`. Bindings use the adapter key as the source id.
//! - TrophyNX's lists, by Switch title id. TrophyNX is GPL-2.0, so they are
//!   not bundled: they are downloaded from its repository when a game needs
//!   one and cached in `%APPDATA%\Ludonimbus\achievements\rules\trophynx\`.
//!
//! A file with the same name (`<title id>.json` or `<adapter key>.json`) in
//! `…\achievements\rules\` (made by hand or with TrophyNX's Save Lab) takes
//! precedence over both.
//!
//! Readers: `json` (optionally XOR-scrambled), `byml` (Nintendo BYML),
//! `gamedata_hash` (BotW: CRC32 name hash / value pairs from 0x0C),
//! `totk_progress` (TotK-style: Murmur3 name hash / value pairs from 0x28,
//! also used by Longshot), `binary_le` (fixed offsets). Rules: `flag`,
//! `count`, `sum`, `launches`, `playtime_hours`. With several save slots the
//! best one counts.

use super::{Achievement, Binding, Scan, Schema, SourceKind, Unlock};
use crate::config::{AppConfig, GameEntry};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const TROPHYNX_RAW: &str = "https://raw.githubusercontent.com/Imp3r1us/TrophyNX/main/romfs/games";
const REFETCH: Duration = Duration::from_secs(7 * 24 * 3600);

/// Switch recompilations and the game whose saves they keep.
const RECOMP_TITLES: &[(&str, &str)] = &[
    ("odysseyrecomp", "0100000000010000"),
    ("botw-vr-recomp", "01007EF00011E000"),
    ("kirby-forgotten-land-recompiled", "01004D300C5AE000"),
    ("plarecomp", "01001F5010DFA000"),
    ("pla-recomp-2", "01001F5010DFA000"),
    ("lza-recomp", "0100F43008C44000"),
];

/// Wii U games (Cemu, `{mlc}/usr/save/00050000/<low>`) whose saves have the
/// same structure as the Switch version's (BotW: the same `game_data.sav`,
/// big-endian).
const WIIU_TITLES: &[(&str, &str)] = &[
    ("101c9300", "01007EF00011E000"),
    ("101c9400", "01007EF00011E000"),
    ("101c9500", "01007EF00011E000"),
];

/// Ludonimbus's own lists, by adapter key (see `docs/achievement-rules.md`).
const BUNDLED: &[(&str, &str)] = &[("zelda-oot-longshot", include_str!("../../../../adapters/achievements/zelda-oot-longshot.json"))];

#[derive(Debug, Clone, Deserialize)]
pub struct RuleList {
    #[serde(default)]
    pub title_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub name_zh: Option<String>,
    #[serde(default)]
    pub reader: String,
    #[serde(default)]
    pub save_files: Vec<String>,
    #[serde(default)]
    pub magic: Option<String>,
    #[serde(default)]
    pub xor_key: Option<String>,
    #[serde(default)]
    pub achievements: Vec<RuleAchievement>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuleAchievement {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub name_zh: Option<String>,
    #[serde(default)]
    pub desc_zh: Option<String>,
    #[serde(default)]
    pub points: Option<u32>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub rule: Value,
}

impl RuleList {
    /// Rules that need the save (not only play statistics).
    pub fn reads_saves(&self) -> bool {
        self.achievements.iter().any(|a| !matches!(rule_type(&a.rule), "launches" | "playtime_hours"))
    }

    /// The game's name in the interface language.
    pub fn display_name(&self) -> String {
        zh_or(&self.name, self.name_zh.as_deref())
    }
}

/// `zh` when the interface is Chinese and there is one, else `en`.
fn zh_or(en: &str, zh: Option<&str>) -> String {
    match zh.filter(|z| !z.is_empty() && !crate::i18n::is_en()) {
        Some(z) => z.to_string(),
        None => en.to_string(),
    }
}

fn rule_type(rule: &Value) -> &str {
    rule["type"].as_str().unwrap_or("")
}

// ---------------------------------------------------------------------------
// Which game, which list

/// The Switch title id of a game: its recompilation, or the `{nxsave-…}`
/// root of an emulated game.
pub fn title_id(g: &GameEntry) -> Option<String> {
    if let Some((_, id)) = g.adapter.as_deref().and_then(|a| RECOMP_TITLES.iter().find(|(k, _)| *k == a)) {
        return Some(id.to_string());
    }
    let spec = crate::spec::effective(g);
    spec.saves.iter().find_map(|p| {
        let (root, rest) = crate::spec::split_pattern(p).ok()?;
        if let Some(hex) = root.strip_prefix("nxsave-").or_else(|| root.strip_prefix("nxdevice-")) {
            return (hex.len() == 16 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| hex.to_ascii_uppercase());
        }
        let rest = rest.to_ascii_lowercase();
        WIIU_TITLES.iter().find(|(low, _)| rest.contains(&format!("00050000/{low}"))).map(|(_, id)| id.to_string())
    })
}

/// Every name the list's rules look up (for hashed saves).
fn rule_names(list: &RuleList) -> Vec<String> {
    let mut out = Vec::new();
    for a in &list.achievements {
        if let Some(n) = a.rule["name"].as_str() {
            out.push(n.to_string());
        }
        out.extend(a.rule["names"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(str::to_string)));
        out.extend(pattern_names(&a.rule));
    }
    out
}

pub fn rules_dir() -> PathBuf {
    super::dir().join("rules")
}

fn valid_title(tid: &str) -> bool {
    tid.len() == 16 && tid.chars().all(|c| c.is_ascii_hexdigit())
}

/// Whether a binding's source id is a Switch title id (TrophyNX) rather
/// than an adapter key (Ludonimbus's own list).
pub fn is_title(id: &str) -> bool {
    valid_title(id.trim())
}

/// Ludonimbus's own list for an adapter key.
pub fn bundled(key: &str) -> Option<RuleList> {
    let (_, text) = BUNDLED.iter().find(|(k, _)| *k == key)?;
    match serde_json::from_str(text) {
        Ok(l) => Some(l),
        Err(e) => {
            super::log(format!("{key}: bundled achievement rules don't parse: {e}"));
            None
        }
    }
}

/// The adapter key of a game that has one of Ludonimbus's own lists.
pub fn bundled_key(g: &GameEntry) -> Option<String> {
    g.adapter.as_deref().filter(|a| BUNDLED.iter().any(|(k, _)| k == a)).map(str::to_string)
}

/// The list a binding points at: a local file, Ludonimbus's own list (adapter
/// key) or TrophyNX's (title id, downloaded when `fetch`).
pub fn load(id: &str, fetch: bool) -> Result<Option<RuleList>> {
    let id = id.trim();
    if is_title(id) {
        return load_list(id, fetch);
    }
    let local = rules_dir().join(format!("{id}.json"));
    if !id.is_empty() && !id.contains(['/', '\\', '.']) && local.is_file() {
        let bytes = std::fs::read(&local)?;
        return serde_json::from_slice(&bytes).with_context(|| local.display().to_string()).map(Some);
    }
    Ok(bundled(id))
}

fn age(p: &Path) -> Option<Duration> {
    std::fs::metadata(p).ok()?.modified().ok().and_then(|m| SystemTime::now().duration_since(m).ok())
}

/// The list for a title: local file, cached TrophyNX copy, or a fresh
/// download (`fetch`). `Ok(None)` when TrophyNX has no list for it.
pub fn load_list(tid: &str, fetch: bool) -> Result<Option<RuleList>> {
    let tid = tid.to_ascii_uppercase();
    if !valid_title(&tid) {
        bail!("bad title id {tid}");
    }
    let parse = |p: &Path| -> Result<RuleList> {
        let bytes = std::fs::read(p)?;
        serde_json::from_slice(&bytes).with_context(|| p.display().to_string())
    };
    let local = rules_dir().join(format!("{tid}.json"));
    if local.is_file() {
        return parse(&local).map(Some);
    }
    let cache = rules_dir().join("trophynx").join(format!("{tid}.json"));
    let none = rules_dir().join("trophynx").join(format!("{tid}.none"));
    let fresh = |p: &Path| p.is_file() && age(p).is_some_and(|a| a < REFETCH);
    if fresh(&cache) {
        return parse(&cache).map(Some);
    }
    if fresh(&none) || !fetch {
        return if cache.is_file() { parse(&cache).map(Some) } else { Ok(None) };
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .user_agent(&format!("Ludonimbus/{}", crate::APP_VERSION))
        .build();
    match agent.get(&format!("{TROPHYNX_RAW}/{tid}.json")).call() {
        Ok(r) => {
            let mut body = Vec::new();
            std::io::Read::read_to_end(&mut r.into_reader(), &mut body)?;
            let list: RuleList = serde_json::from_slice(&body).context("TrophyNX list")?;
            crate::util::atomic_write(&cache, &body)?;
            let _ = std::fs::remove_file(&none);
            Ok(Some(list))
        }
        Err(ureq::Error::Status(404, _)) => {
            crate::util::atomic_write(&none, b"")?;
            Ok(None)
        }
        // Offline: an old copy is better than nothing.
        Err(e) if cache.is_file() => {
            super::log(format!("{tid}: TrophyNX list not refreshed: {e}"));
            parse(&cache).map(Some)
        }
        Err(e) => Err(anyhow!(crate::cloud::Unavailable(e.to_string()))),
    }
}

// ---------------------------------------------------------------------------
// Save files

/// Folders the game's saves live in on this PC.
fn save_bases(g: &GameEntry) -> Vec<PathBuf> {
    let spec = crate::spec::effective(g);
    let Ok(roots) = crate::spec::resolve_roots(g, &spec) else { return Vec::new() };
    let mut out = Vec::new();
    for p in &spec.saves {
        let Ok((root, rest)) = crate::spec::split_pattern(p) else { continue };
        let Some(base) = roots.get(&root) else { continue };
        let mut dir = base.clone();
        for seg in rest.split('/').filter(|s| !s.is_empty()) {
            if seg.contains(['*', '?', '[']) {
                break;
            }
            dir.push(seg);
        }
        if dir.is_dir() && !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// The listed save files (paths inside the Switch save) found in the game's save folders.
pub fn find_save_files(g: &GameEntry, rels: &[String]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let wanted: Vec<String> = rels.iter().map(|r| r.replace('\\', "/").trim_matches('/').to_ascii_lowercase()).collect();
    for base in save_bases(g) {
        for (rel, low) in rels.iter().zip(&wanted) {
            let direct = base.join(rel);
            if direct.is_file() {
                if !out.contains(&direct) {
                    out.push(direct);
                }
                continue;
            }
            // Recompilations keep the save one or two folders deeper (account, slot…).
            for e in walkdir::WalkDir::new(&base).max_depth(6).into_iter().flatten() {
                if !e.file_type().is_file() {
                    continue;
                }
                let p = e.path().to_string_lossy().replace('\\', "/").to_ascii_lowercase();
                if p.ends_with(&format!("/{low}")) && !out.iter().any(|o| o == e.path()) {
                    out.push(e.into_path());
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Readers

enum Doc {
    Tree(Value),
    Hashes { map: HashMap<u32, u32>, hash: fn(&str) -> u32 },
    Bytes(Vec<u8>),
}

fn le32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

pub fn crc32(name: &str) -> u32 {
    crc32fast::hash(name.as_bytes())
}

/// MurmurHash3 (x86, 32-bit, seed 0), as TotK hashes its save keys.
pub fn murmur3(name: &str) -> u32 {
    let data = name.as_bytes();
    let (c1, c2) = (0xcc9e_2d51u32, 0x1b87_3593u32);
    let mut h = 0u32;
    let chunks = data.chunks_exact(4);
    let tail = chunks.remainder();
    for c in chunks {
        let mut k = u32::from_le_bytes(c.try_into().unwrap());
        k = k.wrapping_mul(c1).rotate_left(15).wrapping_mul(c2);
        h ^= k;
        h = h.rotate_left(13).wrapping_mul(5).wrapping_add(0xe654_6b64);
    }
    let mut k = 0u32;
    for (i, b) in tail.iter().enumerate() {
        k |= (*b as u32) << (8 * i);
    }
    if !tail.is_empty() {
        k = k.wrapping_mul(c1).rotate_left(15).wrapping_mul(c2);
        h ^= k;
    }
    h ^= data.len() as u32;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

fn be32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))
}

fn pairs(d: &[u8], from: usize, stop: Option<u32>) -> HashMap<u32, u32> {
    pairs_with(d, from, stop, le32)
}

fn pairs_with(d: &[u8], from: usize, stop: Option<u32>, read: fn(&[u8], usize) -> Option<u32>) -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    let mut o = from;
    while let (Some(k), Some(v)) = (read(d, o), read(d, o + 4)) {
        if Some(k) == stop {
            break;
        }
        // Arrays repeat their hash; the first entry is the value rules read.
        map.entry(k).or_insert(v);
        o += 8;
    }
    map
}

fn read_doc(list: &RuleList, data: &[u8]) -> Result<Doc> {
    match list.reader.as_str() {
        "json" => {
            let mut bytes = data.to_vec();
            if let Some(key) = list.xor_key.as_deref().filter(|k| !k.is_empty()) {
                let k = key.as_bytes();
                for (i, b) in bytes.iter_mut().enumerate() {
                    *b ^= k[i % k.len()];
                }
            }
            let text = String::from_utf8_lossy(&bytes);
            let text = text.trim_start_matches('\u{feff}').trim_end_matches('\0').trim();
            Ok(Doc::Tree(serde_json::from_str(text).context("JSON save")?))
        }
        "byml" => Ok(Doc::Tree(byml::parse(data)?)),
        "gamedata_hash" => {
            // Switch saves are little-endian, Wii U (Cemu) saves big-endian:
            // use the byte order in which more of the rules' flags exist.
            let le = pairs_with(data, 0x0C, None, le32);
            let be = pairs_with(data, 0x0C, None, be32);
            let names = rule_names(list);
            let hits = |m: &HashMap<u32, u32>| names.iter().filter(|n| m.contains_key(&crc32(n))).count();
            let map = if hits(&be) > hits(&le) { be } else { le };
            Ok(Doc::Hashes { map, hash: crc32 })
        }
        "totk_progress" => {
            if le32(data, 0) != Some(0x0102_0304) {
                bail!("not a TotK-style progress.sav");
            }
            Ok(Doc::Hashes { map: pairs(data, 0x28, Some(murmur3("MetaData.SaveTypeHash"))), hash: murmur3 })
        }
        "binary_le" => {
            if let Some(m) = list.magic.as_deref().filter(|m| !m.is_empty()) {
                if !data.starts_with(m.as_bytes()) {
                    bail!("save does not start with {m}");
                }
            }
            Ok(Doc::Bytes(data.to_vec()))
        }
        other => bail!(crate::tr!("还不支持这种存档格式：{other}", "This save format isn't supported yet: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Paths (TrophyNX syntax) and rules

fn num(v: &Value) -> f64 {
    match v {
        Value::Bool(b) => *b as u8 as f64,
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(_) => num(v) != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn same(v: &Value, want: &Value) -> bool {
    match (v, want) {
        (Value::String(a), Value::String(b)) => a == b,
        (Value::String(a), b) => a.trim() == b.to_string().trim_matches('"'),
        (a, Value::String(b)) => a.to_string().trim_matches('"') == b,
        (a, b) => num(a) == num(b),
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Walk a path like `ShineData[*]/IsGet`, `list[3]/x`, `Unlocks[Key=Ending_A]/Value`, `Tags[=Wanwan]`.
pub fn walk(root: &Value, path: &str) -> Vec<Value> {
    let mut cur = vec![root.clone()];
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let (name, sel) = match seg.find('[') {
            Some(i) if seg.ends_with(']') => (&seg[..i], Some(&seg[i + 1..seg.len() - 1])),
            _ => (seg, None),
        };
        let mut next = Vec::new();
        for c in cur {
            let base = if name.is_empty() { Some(c) } else { c.get(name).cloned() };
            let Some(base) = base else { continue };
            match sel {
                None => next.push(base),
                Some("*") => match base {
                    Value::Array(a) => next.extend(a),
                    Value::Object(o) => next.extend(o.into_iter().map(|(_, v)| v)),
                    _ => {}
                },
                Some(s) if s.chars().all(|c| c.is_ascii_digit()) => {
                    if let Some(v) = base.get(s.parse::<usize>().unwrap_or(usize::MAX)) {
                        next.push(v.clone());
                    }
                }
                Some(s) if s.starts_with('=') => {
                    let want = &s[1..];
                    let found = match &base {
                        Value::Array(a) => a.iter().any(|v| text(v) == want),
                        Value::Object(o) => o.contains_key(want),
                        _ => false,
                    };
                    next.push(json!(found as u8));
                }
                Some(s) => {
                    let Some((k, want)) = s.split_once('=') else { continue };
                    if let Value::Array(a) = base {
                        next.extend(a.into_iter().filter(|e| e.get(k).is_some_and(|v| text(v) == want)));
                    }
                }
            }
        }
        cur = next;
    }
    cur
}

fn offset(v: &Value) -> Option<usize> {
    match v {
        Value::Number(n) => n.as_u64().map(|n| n as usize),
        Value::String(s) => {
            let s = s.trim();
            match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                Some(h) => usize::from_str_radix(h, 16).ok(),
                None => s.parse().ok(),
            }
        }
        _ => None,
    }
}

fn pattern_names(rule: &Value) -> Vec<String> {
    let Some(p) = rule["pattern"].as_str() else { return Vec::new() };
    let (from, to) = (rule["from"].as_i64().unwrap_or(0), rule["to"].as_i64().unwrap_or(0));
    (from..=to.min(from + 10_000))
        .map(|i| {
            // printf-style %d / %0Nd
            match p.find('%') {
                Some(at) => {
                    let rest = &p[at + 1..];
                    let end = rest.find('d').unwrap_or(0);
                    let width: usize = rest[..end].trim_start_matches('0').parse().unwrap_or(0);
                    let pad = rest.starts_with('0');
                    let n = if pad { format!("{i:0width$}") } else { format!("{i}") };
                    format!("{}{}{}", &p[..at], n, &rest[end + 1..])
                }
                None => p.to_string(),
            }
        })
        .collect()
}

/// The values a rule looks at, in one save.
fn values(doc: &Doc, rule: &Value) -> Vec<Value> {
    let list = |key: &str| -> Vec<Value> { rule[key].as_array().cloned().unwrap_or_default() };
    match doc {
        Doc::Tree(root) => {
            let mut paths: Vec<String> = list("paths").iter().filter_map(|p| p.as_str().map(str::to_string)).collect();
            if let Some(p) = rule["path"].as_str() {
                paths.insert(0, p.to_string());
            }
            paths.iter().flat_map(|p| walk(root, p)).collect()
        }
        Doc::Hashes { map, hash } => {
            let mut names: Vec<String> = list("names").iter().filter_map(|p| p.as_str().map(str::to_string)).collect();
            if let Some(n) = rule["name"].as_str() {
                names.insert(0, n.to_string());
            }
            names.extend(pattern_names(rule));
            names.iter().filter_map(|n| map.get(&hash(n)).map(|v| json!(*v))).collect()
        }
        Doc::Bytes(data) => {
            let size = rule["size"].as_u64().unwrap_or(4) as usize;
            let max = rule["max"].as_f64();
            let mut offs: Vec<usize> = list("offsets").iter().filter_map(offset).collect();
            if let Some(o) = offset(&rule["offset"]) {
                offs.insert(0, o);
            }
            offs.iter()
                .filter_map(|&o| {
                    let b = data.get(o..o + size)?;
                    let mut v = 0u64;
                    for (i, byte) in b.iter().enumerate().take(8) {
                        v |= (*byte as u64) << (8 * i);
                    }
                    let v = v as f64;
                    // Values beyond `max` mean the offset doesn't fit this save version.
                    Some(json!(if max.is_some_and(|m| v > m) { 0.0 } else { v }))
                })
                .collect()
        }
    }
}

/// Whether `want` (TotK-style enum text) matches a hashed value.
fn equals_in(doc: &Doc, want: &Value) -> Value {
    match (doc, want) {
        (Doc::Hashes { hash, .. }, Value::String(s)) => json!(hash(s)),
        _ => want.clone(),
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    pub launches: u64,
    pub hours: f64,
}

/// Evaluate one rule in one save (`None`: needs a save that isn't readable).
fn holds(doc: Option<&Doc>, rule: &Value, stats: Stats) -> Option<bool> {
    let target = rule["target"].as_f64().unwrap_or(1.0);
    match rule_type(rule) {
        "launches" => return Some(stats.launches as f64 >= target),
        "playtime_hours" => return Some(stats.hours >= target),
        "flag" | "count" | "sum" => {}
        _ => return None, // memory rules and unknown types
    }
    let doc = doc?;
    let vals = values(doc, rule);
    let equals = rule.get("equals").map(|w| equals_in(doc, w));
    Some(match rule_type(rule) {
        "flag" => vals.first().is_some_and(|v| match &equals {
            Some(w) => same(v, w),
            None => num(v) >= target,
        }),
        "count" => {
            let n = vals
                .iter()
                .filter(|v| match &equals {
                    Some(w) => same(v, w),
                    None => truthy(v),
                })
                .count();
            n as f64 >= target
        }
        _ => vals.iter().map(num).sum::<f64>() >= target,
    })
}

// ---------------------------------------------------------------------------
// Source

fn stats(cfg: &AppConfig, g: &GameEntry) -> Stats {
    let summary = crate::playtime::summary(cfg, &g.key);
    let launches = crate::playtime::load_local(&g.key).map(|l| l.sessions.len() as u64).unwrap_or(0);
    Stats { launches, hours: summary.total_secs as f64 / 3600.0 }
}

fn mtime(p: &Path) -> DateTime<Utc> {
    std::fs::metadata(p).and_then(|m| m.modified()).map(DateTime::<Utc>::from).unwrap_or_else(|_| Utc::now())
}

/// Evaluate a list against save files (best slot counts) and play statistics.
pub fn evaluate(list: &RuleList, files: &[PathBuf], stats: Stats) -> Result<Vec<Unlock>> {
    let mut docs = Vec::new();
    let mut error = None;
    if list.reads_saves() {
        for f in files {
            match std::fs::read(f).map_err(anyhow::Error::from).and_then(|b| read_doc(list, &b)) {
                Ok(d) => docs.push((f.clone(), d)),
                Err(e) => error = Some(format!("{}: {e:#}", f.display())),
            }
        }
    }
    let mut unlocks = Vec::new();
    for a in &list.achievements {
        let at = match rule_type(&a.rule) {
            "launches" | "playtime_hours" => holds(None, &a.rule, stats).filter(|h| *h).map(|_| Utc::now()),
            _ => docs
                .iter()
                .filter(|(_, d)| holds(Some(d), &a.rule, stats) == Some(true))
                .map(|(f, _)| mtime(f))
                .min(),
        };
        if let Some(at) = at {
            unlocks.push(Unlock { id: a.id.clone(), unlocked_at: at, hardcore: false });
        }
    }
    if docs.is_empty() && list.reads_saves() {
        if let Some(e) = error {
            let who = if list.title_id.is_empty() { &list.name } else { &list.title_id };
            super::log(format!("{who}: save rules not evaluated: {e}"));
        }
    }
    Ok(unlocks)
}

pub fn schema(list: &RuleList, tid: &str) -> Schema {
    Schema {
        source: SourceKind::Rules,
        source_id: tid.to_string(),
        title: Some(list.display_name()).filter(|n| !n.is_empty()),
        achievements: list
            .achievements
            .iter()
            .map(|a| Achievement {
                id: a.id.clone(),
                name: Some(zh_or(&a.name, a.name_zh.as_deref())).filter(|n| !n.is_empty()).unwrap_or_else(|| a.id.clone()),
                description: zh_or(&a.desc, a.desc_zh.as_deref()),
                icon: None,
                hidden: a.hidden,
                points: a.points,
                grade: None,
            })
            .collect(),
    }
}

pub fn scan(cfg: &AppConfig, g: &GameEntry, b: &Binding) -> Result<Scan> {
    let tid = b.source_id.trim();
    let list = load(tid, true)?.ok_or_else(|| anyhow!(crate::tr!("没有这个游戏的成就规则（{tid}）", "No achievement rules for this game ({tid})")))?;
    let files = find_save_files(g, &list.save_files);
    let unlocks = evaluate(&list, &files, stats(cfg, g))?;
    Ok(Scan { schema: Some(schema(&list, tid)), unlocks, quiet: Vec::new() })
}

/// Save files to watch while the game runs (from the cached list, no download).
pub fn watch_files(g: &GameEntry, b: &Binding) -> Vec<PathBuf> {
    match load(&b.source_id, false) {
        Ok(Some(list)) if list.reads_saves() => find_save_files(g, &list.save_files),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// BYML (Nintendo binary YAML), versions 2–7, little or big endian

mod byml {
    use anyhow::{bail, Result};
    use serde_json::{json, Map, Value};

    struct R<'a> {
        d: &'a [u8],
        le: bool,
        keys: Vec<String>,
        strings: Vec<String>,
    }

    impl R<'_> {
        fn u16(&self, o: usize) -> Option<u16> {
            let b: [u8; 2] = self.d.get(o..o + 2)?.try_into().ok()?;
            Some(if self.le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
        }
        fn u32(&self, o: usize) -> Option<u32> {
            let b: [u8; 4] = self.d.get(o..o + 4)?.try_into().ok()?;
            Some(if self.le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
        }
        fn u64(&self, o: usize) -> Option<u64> {
            let b: [u8; 8] = self.d.get(o..o + 8)?.try_into().ok()?;
            Some(if self.le { u64::from_le_bytes(b) } else { u64::from_be_bytes(b) })
        }
        fn u24(&self, o: usize) -> Option<u32> {
            let b = self.d.get(o..o + 3)?;
            Some(if self.le {
                b[0] as u32 | (b[1] as u32) << 8 | (b[2] as u32) << 16
            } else {
                (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32
            })
        }
        fn string_table(&self, at: usize) -> Vec<String> {
            if at == 0 || self.d.get(at) != Some(&0xC2) {
                return Vec::new();
            }
            let n = self.u24(at + 1).unwrap_or(0) as usize;
            (0..n.min(1 << 20))
                .filter_map(|i| {
                    let start = at + self.u32(at + 4 + i * 4)? as usize;
                    let s = self.d.get(start..)?;
                    let end = s.iter().position(|&b| b == 0)?;
                    Some(String::from_utf8_lossy(&s[..end]).to_string())
                })
                .collect()
        }
        fn value(&self, kind: u8, raw: u32, depth: usize) -> Value {
            match kind {
                0xA0 => json!(self.strings.get(raw as usize).cloned().unwrap_or_default()),
                0xC0 | 0xC1 if depth < 64 => self.node(raw as usize, depth + 1),
                0xD0 => json!(raw != 0),
                0xD1 => json!(raw as i32),
                0xD2 => json!(f32::from_bits(raw)),
                0xD3 => json!(raw),
                0xD4 => json!(self.u64(raw as usize).map(|v| v as i64)),
                0xD5 => json!(self.u64(raw as usize)),
                0xD6 => json!(self.u64(raw as usize).map(f64::from_bits)),
                _ => Value::Null,
            }
        }
        fn node(&self, at: usize, depth: usize) -> Value {
            let Some(&kind) = self.d.get(at) else { return Value::Null };
            let n = self.u24(at + 1).unwrap_or(0) as usize;
            match kind {
                0xC0 => {
                    let values = at + 4 + ((n + 3) & !3);
                    Value::Array(
                        (0..n.min(1 << 20))
                            .map(|i| {
                                let k = self.d.get(at + 4 + i).copied().unwrap_or(0xFF);
                                self.value(k, self.u32(values + i * 4).unwrap_or(0), depth)
                            })
                            .collect(),
                    )
                }
                0xC1 => {
                    let mut m = Map::new();
                    for i in 0..n.min(1 << 20) {
                        let e = at + 4 + i * 8;
                        let (Some(key), Some(&k), Some(raw)) = (self.u24(e), self.d.get(e + 3), self.u32(e + 4)) else { break };
                        let name = self.keys.get(key as usize).cloned().unwrap_or_else(|| key.to_string());
                        m.insert(name, self.value(k, raw, depth));
                    }
                    Value::Object(m)
                }
                _ => Value::Null,
            }
        }
    }

    pub fn parse(data: &[u8]) -> Result<Value> {
        for base in [0usize, 0x10] {
            let Some(d) = data.get(base..) else { continue };
            let le = match d.get(0..2) {
                Some(b"YB") => true,
                Some(b"BY") => false,
                _ => continue,
            };
            let mut r = R { d, le, keys: Vec::new(), strings: Vec::new() };
            let version = r.u16(2).unwrap_or(0);
            if !(2..=10).contains(&version) {
                continue;
            }
            r.keys = r.string_table(r.u32(4).unwrap_or(0) as usize);
            r.strings = r.string_table(r.u32(8).unwrap_or(0) as usize);
            let root = r.u32(12).unwrap_or(0) as usize;
            return Ok(if root == 0 { Value::Null } else { r.node(root, 0) });
        }
        bail!("not a BYML file")
    }

    #[cfg(test)]
    pub fn build(le: bool) -> Vec<u8> {
        // { "ShineData": [ {"IsGet": true}, {"IsGet": false}, {"IsGet": true} ], "Name": "Mario", "Coins": 42 }
        let w32 = |v: u32| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        let hdr = |kind: u8, n: u32| {
            let mut b = vec![kind];
            let n = if le { n.to_le_bytes() } else { n.to_be_bytes() };
            if le { b.extend_from_slice(&n[..3]) } else { b.extend_from_slice(&n[1..]) }
            b
        };
        let table = |items: &[&str]| -> Vec<u8> {
            let mut b = hdr(0xC2, items.len() as u32);
            let mut off = 4 + 4 * (items.len() + 1);
            let mut body = Vec::new();
            for s in items {
                b.extend(w32(off as u32));
                body.extend(s.as_bytes());
                body.push(0);
                off += s.len() + 1;
            }
            b.extend(w32(off as u32));
            b.extend(body);
            while b.len() % 4 != 0 {
                b.push(0);
            }
            b
        };
        let keys = table(&["Coins", "IsGet", "Name", "ShineData"]);
        let strings = table(&["Mario"]);
        let mut d = vec![0u8; 16];
        d[0..2].copy_from_slice(if le { b"YB" } else { b"BY" });
        d[2..4].copy_from_slice(&if le { 3u16.to_le_bytes() } else { 3u16.to_be_bytes() });
        let key_at = d.len();
        d.extend(keys);
        let str_at = d.len();
        d.extend(strings);
        // A dictionary entry: 24-bit key index, type byte, 32-bit value.
        let entry = |key: u32, kind: u8, v: u32| {
            let k = if le { key.to_le_bytes() } else { key.to_be_bytes() };
            let mut x = if le { k[..3].to_vec() } else { k[1..].to_vec() };
            x.push(kind);
            x.extend(w32(v));
            x
        };
        // three {IsGet} dicts (key 1 = IsGet)
        let mut dicts = Vec::new();
        for v in [1u32, 0, 1] {
            dicts.push(d.len());
            d.extend(hdr(0xC1, 1));
            d.extend(entry(1, 0xD0, v));
        }
        let arr_at = d.len();
        d.extend(hdr(0xC0, 3));
        d.extend([0xC1, 0xC1, 0xC1, 0x00]);
        for a in &dicts {
            d.extend(w32(*a as u32));
        }
        let root_at = d.len();
        d.extend(hdr(0xC1, 3));
        d.extend(entry(0, 0xD1, 42));
        d.extend(entry(2, 0xA0, 0));
        d.extend(entry(3, 0xC0, arr_at as u32));
        d[4..8].copy_from_slice(&w32(key_at as u32));
        d[8..12].copy_from_slice(&w32(str_at as u32));
        d[12..16].copy_from_slice(&w32(root_at as u32));
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(reader: &str, achievements: Value) -> RuleList {
        serde_json::from_value(json!({ "title_id": "0100000000010000", "name": "Test", "reader": reader, "save_files": ["File0.bin"], "achievements": achievements })).unwrap()
    }

    #[test]
    fn murmur3_matches_reference_values() {
        // MurmurHash3_x86_32 with seed 0.
        assert_eq!(murmur3(""), 0);
        assert_eq!(murmur3("hello"), 0x248b_fa47);
        assert_eq!(murmur3("Hello, world!"), 0xc036_3e43);
    }

    #[test]
    fn paths() {
        let v = json!({ "playerData": { "geo": 120, "killed": true }, "list": [{"Key": "A", "Value": 1}, {"Key": "B", "Value": 0}], "tags": ["x", "Wanwan"] });
        assert_eq!(walk(&v, "playerData/geo"), vec![json!(120)]);
        assert_eq!(walk(&v, "list[*]/Value"), vec![json!(1), json!(0)]);
        assert_eq!(walk(&v, "list[Key=B]/Value"), vec![json!(0)]);
        assert_eq!(walk(&v, "list[1]/Key"), vec![json!("B")]);
        assert_eq!(walk(&v, "tags[=Wanwan]"), vec![json!(1)]);
        assert_eq!(walk(&v, "playerData[*]").len(), 2);
        assert!(walk(&v, "missing/x").is_empty());
    }

    #[test]
    fn json_rules_best_slot() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("user1.dat");
        let b = dir.path().join("user2.dat");
        std::fs::write(&a, r#"{"playerData":{"hornet1Defeated":false,"grubsCollected":5}}"#).unwrap();
        std::fs::write(&b, r#"{"playerData":{"hornet1Defeated":true,"grubsCollected":30}}"#).unwrap();
        let l = list(
            "json",
            json!([
                { "id": "hornet", "name": "Protector", "rule": { "type": "flag", "path": "playerData/hornet1Defeated" } },
                { "id": "grubs", "name": "Grubfriend", "rule": { "type": "flag", "path": "playerData/grubsCollected", "target": 23 } },
                { "id": "grubs46", "name": "All", "rule": { "type": "flag", "path": "playerData/grubsCollected", "target": 46 } },
                { "id": "play10", "name": "Ten hours", "rule": { "type": "playtime_hours", "target": 10 } },
                { "id": "launch1", "name": "Started", "rule": { "type": "launches", "target": 1 } }
            ]),
        );
        let got = evaluate(&l, &[a, b], Stats { launches: 3, hours: 2.0 }).unwrap();
        let ids: Vec<&str> = got.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(ids, vec!["hornet", "grubs", "launch1"]);
    }

    #[test]
    fn byml_count_rules() {
        for le in [true, false] {
            let data = byml::build(le);
            let v = byml::parse(&data).unwrap();
            assert_eq!(walk(&v, "Name"), vec![json!("Mario")]);
            assert_eq!(walk(&v, "Coins"), vec![json!(42)]);
            assert_eq!(walk(&v, "ShineData[*]/IsGet"), vec![json!(true), json!(false), json!(true)]);
            // With a 0x10-byte prefix too (Super Mario Odyssey's File*.bin).
            let mut prefixed = vec![0u8; 0x10];
            prefixed.extend(&data);
            assert_eq!(walk(&byml::parse(&prefixed).unwrap(), "Coins"), vec![json!(42)]);
        }
        let l = list("byml", json!([
            { "id": "m2", "rule": { "type": "count", "path": "ShineData[*]/IsGet", "target": 2 } },
            { "id": "m3", "rule": { "type": "count", "path": "ShineData[*]/IsGet", "target": 3 } }
        ]));
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("File0.bin");
        std::fs::write(&f, byml::build(true)).unwrap();
        let ids: Vec<String> = evaluate(&l, &[f], Stats::default()).unwrap().into_iter().map(|u| u.id).collect();
        assert_eq!(ids, vec!["m2".to_string()]);
    }

    #[test]
    fn hashed_pairs_and_offsets() {
        // BotW-style: header 0x0C, then (crc32(name), value) pairs.
        let mut botw = vec![0u8; 0x0C];
        for (name, v) in [("IsGet_Obj_DRStone_Get", 1u32), ("Open_DLC_Gate", 0)] {
            botw.extend(crc32(name).to_le_bytes());
            botw.extend(v.to_le_bytes());
        }
        // TotK-style: magic, header to 0x28, (murmur3(name), value) pairs, enum values hashed.
        let mut totk = vec![0u8; 0x28];
        totk[0..4].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        for (name, v) in [("DungeonState.Dungeon001", murmur3("Clear")), ("DungeonState.Dungeon002", murmur3("Open")), ("MetaData.SaveTypeHash", 0), ("DungeonState.Dungeon003", murmur3("Clear"))] {
            totk.extend(murmur3(name).to_le_bytes());
            totk.extend(v.to_le_bytes());
        }
        let mut mk = b"SUTC".to_vec();
        mk.resize(0x2000, 0);
        mk[0x195C..0x1960].copy_from_slice(&5000u32.to_le_bytes());
        let dir = tempfile::tempdir().unwrap();
        let check = |reader: &str, data: &[u8], rules: Value, extra: Value| -> Vec<String> {
            let mut l = list(reader, rules);
            if let Some(m) = extra["magic"].as_str() {
                l.magic = Some(m.to_string());
            }
            let f = dir.path().join(format!("{reader}.bin"));
            std::fs::write(&f, data).unwrap();
            evaluate(&l, &[f], Stats::default()).unwrap().into_iter().map(|u| u.id).collect()
        };
        assert_eq!(
            check("gamedata_hash", &botw, json!([
                { "id": "stone", "rule": { "type": "flag", "name": "IsGet_Obj_DRStone_Get" } },
                { "id": "gate", "rule": { "type": "flag", "name": "Open_DLC_Gate" } }
            ]), json!({})),
            vec!["stone".to_string()]
        );
        assert_eq!(
            check("totk_progress", &totk, json!([
                { "id": "one", "rule": { "type": "count", "pattern": "DungeonState.Dungeon%03d", "from": 0, "to": 151, "equals": "Clear", "target": 1 } },
                { "id": "two", "rule": { "type": "count", "pattern": "DungeonState.Dungeon%03d", "from": 0, "to": 151, "equals": "Clear", "target": 2 } }
            ]), json!({})),
            vec!["one".to_string()],
            "pairs after MetaData.SaveTypeHash are not read"
        );
        assert_eq!(
            check("binary_le", &mk, json!([
                { "id": "c1k", "rule": { "type": "flag", "offset": "0x195C", "size": 4, "target": 1000, "max": 10000000 } },
                { "id": "c10k", "rule": { "type": "flag", "offset": "0x195C", "size": 4, "target": 10000, "max": 10000000 } }
            ]), json!({ "magic": "SUTC" })),
            vec!["c1k".to_string()]
        );
    }

    #[test]
    fn botw_byte_order_is_detected() {
        let mut wiiu = vec![0u8; 0x0C];
        for (name, v) in [("IsGet_Obj_DRStone_Get", 1u32), ("Open_DLC_Gate", 0)] {
            wiiu.extend(crc32(name).to_be_bytes());
            wiiu.extend(v.to_be_bytes());
        }
        let l = list("gamedata_hash", json!([
            { "id": "stone", "rule": { "type": "flag", "name": "IsGet_Obj_DRStone_Get" } },
            { "id": "gate", "rule": { "type": "flag", "name": "Open_DLC_Gate" } }
        ]));
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("game_data.sav");
        std::fs::write(&f, wiiu).unwrap();
        let ids: Vec<String> = evaluate(&l, &[f], Stats::default()).unwrap().into_iter().map(|u| u.id).collect();
        assert_eq!(ids, vec!["stone".to_string()]);
    }

    #[test]
    fn title_ids() {
        let g = GameEntry { key: "botw".into(), adapter: Some("cemu".into()), saves: vec!["{mlc}/usr/save/00050000/101c9300/**".into()], ..Default::default() };
        assert_eq!(title_id(&g).as_deref(), Some("01007EF00011E000"));
        let g = GameEntry { key: "smo".into(), adapter: Some("odysseyrecomp".into()), ..Default::default() };
        assert_eq!(title_id(&g).as_deref(), Some("0100000000010000"));
        let g = GameEntry { key: "hk".into(), saves: vec!["{nxsave-0100633007d48000}/**".into()], ..Default::default() };
        assert_eq!(title_id(&g).as_deref(), Some("0100633007D48000"));
    }

    /// Every bundled list parses, belongs to an adapter, uses a known reader
    /// and rule types, has unique ids and texts in both languages.
    #[test]
    fn bundled_lists_are_well_formed() {
        for (key, _) in BUNDLED {
            let l = bundled(key).unwrap_or_else(|| panic!("{key} doesn't parse"));
            assert!(crate::adapters::find(key).is_some(), "{key} is not an adapter key");
            assert!(["json", "byml", "gamedata_hash", "totk_progress", "binary_le"].contains(&l.reader.as_str()), "{key}: reader {}", l.reader);
            assert!(!l.save_files.is_empty() || !l.reads_saves(), "{key}: no save files");
            assert!(l.name_zh.is_some(), "{key}: no Chinese name");
            let mut ids = std::collections::HashSet::new();
            for a in &l.achievements {
                assert!(ids.insert(a.id.clone()), "{key}: duplicate id {}", a.id);
                assert!(!a.name.is_empty() && !a.desc.is_empty(), "{key}/{}: English text", a.id);
                assert!(a.name_zh.as_deref().is_some_and(|s| !s.is_empty()) && a.desc_zh.as_deref().is_some_and(|s| !s.is_empty()), "{key}/{}: Chinese text", a.id);
                assert!(matches!(rule_type(&a.rule), "flag" | "count" | "sum" | "launches" | "playtime_hours"), "{key}/{}: rule type", a.id);
            }
        }
    }

    #[test]
    fn bundled_lists_by_adapter() {
        let g = GameEntry { key: "oot".into(), adapter: Some("zelda-oot-longshot".into()), ..Default::default() };
        assert_eq!(bundled_key(&g).as_deref(), Some("zelda-oot-longshot"));
        assert!(bundled_key(&GameEntry { adapter: Some("cemu".into()), ..Default::default() }).is_none());
        assert!(is_title("01007EF00011E000") && !is_title("zelda-oot-longshot"));
        assert!(load("zelda-oot-longshot", false).unwrap().is_some_and(|l| l.reader == "totk_progress"));
        assert!(load("no-such-game", false).unwrap().is_none());
        // Detected and linked automatically.
        let c = super::super::detect::candidates(&g);
        assert!(c.iter().any(|c| c.sure && c.binding.source == SourceKind::Rules && c.binding.source_id == "zelda-oot-longshot"), "{c:?}");
        let b = Binding { source: SourceKind::Rules, source_id: "zelda-oot-longshot".into(), path: None, popup: None };
        assert_eq!(super::super::binding_label(&b), "Ludonimbus");
    }

    #[test]
    fn longshot_list_on_made_up_saves() {
        // Longshot keeps Ocarina of Time's progress in a TotK-style progress.sav per slot.
        let save = |pairs: &[(&str, u32)]| -> Vec<u8> {
            let mut d = vec![0u8; 0x28];
            d[0..4].copy_from_slice(&0x0102_0304u32.to_le_bytes());
            for (name, v) in pairs.iter().copied().chain([("MetaData.SaveTypeHash", 0)]) {
                d.extend(murmur3(name).to_le_bytes());
                d.extend(v.to_le_bytes());
            }
            d
        };
        let common = [("IsGet.Weapon_Sword_Kokiri", 1), ("IsGet.Item_Stone_Forest", 1), ("IsGet.Item_Stone_Fire", 1), ("IsGet.Item_Stone_Water", 1), ("PouchData.Item_Token_Gold.StockNum", 21), ("PlayerStatus.MaxLife", 36)];
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("slot_00/progress.sav"), dir.path().join("slot_01/progress.sav"));
        for (f, medal) in [(&a, 0), (&b, 1)] {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut pairs = common.to_vec();
            pairs.push(("IsGet.Item_Medal_Forest", medal));
            std::fs::write(f, save(&pairs)).unwrap();
        }
        let l = bundled("zelda-oot-longshot").unwrap();
        let ids: Vec<String> = evaluate(&l, &[a, b], Stats::default()).unwrap().into_iter().map(|u| u.id).collect();
        assert_eq!(ids, ["kokiri_sword", "kokiri_emerald", "goron_ruby", "zora_sapphire", "spiritual_stones", "forest_medallion", "skulltula_10"]);
    }

    #[test]
    fn save_files_one_folder_deeper() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("OdysseyRecomp");
        let deep = install.join("save/0000000000000001/File0.bin");
        std::fs::create_dir_all(deep.parent().unwrap()).unwrap();
        std::fs::write(&deep, b"x").unwrap();
        std::fs::write(install.join("OdysseyRecomp.exe"), b"x").unwrap();
        let g = GameEntry {
            key: "smo".into(),
            adapter: Some("odysseyrecomp".into()),
            exe: Some(install.join("OdysseyRecomp.exe")),
            saves: vec!["{install}/save/**".into()],
            ..Default::default()
        };
        assert_eq!(find_save_files(&g, &["File0.bin".to_string(), "File1.bin".to_string()]), vec![deep]);
    }
}
