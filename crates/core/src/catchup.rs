//! Work the tray app normally does in the background, done by the launch
//! wrapper (or the handheld window) when the tray app isn't running: in the
//! Xbox full screen experience and in Deck Game Mode there is no tray
//! (docs/plan-phase5.md §2.2).
//!
//! - A wrapper that was terminated before it finished (Steam's "Stop", a
//!   handheld put to sleep and the game killed) left its running marker: its
//!   session's playtime is recorded and the game's saves are synced.
//! - Screenshots of the game just played are uploaded.

use crate::config::AppConfig;
use crate::playtime::{self, Via};
use crate::snapshot::Trigger;
use crate::sync::{self, Ctx, Resolution};
use crate::{paths, process};
use chrono::{DateTime, Utc};
use std::path::Path;
use std::time::Duration;

/// The local API that runs (`agent.json`): the tray app's, or the headless
/// agent's (Deck Game Mode), which does this work too.
fn local_api() -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(paths::data_dir().join("agent.json")).ok()?;
    let v = serde_json::from_str::<serde_json::Value>(&text).ok()?;
    let pid = v["pid"].as_u64().unwrap_or(0) as u32;
    (pid != 0 && process::pid_alive(pid)).then_some(v)
}

/// Whether the tray app (its local API) is running.
pub fn app_running() -> bool {
    local_api().is_some()
}

/// Whether the desktop app runs, not just the headless agent: only the app
/// finishes a sync after playing that a wrapper hands over (docs/plan-v2.md,
/// section 5.5).
pub fn desktop_app_running() -> bool {
    local_api().is_some_and(|v| v["headless"].as_bool() != Some(true))
}

/// What the launch wrapper of a game is doing: "preLaunch", "playing" or
/// "uploading"; `None` when none is running. A marker being replaced right
/// now counts as "playing": a wrapper is there.
pub fn runner_phase(key: &str) -> Option<String> {
    match read_marker(&paths::running_dir().join(format!("{key}.json"))) {
        Marker::None => None,
        Marker::Unknown => Some("playing".to_string()),
        Marker::Read { pid, phase, .. } => (pid != 0 && process::pid_alive(pid)).then(|| phase.unwrap_or_else(|| "playing".to_string())),
    }
}

/// Process names that mean "this game is running": its `waitFor` and its
/// program (not the Xbox app's launch helper, which every Xbox game shares).
pub fn watch_names(g: &crate::config::GameEntry) -> Vec<String> {
    let mut names: Vec<String> = crate::spec::effective(g).wait_for.iter().map(|s| s.to_lowercase()).collect();
    if let Some(exe) = g.exe.as_deref().filter(|e| !crate::xbox::is_launch_helper(e)) {
        names.push(crate::util::file_name_lower(exe));
    }
    names
}

/// What a wrapper's marker (`running/<key>.json`) says.
enum Marker {
    /// There is none: no wrapper handles the game.
    None,
    /// It can't be read or understood even after a few tries, and it was
    /// written a moment ago: a wrapper is there, what it does isn't known.
    Unknown,
    /// What the wrapper wrote. `pid` 0: there was nothing to read in it,
    /// and it is too old to be in the middle of being written (its writer
    /// died while writing it).
    Read { pid: u32, started: Option<DateTime<Utc>>, phase: Option<String> },
}

/// A marker that couldn't be read for this long is no longer being written.
const MARKER_WRITTEN_WITHIN: Duration = Duration::from_secs(60);

/// Read a wrapper's marker. The wrapper replaces it in one step, so it is
/// never half written; still, on Windows a reader can catch the moment it is
/// replaced, and older versions wrote it in place (an empty file for a
/// moment). So one that can't be read is read again, and then taken for a
/// wrapper that is there, never for one that is gone, unless it hasn't
/// changed for a minute.
fn read_marker(path: &Path) -> Marker {
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(50));
        }
        match std::fs::read_to_string(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Marker::None,
            Ok(text) => {
                if let Ok(v @ serde_json::Value::Object(_)) = serde_json::from_str::<serde_json::Value>(&text) {
                    return Marker::Read {
                        pid: v["pid"].as_u64().unwrap_or(0) as u32,
                        started: serde_json::from_value(v["startedAt"].clone()).ok(),
                        phase: v["phase"].as_str().map(str::to_string),
                    };
                }
            }
            Err(_) => {}
        }
    }
    let age = std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    match age {
        Some(age) if age > MARKER_WRITTEN_WITHIN => Marker::Read { pid: 0, started: None, phase: None },
        _ => Marker::Unknown,
    }
}

/// Finish the sessions of wrappers that were terminated (other than
/// `except`, the game being launched now). Each marker is claimed by
/// renaming it first, so two programs never handle the same one. Returns
/// the games handled.
///
/// A catch-up sync that doesn't get the saves into the cloud (the cloud
/// can't be reached, an error) leaves the game owing a sync after playing
/// ([`sync::owe`]): the tray app, or the game's next sync, does it then.
pub fn stale_runners(cfg: &AppConfig, except: Option<&str>, log: &dyn Fn(String)) -> Vec<String> {
    let mut done = Vec::new();
    let Ok(rd) = std::fs::read_dir(paths::running_dir()) else { return done };
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Some(key) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
        if Some(key.as_str()) == except {
            continue;
        }
        let started = match read_marker(&path) {
            Marker::None => continue,
            // Being written: its wrapper is there. Looked at again next time.
            Marker::Unknown => continue,
            Marker::Read { pid, .. } if pid != 0 && process::pid_alive(pid) => continue,
            Marker::Read { started, .. } => started,
        };
        let claimed = path.with_extension("claimed");
        if std::fs::rename(&path, &claimed).is_err() {
            continue;
        }
        if let Some(g) = cfg.game(&key) {
            log(format!("{key}: its launch wrapper was terminated; finishing the session"));
            if let Some(start) = started {
                if let Err(e) = playtime::record(cfg, g, start, Utc::now(), Via::Steam) {
                    log(format!("{key}: playtime: {e:#}"));
                }
            }
            if g.auto_sync {
                let ctx = Ctx::from_config(cfg.clone());
                let settled = match sync::sync(&ctx, g, Resolution::Auto, Trigger::Passive) {
                    Ok(out) => {
                        log(format!("{key}: catch-up sync {out:?}"));
                        out.is_final()
                    }
                    Err(e) => {
                        log(format!("{key}: catch-up sync failed: {e:#}"));
                        false
                    }
                };
                // The marker is only the claim; what is still to do is noted
                // where the syncs after playing are tried again.
                if !settled {
                    if let Err(e) = sync::owe(&key) {
                        log(format!("{key}: can't note the sync for later ({e:#})"));
                    }
                }
            }
            done.push(key);
        }
        let _ = std::fs::remove_file(&claimed);
    }
    done
}

/// Upload (and with "sync", download) the screenshots of one game.
pub fn screenshots(cfg: &AppConfig, key: &str, log: &dyn Fn(String)) {
    if cfg.screenshots == crate::config::ScreenshotMode::Off {
        return;
    }
    let Some(g) = cfg.game(key) else { return };
    match crate::cloud::open(cfg) {
        Ok(Some(cloud)) => {
            let result = crate::cloud::gamedata::Folders::of(&cloud, key)
                .and_then(|at| crate::screenshots::sync_at(cloud.files(), cfg, &at, g, log));
            if let Err(e) = result {
                log(format!("{key}: screenshots: {e:#}"));
            }
        }
        Ok(None) => {}
        Err(e) => log(format!("{key}: screenshots: {e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_that_cant_be_read_is_a_wrapper_still_there() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("celeste.json");
        assert!(matches!(read_marker(&path), Marker::None));
        std::fs::write(&path, r#"{"pid":1234,"phase":"uploading","startedAt":"2026-11-27T10:00:00Z"}"#).unwrap();
        match read_marker(&path) {
            Marker::Read { pid, started, phase } => {
                assert_eq!(pid, 1234);
                assert!(started.is_some());
                assert_eq!(phase.as_deref(), Some("uploading"));
            }
            _ => panic!("expected what the wrapper wrote"),
        }
        // Empty or cut off, as it was for a moment when it was written in
        // place: not "gone", whatever its pid would have been.
        for text in ["", r#"{"pid":12"#, "[]"] {
            std::fs::write(&path, text).unwrap();
            assert!(matches!(read_marker(&path), Marker::Unknown), "{text:?}");
        }
        // Still like that a minute later: its writer died while writing it.
        std::fs::write(&path, "").unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(120);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(old).unwrap();
        assert!(matches!(read_marker(&path), Marker::Read { pid: 0, .. }));
    }
}
