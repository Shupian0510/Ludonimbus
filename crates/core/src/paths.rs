//! Where Ludonimbus keeps its own files.
//!
//! Everything lives under `%APPDATA%\Ludonimbus` unless the `LUDONIMBUS_HOME`
//! environment variable points somewhere else (used by tests).

use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("LUDONIMBUS_HOME") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("Ludonimbus")
}

/// Running inside the Flatpak sandbox (it has `/.flatpak-info`).
pub fn in_flatpak() -> bool {
    static IN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *IN.get_or_init(|| std::path::Path::new("/.flatpak-info").is_file())
}

/// Inside the Flatpak sandbox the XDG folders are the app's own, in
/// `~/.var/app/<id>`. They are put back to the user's (the sandbox may read
/// the home folder): Ludonimbus then keeps its data in `~/.config/Ludonimbus`,
/// where the launch wrapper and the Decky plugin, which run outside the
/// sandbox, find it, and other programs' folders (emulators, Steam) are
/// found where they are. Called first thing by each program, before any
/// thread starts. Nothing happens outside the sandbox.
pub fn use_host_folders() {
    if !in_flatpak() {
        return;
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return };
    for (var, host, default) in [("XDG_CONFIG_HOME", "HOST_XDG_CONFIG_HOME", ".config"), ("XDG_DATA_HOME", "HOST_XDG_DATA_HOME", ".local/share")] {
        let value = std::env::var_os(host).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(default));
        std::env::set_var(var, value);
    }
}

fn ensure(p: PathBuf) -> PathBuf {
    let _ = std::fs::create_dir_all(&p);
    p
}

pub fn config_file() -> PathBuf {
    data_dir().join("config.json")
}
pub fn state_file() -> PathBuf {
    data_dir().join("state.json")
}
pub fn staging_dir() -> PathBuf {
    ensure(data_dir().join("staging"))
}
pub fn pending_dir() -> PathBuf {
    ensure(data_dir().join("pending"))
}
pub fn local_backups_dir() -> PathBuf {
    ensure(data_dir().join("local-backups"))
}
pub fn running_dir() -> PathBuf {
    ensure(data_dir().join("running"))
}
/// Games whose sync after playing is still owed: the launch wrapper stopped
/// waiting for it (docs/plan-v2.md, section 5.5).
pub fn owed_dir() -> PathBuf {
    ensure(data_dir().join("owed"))
}
/// Locks that keep two processes from syncing one game at once.
pub fn locks_dir() -> PathBuf {
    ensure(data_dir().join("locks"))
}
pub fn logs_dir() -> PathBuf {
    ensure(data_dir().join("logs"))
}
pub fn steam_backups_dir() -> PathBuf {
    ensure(data_dir().join("steam-backups"))
}
pub fn ludusavi_config_dir() -> PathBuf {
    ensure(data_dir().join("ludusavi"))
}
/// Save files read out of an Xbox app game's containers, while a sync reads
/// them (see `snapshot::scan`).
pub fn decoded_dir() -> PathBuf {
    ensure(data_dir().join("decoded"))
}

/// Where Linux and SteamOS mount drives besides the system one: SD cards
/// and USB drives under `/run/media/<user>/<label>` (older SteamOS mounts
/// the SD card as `/run/media/mmcblk0p1`), fixed drives under `/var/mnt`
/// (Bazzite and other image-based systems) and `/mnt`.
pub const DRIVE_BASES: [&str; 3] = ["/run/media", "/var/mnt", "/mnt"];

/// Drive roots one scan looks at, at most.
const MAX_DRIVES: usize = 32;

/// The roots of the other drives mounted here: a Steam Deck's SD card,
/// USB drives, other disks (Linux; none elsewhere, where the drive letters
/// are looked at instead). Scanning for Steam libraries, emulators and game
/// folders, and the save finder, look on them too.
pub fn extra_drives() -> Vec<PathBuf> {
    if !cfg!(target_os = "linux") {
        return Vec::new();
    }
    let user =
        std::env::var("USER").ok().filter(|u| !u.is_empty()).or_else(|| dirs::home_dir().and_then(|h| h.file_name().map(|n| n.to_string_lossy().into_owned())));
    let bases: Vec<PathBuf> = DRIVE_BASES.iter().map(PathBuf::from).collect();
    drives_under(&bases, user.as_deref())
}

/// [`extra_drives`] under `bases`. A base named `media` holds drives
/// directly (`mmcblk0p1`) or a user's folder of drives (`deck/<label>`): the
/// user's folder is looked into, and every other folder there is a drive.
/// The other bases hold drives directly. A drive reached twice (`/mnt` is
/// `/var/mnt` on some systems) counts once; at most [`MAX_DRIVES`].
pub fn drives_under(bases: &[PathBuf], user: Option<&str>) -> Vec<PathBuf> {
    fn subdirs(dir: &std::path::Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
        v.sort();
        v
    }
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |p: PathBuf, out: &mut Vec<PathBuf>| {
        let real = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
        if out.len() < MAX_DRIVES && seen.insert(real) {
            out.push(p);
        }
    };
    for base in bases {
        let media = base.file_name().is_some_and(|n| n == "media");
        for d in subdirs(base) {
            let users_folder = media && user.is_some_and(|u| d.file_name().is_some_and(|n| n == u));
            if users_folder {
                for drive in subdirs(&d) {
                    add(drive, &mut out);
                }
            } else {
                add(d, &mut out);
            }
        }
    }
    out
}

/// Directory of the running executable (where sidecars live).
pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| PathBuf::from("."))
}

/// File name of the Steam launch wrapper on this system.
pub const RUNNER_NAME: &str = if cfg!(windows) { "ludonimbus-run.exe" } else { "ludonimbus-run" };

/// The Steam launch wrapper: next to the current executable on Windows; on
/// Linux and macOS the copy in `<data>/bin` (see [`install_runner`]).
pub fn runner_exe() -> PathBuf {
    if let Ok(p) = std::env::var("LUDONIMBUS_RUNNER") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    if cfg!(unix) {
        let copy = data_dir().join("bin").join(RUNNER_NAME);
        if copy.is_file() {
            return copy;
        }
    }
    exe_dir().join(RUNNER_NAME)
}

/// Linux / macOS: keep the wrapper (and a note where the app is) in
/// `<data>/bin`, outside the AppImage, which is only mounted while it runs
/// and may be moved: Steam's launch options must keep pointing at it.
/// Called by the app when it starts.
#[cfg(unix)]
pub fn install_runner() -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let bin = data_dir().join("bin");
    std::fs::create_dir_all(&bin)?;
    let app = std::env::var_os("APPIMAGE").map(PathBuf::from).or_else(|| std::env::current_exe().ok());
    if let Some(app) = app {
        std::fs::write(bin.join("app-path"), app.to_string_lossy().as_bytes())?;
    }
    let src = exe_dir().join(RUNNER_NAME);
    let dest = bin.join(RUNNER_NAME);
    let same = |a: &std::path::Path, b: &std::path::Path| match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.len() == y.len() && x.modified().ok() <= y.modified().ok(),
        _ => false,
    };
    if src.is_file() && !same(&src, &dest) {
        let tmp = bin.join(format!("{RUNNER_NAME}.new"));
        std::fs::copy(&src, &tmp)?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&tmp, &dest)?;
    }
    Ok(())
}

/// For tests of this crate that set environment variables (`LUDONIMBUS_HOME`
/// and the like). The tests of a crate run on threads of one process, and
/// they all share its environment: holding this, a test has it to itself,
/// and once it is dropped the variables it set are back as they were.
#[cfg(test)]
pub(crate) struct TestEnv {
    old: Vec<(&'static str, Option<std::ffi::OsString>)>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl TestEnv {
    pub(crate) fn new() -> TestEnv {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        TestEnv { old: Vec::new(), _lock: LOCK.lock().unwrap_or_else(|e| e.into_inner()) }
    }

    pub(crate) fn set(&mut self, name: &'static str, value: impl AsRef<std::ffi::OsStr>) {
        if !self.old.iter().any(|(n, _)| *n == name) {
            self.old.push((name, std::env::var_os(name)));
        }
        std::env::set_var(name, value);
    }
}

#[cfg(test)]
impl Drop for TestEnv {
    fn drop(&mut self) {
        for (name, value) in self.old.drain(..).rev() {
            match value {
                Some(v) => std::env::set_var(name, v),
                None => std::env::remove_var(name),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drives_of_linux_and_steamos() {
        let tmp = tempfile::tempdir().unwrap();
        let (media, var_mnt, mnt) = (tmp.path().join("run").join("media"), tmp.path().join("var").join("mnt"), tmp.path().join("mnt"));
        let made = [media.join("mmcblk0p1"), media.join("deck").join("SDCARD"), media.join("deck").join("USB"), var_mnt.join("games"), mnt.join("data")];
        for d in &made {
            std::fs::create_dir_all(d).unwrap();
        }
        // A file is no drive.
        std::fs::write(mnt.join("notes.txt"), b"x").unwrap();
        let bases = [media.clone(), var_mnt, mnt.clone(), tmp.path().join("missing")];
        // The user's folder under /run/media holds drives; other folders there are drives.
        assert_eq!(drives_under(&bases, Some("deck")), vec![made[1].clone(), made[2].clone(), made[0].clone(), made[3].clone(), made[4].clone()]);
        assert_eq!(drives_under(&bases, Some("someone")), vec![media.join("deck"), made[0].clone(), made[3].clone(), made[4].clone()]);
        // A drive reached twice counts once (`/mnt` is `/var/mnt` on some systems).
        assert_eq!(drives_under(&[mnt.clone(), mnt], None), vec![made[4].clone()]);
        if !cfg!(target_os = "linux") {
            assert!(extra_drives().is_empty());
        }
    }
}
