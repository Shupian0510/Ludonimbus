//! Helping users find where a game keeps its saves ("找存档位置"):
//!
//! - [`titles`]: the game in Ludusavi's database, by its Steam app id, its
//!   name, or a similar name.
//! - [`suggest`]: folders that may hold its saves, best first: what the
//!   game's engine and Steam say ([`hints`]), save folders in the game
//!   folder, and folders in the usual save locations named like the game.
//! - [`Watch`]: record which files change while the user plays, then point
//!   at the folders that changed (ignoring caches, logs and other programs'
//!   usual noise).
//!
//! All of them only suggest: the user picks what is used.

pub mod hints;

use crate::discover::match_key;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use hints::{Hint, Places};
use notify::{EventKind, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SaveCandidate")]
pub struct Candidate {
    pub path: String,
    /// The save pattern to use for it (`{localappdata}/Game/Saved/SaveGames/**`).
    pub pattern: String,
    pub files: usize,
    pub bytes: u64,
    pub modified: Option<DateTime<Utc>>,
    /// A few file names inside, for the user to recognise.
    pub sample: Vec<String>,
    /// Why it is suggested (shown to the user).
    pub reason: String,
    /// The game's engine or Steam says this is where it keeps saves (it may
    /// not have saved yet: no files).
    pub likely: bool,
}

/// The game in Ludusavi's database, as the save finder offers it.
#[derive(Debug, Clone, Serialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SaveTitle")]
pub struct TitleMatch {
    pub title: String,
    /// Found by the game's Steam app id or its exact name.
    pub likely: bool,
    /// Why it is offered (shown to the user).
    pub reason: String,
    /// Save files Ludusavi finds for it on this device (counted for the
    /// likely ones only).
    pub files: usize,
}

/// The folders games usually write saves to.
pub fn save_roots() -> Vec<PathBuf> {
    Places::here().roots()
}

/// Name fragments to look for: words of the name, the exe name, the folder.
fn tokens(name: &str, exe: Option<&Path>, install: Option<&Path>) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |s: &str| {
        let k = match_key(s);
        let long_enough = k.chars().count() >= 4 || (k.chars().count() >= 2 && !k.is_ascii());
        if long_enough && !out.contains(&k) {
            out.push(k);
        }
    };
    push(name);
    for w in name.split(|c: char| !c.is_alphanumeric()) {
        push(w);
    }
    if let Some(stem) = exe.and_then(Path::file_stem) {
        push(&stem.to_string_lossy());
    }
    if let Some(dir) = install.and_then(Path::file_name) {
        push(&dir.to_string_lossy());
    }
    // Generic words that would match everything.
    out.retain(|t| !["game", "games", "the", "save", "saves", "launcher", "win64", "shipping"].contains(&t.as_str()));
    out
}

/// Size, file count, newest change and a few names of a folder (bounded);
/// with `files`, of the matching files directly in it.
fn describe(dir: &Path, files: Option<&str>) -> (usize, u64, Option<DateTime<Utc>>, Vec<String>) {
    let glob = files.and_then(|f| globset::Glob::new(&f.to_lowercase()).ok()).map(|g| g.compile_matcher());
    let (mut count, mut bytes, mut newest, mut sample) = (0usize, 0u64, None::<std::time::SystemTime>, Vec::new());
    let depth = if glob.is_some() { 1 } else { 5 };
    for e in walkdir::WalkDir::new(dir).max_depth(depth).into_iter().flatten().take(5000) {
        if !e.file_type().is_file() {
            continue;
        }
        if glob.as_ref().is_some_and(|g| !g.is_match(e.file_name().to_string_lossy().to_lowercase())) {
            continue;
        }
        count += 1;
        if let Ok(md) = e.metadata() {
            bytes += md.len();
            if let Ok(m) = md.modified() {
                if newest.is_none_or(|n| m > n) {
                    newest = Some(m);
                }
            }
        }
        if sample.len() < 4 {
            sample.push(e.file_name().to_string_lossy().into_owned());
        }
    }
    (count, bytes, newest.map(DateTime::<Utc>::from), sample)
}

/// The save pattern for a folder (or the `files` in it) of a game started
/// by `exe`: under the most specific root, `{install}` being the program's
/// folder.
pub fn pattern_for(path: &Path, exe: Option<&Path>, files: Option<&str>) -> String {
    let install = exe.and_then(Path::parent).map(Path::to_path_buf);
    let roots = crate::spec::builtin_roots(install.as_deref());
    let same = roots.iter().find(|(_, p)| crate::util::same_path(&p.to_string_lossy(), &path.to_string_lossy())).map(|(n, _)| n.clone());
    let logical = same.unwrap_or_else(|| crate::spec::tokenize(path, &roots));
    let (root, rest) = logical.split_once('/').unwrap_or((&logical, ""));
    let tail = files.unwrap_or("**");
    if rest.is_empty() {
        format!("{{{root}}}/{tail}")
    } else {
        format!("{{{root}}}/{rest}/{tail}")
    }
}

/// What [`suggest_in`] looks at; tests point it at made-up folders.
#[derive(Debug, Clone)]
pub struct Look {
    pub places: Places,
    /// Steam's `userdata` folder.
    pub steam_userdata: Option<PathBuf>,
    /// The home whose launchers' Wine prefixes (Heroic, Lutris, Bottles) to
    /// look in: Linux only.
    pub prefixes_home: Option<PathBuf>,
}

impl Look {
    pub fn here() -> Look {
        let steam = crate::config::load().ok().and_then(|c| crate::steam::root(&c));
        Look { places: Places::here(), steam_userdata: steam.map(|r| r.join("userdata")), prefixes_home: if cfg!(windows) { None } else { dirs::home_dir() } }
    }
}

/// Folders that may hold the game's saves, best first.
pub fn suggest(name: &str, exe: Option<&Path>, install: Option<&Path>) -> Vec<Candidate> {
    suggest_in(&Look::here(), name, exe, install)
}

pub fn suggest_in(look: &Look, name: &str, exe: Option<&Path>, install: Option<&Path>) -> Vec<Candidate> {
    let toks = tokens(name, exe, install);
    let score_name = |dir_name: &str| -> i64 {
        let k = match_key(dir_name);
        if k.is_empty() {
            return 0;
        }
        toks.iter()
            .map(|t| {
                if k == *t {
                    100
                } else if (k.contains(t.as_str()) || t.contains(k.as_str())) && k.chars().count() >= 4 {
                    60
                } else {
                    0
                }
            })
            .max()
            .unwrap_or(0)
    };

    // A Windows game on Linux keeps its saves in its Wine / Proton prefix:
    // the one it is installed in comes first, then a launcher's prefix named
    // like the game.
    let mut places: Vec<Places> = Vec::new();
    let own_prefix = exe.or(install).and_then(hints::wine_prefix_of);
    if let Some(p) = &own_prefix {
        places.extend(hints::prefix_user_dirs(p).iter().map(|u| Places::windows(u)));
    }
    places.push(look.places.clone());
    if let Some(home) = &look.prefixes_home {
        for (prefix, _) in hints::launcher_prefixes(home) {
            let named = prefix.file_name().is_some_and(|n| score_name(&n.to_string_lossy()) >= 100);
            if named && own_prefix.as_ref().is_none_or(|o| !crate::util::same_path(&o.to_string_lossy(), &prefix.to_string_lossy())) {
                places.extend(hints::prefix_user_dirs(&prefix).iter().map(|u| Places::windows(u)));
            }
        }
    }

    let mut found: Vec<Hint> = Vec::new();
    for (i, p) in places.iter().enumerate() {
        // Where the engine will write even before the first save: only for
        // the places the game runs in.
        let first = i == 0;
        found.extend(hints::engine(p, exe, install, name).into_iter().filter(|h| first || h.path.exists()));
    }
    if let (Some(userdata), Some(appid)) = (&look.steam_userdata, hints::steam_appid(exe, install)) {
        found.extend(hints::steam_cloud(userdata, appid));
    }
    let game_dirs: Vec<&Path> = install.into_iter().chain(exe.and_then(Path::parent)).collect();
    for d in game_dirs {
        for s in hints::save_dirs_in(d) {
            found.push(Hint {
                path: s,
                files: None,
                reason: crate::tr!("游戏目录里的存档文件夹", "save folder in the game folder"),
                likely: false,
                score: 200,
            });
        }
    }
    for p in &places {
        for root in p.roots() {
            let Ok(rd) = std::fs::read_dir(&root) else { continue };
            for e in rd.flatten().filter(|e| e.path().is_dir()) {
                let n = e.file_name().to_string_lossy().into_owned();
                if hints::is_vendor_name(&n) || hints::is_cache_name(&n) {
                    continue;
                }
                let s = score_name(&n);
                if s > 0 {
                    found.push(Hint {
                        path: e.path(),
                        files: None,
                        reason: crate::tr!("名字和游戏相近（{}）", "similar name ({})", root.display()),
                        likely: false,
                        score: s,
                    });
                }
                // Company/Game layouts (LocalLow, My Games, AppData).
                if let Ok(rd2) = std::fs::read_dir(e.path()) {
                    for e2 in rd2.flatten().filter(|e| e.path().is_dir()) {
                        let n2 = e2.file_name().to_string_lossy().into_owned();
                        if hints::is_cache_name(&n2) {
                            continue;
                        }
                        let s2 = score_name(&n2);
                        if s2 > 0 {
                            let reason = crate::tr!("名字和游戏相近（{}）", "similar name ({})", e.path().display());
                            found.push(Hint { path: e2.path(), files: None, reason, likely: false, score: s2 + 5 });
                        }
                    }
                }
            }
        }
    }

    // Best first; one row per folder (with the best reason it has).
    found.sort_by_key(|h| (std::cmp::Reverse(h.likely), std::cmp::Reverse(h.score)));
    let mut out: Vec<(i64, Candidate)> = Vec::new();
    for h in found {
        let p = h.path.to_string_lossy().into_owned();
        let pattern = pattern_for(&h.path, exe, h.files.as_deref());
        if out.iter().any(|(_, c)| c.pattern == pattern) || places.iter().any(|pl| pl.never_offered(&h.path)) {
            continue;
        }
        let (files, bytes, modified, sample) = describe(&h.path, h.files.as_deref());
        if files == 0 && !h.likely {
            continue;
        }
        out.push((h.score, Candidate { path: p, pattern, files, bytes, modified, sample, reason: h.reason, likely: h.likely }));
        if out.len() >= 12 {
            break;
        }
    }
    // A convention with saves in it, then one without (not played yet),
    // then the others by how well they match; among equals the folder
    // written to last.
    out.sort_by_key(|(score, c)| (std::cmp::Reverse(c.likely), std::cmp::Reverse(c.files > 0), std::cmp::Reverse(*score), std::cmp::Reverse(c.modified)));
    out.into_iter().map(|(_, c)| c).collect()
}

/// The game in Ludusavi's database: by the Steam app id of its files, by
/// its exact name, then titles with similar names. Counts the save files
/// Ludusavi finds on this device for the likely ones (one run of Ludusavi).
pub fn titles(name: &str, exe: Option<&Path>, install: Option<&Path>) -> Result<Vec<TitleMatch>> {
    let mut out: Vec<TitleMatch> = Vec::new();
    let mut push = |title: String, likely: bool, reason: String| {
        if !out.iter().any(|t| t.title == title) {
            out.push(TitleMatch { title, likely, reason, files: 0 });
        }
    };
    if let Some(appid) = hints::steam_appid(exe, install) {
        if let Some(t) = crate::ludusavi::titles_by_steam_id(&[appid].into_iter().collect()).remove(&appid) {
            push(t, true, crate::tr!("Steam 应用 ID {appid}", "Steam app id {appid}"));
        }
    }
    let name = name.trim();
    if !name.is_empty() {
        if let Some(Some(t)) = crate::ludusavi::find_titles(&[name.to_string()])?.into_iter().next() {
            push(t, true, crate::tr!("名字相同", "same name"));
        }
        for t in crate::ludusavi::search(name)?.into_iter().take(6) {
            push(t, false, crate::tr!("名字相近", "similar name"));
        }
    }
    let likely: Vec<String> = out.iter().filter(|t| t.likely).map(|t| t.title.clone()).collect();
    if !likely.is_empty() {
        let counts = crate::ludusavi::file_counts(&likely).unwrap_or_default();
        for t in out.iter_mut() {
            t.files = counts.get(&t.title).copied().unwrap_or(0);
        }
    }
    Ok(out)
}

/// Path fragments of folders that never hold saves: temp files, caches, logs,
/// and the busy folders of common programs (anchored to AppData so a game
/// folder that happens to share a name is not ignored).
const NOISE_DIRS: [&str; 30] = [
    "\\temp\\",
    "\\tmp\\",
    "cache",
    "\\logs\\",
    "\\log\\",
    "crash",
    "dump",
    "\\appdata\\local\\nvidia",
    "\\appdata\\local\\amd\\",
    "\\appdata\\local\\intel\\",
    "\\microsoft\\",
    "\\appdata\\local\\packages\\",
    "\\appdata\\local\\google\\",
    "\\appdata\\roaming\\mozilla\\",
    "\\appdata\\local\\mozilla\\",
    "\\discord",
    "\\epic games\\launcher\\",
    "\\appdata\\local\\epicgameslauncher\\",
    "\\appdata\\roaming\\claude",
    "\\appdata\\local\\anthropic",
    "\\appdata\\roaming\\code\\",
    "\\.vscode",
    "\\appdata\\roaming\\ludonimbus\\",
    "\\tencent\\",
    "\\wechat",
    "\\appdata\\local\\programs\\",
    "\\connecteddevicesplatform\\",
    "\\squirreltemp",
    "\\appdata\\local\\unity\\",
    "\\appdata\\roaming\\baidu\\",
];
const NOISE_EXTS: [&str; 12] = [".log", ".tmp", ".etl", ".lock", ".dmp", ".pf", ".journal", "-journal", ".cache", ".old", ".bak~", ".crdownload"];

/// Changes that are almost never saves.
pub fn is_noise(path: &Path) -> bool {
    // Matched in Windows form: Linux and macOS paths get `\` too.
    let lower = path.to_string_lossy().replace('/', "\\").to_lowercase();
    // The Steam client's own folders (not games in a Steam library).
    let steam_client = lower.contains("\\steam\\") && !lower.contains("\\steamapps\\");
    steam_client || NOISE_DIRS.iter().any(|d| lower.contains(d)) || NOISE_EXTS.iter().any(|e| lower.ends_with(e)) || lower.ends_with("player.log")
}

/// Records file changes under some folders until stopped.
pub struct Watch {
    _watcher: notify::RecommendedWatcher,
    changes: Arc<Mutex<BTreeMap<PathBuf, usize>>>,
}

impl Watch {
    /// Start watching `roots` recursively (missing folders are skipped).
    pub fn start(roots: &[PathBuf]) -> Result<Watch> {
        let changes: Arc<Mutex<BTreeMap<PathBuf, usize>>> = Arc::default();
        let sink = changes.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(ev) = res else { return };
            if !matches!(ev.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                return;
            }
            let created = matches!(ev.kind, EventKind::Create(_));
            let mut map = sink.lock().unwrap();
            for p in ev.paths {
                if is_noise(&p) {
                    continue;
                }
                // Linux watches a new folder only once it exists: files
                // written into it right away come without events of their own.
                if created && p.is_dir() {
                    for e in walkdir::WalkDir::new(&p).max_depth(4).into_iter().flatten().take(2000) {
                        if e.file_type().is_file() && !is_noise(e.path()) {
                            *map.entry(e.into_path()).or_insert(0) += 1;
                        }
                    }
                    continue;
                }
                *map.entry(p).or_insert(0) += 1;
            }
        })
        .context(crate::tr!("无法开始记录文件变化", "Couldn't start recording file changes"))?;
        for r in roots {
            if r.is_dir() {
                let _ = watcher.watch(r, RecursiveMode::Recursive);
            }
        }
        Ok(Watch { _watcher: watcher, changes })
    }

    /// Changed files so far (existing regular files only).
    pub fn changed_files(&self) -> Vec<PathBuf> {
        self.changes.lock().unwrap().keys().filter(|p| p.is_file()).cloned().collect()
    }

    /// Stop and turn the changes into folder suggestions for a game started
    /// by `exe`.
    pub fn finish(self, exe: Option<&Path>) -> Vec<Candidate> {
        folders_from_changes(&self.changed_files(), exe)
    }
}

/// Group changed files by folder; nested folders fold into their parent.
/// Roots that hold many programs' data (Documents, AppData…) are left out.
pub fn folders_from_changes(files: &[PathBuf], exe: Option<&Path>) -> Vec<Candidate> {
    let mut dirs: BTreeMap<String, (PathBuf, usize)> = BTreeMap::new();
    for f in files {
        if let Some(parent) = f.parent() {
            let key = crate::util::norm_path(&parent.to_string_lossy());
            dirs.entry(key).or_insert_with(|| (parent.to_path_buf(), 0)).1 += 1;
        }
    }
    // Fold a folder into an already kept ancestor (keys are sorted, so
    // ancestors come first).
    let mut kept: Vec<(String, PathBuf, usize)> = Vec::new();
    for (key, (path, n)) in dirs {
        if let Some(k) = kept.iter_mut().find(|(k, _, _)| key.starts_with(&format!("{k}{}", std::path::MAIN_SEPARATOR))) {
            k.2 += n;
        } else {
            kept.push((key, path, n));
        }
    }
    let here = Places::here();
    let mut out: Vec<Candidate> = kept
        .into_iter()
        .filter(|(_, path, _)| !here.never_offered(path))
        .map(|(_, path, n)| {
            let (files, bytes, modified, sample) = describe(&path, None);
            Candidate {
                pattern: pattern_for(&path, exe, None),
                path: path.to_string_lossy().into_owned(),
                files,
                bytes,
                modified,
                sample,
                reason: crate::tr!("玩游戏时有 {n} 个文件发生变化", "{n} files changed while playing"),
                likely: false,
            }
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.modified));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_noise() {
        assert!(is_noise(Path::new(r"C:\Users\me\AppData\Local\Temp\x.dat")));
        assert!(is_noise(Path::new(r"C:\Users\me\AppData\Local\Google\Chrome\User Data\Default\History")));
        assert!(is_noise(Path::new(r"C:\Users\me\AppData\LocalLow\Studio\Game\Player.log")));
        assert!(is_noise(Path::new(r"C:\Users\me\AppData\Local\Game\ShaderCache\a.bin")));
        assert!(!is_noise(Path::new(r"C:\Users\me\AppData\LocalLow\Team Cherry\Hollow Knight\user1.dat")));
        assert!(!is_noise(Path::new(r"C:\Users\me\Documents\My Games\Game\Saves\slot1.sav")));
        assert!(!is_noise(Path::new(r"D:\Projects\Ludonimbus\Saves\slot1.sav")));
        assert!(!is_noise(Path::new(r"D:\SteamLibrary\steamapps\common\Breathedge\Saved\save.sav")));
        assert!(is_noise(Path::new(r"C:\Program Files (x86)\Steam\config\loginusers.vdf")));
        assert!(is_noise(Path::new(r"C:\Users\me\AppData\Roaming\Ludonimbus\state.json")));
    }

    #[test]
    fn tokens_skip_generic_words() {
        let t = tokens("The Witcher 3: Wild Hunt", Some(Path::new("C:/G/bin/x64/witcher3.exe")), None);
        assert!(t.contains(&"thewitcher3wildhunt".to_string()));
        assert!(t.contains(&"witcher".to_string()));
        assert!(t.contains(&"witcher3".to_string()));
        assert!(!t.contains(&"the".to_string()));
        assert!(tokens("火焰纹章", None, None).contains(&"火焰纹章".to_string()));
    }

    #[test]
    fn folds_changed_files_into_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let save = tmp.path().join("Studio").join("Game");
        std::fs::create_dir_all(save.join("Slots")).unwrap();
        let cfg = tmp.path().join("Other");
        std::fs::create_dir_all(&cfg).unwrap();
        let files = [save.join("profile.dat"), save.join("Slots").join("1.sav"), save.join("Slots").join("2.sav"), cfg.join("settings.ini")];
        for f in &files {
            std::fs::write(f, b"x").unwrap();
        }
        let out = folders_from_changes(&files, None);
        assert_eq!(out.len(), 2);
        let game = out.iter().find(|c| Path::new(&c.path).ends_with("Studio/Game")).unwrap();
        assert_eq!(game.files, 3);
        assert!(game.reason.contains('3'));
        assert!(game.pattern.ends_with("Studio/Game/**"), "{}", game.pattern);
    }

    #[test]
    fn patterns_for_folders_and_files() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("Game.exe");
        assert_eq!(pattern_for(&tmp.path().join("www").join("save"), Some(&exe), None), "{install}/www/save/**");
        // The program's own folder, for files in it.
        assert_eq!(pattern_for(tmp.path(), Some(&exe), Some("Save*.rvdata2")), "{install}/Save*.rvdata2");
    }

    #[test]
    fn watches_real_changes() {
        // Not under %TEMP%: that is filtered as noise.
        let base = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let tmp = tempfile::Builder::new().prefix("watch-test").tempdir_in(base).unwrap();
        let w = Watch::start(&[tmp.path().to_path_buf()]).unwrap();
        let dir = tmp.path().join("Saves");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("slot1.sav"), b"progress").unwrap();
        std::fs::write(tmp.path().join("debug.log"), b"noise").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while w.changed_files().is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        let out = w.finish(None);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].path.ends_with("Saves"));
        assert_eq!(out[0].sample, vec!["slot1.sav".to_string()]);
    }
}
