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
pub fn logs_dir() -> PathBuf {
    ensure(data_dir().join("logs"))
}
pub fn steam_backups_dir() -> PathBuf {
    ensure(data_dir().join("steam-backups"))
}
pub fn ludusavi_config_dir() -> PathBuf {
    ensure(data_dir().join("ludusavi"))
}

/// Directory of the running executable (where sidecars live).
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The Steam launch wrapper, expected next to the current executable.
pub fn runner_exe() -> PathBuf {
    if let Ok(p) = std::env::var("LUDONIMBUS_RUNNER") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    exe_dir().join("ludonimbus-run.exe")
}
