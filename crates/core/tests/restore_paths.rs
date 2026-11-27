//! Where a version's files may be written. A version comes from the cloud,
//! from a file someone shared, from another device, and its paths say where
//! its files go: only files a scan of this device would find are written.
//! A file under none of the game's save locations, one left out of them, or
//! one reached through a link is left out. One test writes, because the
//! data directory is chosen through an environment variable.

use ludonimbus_core::bind::Bind;
use ludonimbus_core::config::GameEntry;
use ludonimbus_core::snapshot::{self, FileEntry};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn entry(path: &str, content: &str) -> FileEntry {
    FileEntry { path: path.into(), size: content.len() as u64, hash: blake3::hash(content.as_bytes()).to_hex().to_string(), mtime: 0 }
}

/// A version's files as a download leaves them, `<dir>/<path>`.
fn version(dir: &Path, files: &[(&str, &str)]) -> Vec<FileEntry> {
    let _ = fs::remove_dir_all(dir);
    files
        .iter()
        .map(|(path, content)| {
            let p = path.split('/').fold(dir.to_path_buf(), |p, c| p.join(c));
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, content).unwrap();
            entry(path, content)
        })
        .collect()
}

/// Every root a version below names, in the test's folder: whatever the
/// code does, nothing can land in the real home folder or on a real drive.
fn roots(base: &Path) -> BTreeMap<String, String> {
    ["home", "appdata", "localappdata", "documents", "drive-c", "drive-q", "abs"].iter().map(|r| (r.to_string(), base.join(r).display().to_string())).collect()
}

/// The files below `dir`, `/`-separated and sorted. Links aren't followed.
fn listing(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    out.sort();
    out
}

/// A link at `at` to the folder `to`: a junction on Windows (it needs no
/// rights), a symbolic link elsewhere.
fn link(to: &Path, at: &Path) {
    #[cfg(windows)]
    let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(at).arg(to).output().is_ok_and(|o| o.status.success());
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(to, at).is_ok();
    assert!(made, "couldn't make a link at {}", at.display());
}

#[test]
fn only_files_a_scan_would_find_are_written() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path();
    std::env::set_var("LUDONIMBUS_HOME", base.join("data"));
    let install = base.join("game");
    fs::create_dir_all(install.join("save")).unwrap();
    fs::write(install.join("game.exe"), b"exe").unwrap();
    let elsewhere = base.join("roots");
    let game = GameEntry {
        key: "restore-paths".into(),
        name: "Restore Paths".into(),
        exe: Some(install.join("game.exe")),
        saves: vec!["{install}/save/**".into()],
        exclude: vec!["{install}/save/**/*.bak".into()],
        roots: roots(&elsewhere),
        ..Default::default()
    };
    let from = base.join("version");
    let read = |p: &Path| fs::read_to_string(p).unwrap();

    // A version with a save, and files that are none of this game's: in its
    // folder but no save, left out of its saves, a name our own writes use,
    // in the Startup folder, the home folder, on a drive, from the root.
    let files = version(
        &from,
        &[
            ("install/save/slot1.sav", "progress"),
            ("install/version.dll", "not a save"),
            ("install/save/old.bak", "left out"),
            ("install/save/slot2.sav.lntmp", "ours"),
            ("appdata/Microsoft/Windows/Start Menu/Programs/Startup/run.bat", "not a save"),
            ("home/.bashrc", "not a save"),
            ("drive-c/Windows/System32/evil.dll", "not a save"),
            ("abs/etc/profile", "not a save"),
        ],
    );
    snapshot::apply_dir(&from, &files, &game).unwrap();
    assert_eq!(listing(&install), ["game.exe", "save/slot1.sav"]);
    assert_eq!(listing(&elsewhere), Vec::<String>::new());

    // Nothing of the version is a save here: nothing is written, and the
    // saves stay as they are.
    let outside = version(&from, &[("install/version.dll", "not a save"), ("home/.bashrc", "not a save")]);
    let err = snapshot::apply_dir(&from, &outside, &game).unwrap_err();
    assert!(format!("{err:#}").contains("install/version.dll"), "{err:#}");
    assert_eq!(listing(&install), ["game.exe", "save/slot1.sav"]);
    assert_eq!(read(&install.join("save/slot1.sav")), "progress");

    // A link in the save folder leads elsewhere. A scan doesn't follow it,
    // and a file through it is not written.
    let away = base.join("away");
    fs::create_dir_all(&away).unwrap();
    link(&away, &install.join("save").join("linked"));
    let files = version(&from, &[("install/save/slot1.sav", "progress 2"), ("install/save/linked/run.bat", "not a save")]);
    snapshot::apply_dir(&from, &files, &game).unwrap();
    assert_eq!(listing(&away), Vec::<String>::new());
    assert_eq!(read(&install.join("save/slot1.sav")), "progress 2");

    // The save folder itself is a link (moved to another drive, say): that
    // is where the saves are, a scan finds them there, and so they go there.
    let moved = base.join("moved");
    let install2 = base.join("game2");
    fs::create_dir_all(&moved).unwrap();
    fs::create_dir_all(&install2).unwrap();
    fs::write(install2.join("game.exe"), b"exe").unwrap();
    link(&moved, &install2.join("save"));
    let game2 = GameEntry { key: "restore-moved".into(), exe: Some(install2.join("game.exe")), ..game.clone() };
    // Made on another device, whose game is in another folder.
    let files = version(&from, &[("install/save/slot1.sav", "on another drive")]);
    snapshot::apply_dir(&from, &files, &game2).unwrap();
    assert_eq!(read(&moved.join("slot1.sav")), "on another drive");
    assert_eq!(snapshot::scan(&game2).unwrap().0.len(), 1);

    // A version that keeps the line's files in another folder: they go
    // where they are bound, as far as that folder holds saves here. A
    // binding into the game's own folder (a game record anyone who can
    // write to the cloud can change says so) puts nothing there.
    let bound = GameEntry {
        key: "restore-bound".into(),
        bind: vec![Bind::files("appdata/Game/Saves", "install/save"), Bind::files("appdata/Game/Plugins", "install")],
        ..game.clone()
    };
    let files = version(&from, &[("appdata/Game/Saves/slot1.sav", "bound"), ("appdata/Game/Plugins/version.dll", "not a save")]);
    snapshot::apply_dir(&from, &files, &bound).unwrap();
    assert_eq!(read(&install.join("save/slot1.sav")), "bound");
    assert!(!install.join("version.dll").exists());
    assert_eq!(listing(&elsewhere), Vec::<String>::new());

    // Saves on a drive the user chose (`{drive-q}`, what picking a folder
    // there gives) go there; elsewhere on that drive, nothing.
    let on_drive = GameEntry { key: "restore-drive".into(), saves: vec!["{drive-q}/Saves/**".into()], exclude: Vec::new(), ..game.clone() };
    let files = version(&from, &[("drive-q/Saves/slot1.sav", "on q"), ("drive-q/Startup/run.bat", "not a save")]);
    snapshot::apply_dir(&from, &files, &on_drive).unwrap();
    assert_eq!(listing(&elsewhere), ["drive-q/Saves/slot1.sav"]);
}

#[test]
fn which_paths_a_scan_would_find() {
    let game = GameEntry {
        key: "paths".into(),
        name: "Paths".into(),
        saves: vec!["{drive-d}/Saves/*.sav".into(), "{abs}/mnt/games/saves/**".into(), "{nxsave-0100E65002BB8000}/**".into()],
        exclude: vec!["{drive-d}/Saves/old*.sav".into()],
        ..Default::default()
    };
    let paths = [
        "drive-d/Saves/slot1.sav",
        // Letter case is no matter, as in a scan.
        "drive-d/saves/SLOT2.SAV",
        "nxsave-0100e65002bb8000/0/save.dat",
        "abs/mnt/games/saves/world/level.dat",
        // Left out of the saves.
        "drive-d/Saves/old1.sav",
        // Not what the location names.
        "drive-d/Saves/sub/x.sav",
        "drive-d/Saves/slot1.sav.lntmp",
        "nxsave-0100E65002BB8001/0/save.dat",
        // Elsewhere on the device.
        "drive-c/Windows/evil.dll",
        "abs/etc/profile",
        "home/.bashrc",
        // No safe path at all.
        "drive-d/Saves/../evil.sav",
    ];
    let outside = snapshot::outside_saves(&game, paths.iter().copied()).unwrap().unwrap();
    assert_eq!(outside, paths[4..]);
    // A game whose saves Ludusavi finds: `ludusavi_saves.rs`.
}
