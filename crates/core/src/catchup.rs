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

/// Whether the tray app (its local API) is running.
pub fn app_running() -> bool {
    let Ok(text) = std::fs::read_to_string(paths::data_dir().join("agent.json")) else { return false };
    let pid = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v["pid"].as_u64()).unwrap_or(0) as u32;
    pid != 0 && process::pid_alive(pid)
}

/// What the launch wrapper of a game is doing: "preLaunch", "playing" or
/// "uploading"; `None` when none is running.
pub fn runner_phase(key: &str) -> Option<String> {
    let path = paths::running_dir().join(format!("{key}.json"));
    let (pid, _) = marker_pid(&path);
    if pid == 0 || !process::pid_alive(pid) {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
    Some(v["phase"].as_str().unwrap_or("playing").to_string())
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

fn marker_pid(path: &Path) -> (u32, Option<DateTime<Utc>>) {
    let v = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).unwrap_or_default();
    let pid = v["pid"].as_u64().unwrap_or(0) as u32;
    let started = serde_json::from_value(v["startedAt"].clone()).ok();
    (pid, started)
}

/// Finish the sessions of wrappers that were terminated (other than
/// `except`, the game being launched now). Each marker is claimed by
/// renaming it first, so two programs never handle the same one. Returns
/// the games handled.
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
        let (pid, started) = marker_pid(&path);
        if pid != 0 && process::pid_alive(pid) {
            continue;
        }
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
                match sync::sync(&ctx, g, Resolution::Auto, Trigger::Passive) {
                    Ok(out) => log(format!("{key}: catch-up sync {out:?}")),
                    Err(e) => log(format!("{key}: catch-up sync failed: {e:#}")),
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
