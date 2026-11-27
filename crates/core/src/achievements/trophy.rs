//! PlayStation trophies written by emulators.
//!
//! | Source  | Definitions | Unlocks |
//! |---|---|---|
//! | shadPS4 | `<user>/trophy/<NPWR>/Xml/TROP[_XX].XML`, icons in `Icons/` | `<user>/home/<uid>/trophy/<NPWR>.xml` (`unlockstate`, `timestamp` = Unix seconds) |
//! | Lantern Engine | icons from `trophy00.trp` (names only when a shadPS4-style XML exists) | `trophies.log` in the saves folder, `logs/last_session.log` (`Runtime: trophy N unlocked`) |
//! | RPCS3   | `<set>/TROPCONF.SFM`, `<set>/TROPnnn.PNG` | `<set>/TROPUSR.DAT` (big-endian tables, CellRtcTick µs since year 1) |
//!
//! Trophy ids are kept as plain decimal strings ("0", "12").

use super::{Achievement, Binding, Grade, Scan, Schema, SourceKind, Unlock};
use crate::config::{AppConfig, GameEntry};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn scan(_cfg: &AppConfig, _game: &GameEntry, b: &Binding) -> Result<Scan> {
    let path = b
        .path
        .clone()
        .ok_or_else(|| anyhow!(crate::tr!("奖杯来源缺少文件夹", "The trophy source has no folder")))?;
    match b.source {
        SourceKind::Shadps4 => shadps4(&path, &b.source_id),
        SourceKind::Lantern => lantern(&path, &b.source_id),
        SourceKind::Rpcs3 => rpcs3(&path),
        SourceKind::Ps4port => ps4port(&path, &b.source_id),
        SourceKind::Retroachievements | SourceKind::Rules => bail!("not a trophy source"),
    }
}

/// Files that change when a trophy unlocks (watched while the game runs).
pub fn watch_files(b: &Binding) -> Vec<PathBuf> {
    let Some(path) = &b.path else { return Vec::new() };
    match b.source {
        SourceKind::Shadps4 => shadps4_save_files(path, &b.source_id),
        SourceKind::Lantern => lantern_log(path)
            .into_iter()
            .chain(path.parent().map(|p| p.join("logs").join("last_session.log")))
            .collect(),
        SourceKind::Rpcs3 => vec![path.join("TROPUSR.DAT")],
        SourceKind::Ps4port => port_files(path),
        SourceKind::Retroachievements | SourceKind::Rules => Vec::new(),
    }
}

fn norm_id(s: &str) -> Option<String> {
    s.trim().parse::<u32>().ok().map(|n| n.to_string())
}

fn icon_name(id: &str) -> String {
    format!("TROP{:03}.PNG", id.parse::<u32>().unwrap_or(0))
}

fn existing_icon(dir: &Path, id: &str) -> Option<String> {
    crate::util::find_file_ci(dir, &icon_name(id)).map(|p| p.display().to_string())
}

// ---------------------------------------------------------------------------
// Trophy XML (TROP.XML, TROPCONF.SFM, shadPS4's per-user copy)

struct XmlTrophy {
    id: String,
    name: String,
    detail: String,
    hidden: bool,
    grade: Option<Grade>,
    unlocked: Option<DateTime<Utc>>,
}

struct XmlSet {
    title: Option<String>,
    trophies: Vec<XmlTrophy>,
}

/// Parse a trophy configuration. Some files start with a signature or junk
/// before the XML; everything before the first `<` is skipped.
fn parse_xml(text: &str) -> Result<XmlSet> {
    let start = text.find('<').ok_or_else(|| anyhow!("not XML"))?;
    let mut text = text[start..].to_string();
    // An XML declaration is only valid at the very start; signatures and
    // comments sometimes come first, so drop it wherever it is.
    if let Some(d) = text.find("<?xml") {
        if let Some(end) = text[d..].find("?>") {
            text.replace_range(d..d + end + 2, "");
        }
    }
    // A DOCTYPE line is let through, entities are not: trophy files declare
    // none, and roxmltree bounds how deep they nest, not how much a few
    // references to them expand to (gigabytes from a small file that came
    // with someone's saves).
    if text.contains("<!ENTITY") {
        bail!("trophy file declares XML entities");
    }
    let opt = roxmltree::ParsingOptions { allow_dtd: true, nodes_limit: 1_000_000 };
    let doc = roxmltree::Document::parse_with_options(&text, opt)?;
    let child_text = |n: roxmltree::Node, tag: &str| {
        n.children()
            .find(|c| c.has_tag_name(tag))
            .and_then(|c| c.text())
            .map(|t| t.trim().to_string())
            .unwrap_or_default()
    };
    let title = doc
        .descendants()
        .find(|n| n.has_tag_name("title-name"))
        .and_then(|n| n.text())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    let mut trophies = Vec::new();
    for n in doc.descendants().filter(|n| n.has_tag_name("trophy")) {
        let Some(id) = n.attribute("id").and_then(norm_id) else { continue };
        let unlocked = match n.attribute("unlockstate") {
            Some(v) if v.eq_ignore_ascii_case("true") || v == "1" => Some(
                n.attribute("timestamp")
                    .and_then(|t| t.trim().parse::<i64>().ok())
                    .and_then(unix_any)
                    .unwrap_or_else(Utc::now),
            ),
            _ => None,
        };
        trophies.push(XmlTrophy {
            id,
            name: child_text(n, "name"),
            detail: child_text(n, "detail"),
            hidden: n.attribute("hidden").is_some_and(|h| h.eq_ignore_ascii_case("yes")),
            grade: n.attribute("ttype").and_then(Grade::from_letter),
            unlocked,
        });
    }
    Ok(XmlSet { title, trophies })
}

fn read_xml(path: &Path) -> Result<XmlSet> {
    let bytes = std::fs::read(path).with_context(|| path.display().to_string())?;
    parse_xml(&String::from_utf8_lossy(&bytes)).with_context(|| path.display().to_string())
}

fn schema_from(set: &XmlSet, source: SourceKind, id: &str, icons: Option<&Path>) -> Schema {
    Schema {
        source,
        source_id: id.to_string(),
        title: set.title.clone(),
        achievements: set
            .trophies
            .iter()
            .map(|t| Achievement {
                id: t.id.clone(),
                name: if t.name.is_empty() { super::placeholder(&t.id).name } else { t.name.clone() },
                description: t.detail.clone(),
                icon: icons.and_then(|d| existing_icon(d, &t.id)),
                hidden: t.hidden,
                points: None,
                grade: t.grade,
            })
            .collect(),
    }
}

/// Unix seconds, or milliseconds when the number is too large for seconds.
fn unix_any(n: i64) -> Option<DateTime<Utc>> {
    if n <= 0 {
        return None;
    }
    if n > 100_000_000_000 {
        Utc.timestamp_millis_opt(n).single()
    } else {
        Utc.timestamp_opt(n, 0).single()
    }
}

// ---------------------------------------------------------------------------
// shadPS4

/// PS4 language file for the interface language (`TROP_11.XML` is Simplified
/// Chinese, `TROP_01.XML` US English), falling back to `TROP.XML`.
fn shadps4_xml(xml_dir: &Path) -> Option<PathBuf> {
    let lang = if crate::i18n::is_en() { ["TROP_01.XML", "TROP_18.XML"] } else { ["TROP_11.XML", "TROP_10.XML"] };
    lang.iter()
        .chain(["TROP.XML"].iter())
        .find_map(|n| crate::util::find_file_ci(xml_dir, n))
}

/// Per-user unlock files: `<user>/home/<uid>/trophy/<NPWR>.xml`.
fn shadps4_save_files(user: &Path, npwr: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(user.join("home")) else { return out };
    for e in rd.flatten() {
        let dir = e.path().join("trophy");
        if let Some(p) = crate::util::find_file_ci(&dir, &format!("{npwr}.xml")) {
            out.push(p);
        }
    }
    out
}

/// Trophy sets known to a shadPS4 `user` folder.
pub fn shadps4_sets(user: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(user.join("trophy"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| is_npwr(n))
        .collect();
    if let Ok(rd) = std::fs::read_dir(user.join("home")) {
        for e in rd.flatten() {
            for f in std::fs::read_dir(e.path().join("trophy")).into_iter().flatten().flatten() {
                if let Some(stem) = f.path().file_stem().and_then(|s| s.to_str()) {
                    if is_npwr(stem) && !out.iter().any(|o| o.eq_ignore_ascii_case(stem)) {
                        out.push(stem.to_string());
                    }
                }
            }
        }
    }
    out.sort();
    out
}

pub fn is_npwr(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 12 && s[..4].eq_ignore_ascii_case("NPWR") && b[4..9].iter().all(u8::is_ascii_digit) && b[9] == b'_' && b[10..].iter().all(u8::is_ascii_digit)
}

pub fn shadps4(user: &Path, npwr: &str) -> Result<Scan> {
    let npwr = if npwr.is_empty() {
        match shadps4_sets(user).as_slice() {
            [one] => one.clone(),
            [] => bail!(crate::tr!("在 {} 里没有找到奖杯数据", "No trophy data in {}", user.display())),
            _ => bail!(crate::tr!("{} 里有多个游戏的奖杯，请选择一个", "{} has trophies for several games; choose one", user.display())),
        }
    } else {
        npwr.to_string()
    };
    let set_dir = user.join("trophy").join(&npwr);
    let mut icons = set_dir.join("Icons");
    let mut scan = Scan::default();
    if let Some(xml) = shadps4_xml(&set_dir.join("Xml")) {
        let set = read_xml(&xml)?;
        scan.schema = Some(schema_from(&set, SourceKind::Shadps4, &npwr, Some(&icons)));
    } else if let Some(dir) = decrypted_set(&npwr) {
        // shadPS4 has no trophy key set: decrypt the game's trophy file ourselves.
        if !icons.is_dir() {
            icons = dir.clone();
        }
        if let Some(xml) = shadps4_xml(&dir) {
            scan.schema = Some(schema_from(&read_xml(&xml)?, SourceKind::Shadps4, &npwr, Some(&icons)));
        }
    }
    let mut unlocks: BTreeMap<String, Unlock> = BTreeMap::new();
    for f in shadps4_save_files(user, &npwr) {
        let set = read_xml(&f)?;
        if scan.schema.is_none() {
            // The per-user file is a copy of TROP.XML with the unlock state added.
            scan.schema = Some(schema_from(&set, SourceKind::Shadps4, &npwr, Some(&icons)));
        }
        for t in set.trophies {
            if let Some(at) = t.unlocked {
                keep_earliest(&mut unlocks, Unlock { id: t.id, unlocked_at: at, hardcore: false });
            }
        }
    }
    if scan.schema.is_none() && unlocks.is_empty() {
        bail!(crate::tr!("在 {} 里没有找到 {npwr} 的奖杯数据", "No trophy data for {npwr} in {}", user.display()));
    }
    scan.unlocks = unlocks.into_values().collect();
    Ok(scan)
}

fn keep_earliest(map: &mut BTreeMap<String, Unlock>, u: Unlock) {
    match map.get(&u.id) {
        Some(old) if old.unlocked_at <= u.unlocked_at => {}
        _ => {
            map.insert(u.id.clone(), u);
        }
    }
}

// ---------------------------------------------------------------------------
// Lantern Engine (Bloodborne)

/// `trophies.log` somewhere in the saves folder (`<user>/savedata/…`).
pub fn lantern_log(user: &Path) -> Option<PathBuf> {
    walkdir::WalkDir::new(user.join("savedata"))
        .max_depth(4)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file() && e.file_name().eq_ignore_ascii_case("trophies.log"))
        .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
        .map(|e| e.into_path())
}

/// One `trophies.log` line: a trophy id and, usually, a time. The exact
/// format is not documented ("one line per unlock: id and time"), so this
/// accepts the common shapes: `12 2026-11-23T10:00:00Z`, `12,1763892000`,
/// `2026-11-23 10:00:00 12`, `id=12 time=…`, JSON objects.
pub fn parse_log_line(line: &str) -> Option<(String, Option<DateTime<Utc>>)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let tokens: Vec<&str> = line
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '|' | '{' | '}' | '"' | '[' | ']'))
        .filter(|t| !t.is_empty())
        .map(|t| t.rsplit_once(['=', ':']).filter(|(k, _)| k.chars().all(|c| c.is_ascii_alphabetic() || c == '_')).map_or(t, |(_, v)| v))
        .filter(|t| !t.is_empty())
        .collect();
    let mut id = None;
    let mut at = None;
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        if let Some(d) = parse_time(t) {
            at = at.or(Some(d));
        } else if let Ok(date) = NaiveDate::parse_from_str(t, "%Y-%m-%d").or_else(|_| NaiveDate::parse_from_str(t, "%Y/%m/%d")) {
            let time = tokens
                .get(i + 1)
                .and_then(|n| NaiveTime::parse_from_str(n, "%H:%M:%S%.f").ok());
            if time.is_some() {
                i += 1;
            }
            at = at.or_else(|| local(date.and_time(time.unwrap_or_default())));
        } else if t.chars().all(|c| c.is_ascii_digit()) {
            if t.len() >= 9 {
                at = at.or_else(|| t.parse().ok().and_then(unix_any));
            } else if id.is_none() {
                id = norm_id(t).filter(|n| n.parse::<u32>().is_ok_and(|v| v < 1000));
            }
        }
        i += 1;
    }
    id.map(|id| (id, at))
}

fn local(n: NaiveDateTime) -> Option<DateTime<Utc>> {
    Local.from_local_datetime(&n).earliest().map(|d| d.with_timezone(&Utc))
}

fn parse_time(t: &str) -> Option<DateTime<Utc>> {
    if let Ok(d) = DateTime::parse_from_rfc3339(t) {
        return Some(d.with_timezone(&Utc));
    }
    for f in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(t, f) {
            return local(n);
        }
    }
    None
}

/// Unlocks seen in the engine's log of the current session. Returns
/// `(new unlocks, already earned in the loaded save)`.
pub fn parse_session_log(text: &str) -> (Vec<String>, Vec<String>) {
    let mut unlocked = Vec::new();
    let mut earned = Vec::new();
    for line in text.lines() {
        // "Runtime: trophy 12 unlocked", "Trophies: trophy 0 (platinum) unlocked"
        // (the forms PSerban93/Achievements also reads for PS4 ports).
        let Some(rest) = line.split("Runtime: trophy ").nth(1).or_else(|| line.split("Trophies: trophy ").nth(1)) else { continue };
        let mut parts = rest.split_whitespace().filter(|p| *p != "(platinum)");
        let Some(id) = parts.next().and_then(norm_id) else { continue };
        match parts.next() {
            Some("unlocked") if !unlocked.contains(&id) => unlocked.push(id),
            Some("earned") if !earned.contains(&id) => earned.push(id),
            _ => {}
        }
    }
    (unlocked, earned)
}

/// Icons for a Lantern game: its own extracted copies, or `trophy00.trp`.
fn lantern_icons(user: &Path, npwr: &str) -> Option<PathBuf> {
    // shadPS4-style folder (present when a trophy key is set).
    let own = user.join("trophy").join(npwr).join("Icons");
    if !npwr.is_empty() && own.is_dir() {
        return Some(own);
    }
    // The launcher's cache: %APPDATA%\Lantern Engine\trophy-icons\<hash>\.
    if let Some(appdata) = dirs::config_dir() {
        let root = appdata.join("Lantern Engine").join("trophy-icons");
        let dirs: Vec<PathBuf> = std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("TROP000.PNG").is_file())
            .collect();
        if dirs.len() == 1 {
            return dirs.into_iter().next();
        }
    }
    // Extract from the game's trophy00.trp (game folder from Lantern's settings).
    let trp = lantern_game_dir().map(|g| g.join("sce_sys").join("trophy").join("trophy00.trp"))?;
    let key = if npwr.is_empty() { "lantern".to_string() } else { npwr.to_string() };
    let out = super::dir().join("trophies").join(key);
    if out.join("TROP000.PNG").is_file() || extract_trp_icons(&trp, &out).is_ok_and(|n| n > 0) {
        return Some(out);
    }
    None
}

/// `game_dir` from `%APPDATA%\Lantern Engine\settings.json`.
pub fn lantern_game_dir() -> Option<PathBuf> {
    let file = dirs::config_dir()?.join("Lantern Engine").join("settings.json");
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    let dir = PathBuf::from(v["game_dir"].as_str()?);
    dir.is_dir().then_some(dir)
}

/// Definitions and icons decrypted from the game's own `trophy00.trp`
/// (cached locally), when the game folder can be found.
pub fn decrypted_set(npwr: &str) -> Option<PathBuf> {
    if !is_npwr(npwr) {
        return None;
    }
    let trp = super::trp::find_trp(npwr)?;
    match super::trp::cached(&trp, npwr) {
        Ok(dir) => Some(dir),
        Err(e) => {
            crate::logger::log("achievements", format!("{npwr}: reading {} failed: {e:#}", trp.display()));
            None
        }
    }
}

pub fn lantern(user: &Path, npwr: &str) -> Result<Scan> {
    let mut scan = Scan::default();
    let decrypted = decrypted_set(npwr);
    let icons = decrypted
        .clone()
        .filter(|d| d.join("TROP000.PNG").is_file())
        .or_else(|| lantern_icons(user, npwr));
    // Names: a shadPS4-style XML for the same set, else the official
    // definitions decrypted from the game's trophy file.
    let xml = (!npwr.is_empty())
        .then(|| shadps4_xml(&user.join("trophy").join(npwr).join("Xml")))
        .flatten()
        .or_else(|| decrypted.as_deref().and_then(shadps4_xml))
        .and_then(|p| read_xml(&p).ok());
    if let Some(set) = &xml {
        scan.schema = Some(schema_from(set, SourceKind::Lantern, npwr, icons.as_deref()));
    } else if let Some(dir) = &icons {
        // Numbered entries, one per icon.
        let mut ids: Vec<u32> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().to_ascii_uppercase();
                n.strip_prefix("TROP")?.strip_suffix(".PNG")?.parse().ok()
            })
            .collect();
        ids.sort();
        scan.schema = Some(Schema {
            source: SourceKind::Lantern,
            source_id: npwr.to_string(),
            title: None,
            achievements: ids
                .into_iter()
                .map(|n| {
                    let mut a = super::placeholder(&n.to_string());
                    a.icon = existing_icon(dir, &a.id);
                    a
                })
                .collect(),
        });
    }

    let mut unlocks: BTreeMap<String, Unlock> = BTreeMap::new();
    if let Some(log) = lantern_log(user) {
        let mtime = file_time(&log);
        let text = String::from_utf8_lossy(&std::fs::read(&log)?).to_string();
        for (id, at) in text.lines().filter_map(parse_log_line) {
            keep_earliest(&mut unlocks, Unlock { id, unlocked_at: at.unwrap_or(mtime), hardcore: false });
        }
    }
    // Live: the engine log of the running session (Lantern's launcher reads the same lines).
    if let Some(session) = user.parent().map(|p| p.join("logs").join("last_session.log")) {
        if let Ok(bytes) = std::fs::read(&session) {
            let mtime = file_time(&session);
            let (unlocked, earned) = parse_session_log(&String::from_utf8_lossy(&bytes));
            for id in unlocked {
                if !unlocks.contains_key(&id) {
                    unlocks.insert(id.clone(), Unlock { id, unlocked_at: mtime, hardcore: false });
                }
            }
            for id in earned {
                if !unlocks.contains_key(&id) {
                    scan.quiet.push(id.clone());
                    unlocks.insert(id.clone(), Unlock { id, unlocked_at: mtime, hardcore: false });
                }
            }
        }
    }
    scan.unlocks = unlocks.into_values().collect();
    if scan.schema.is_none() && scan.unlocks.is_empty() && !user.is_dir() {
        bail!(crate::tr!("找不到 Lantern 的 user 文件夹：{}", "Lantern's user folder is missing: {}", user.display()));
    }
    Ok(scan)
}

// ---------------------------------------------------------------------------
// PS4 ports and recompilations (one folder per game)
//
// Ports keep the original game's files (and its `trophy00.trp`) and record
// unlocks in one of a few shapes, the same ones PSerban93/Achievements reads:
// `trophies.log` / `unlocked.txt` ("<id> <unix seconds>"), `progress_user*.txt`
// ("DCP-TROPHIES 1", "npcommid …", then "unlocked <id> <time>"), `trophies.dat`
// / `CUSAxxxxx-userN.bin` (128-bit bitmap, little-endian words), shadPS4-style
// `<NPWR>.xml`, or run logs with "Runtime: trophy N unlocked".

/// The trophy set of a port: `sce_sys/npbind.dat` and `sce_sys/trophy/trophy00.trp` under its folder.
pub fn port_set(install: &Path) -> Option<(String, PathBuf)> {
    walkdir::WalkDir::new(install)
        .max_depth(4)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || e.file_type().is_dir())
        .flatten()
        .filter(|e| e.file_name().eq_ignore_ascii_case("sce_sys"))
        .find_map(|e| {
            let game = e.path().parent()?.to_path_buf();
            let npwr = npbind_id(&game)?;
            let trp = e.path().join("trophy").join("trophy00.trp");
            trp.is_file().then_some((npwr, trp))
        })
}

/// Unlock records a port may have written.
fn port_files(install: &Path) -> Vec<PathBuf> {
    let wanted = |name: &str| {
        let n = name.to_ascii_lowercase();
        n == "trophies.log"
            || n == "unlocked.txt"
            || n == "trophies.dat"
            || (n.starts_with("progress_user") && n.ends_with(".txt"))
            || (n.starts_with("cusa") && n.contains("-user") && n.ends_with(".bin"))
            || n == "last_run.log"
            || n == "last_session.log"
            || n == "game.log"
            || (is_npwr(n.trim_end_matches(".xml")) && n.ends_with(".xml"))
    };
    let mut out = Vec::new();
    for (dir, depth) in [(install.to_path_buf(), 1), (install.join("logs"), 1), (install.join("user"), 6), (install.join("bin").join("user"), 6)] {
        for e in walkdir::WalkDir::new(&dir).max_depth(depth).into_iter().flatten() {
            if e.file_type().is_file() && wanted(&e.file_name().to_string_lossy()) {
                out.push(e.into_path());
            }
        }
    }
    out
}

/// Ids set in a 16-byte trophy bitmap.
pub fn decode_bitset(data: &[u8]) -> Vec<String> {
    if data.len() != 16 {
        return Vec::new();
    }
    (0..128u32)
        .filter(|id| {
            let w = u32::from_le_bytes(data[(id / 32) as usize * 4..(id / 32) as usize * 4 + 4].try_into().unwrap());
            w & (1 << (id % 32)) != 0
        })
        .map(|id| id.to_string())
        .collect()
}

pub fn ps4port(install: &Path, npwr: &str) -> Result<Scan> {
    let mut scan = Scan::default();
    let found = port_set(install);
    let npwr = if npwr.is_empty() { found.as_ref().map(|(n, _)| n.clone()).unwrap_or_default() } else { npwr.to_string() };
    let trp = found.map(|(_, t)| t).or_else(|| super::trp::find_trp(&npwr));
    if let Some(dir) = trp.filter(|_| is_npwr(&npwr)).and_then(|t| super::trp::cached(&t, &npwr).ok()) {
        if let Some(xml) = shadps4_xml(&dir) {
            scan.schema = Some(schema_from(&read_xml(&xml)?, SourceKind::Ps4port, &npwr, Some(&dir)));
        }
    }
    let mut unlocks: BTreeMap<String, Unlock> = BTreeMap::new();
    for f in port_files(install) {
        let name = f.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
        let mtime = file_time(&f);
        let Ok(bytes) = std::fs::read(&f) else { continue };
        let text = || String::from_utf8_lossy(&bytes).to_string();
        let mut add = |id: String, at: DateTime<Utc>| keep_earliest(&mut unlocks, Unlock { id, unlocked_at: at, hardcore: false });
        if name == "trophies.dat" || name.ends_with(".bin") {
            for id in decode_bitset(&bytes) {
                add(id, mtime);
            }
        } else if name.ends_with(".xml") {
            if !name.eq_ignore_ascii_case(&format!("{npwr}.xml")) {
                continue;
            }
            for t in read_xml(&f).map(|s| s.trophies).unwrap_or_default() {
                if let Some(at) = t.unlocked {
                    add(t.id, at);
                }
            }
        } else if name.ends_with(".log") && name != "trophies.log" {
            let (unlocked, earned) = parse_session_log(&text());
            for id in unlocked {
                add(id, mtime);
            }
            for id in earned {
                if !scan.quiet.contains(&id) {
                    scan.quiet.push(id.clone());
                }
                add(id, mtime);
            }
        } else {
            for line in text().lines() {
                let l = line.trim();
                if l.starts_with("DCP-TROPHIES") || l.to_ascii_lowercase().starts_with("npcommid") {
                    continue;
                }
                if let Some((id, at)) = parse_log_line(l) {
                    add(id, at.unwrap_or(mtime));
                }
            }
        }
    }
    // Ids in quiet that also have a real unlock line stay announceable.
    scan.unlocks = unlocks.into_values().collect();
    if scan.schema.is_none() && scan.unlocks.is_empty() {
        bail!(crate::tr!("在 {} 里没有找到 PS4 奖杯数据", "No PS4 trophy data in {}", install.display()));
    }
    Ok(scan)
}

fn file_time(p: &Path) -> DateTime<Utc> {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| Utc::now())
}

// ---------------------------------------------------------------------------
// trophy00.trp: icons are stored unencrypted

/// Copy the PNG entries of a `.trp` file into `out`. Returns how many.
pub fn extract_trp_icons(trp: &Path, out: &Path) -> Result<usize> {
    super::trp::extract_with(trp, None, out, None).map(|(icons, _)| icons)
}

/// The trophy set id in a PS4 game's `sce_sys/npbind.dat`.
pub fn npbind_id(game_dir: &Path) -> Option<String> {
    let data = std::fs::read(game_dir.join("sce_sys").join("npbind.dat")).ok()?;
    data.windows(12)
        .find(|w| std::str::from_utf8(w).is_ok_and(is_npwr))
        .map(|w| String::from_utf8_lossy(w).to_string())
}

// ---------------------------------------------------------------------------
// RPCS3

/// Seconds between 0001-01-01 and 1970-01-01 (CellRtcTick epoch).
const RTC_EPOCH_OFFSET: i64 = 62_135_596_800;

fn rtc_tick(us: u64) -> Option<DateTime<Utc>> {
    let secs = (us / 1_000_000) as i64 - RTC_EPOCH_OFFSET;
    (secs > 0).then(|| Utc.timestamp_opt(secs, 0).single()).flatten()
}

/// `(trophy id, unlock time)` of unlocked trophies in a `TROPUSR.DAT`.
pub fn parse_tropusr(data: &[u8]) -> Result<Vec<(u32, Option<DateTime<Utc>>)>> {
    let be32 = |o: usize| -> Result<u32> {
        data.get(o..o + 4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
            .ok_or_else(|| anyhow!("TROPUSR.DAT is truncated"))
    };
    let be64 = |o: usize| -> Result<u64> {
        data.get(o..o + 8)
            .map(|b| u64::from_be_bytes(b.try_into().unwrap()))
            .ok_or_else(|| anyhow!("TROPUSR.DAT is truncated"))
    };
    if be32(0)? != 0x818F_54AD {
        bail!("not a TROPUSR.DAT file");
    }
    let tables = be32(8)? as usize;
    if tables > 64 {
        bail!("bad TROPUSR.DAT header");
    }
    let mut out = Vec::new();
    for t in 0..tables {
        let h = 0x30 + t * 0x20;
        let (kind, entry_size, count, offset) = (be32(h)?, be32(h + 4)? as usize, be32(h + 12)? as usize, be64(h + 16)? as usize);
        if kind != 6 || entry_size < 0x30 || count > 4096 {
            continue;
        }
        for i in 0..count {
            let e = offset + i * entry_size;
            let id = be32(e + 16)?;
            let state = be32(e + 20)?;
            if state == 1 {
                out.push((id, rtc_tick(be64(e + 32)?)));
            }
        }
    }
    Ok(out)
}

pub fn rpcs3(set_dir: &Path) -> Result<Scan> {
    let npwr = set_dir.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let mut scan = Scan::default();
    let conf = crate::util::find_file_ci(set_dir, "TROPCONF.SFM")
        .ok_or_else(|| anyhow!(crate::tr!("在 {} 里没有找到 TROPCONF.SFM", "No TROPCONF.SFM in {}", set_dir.display())))?;
    let set = read_xml(&conf)?;
    scan.schema = Some(schema_from(&set, SourceKind::Rpcs3, &npwr, Some(set_dir)));
    if let Some(usr) = crate::util::find_file_ci(set_dir, "TROPUSR.DAT") {
        let mtime = file_time(&usr);
        for (id, at) in parse_tropusr(&std::fs::read(&usr)?)? {
            scan.unlocks.push(Unlock { id: id.to_string(), unlocked_at: at.unwrap_or(mtime), hardcore: false });
        }
    }
    Ok(scan)
}

/// Trophy sets in an RPCS3 folder: `(folder, game title)`.
pub fn rpcs3_sets(rpcs3_dir: &Path) -> Vec<(PathBuf, String)> {
    let root = rpcs3_dir.join("dev_hdd0").join("home").join("00000001").join("trophy");
    let mut out: Vec<(PathBuf, String)> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter_map(|p| {
            let conf = crate::util::find_file_ci(&p, "TROPCONF.SFM")?;
            let title = read_xml(&conf).ok().and_then(|s| s.title).unwrap_or_else(|| p.file_name().unwrap().to_string_lossy().to_string());
            Some((p, title))
        })
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONF: &str = r#"<!--Sce-Np-Trophy-Signature: abc-->
<?xml version="1.0" encoding="utf-8"?>
<trophyconf version="1.0">
<npcommid>NPWR00001_00</npcommid>
<title-name>Test Game</title-name>
<trophy id="000" hidden="no" ttype="P" pid="-1"><name>All</name><detail>Get them all</detail></trophy>
<trophy id="001" hidden="yes" ttype="B" pid="000"><name>Secret</name><detail>Hidden one</detail></trophy>
<trophy id="002" hidden="no" ttype="G" pid="000"><name>Boss</name><detail>Beat the boss</detail></trophy>
</trophyconf>"#;

    #[test]
    fn parses_trophy_conf() {
        let set = parse_xml(CONF).unwrap();
        assert_eq!(set.title.as_deref(), Some("Test Game"));
        assert_eq!(set.trophies.len(), 3);
        assert_eq!(set.trophies[1].id, "1");
        assert!(set.trophies[1].hidden);
        assert_eq!(set.trophies[0].grade, Some(Grade::Platinum));
        assert_eq!(set.trophies[2].name, "Boss");
    }

    #[test]
    fn entities_are_refused_a_doctype_is_not() {
        let with_doctype = CONF.replace("<trophyconf", "<!DOCTYPE trophyconf>\n<trophyconf");
        assert_eq!(parse_xml(&with_doctype).unwrap().trophies.len(), 3);
        // Two kilobytes that would expand to 50 MB (and more of the same to
        // any size): 255 references to a long entity in each reference, and
        // as many of those as one likes.
        let long = "x".repeat(1000);
        let refs = "&b;".repeat(200);
        let laughs = format!("<?xml version=\"1.0\"?><!DOCTYPE t [<!ENTITY a \"{long}\"><!ENTITY b \"{}\">]><trophyconf><title-name>{refs}</title-name></trophyconf>", "&a;".repeat(255));
        let started = std::time::Instant::now();
        assert!(parse_xml(&laughs).is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    fn tropusr(entries: &[(u32, u32, u64)]) -> Vec<u8> {
        let mut d = vec![0u8; 0x30];
        d[0..4].copy_from_slice(&0x818F54ADu32.to_be_bytes());
        d[8..12].copy_from_slice(&2u32.to_be_bytes());
        let table4_off = 0x30 + 2 * 0x20;
        let table6_off = table4_off + entries.len() * 0x50;
        let header = |kind: u32, size: u32, off: usize| {
            let mut h = vec![0u8; 0x20];
            h[0..4].copy_from_slice(&kind.to_be_bytes());
            h[4..8].copy_from_slice(&size.to_be_bytes());
            h[8..12].copy_from_slice(&1u32.to_be_bytes());
            h[12..16].copy_from_slice(&(entries.len() as u32).to_be_bytes());
            h[16..24].copy_from_slice(&(off as u64).to_be_bytes());
            h
        };
        let h4 = header(4, 0x50, table4_off);
        let h6 = header(6, 0x60, table6_off);
        d.extend(h4);
        d.extend(h6);
        for (id, _, _) in entries {
            let mut e = vec![0u8; 0x50];
            e[0..4].copy_from_slice(&4u32.to_be_bytes());
            e[16..20].copy_from_slice(&id.to_be_bytes());
            d.extend(e);
        }
        for (id, state, tick) in entries {
            let mut e = vec![0u8; 0x60];
            e[0..4].copy_from_slice(&6u32.to_be_bytes());
            e[4..8].copy_from_slice(&0x60u32.to_be_bytes());
            e[16..20].copy_from_slice(&id.to_be_bytes());
            e[20..24].copy_from_slice(&state.to_be_bytes());
            e[32..40].copy_from_slice(&tick.to_be_bytes());
            e[40..48].copy_from_slice(&tick.to_be_bytes());
            d.extend(e);
        }
        d
    }

    #[test]
    fn parses_tropusr() {
        // 2026-11-23T10:00:00Z as CellRtcTick.
        let unix = Utc.with_ymd_and_hms(2026, 11, 23, 10, 0, 0).unwrap().timestamp();
        let tick = ((unix + RTC_EPOCH_OFFSET) as u64) * 1_000_000;
        let data = tropusr(&[(0, 0, 0), (1, 1, tick), (2, 1, 0)]);
        let got = parse_tropusr(&data).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, 1);
        assert_eq!(got[0].1.unwrap().timestamp(), unix);
        assert_eq!(got[1], (2, None));
    }

    #[test]
    fn rpcs3_set_folder() {
        let dir = tempfile::tempdir().unwrap();
        let set = dir.path().join("NPWR00001_00");
        std::fs::create_dir_all(&set).unwrap();
        std::fs::write(set.join("TROPCONF.SFM"), CONF).unwrap();
        std::fs::write(set.join("TROP002.PNG"), b"\x89PNG").unwrap();
        std::fs::write(set.join("TROPUSR.DAT"), tropusr(&[(0, 0, 0), (1, 0, 0), (2, 1, 63_900_000_000_000_000)])).unwrap();
        let scan = rpcs3(&set).unwrap();
        let schema = scan.schema.unwrap();
        assert_eq!(schema.achievements.len(), 3);
        assert!(schema.achievements[2].icon.as_deref().unwrap().ends_with("TROP002.PNG"));
        assert!(schema.achievements[0].icon.is_none());
        assert_eq!(scan.unlocks.len(), 1);
        assert_eq!(scan.unlocks[0].id, "2");
    }

    #[test]
    fn shadps4_user_folder() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path();
        let xml_dir = user.join("trophy/NPWR05818_00/Xml");
        std::fs::create_dir_all(&xml_dir).unwrap();
        std::fs::create_dir_all(user.join("trophy/NPWR05818_00/Icons")).unwrap();
        std::fs::write(user.join("trophy/NPWR05818_00/Icons/TROP001.PNG"), b"x").unwrap();
        std::fs::write(xml_dir.join("TROP.XML"), CONF).unwrap();
        let save = user.join("home/1000/trophy");
        std::fs::create_dir_all(&save).unwrap();
        let unlocked = CONF.replace(r#"<trophy id="001" hidden="yes""#, r#"<trophy id="001" unlockstate="true" timestamp="1763892000" hidden="yes""#);
        std::fs::write(save.join("NPWR05818_00.xml"), unlocked).unwrap();

        assert_eq!(shadps4_sets(user), vec!["NPWR05818_00".to_string()]);
        let scan = shadps4(user, "").unwrap();
        assert_eq!(scan.schema.as_ref().unwrap().achievements.len(), 3);
        assert!(scan.schema.as_ref().unwrap().achievements[1].icon.is_some());
        assert_eq!(scan.unlocks.len(), 1);
        assert_eq!(scan.unlocks[0].id, "1");
        assert_eq!(scan.unlocks[0].unlocked_at.timestamp(), 1_763_892_000);
    }

    #[test]
    fn log_lines() {
        let (id, at) = parse_log_line("12 2026-11-23T10:00:00Z").unwrap();
        assert_eq!(id, "12");
        assert_eq!(at.unwrap().timestamp(), Utc.with_ymd_and_hms(2026, 11, 23, 10, 0, 0).unwrap().timestamp());
        assert_eq!(parse_log_line("7,1763892000").unwrap(), ("7".into(), Utc.timestamp_opt(1_763_892_000, 0).single()));
        let (id, at) = parse_log_line("2026-11-23 10:00:00 5").unwrap();
        assert_eq!(id, "5");
        assert!(at.is_some());
        assert_eq!(parse_log_line(r#"{"id":3,"time":"2026-11-23T10:00:00Z"}"#).unwrap().0, "3");
        assert_eq!(parse_log_line("id=09 time=1763892000123").unwrap().0, "9");
        assert_eq!(parse_log_line("trophy 4").unwrap(), ("4".into(), None));
        assert!(parse_log_line("").is_none());
        assert!(parse_log_line("# header").is_none());
    }

    #[test]
    fn session_log() {
        let text = "x\nRuntime: trophy 3 earned in this save as own\nRuntime: trophy 12 unlocked as own\nRuntime: trophy 12 unlocked\nRuntime: trophy 5 unlocked as guest\nTrophies: trophy 0 (platinum) unlocked\n";
        let (unlocked, earned) = parse_session_log(text);
        assert_eq!(unlocked, vec!["12".to_string(), "5".to_string(), "0".to_string()]);
        assert_eq!(earned, vec!["3".to_string()]);
        // trophies.log rows are "<id> <unix seconds>" (confirmed by PSerban93/Achievements).
        assert_eq!(parse_log_line("12 1795428000").unwrap(), ("12".into(), Utc.timestamp_opt(1_795_428_000, 0).single()));
    }

    #[test]
    fn trp_icons() {
        let png = b"\x89PNG\r\n\x1a\nDATA".to_vec();
        let mut d = vec![0u8; 0x60];
        d[0..4].copy_from_slice(&0xDCA24D00u32.to_be_bytes());
        d[0x10..0x14].copy_from_slice(&2u32.to_be_bytes());
        d[0x14..0x18].copy_from_slice(&0x40u32.to_be_bytes());
        let data_off = 0x60 + 2 * 0x40;
        let entry = |name: &str, pos: usize, len: usize| {
            let mut e = vec![0u8; 0x40];
            e[..name.len()].copy_from_slice(name.as_bytes());
            e[32..40].copy_from_slice(&(pos as u64).to_be_bytes());
            e[40..48].copy_from_slice(&(len as u64).to_be_bytes());
            e
        };
        let e1 = entry("TROP000.PNG", data_off, png.len());
        let e2 = entry("TROPCONF.ESFM", data_off + png.len(), 4);
        d.extend(e1);
        d.extend(e2);
        d.extend(&png);
        d.extend(b"ENCR");
        let dir = tempfile::tempdir().unwrap();
        let trp = dir.path().join("trophy00.trp");
        std::fs::write(&trp, d).unwrap();
        let out = dir.path().join("icons");
        assert_eq!(extract_trp_icons(&trp, &out).unwrap(), 1);
        assert_eq!(std::fs::read(out.join("TROP000.PNG")).unwrap(), png);
    }

    #[test]
    fn ps4_port_records() {
        let dir = tempfile::tempdir().unwrap();
        let port = dir.path();
        let user = port.join("user");
        std::fs::create_dir_all(user.join("trophies/CUSA00001")).unwrap();
        // Bitmap: trophies 0 and 33 (word 1, bit 1).
        let mut bits = [0u8; 16];
        bits[0] = 0b1;
        bits[4] = 0b10;
        std::fs::write(user.join("trophies.dat"), bits).unwrap();
        std::fs::write(user.join("progress_user1.txt"), "DCP-TROPHIES 1\nnpcommid NPWR00001_00\nunlocked 5 1795428000\n").unwrap();
        std::fs::write(user.join("trophies/CUSA00001/unlocked.txt"), "7 1795428100\n").unwrap();
        std::fs::write(port.join("last_run.log"), "boot\nRuntime: trophy 9 unlocked\nRuntime: trophy 3 earned in this save\n").unwrap();
        assert_eq!(decode_bitset(&bits), vec!["0".to_string(), "33".to_string()]);
        let scan = ps4port(port, "NPWR00001_00").unwrap();
        let mut ids: Vec<&str> = scan.unlocks.iter().map(|u| u.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["0", "3", "33", "5", "7", "9"], "the progress header is not trophy 1");
        assert_eq!(scan.quiet, vec!["3".to_string()]);
        let five = scan.unlocks.iter().find(|u| u.id == "5").unwrap();
        assert_eq!(five.unlocked_at.timestamp(), 1_795_428_000);
    }

    #[test]
    fn npwr_ids() {
        assert!(is_npwr("NPWR05818_00"));
        assert!(!is_npwr("NPWR0581_00"));
        assert!(!is_npwr("CUSA03173"));
    }
}
