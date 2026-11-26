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
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Ludonimbus")
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

/// Directory of the running executable (where sidecars live).
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
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
