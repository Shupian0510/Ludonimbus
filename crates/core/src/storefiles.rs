//! Which of the files Ludusavi finds for a title are one version's saves.
//!
//! Ludusavi knows games, not versions: for a title it lists the save paths
//! of all the game's versions (the Steam version's, the Xbox app version's
//! containers, folders every version uses) and finds whichever exist on
//! this device. With a game installed from two stores on one PC, each
//! version would take the other's saves for its own: an Xbox app version
//! would back up, and write into, the folder Steam Cloud keeps for the
//! Steam version, and never see its own containers.
//!
//! - A version the Xbox app installed keeps its saves in its package's
//!   containers ([`crate::wgs`]). Files that are visibly the Steam
//!   version's are left out: Steam's own cloud folder (`<Steam>/userdata`),
//!   a folder Steam Auto-Cloud syncs (it holds `steam_autocloud.vdf`), a
//!   folder named after a Steam account (`7656119…`). So are other
//!   packages' containers. Its own containers are added when Ludusavi
//!   doesn't name them (a game newer than its database knows).
//! - A version installed any other way (Steam installed it, or its program
//!   is somewhere else) never keeps its saves in an Xbox app game's
//!   containers.
//! - A version whose program isn't on this device is left as Ludusavi
//!   finds it: what it is can't be told.

use crate::config::GameEntry;
use crate::{spec, util, xbox};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// What Steam Auto-Cloud leaves in each folder it syncs.
const AUTOCLOUD: &str = "steam_autocloud.vdf";

/// How far above a file to look for [`AUTOCLOUD`].
const AUTOCLOUD_DEPTH: usize = 6;

enum Kind {
    /// Installed by the Xbox app: its package name.
    Xbox(String),
    /// Installed some other way.
    Other,
    /// Can't be told on this device.
    Unknown,
}

fn kind(game: &GameEntry) -> Kind {
    if game.store.is_some() {
        return Kind::Other;
    }
    match game.exe.as_deref() {
        Some(e) if !e.exists() => Kind::Unknown,
        Some(e) if xbox::content_dir(e).is_some() => xbox::package_name(e).map_or(Kind::Unknown, Kind::Xbox),
        Some(_) => Kind::Other,
        None => Kind::Unknown,
    }
}

/// The files among `files` (found by Ludusavi for the game's title) that
/// are this version's saves, with an Xbox app version's own containers
/// added (see the module).
pub fn of_version(game: &GameEntry, files: Vec<PathBuf>) -> Vec<PathBuf> {
    of_version_in(game, files, dirs::data_local_dir().as_deref(), spec::steam_root().as_deref())
}

/// [`of_version`] with the folders given: the user's local app data (where
/// `Packages` is) and Steam's folder.
pub fn of_version_in(game: &GameEntry, files: Vec<PathBuf>, local_app_data: Option<&Path>, steam: Option<&Path>) -> Vec<PathBuf> {
    match kind(game) {
        Kind::Unknown => files,
        Kind::Other => files.into_iter().filter(|f| container_package(f).is_none()).collect(),
        Kind::Xbox(package) => {
            let own = local_app_data.and_then(|l| xbox::package_data_dir(l, &package));
            let own_name = own.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy().to_lowercase());
            let mut steam_files = SteamFiles::new(steam);
            let mut out: Vec<PathBuf> = files
                .into_iter()
                .filter(|f| match container_package(f) {
                    Some(p) => own_name.as_deref() == Some(p.as_str()),
                    None => !steam_files.holds(f),
                })
                .collect();
            if let Some(dir) = own.map(|o| o.join("SystemAppData").join("wgs")) {
                if !out.iter().any(|f| container_package(f).is_some()) {
                    out.extend(files_in(&dir));
                }
            }
            out
        }
    }
}

/// The folder of an Xbox app version's own containers on this device, which
/// [`of_version`] adds when Ludusavi doesn't name them. `None` for any
/// other version, or before the Xbox app made the package's data folder.
pub fn own_containers(game: &GameEntry) -> Option<PathBuf> {
    let Kind::Xbox(package) = kind(game) else { return None };
    xbox::package_data_dir(&dirs::data_local_dir()?, &package).map(|own| own.join("SystemAppData").join("wgs"))
}

/// The package folder name (lowercase) when `path` is in an Xbox app
/// game's containers: `…\Packages\<package folder>\SystemAppData\wgs\…`.
fn container_package(path: &Path) -> Option<String> {
    let parts: Vec<String> = path.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
    (2..parts.len().saturating_sub(1))
        .find(|&i| parts[i] == "systemappdata" && parts[i + 1] == "wgs" && parts[i - 2] == "packages")
        .map(|i| parts[i - 1].clone())
}

/// A Steam account's id as a folder name (SteamID64: 17 digits, `7656119…`).
fn is_steam_id(name: &str) -> bool {
    name.len() == 17 && name.starts_with("7656119") && name.bytes().all(|b| b.is_ascii_digit())
}

/// Tells files the Steam version keeps: in Steam's own cloud folder, in a
/// folder Steam Auto-Cloud syncs, or in a folder named after a Steam
/// account.
struct SteamFiles {
    userdata: Option<String>,
    autocloud: HashMap<PathBuf, bool>,
}

impl SteamFiles {
    fn new(steam: Option<&Path>) -> Self {
        let userdata = steam.map(|s| util::norm_path(&s.join("userdata").to_string_lossy()));
        SteamFiles { userdata, autocloud: HashMap::new() }
    }

    fn holds(&mut self, file: &Path) -> bool {
        if let Some(u) = &self.userdata {
            let f = util::norm_path(&file.to_string_lossy());
            if f.strip_prefix(u.as_str()).is_some_and(|rest| rest.starts_with(std::path::MAIN_SEPARATOR)) {
                return true;
            }
        }
        let parents = || file.ancestors().skip(1);
        if parents().filter_map(Path::file_name).any(|n| is_steam_id(&n.to_string_lossy())) {
            return true;
        }
        for dir in parents().take(AUTOCLOUD_DEPTH) {
            let synced = *self.autocloud.entry(dir.to_path_buf()).or_insert_with(|| dir.join(AUTOCLOUD).is_file());
            if synced {
                return true;
            }
        }
        false
    }
}

/// Every file below `dir`.
fn files_in(dir: &Path) -> Vec<PathBuf> {
    if !dir.is_dir() {
        return Vec::new();
    }
    walkdir::WalkDir::new(dir).follow_links(false).into_iter().flatten().filter(|e| e.file_type().is_file()).map(|e| e.into_path()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StoreLink;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    /// An Xbox app install of `package` under `root`, and its entry.
    fn xbox_game(root: &Path, package: &str) -> GameEntry {
        let content = root.join("XboxGames").join("Game").join("Content");
        touch(&content.join("gamelaunchhelper.exe"));
        std::fs::write(content.join("MicrosoftGame.config"), r#"<Game><ExecutableList><Executable Name="Game.exe" /></ExecutableList></Game>"#).unwrap();
        std::fs::write(content.join("appxmanifest.xml"), format!(r#"<Package><Identity Name="{package}" Publisher="CN=X" /></Package>"#)).unwrap();
        GameEntry { key: "game".into(), exe: Some(content.join("gamelaunchhelper.exe")), ludusavi_title: Some("Game".into()), ..Default::default() }
    }

    #[test]
    fn an_xbox_version_leaves_the_steam_versions_saves_alone_and_reads_its_containers() {
        let tmp = tempfile::tempdir().unwrap();
        let local = tmp.path().join("Local");
        let steam = tmp.path().join("Steam");
        let game = xbox_game(tmp.path(), "Studio.Game");

        // What Ludusavi finds for the title: the Steam version's Auto-Cloud
        // folder (Unreal: SaveGames/<Steam id>), Steam's own cloud folder,
        // another package's containers, and a folder no store marks.
        let autocloud = local.join("Game").join("Saved").join("SaveGames").join("Profile");
        touch(&autocloud.join(AUTOCLOUD));
        touch(&autocloud.join("Main.sav"));
        let by_id = local.join("Game").join("Saved").join("SaveGames").join("76561190000000001").join("Main.sav");
        touch(&by_id);
        let remote = steam.join("userdata").join("42").join("100").join("remote").join("save.dat");
        touch(&remote);
        let other = local.join("Packages").join("Other.Game_abc").join("SystemAppData").join("wgs").join("t").join("containers.index");
        touch(&other);
        let plain = local.join("Game").join("Config").join("settings.ini");
        touch(&plain);
        // The game's own containers, which Ludusavi doesn't know.
        let wgs = local.join("Packages").join("Studio.Game_8wekyb3d8bbwe").join("SystemAppData").join("wgs");
        let index = wgs.join("0009_00AB").join("containers.index");
        let blob = wgs.join("0009_00AB").join("ABCD").join("container.1");
        touch(&index);
        touch(&blob);

        let found = vec![autocloud.join(AUTOCLOUD), autocloud.join("Main.sav"), by_id, remote, other, plain.clone()];
        let mut got = of_version_in(&game, found, Some(&local), Some(&steam));
        got.sort();
        let mut want = vec![plain.clone(), index.clone(), blob.clone()];
        want.sort();
        assert_eq!(got, want);

        // Containers Ludusavi names are kept, and not added twice.
        let got = of_version_in(&game, vec![index.clone(), blob.clone()], Some(&local), Some(&steam));
        assert_eq!(got, vec![index.clone(), blob.clone()]);

        // A version Steam installed never takes the containers.
        let steam_game = GameEntry { key: "s".into(), store: Some(StoreLink::default()), ludusavi_title: Some("Game".into()), ..Default::default() };
        let got = of_version_in(&steam_game, vec![index.clone(), plain.clone(), autocloud.join("Main.sav")], Some(&local), Some(&steam));
        assert_eq!(got, vec![plain.clone(), autocloud.join("Main.sav")]);

        // Not on this device: as Ludusavi finds it.
        let gone = GameEntry { exe: Some(tmp.path().join("missing").join("gamelaunchhelper.exe")), ..game.clone() };
        assert_eq!(of_version_in(&gone, vec![index.clone(), autocloud.join("Main.sav")], Some(&local), Some(&steam)).len(), 2);
    }

    #[test]
    fn tells_container_paths_and_steam_ids() {
        let p = Path::new("C:/Users/me/AppData/Local/Packages/A.B_x1/SystemAppData/wgs/0009_00AB/containers.index");
        assert_eq!(container_package(p).as_deref(), Some("a.b_x1"));
        assert_eq!(container_package(Path::new("C:/Users/me/AppData/Local/Packages/A.B_x1/LocalState/save.dat")), None);
        assert!(is_steam_id("76561199396602582"));
        assert!(!is_steam_id("2535411111111111"));
        assert!(!is_steam_id("7656119939660258x"));
    }
}
