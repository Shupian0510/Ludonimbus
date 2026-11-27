//! Helping users find where an unknown game keeps its saves:
//!
//! - [`suggest`]: folders in the usual save locations whose names look like
//!   the game.
//! - [`Watch`]: record which files change while the user plays, then point
//!   at the folders that changed (ignoring caches, logs and other programs'
//!   usual noise).

use crate::discover::match_key;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
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
    pub files: usize,
    pub bytes: u64,
    pub modified: Option<DateTime<Utc>>,
    /// A few file names inside, for the user to recognise.
    pub sample: Vec<String>,
    /// Why it is suggested (shown to the user).
    pub reason: String,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(r"C:\Users\Default"))
}

/// The folders games usually write saves to.
pub fn save_roots() -> Vec<PathBuf> {
    let h = home();
    let docs = dirs::document_dir().unwrap_or_else(|| h.join("Documents"));
    let mut roots = vec![
        docs.join("My Games"),
        docs.clone(),
        h.join("Saved Games"),
        dirs::config_dir().unwrap_or_else(|| h.join("AppData").join("Roaming")),
        dirs::data_local_dir().unwrap_or_else(|| h.join("AppData").join("Local")),
        h.join("AppData").join("LocalLow"),
    ];
    roots.retain(|r| r.is_dir());
    roots
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

/// Size, file count, newest change and a few names of a folder (bounded).
fn describe(dir: &Path) -> (usize, u64, Option<DateTime<Utc>>, Vec<String>) {
    let (mut files, mut bytes, mut newest, mut sample) = (0usize, 0u64, None::<std::time::SystemTime>, Vec::new());
    for e in walkdir::WalkDir::new(dir).max_depth(5).into_iter().flatten().take(5000) {
        if !e.file_type().is_file() {
            continue;
        }
        files += 1;
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
    (files, bytes, newest.map(DateTime::<Utc>::from), sample)
}

/// Folders in the usual save locations (and save-like folders inside the
/// game folder) that look like they belong to the game, best first.
pub fn suggest(name: &str, exe: Option<&Path>, install: Option<&Path>) -> Vec<Candidate> {
    let toks = tokens(name, exe, install);
    let mut scored: Vec<(i64, PathBuf, String)> = Vec::new();
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
    for root in save_roots() {
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        for e in rd.flatten().filter(|e| e.path().is_dir()) {
            let n = e.file_name().to_string_lossy().into_owned();
            let s = score_name(&n);
            if s > 0 {
                scored.push((s, e.path(), crate::tr!("名字和游戏相近（{}）", "similar name ({})", root.display())));
            }
            // Company/Game layouts (LocalLow, My Games, AppData).
            if let Ok(rd2) = std::fs::read_dir(e.path()) {
                for e2 in rd2.flatten().filter(|e| e.path().is_dir()) {
                    let s2 = score_name(&e2.file_name().to_string_lossy());
                    if s2 > 0 {
                        scored.push((s2 + 5, e2.path(), crate::tr!("名字和游戏相近（{}）", "similar name ({})", e.path().display())));
                    }
                }
            }
        }
    }
    if let Some(dir) = install {
        for e in walkdir::WalkDir::new(dir).max_depth(3).into_iter().flatten() {
            if !e.file_type().is_dir() {
                continue;
            }
            let n = e.file_name().to_string_lossy().to_lowercase();
            if ["save", "saves", "savedata", "savegames", "savegame", "saved", "userdata", "profiles", "profile"].contains(&n.as_str()) {
                scored.push((80, e.path().to_path_buf(), crate::tr!("游戏目录里的存档文件夹", "save folder in the game folder")));
            }
        }
    }
    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    let mut out: Vec<Candidate> = Vec::new();
    for (_, path, reason) in scored {
        let p = path.to_string_lossy().into_owned();
        if out.iter().any(|c| c.path.eq_ignore_ascii_case(&p)) {
            continue;
        }
        let (files, bytes, modified, sample) = describe(&path);
        if files == 0 {
            continue;
        }
        out.push(Candidate { path: p, files, bytes, modified, sample, reason });
        if out.len() >= 12 {
            break;
        }
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.modified));
    out
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

    /// Stop and turn the changes into folder suggestions.
    pub fn finish(self) -> Vec<Candidate> {
        folders_from_changes(&self.changed_files())
    }
}

/// Group changed files by folder; nested folders fold into their parent.
pub fn folders_from_changes(files: &[PathBuf]) -> Vec<Candidate> {
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
    let mut out: Vec<Candidate> = kept
        .into_iter()
        .map(|(_, path, n)| {
            let (files, bytes, modified, sample) = describe(&path);
            Candidate {
                path: path.to_string_lossy().into_owned(),
                files,
                bytes,
                modified,
                sample,
                reason: crate::tr!("玩游戏时有 {n} 个文件发生变化", "{n} files changed while playing"),
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
        let out = folders_from_changes(&files);
        assert_eq!(out.len(), 2);
        let game = out.iter().find(|c| Path::new(&c.path).ends_with("Studio/Game")).unwrap();
        assert_eq!(game.files, 3);
        assert!(game.reason.contains('3'));
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
        let out = w.finish();
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].path.ends_with("Saves"));
        assert_eq!(out[0].sample, vec!["slot1.sav".to_string()]);
    }
}
