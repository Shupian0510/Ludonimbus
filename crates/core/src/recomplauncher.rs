//! RecompLauncher (recomplauncher.com) installs PC recomps. The Steam
//! shortcuts it makes start `recomplauncher.exe --play <id>`, and its
//! `library.json` says where each game is installed, so games it manages can
//! be recognised by their catalog id.

use crate::adapters::{self, Detected};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Installed {
    pub id: String,
    pub name: String,
    /// The original game, when RecompLauncher knows it.
    pub game: String,
    /// The game's folder.
    pub path: PathBuf,
    /// The program, relative to `path`.
    pub exe: Option<PathBuf>,
    /// A save folder the player picked in RecompLauncher (its `{local}/...` syntax).
    pub save_place: Option<String>,
    pub more_save_places: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct Library {
    installed: BTreeMap<String, Installed>,
    project_aliases: BTreeMap<String, String>,
}

/// Whether `exe` is RecompLauncher itself.
pub fn is_launcher(exe: &Path) -> bool {
    let n = crate::util::file_name_lower(exe);
    n.starts_with("recomplauncher") && n.ends_with(".exe")
}

/// The catalog id in `--play <id>`.
pub fn play_id(args: &str) -> Option<String> {
    let toks = crate::util::split_args(args);
    toks.windows(2).find(|w| w[0] == "--play").map(|w| w[1].trim().to_string()).filter(|s| !s.is_empty())
}

/// RecompLauncher's data folders: next to a portable launcher, and the
/// usual one in %APPDATA%.
fn data_dirs(launcher: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = launcher.and_then(Path::parent) {
        if dir.join("portable").is_file() || dir.join("portable.txt").is_file() {
            out.push(dir.join("data"));
        }
    }
    if let Some(d) = dirs::config_dir() {
        out.push(d.join("com.recomplauncher.app"));
    }
    out
}

fn read_library(dir: &Path) -> Option<Library> {
    let text = std::fs::read_to_string(dir.join("library.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Games RecompLauncher has installed (from every data folder found).
pub fn installed(launcher: Option<&Path>) -> Vec<Installed> {
    let mut out: Vec<Installed> = Vec::new();
    for dir in data_dirs(launcher) {
        let Some(lib) = read_library(&dir) else { continue };
        for (key, mut g) in lib.installed {
            if g.id.is_empty() {
                g.id = key;
            }
            if !out.iter().any(|x| x.id == g.id) {
                out.push(g);
            }
        }
    }
    out
}

/// The installed game a `--play` id names (following renamed ids).
pub fn find_installed(launcher: Option<&Path>, id: &str) -> Option<Installed> {
    for dir in data_dirs(launcher) {
        let Some(lib) = read_library(&dir) else { continue };
        let id = lib.project_aliases.get(id).map(String::as_str).unwrap_or(id);
        let base = id.split("--").next().unwrap_or(id);
        if let Some(g) = lib.installed.get(id).or_else(|| lib.installed.get(base)) {
            let mut g = g.clone();
            if g.id.is_empty() {
                g.id = base.to_string();
            }
            return Some(g);
        }
    }
    None
}

/// A Steam shortcut that starts a game through RecompLauncher.
pub fn detect(exe: &Path, args: &str) -> Option<Detected> {
    if !is_launcher(exe) {
        return None;
    }
    let id = play_id(args)?;
    let adapter = adapters::find_recomplauncher(&id)?;
    Some(Detected {
        adapter,
        install: find_installed(Some(exe), &id).map(|g| g.path),
        via_recomplauncher: true,
    })
}

/// A save place picked in RecompLauncher, in Ludonimbus' syntax.
pub fn convert_place(template: &str) -> Option<String> {
    let t = template.trim().replace('\\', "/");
    let t = t.trim_end_matches('/');
    for (from, to) in [
        ("{install}", "{install}"),
        ("{appdata}", "{appdata}"),
        ("{config}", "{appdata}"),
        ("{local}", "{localappdata}"),
        ("{documents}", "{documents}"),
        ("{home}", "{home}"),
        ("{data}", "{install}/recompdata"),
    ] {
        if let Some(rest) = t.strip_prefix(from) {
            if rest.is_empty() || rest.starts_with('/') {
                return Some(format!("{to}{rest}"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_play_ids_and_places() {
        assert_eq!(play_id("--play zelda64recomp").as_deref(), Some("zelda64recomp"));
        assert_eq!(play_id("--fullscreen --play \"project-helix--fe2\"").as_deref(), Some("project-helix--fe2"));
        assert_eq!(play_id("--bigpicture"), None);
        assert!(is_launcher(Path::new(r"C:\Apps\RecompLauncher\recomplauncher.exe")));
        assert!(!is_launcher(Path::new(r"C:\Games\Zelda64Recompiled.exe")));
        assert_eq!(convert_place("{local}/Zelda64Recompiled/saves/").as_deref(), Some("{localappdata}/Zelda64Recompiled/saves"));
        assert_eq!(convert_place("{data}/saves").as_deref(), Some("{install}/recompdata/saves"));
        assert_eq!(convert_place("{localx}/a"), None);
        assert_eq!(convert_place("C:/Saves"), None);
    }

    #[test]
    fn finds_games_in_a_portable_launcher() {
        let tmp = tempfile::tempdir().unwrap();
        let launcher = tmp.path().join("RL").join("recomplauncher.exe");
        std::fs::create_dir_all(tmp.path().join("RL").join("data")).unwrap();
        std::fs::write(tmp.path().join("RL").join("portable.txt"), "").unwrap();
        let game = tmp.path().join("Library").join("zelda64recomp");
        std::fs::write(
            tmp.path().join("RL").join("data").join("library.json"),
            serde_json::json!({
                "installed": { "zelda64recomp": { "id": "zelda64recomp", "name": "Zelda 64: Recompiled", "tag": "v1.2.2",
                    "path": game, "exe": "Zelda64Recompiled.exe", "save_place": "{local}/Mine" } },
                "project_aliases": { "old-zelda": "zelda64recomp" }
            })
            .to_string(),
        )
        .unwrap();
        let d = detect(&launcher, "--play zelda64recomp").unwrap();
        assert_eq!(d.adapter.key, "zelda64recomp");
        assert_eq!(d.install.as_deref(), Some(game.as_path()));
        assert!(d.via_recomplauncher);
        assert_eq!(find_installed(Some(&launcher), "old-zelda").unwrap().path, game);
        assert!(installed(Some(&launcher)).iter().any(|g| g.id == "zelda64recomp" && g.save_place.as_deref() == Some("{local}/Mine")));
        // Unknown ids and other programs are not RecompLauncher games.
        assert!(detect(&launcher, "--play not-in-the-catalog").is_none());
        assert!(detect(Path::new(r"C:\x\other.exe"), "--play zelda64recomp").is_none());
    }
}
