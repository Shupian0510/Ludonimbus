//! Where a game keeps its saves when Ludusavi's database doesn't know it.
//! Every rule here only suggests a folder: the user confirms it in the save
//! finder before anything is synced.
//!
//! - **Engines** keep saves in fixed places, worked out from the game's own
//!   files: Unity (`<Game>_Data/app.info` names the company and product →
//!   `LocalLow/<company>/<product>`), Unreal (`<Project>/Binaries` →
//!   `Local/<Project>/Saved/SaveGames`, or `Saved/SaveGames` in the game
//!   folder), Godot (a `.pck` → `Godot/app_userdata/<project>`), Ren'Py
//!   (`config.save_directory` in `game/options.rpy` → `RenPy/<dir>`),
//!   GameMaker (`data.win` → `Local/<game>`) and RPG Maker (MV `www/save`, MZ
//!   `save`, VX Ace / VX / XP `Save*.rvdata2` / `.rvdata` / `.rxdata` next to
//!   `Game.exe`).
//! - **Steam**: the game's app id (`steam_appid.txt`, or the app manifest of
//!   the library it is installed in) names its Steam Cloud folder,
//!   `userdata/<account>/<appid>/remote`.
//! - **The game folder**: folders whose names say "save".
//! - **Wine / Proton prefixes**: the user folders inside the prefix a game
//!   runs in, and the prefixes Heroic, Lutris and Bottles keep (Linux).
//! - **Never offered as a whole**: the usual roots themselves (Documents, My
//!   Games, AppData…), engines' shared folders, caches and the folders of
//!   drivers and launchers.
//!
//! The save folders in the game folder (`save_dirs_in`), Unreal's
//! `Saved/SaveGames` in the game folder, Steam's `remote` folder, the cache,
//! vendor and never-offered folder names, and the launchers' prefix folders
//! are derived from OpenSave (<https://github.com/Liquid-co/OpenSave>,
//! `internal/presets`: `savedirs.go`, `junkdirs.go`, `steamlib.go`,
//! `scanner.go`, `proton.go`, `wineprefix.go`, `ludusavi.go`), under its
//! licence:
//!
//! ```text
//! MIT License
//!
//! Copyright (c) 2026 Siva Prakash (original OpenSave)
//! Copyright (c) 2026 OpenSave contributors (Go rewrite)
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.
//! ```

use crate::discover::match_key;
use std::path::{Path, PathBuf};

/// The folders saves usually go to, on this device or inside a Wine prefix.
#[derive(Debug, Clone)]
pub struct Places {
    pub home: PathBuf,
    pub documents: PathBuf,
    /// `AppData\Roaming`; `~/.config` on Linux.
    pub roaming: PathBuf,
    /// `AppData\Local`; `~/.local/share` on Linux.
    pub local: PathBuf,
    /// `AppData\LocalLow` (Windows and Wine prefixes).
    pub locallow: Option<PathBuf>,
    pub saved_games: Option<PathBuf>,
    /// Native Linux folders: engines use their Linux places.
    pub linux: bool,
}

impl Places {
    /// The folders of a Windows user home (`C:\Users\me`, or `steamuser` in
    /// a Proton prefix).
    pub fn windows(home: &Path) -> Places {
        // Older Wine prefixes call it "My Documents".
        let documents = ["Documents", "My Documents"].iter().map(|d| home.join(d)).find(|p| p.is_dir()).unwrap_or_else(|| home.join("Documents"));
        let appdata = home.join("AppData");
        Places {
            home: home.to_path_buf(),
            documents,
            roaming: appdata.join("Roaming"),
            local: appdata.join("Local"),
            locallow: Some(appdata.join("LocalLow")),
            saved_games: Some(home.join("Saved Games")),
            linux: false,
        }
    }

    /// The folders of a Linux user home (XDG defaults under `home`).
    pub fn linux(home: &Path) -> Places {
        Places {
            home: home.to_path_buf(),
            documents: home.join("Documents"),
            roaming: home.join(".config"),
            local: home.join(".local").join("share"),
            locallow: None,
            saved_games: None,
            linux: true,
        }
    }

    /// This device's folders.
    pub fn here() -> Places {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from(r"C:\Users\Default"));
        if cfg!(windows) {
            let mut p = Places::windows(&home);
            if let Some(d) = dirs::document_dir() {
                p.documents = d;
            }
            if let Some(d) = dirs::config_dir() {
                p.roaming = d;
            }
            if let Some(d) = dirs::data_local_dir() {
                p.local = d;
            }
            p
        } else {
            let mut p = Places::linux(&home);
            if let Some(d) = dirs::config_dir() {
                p.roaming = d;
            }
            if let Some(d) = dirs::data_dir() {
                p.local = d;
            }
            if let Some(d) = dirs::document_dir() {
                p.documents = d;
            }
            p
        }
    }

    /// The roots whose sub-folders (and theirs) may be a game's saves.
    pub fn roots(&self) -> Vec<PathBuf> {
        let mut out = vec![self.documents.join("My Games"), self.documents.clone()];
        out.extend(self.saved_games.clone());
        out.push(self.roaming.clone());
        out.push(self.local.clone());
        out.extend(self.locallow.clone());
        if self.linux {
            // Native Unity games and Ren'Py keep their own folders here.
            out.push(self.roaming.join("unity3d"));
            out.push(self.home.join(".renpy"));
        }
        out.retain(|r| r.is_dir());
        out
    }

    fn renpy(&self) -> PathBuf {
        if self.linux {
            self.home.join(".renpy")
        } else {
            self.roaming.join("RenPy")
        }
    }

    fn godot_userdata(&self) -> PathBuf {
        if self.linux {
            self.local.join("godot").join("app_userdata")
        } else {
            self.roaming.join("Godot").join("app_userdata")
        }
    }

    /// Folders that hold many programs' data: never offered as one game's
    /// saves (a match must be a folder inside them).
    pub fn never_offered(&self, path: &Path) -> bool {
        let mut roots = vec![
            self.home.clone(),
            self.documents.clone(),
            self.documents.join("My Games"),
            self.roaming.clone(),
            self.local.clone(),
            self.home.join("Desktop"),
            self.home.join("Downloads"),
            self.local.join("Programs"),
            self.local.join("Packages"),
            self.local.join("Temp"),
            self.renpy(),
            self.godot_userdata(),
            self.roaming.join("Godot"),
            self.local.join("godot"),
            self.roaming.join("LOVE"),
            self.roaming.join("unity3d"),
            self.home.join(".local"),
        ];
        roots.extend(self.saved_games.clone());
        if let Some(low) = &self.locallow {
            roots.push(low.clone());
            roots.push(low.join("DefaultCompany"));
        }
        let key = norm(path);
        roots.iter().any(|r| norm(r) == key)
    }
}

fn norm(p: &Path) -> String {
    crate::util::norm_path(&p.to_string_lossy())
}

/// A folder (or, with `files`, some files in it) that may hold a game's saves.
#[derive(Debug, Clone, PartialEq)]
pub struct Hint {
    pub path: PathBuf,
    /// Only these files in `path` (`Save*.rvdata2`), not the whole folder.
    pub files: Option<String>,
    /// Why it is suggested (shown to the user).
    pub reason: String,
    /// An engine's or Steam's own convention for this very game.
    pub likely: bool,
    /// Order among the suggestions: higher first.
    pub score: i64,
}

fn hint(path: PathBuf, reason: String, likely: bool, score: i64) -> Hint {
    Hint { path, files: None, reason, likely, score }
}

/// The folders to look at for a game: its program's folder and the game
/// folder (when it is another one).
fn game_dirs(exe: Option<&Path>, install: Option<&Path>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for d in exe.and_then(Path::parent).into_iter().chain(install) {
        if d.is_dir() && !out.iter().any(|o| norm(o) == norm(d)) {
            out.push(d.to_path_buf());
        }
    }
    out
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    out.sort();
    out
}

fn has_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut rd| rd.next().is_some())
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn eq_name(p: &Path, name: &str) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
}

/// Everything the engines of the game say about its saves, for `places`
/// (this device's, or a Wine prefix's when the game runs in one).
pub fn engine(places: &Places, exe: Option<&Path>, install: Option<&Path>, name: &str) -> Vec<Hint> {
    let dirs = game_dirs(exe, install);
    let mut out = Vec::new();
    out.extend(unity(places, &dirs, exe));
    out.extend(unreal(places, &dirs, exe));
    out.extend(godot(places, &dirs, exe, name));
    out.extend(renpy(places, &dirs, exe, name));
    out.extend(gamemaker(places, &dirs, exe));
    out.extend(rpgmaker(&dirs));
    out
}

// ---------------------------------------------------------------------------
// Unity

/// `(company, product)` from Unity's `<Game>_Data/app.info`: the program's
/// own data folder first, then any in the game's folders.
pub fn unity_app_info(dirs: &[PathBuf], exe: Option<&Path>) -> Option<(String, String)> {
    let read = |p: &Path| -> Option<(String, String)> {
        let text = std::fs::read_to_string(p).ok()?;
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
        Some((lines.next()?.to_string(), lines.next()?.to_string()))
    };
    let stem = exe.and_then(Path::file_stem).map(|s| s.to_string_lossy().into_owned());
    for d in dirs {
        if let Some(s) = &stem {
            if let Some(info) = read(&d.join(format!("{s}_Data")).join("app.info")) {
                return Some(info);
            }
        }
    }
    dirs.iter().flat_map(|d| subdirs(d)).filter(|d| name_of(d).to_lowercase().ends_with("_data")).find_map(|d| read(&d.join("app.info")))
}

fn unity(places: &Places, dirs: &[PathBuf], exe: Option<&Path>) -> Vec<Hint> {
    let Some((company, product)) = unity_app_info(dirs, exe) else { return Vec::new() };
    let path = match (&places.locallow, places.linux) {
        (_, true) => places.roaming.join("unity3d").join(&company).join(&product),
        (Some(low), false) => low.join(&company).join(&product),
        (None, false) => return Vec::new(),
    };
    vec![hint(path, crate::tr!("Unity 游戏的存档位置（{company} / {product}）", "where Unity games keep saves ({company} / {product})"), true, 300)]
}

// ---------------------------------------------------------------------------
// Unreal

/// Unreal projects of the game: `<Project>/Binaries/<platform>/<exe>` for a
/// shipping program, and folders with `Binaries` and `Content` beside a
/// launcher. Returns `(project folder, names the saves may be under)`.
pub fn unreal_projects(dirs: &[PathBuf], exe: Option<&Path>) -> Vec<(PathBuf, Vec<String>)> {
    let mut out: Vec<(PathBuf, Vec<String>)> = Vec::new();
    let mut push = |dir: PathBuf, extra: Option<String>| {
        let mut names = vec![name_of(&dir)];
        if let Some(e) = extra.filter(|e| !e.is_empty() && !names.iter().any(|n| n.eq_ignore_ascii_case(e))) {
            names.push(e);
        }
        if let Some(o) = out.iter_mut().find(|(d, _)| norm(d) == norm(&dir)) {
            for n in names {
                if !o.1.iter().any(|x| x.eq_ignore_ascii_case(&n)) {
                    o.1.push(n);
                }
            }
        } else {
            out.push((dir, names));
        }
    };
    if let Some(exe) = exe {
        let bin = exe.parent().and_then(Path::parent);
        if let Some(project) = bin.filter(|b| eq_name(b, "Binaries")).and_then(Path::parent) {
            // `Game-Win64-Shipping.exe`: the part before the platform is the project's name too.
            let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let short = stem.to_lowercase().contains("shipping").then(|| stem.split('-').next().unwrap_or_default().to_string());
            push(project.to_path_buf(), short);
        }
    }
    for d in dirs {
        for sub in subdirs(d) {
            if !eq_name(&sub, "Engine") && sub.join("Binaries").is_dir() && sub.join("Content").is_dir() {
                push(sub, None);
            }
        }
    }
    out
}

fn unreal(places: &Places, dirs: &[PathBuf], exe: Option<&Path>) -> Vec<Hint> {
    let mut out = Vec::new();
    for (i, (project, names)) in unreal_projects(dirs, exe).into_iter().enumerate() {
        // Next to the game (portable releases, some ports).
        let beside = project.join("Saved").join("SaveGames");
        if has_files(&beside) {
            out.push(hint(beside, crate::tr!("Unreal 游戏目录里的存档", "Unreal saves in the game folder"), true, 290));
        }
        let base = if places.linux { places.roaming.join("Epic") } else { places.local.clone() };
        let found: Vec<PathBuf> = names.iter().map(|n| base.join(n).join("Saved").join("SaveGames")).filter(|p| p.is_dir()).collect();
        if found.is_empty() && i == 0 {
            // Not played yet: where the game will write.
            out.push(hint(
                base.join(&names[0]).join("Saved").join("SaveGames"),
                crate::tr!("Unreal 游戏的存档位置（{}）", "where Unreal games keep saves ({})", names[0]),
                true,
                280,
            ));
        }
        for p in found {
            let n = p.parent().and_then(Path::parent).map(name_of).unwrap_or_default();
            out.push(hint(p, crate::tr!("Unreal 游戏的存档位置（{n}）", "where Unreal games keep saves ({n})"), true, 300));
        }
    }
    // Saved/SaveGames in the game folder or one level below it.
    for d in dirs {
        for p in std::iter::once(d.clone()).chain(subdirs(d).into_iter().take(100)) {
            let s = p.join("Saved").join("SaveGames");
            if has_files(&s) && !out.iter().any(|h| norm(&h.path) == norm(&s)) {
                out.push(hint(s, crate::tr!("Unreal 游戏目录里的存档", "Unreal saves in the game folder"), true, 290));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Godot, Ren'Py, GameMaker

fn tokens(name: &str, extra: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in std::iter::once(name.to_string()).chain(extra.iter().cloned()) {
        let k = match_key(&s);
        if k.chars().count() >= 3 && !out.contains(&k) {
            out.push(k);
        }
    }
    out
}

/// Sub-folders of `dir` named like the game (`Game`, `Game-1234567890`).
fn named_like(dir: &Path, toks: &[String]) -> Vec<PathBuf> {
    subdirs(dir)
        .into_iter()
        .filter(|d| {
            let k = match_key(&name_of(d));
            !k.is_empty() && toks.iter().any(|t| k == *t || (k.starts_with(t.as_str()) && k[t.len()..].chars().all(|c| c.is_ascii_digit())))
        })
        .collect()
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn godot(places: &Places, dirs: &[PathBuf], exe: Option<&Path>, name: &str) -> Vec<Hint> {
    let packs: Vec<String> = dirs
        .iter()
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pck")))
        .map(|p| stem(&p))
        .collect();
    if packs.is_empty() {
        return Vec::new();
    }
    let extra: Vec<String> = packs.into_iter().chain(exe.map(stem)).collect();
    let toks = tokens(name, &extra);
    named_like(&places.godot_userdata(), &toks)
        .into_iter()
        .chain(named_like(&places.roaming, &toks).into_iter().filter(|d| has_files(d)))
        .map(|p| hint(p, crate::tr!("Godot 游戏的存档位置", "where Godot games keep saves"), true, 280))
        .collect()
}

/// `config.save_directory` from a Ren'Py game's `game/options.rpy`.
pub fn renpy_save_directory(game_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(game_dir.join("game").join("options.rpy")).ok()?;
    text.lines().map(str::trim).filter(|l| !l.starts_with('#')).find_map(|l| {
        let rest = l.strip_prefix("define ").unwrap_or(l).trim_start();
        let rest = rest.strip_prefix("config.save_directory")?.trim_start().strip_prefix('=')?.trim();
        let q = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let inner = &rest[1..];
        Some(inner[..inner.find(q)?].to_string()).filter(|s| !s.is_empty())
    })
}

fn renpy(places: &Places, dirs: &[PathBuf], exe: Option<&Path>, name: &str) -> Vec<Hint> {
    let Some(game) = dirs.iter().find(|d| d.join("renpy").is_dir() && d.join("game").is_dir()) else { return Vec::new() };
    let reason = || crate::tr!("Ren'Py 游戏的存档位置", "where Ren'Py games keep saves");
    let mut out = Vec::new();
    match renpy_save_directory(game) {
        Some(dir) => out.push(hint(places.renpy().join(dir), reason(), true, 300)),
        None => {
            let toks = tokens(name, &exe.map(stem).into_iter().chain([name_of(game)]).collect::<Vec<_>>());
            out.extend(named_like(&places.renpy(), &toks).into_iter().map(|p| hint(p, reason(), true, 280)));
        }
    }
    let local = game.join("game").join("saves");
    if has_files(&local) {
        out.push(hint(local, crate::tr!("Ren'Py 游戏目录里的存档", "Ren'Py saves in the game folder"), true, 260));
    }
    out
}

fn gamemaker(places: &Places, dirs: &[PathBuf], exe: Option<&Path>) -> Vec<Hint> {
    let Some(exe) = exe else { return Vec::new() };
    if !dirs.iter().any(|d| d.join("data.win").is_file() || d.join("game.unx").is_file()) {
        return Vec::new();
    }
    let base = if places.linux { &places.roaming } else { &places.local };
    vec![hint(base.join(stem(exe)), crate::tr!("GameMaker 游戏的存档位置", "where GameMaker games keep saves"), true, 280)]
}

// ---------------------------------------------------------------------------
// RPG Maker

fn rpgmaker(dirs: &[PathBuf]) -> Vec<Hint> {
    let mut out = Vec::new();
    for d in dirs {
        let reason = |v: &str| crate::tr!("RPG Maker {v} 游戏的存档", "RPG Maker {v} saves");
        if d.join("www").join("js").join("rpg_core.js").is_file() || d.join("www").join("data").join("System.json").is_file() {
            out.push(hint(d.join("www").join("save"), reason("MV"), true, 300));
        } else if d.join("js").join("rmmz_core.js").is_file() {
            out.push(hint(d.join("save"), reason("MZ"), true, 300));
        }
        let has = |ext: &str| std::fs::read_dir(d).into_iter().flatten().flatten().any(|e| name_of(&e.path()).to_lowercase().ends_with(ext));
        let classic = [(".rgss3a", "Save*.rvdata2", "VX Ace"), (".rgss2a", "Save*.rvdata", "VX"), (".rgssad", "Save*.rxdata", "XP")];
        let saves = [("rvdata2", "Save*.rvdata2", "VX Ace"), ("rvdata", "Save*.rvdata", "VX"), ("rxdata", "Save*.rxdata", "XP")];
        let found = classic
            .iter()
            .find(|(ext, _, _)| has(ext))
            .map(|(_, f, v)| (*f, *v))
            .or_else(|| d.join("Game.ini").is_file().then(|| saves.iter().find(|(ext, _, _)| has(&format!(".{ext}"))).map(|(_, f, v)| (*f, *v))).flatten());
        if let Some((files, v)) = found {
            out.push(Hint { path: d.clone(), files: Some(files.to_string()), reason: reason(v), likely: true, score: 300 });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Steam

/// The Steam app id of a game: `steam_appid.txt` beside the program or in
/// the game folder, or the app manifest of the Steam library the game is
/// installed in (`steamapps/common/<folder>`).
pub fn steam_appid(exe: Option<&Path>, install: Option<&Path>) -> Option<u32> {
    let dirs = game_dirs(exe, install);
    let from_file = dirs.iter().find_map(|d| {
        let text = std::fs::read_to_string(d.join("steam_appid.txt")).ok()?;
        text.split_whitespace().next()?.parse::<u32>().ok().filter(|id| *id > 0)
    });
    from_file.or_else(|| dirs.iter().find_map(|d| appid_from_library(d)))
}

fn appid_from_library(dir: &Path) -> Option<u32> {
    let mut below: Option<&Path> = None;
    for a in dir.ancestors() {
        if eq_name(a, "common") && a.parent().is_some_and(|p| eq_name(p, "steamapps")) {
            let folder = name_of(below?);
            let apps = a.parent()?;
            for e in std::fs::read_dir(apps).ok()?.flatten() {
                let file = name_of(&e.path()).to_lowercase();
                if !(file.starts_with("appmanifest_") && file.ends_with(".acf")) {
                    continue;
                }
                let Some(doc) = std::fs::read_to_string(e.path()).ok().and_then(|s| crate::vdf::parse_text(&s).ok()) else { continue };
                let Some(state) = doc.get("AppState") else { continue };
                let field = |k: &str| state.get(k).and_then(crate::vdf::Text::as_str).unwrap_or("").to_string();
                if field("installdir").eq_ignore_ascii_case(&folder) {
                    return field("appid").parse().ok();
                }
            }
            return None;
        }
        below = Some(a);
    }
    None
}

/// The game's Steam Cloud folders: `<userdata>/<account>/<appid>/remote`,
/// where Steam keeps what it syncs (the bookkeeping beside it is not saves).
pub fn steam_cloud(userdata: &Path, appid: u32) -> Vec<Hint> {
    subdirs(userdata)
        .into_iter()
        .map(|account| account.join(appid.to_string()).join("remote"))
        .filter(|p| has_files(p))
        .map(|p| hint(p, crate::tr!("Steam 云存档的文件夹（应用 {appid}）", "Steam Cloud's folder (app {appid})"), false, 240))
        .collect()
}

// ---------------------------------------------------------------------------
// The game folder

/// Folder names that are caches and other data the game makes again.
pub fn is_cache_name(name: &str) -> bool {
    const NAMES: [&str; 35] = [
        "dx12cache",
        "dxcache",
        "d3dcache",
        "d3dscache",
        "dxil",
        "dxbc",
        "pipelinecache",
        "psocache",
        "shadercache",
        "shadercachedb",
        "shaders",
        "shadercompiler",
        "derivedatacache",
        "ddc",
        "gpucache",
        "glcache",
        "vulkancache",
        "nvidiacache",
        "cache",
        "caches",
        "cacheddata",
        "temp",
        "tmp",
        "logs",
        "log",
        "crashes",
        "crashdumps",
        "crashreports",
        "webcache",
        "mediacache",
        "config",
        "screenshots",
        "movies",
        "paks",
        "binaries",
    ];
    let n = dir_key(name);
    !n.is_empty() && (NAMES.contains(&n.as_str()) || n.ends_with("cache"))
}

/// Lowercase without the separators people use interchangeably.
fn dir_key(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, ' ' | '_' | '-' | '.')).flat_map(char::to_lowercase).collect()
}

/// Folders of drivers, launchers and Windows itself in the usual roots:
/// never one game's saves.
pub fn is_vendor_name(name: &str) -> bool {
    const VENDORS: [&str; 34] = [
        "microsoft",
        "windows",
        "wine",
        "adobe",
        "nvidia",
        "nvidia corporation",
        "intel",
        "intel corporation",
        "amd",
        "oracle",
        "sun",
        "temp",
        "google",
        "discord",
        "mozilla",
        "unity",
        "unitytechnologies",
        "unity technologies",
        "epic games",
        "epicgameslauncher",
        "valve",
        "steam",
        "packages",
        "programs",
        "connecteddevicesplatform",
        "comms",
        "d3dscache",
        "inputmethod",
        "vulkan",
        "crashdumps",
        "criware",
        "easyanticheat",
        "battleye",
        "goginstaller",
    ];
    let lower = name.trim().to_lowercase();
    VENDORS.contains(&lower.as_str()) || looks_like_hash(&lower)
}

/// Cache folders named by a long hex digest.
fn looks_like_hash(s: &str) -> bool {
    s.len() >= 32 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Save folders inside the game folder, found by name ("Saves",
/// "savegames", "SaveData"…), at most three levels down and four of them;
/// caches skipped, empty folders left out, nothing below a match. A match
/// holding a more specific one ("Saved" → "SaveGames") gives that one.
pub fn save_dirs_in(root: &Path) -> Vec<PathBuf> {
    const DEPTH: usize = 3;
    const FANOUT: usize = 120;
    const HITS: usize = 4;
    fn named_save(name: &str) -> bool {
        let k = dir_key(name);
        k.contains("save") || ["userdata", "profiles", "profile"].contains(&k.as_str())
    }
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > DEPTH || out.len() >= HITS {
            return;
        }
        let subs = subdirs(dir);
        if subs.len() > FANOUT {
            return;
        }
        for sub in subs {
            if out.len() >= HITS {
                return;
            }
            let name = name_of(&sub);
            if is_cache_name(&name) {
                continue;
            }
            if named_save(&name) {
                let deeper: Vec<PathBuf> =
                    subdirs(&sub).into_iter().filter(|c| !is_cache_name(&name_of(c)) && dir_key(&name_of(c)).contains("save") && has_files(c)).collect();
                if !deeper.is_empty() {
                    out.extend(deeper);
                } else if has_files(&sub) {
                    out.push(sub);
                }
                continue;
            }
            walk(&sub, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    if root.is_dir() {
        walk(root, 1, &mut out);
    }
    out.truncate(HITS);
    out
}

// ---------------------------------------------------------------------------
// Wine / Proton prefixes

/// The Wine / Proton prefix a program is installed in (`…/pfx` for
/// `…/pfx/drive_c/Games/Game/game.exe`).
pub fn wine_prefix_of(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|a| eq_name(a, "drive_c")).and_then(Path::parent).map(Path::to_path_buf)
}

/// The users' home folders in a prefix: Proton's `steamuser`, and the
/// account name Heroic, Lutris and Bottles use.
pub fn prefix_user_dirs(prefix: &Path) -> Vec<PathBuf> {
    subdirs(&prefix.join("drive_c").join("users"))
        .into_iter()
        .filter(|u| !["public", "default", "default user", "all users"].contains(&name_of(u).to_lowercase().as_str()))
        .collect()
}

/// The prefixes of the launchers that run Windows games on Linux (Heroic,
/// Lutris, Bottles, plain Wine), native and Flatpak, with the launcher's
/// name. Each prefix's folder is usually named after its game.
pub fn launcher_prefixes(home: &Path) -> Vec<(PathBuf, &'static str)> {
    let flatpak = |app: &str| home.join(".var").join("app").join(app);
    let parents: Vec<(PathBuf, &'static str)> = vec![
        (home.join("Games").join("Heroic").join("Prefixes"), "Heroic"),
        (home.join("Games").join("Heroic").join("Prefixes").join("default"), "Heroic"),
        (flatpak("com.heroicgameslauncher.hgl").join("config").join("heroic").join("Prefixes"), "Heroic"),
        (flatpak("com.heroicgameslauncher.hgl").join("config").join("heroic").join("Prefixes").join("default"), "Heroic"),
        (flatpak("com.usebottles.bottles").join("data").join("bottles").join("bottles"), "Bottles"),
        (home.join(".local").join("share").join("bottles").join("bottles"), "Bottles"),
        (home.join("Games"), "Lutris"),
        (flatpak("net.lutris.Lutris").join("data").join("lutris").join("prefixes"), "Lutris"),
        (home.join(".local").join("share").join("lutris").join("prefixes"), "Lutris"),
        (home.join(".local").join("share").join("wineprefixes"), "Wine"),
    ];
    let mut out: Vec<(PathBuf, &'static str)> = Vec::new();
    for (parent, launcher) in parents {
        for p in subdirs(&parent).into_iter().take(60) {
            if p.join("drive_c").is_dir() && !out.iter().any(|(o, _)| norm(o) == norm(&p)) {
                out.push((p, launcher));
            }
        }
    }
    let wine = home.join(".wine");
    if wine.join("drive_c").is_dir() {
        out.push((wine, "Wine"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn mk(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    #[test]
    fn cache_vendor_and_save_names() {
        assert!(is_cache_name("ShaderCache") && is_cache_name("Shader Cache") && is_cache_name("AnvilDX12Cache") && is_cache_name("Logs"));
        assert!(!is_cache_name("SaveGames"));
        assert!(is_vendor_name("NVIDIA Corporation") && is_vendor_name("Unity") && is_vendor_name(&"a".repeat(40)));
        assert!(!is_vendor_name("Team Cherry"));
    }

    #[test]
    fn never_offers_shared_roots() {
        let home = Path::new(r"C:\Users\me");
        let p = Places::windows(home);
        assert!(p.never_offered(&home.join("AppData").join("Roaming").join("Godot").join("app_userdata")));
        assert!(p.never_offered(&home.join("Documents").join("My Games")));
        assert!(p.never_offered(&home.join("AppData").join("LocalLow").join("DefaultCompany")));
        assert!(!p.never_offered(&home.join("AppData").join("LocalLow").join("Team Cherry").join("Hollow Knight")));
    }

    #[test]
    fn renpy_options() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("game")).unwrap();
        fs::write(tmp.path().join("game").join("options.rpy"), "## comment\n    define config.save_directory = \"MyNovel-1600000000\"\n").unwrap();
        assert_eq!(renpy_save_directory(tmp.path()).as_deref(), Some("MyNovel-1600000000"));
    }

    #[test]
    fn save_dirs_in_the_game_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let g = tmp.path();
        mk(&g.join("savegames").join("1234").join("1.save"));
        mk(&g.join("Game").join("Saved").join("SaveGames").join("slot.sav"));
        mk(&g.join("Game").join("Saved").join("Config").join("x.ini"));
        mk(&g.join("ShaderCache").join("save").join("a.bin"));
        fs::create_dir_all(g.join("EmptySaves")).unwrap();
        let found = save_dirs_in(g);
        assert!(found.contains(&g.join("savegames")));
        // The more specific folder inside a match.
        assert!(found.contains(&g.join("Game").join("Saved").join("SaveGames")));
        assert!(!found.iter().any(|p| p.starts_with(g.join("ShaderCache"))));
        assert!(!found.contains(&g.join("EmptySaves")));
    }

    #[test]
    fn steam_ids_and_cloud_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("SteamLibrary").join("steamapps");
        let game = lib.join("common").join("Some Game");
        mk(&game.join("bin").join("game.exe"));
        fs::write(
            lib.join("appmanifest_4242.acf"),
            "\"AppState\"\n{\n\t\"appid\"\t\t\"4242\"\n\t\"name\"\t\t\"Some Game\"\n\t\"installdir\"\t\t\"Some Game\"\n}\n",
        )
        .unwrap();
        assert_eq!(steam_appid(Some(&game.join("bin").join("game.exe")), None), Some(4242));
        let other = tmp.path().join("Other");
        mk(&other.join("steam_appid.txt"));
        fs::write(other.join("steam_appid.txt"), "367520\n").unwrap();
        assert_eq!(steam_appid(None, Some(&other)), Some(367520));

        let userdata = tmp.path().join("userdata");
        mk(&userdata.join("1001").join("4242").join("remote").join("save.dat"));
        mk(&userdata.join("1001").join("4242").join("remotecache.vdf"));
        let hints = steam_cloud(&userdata, 4242);
        assert_eq!(hints.len(), 1);
        assert!(hints[0].path.ends_with("remote"));
    }

    #[test]
    fn prefixes_of_launchers() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let heroic = home.join("Games").join("Heroic").join("Prefixes").join("Hades");
        fs::create_dir_all(heroic.join("drive_c").join("users").join("deck").join("AppData")).unwrap();
        fs::create_dir_all(heroic.join("drive_c").join("users").join("Public")).unwrap();
        fs::create_dir_all(home.join("Games").join("NotAPrefix")).unwrap();
        let found = launcher_prefixes(home);
        assert_eq!(found, vec![(heroic.clone(), "Heroic")]);
        assert_eq!(prefix_user_dirs(&heroic), vec![heroic.join("drive_c").join("users").join("deck")]);
        let exe = heroic.join("drive_c").join("Games").join("Hades").join("Hades.exe");
        assert_eq!(wine_prefix_of(&exe), Some(heroic));
    }
}
