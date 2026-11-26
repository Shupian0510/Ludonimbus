//! Playtime: every install of a game records its own play sessions and
//! uploads them to `games/<folder>/playtime/<device-id>.json`, in the
//! game's folder [`gamedata`] says (`<device-id>~<key>.json` for an install
//! that joined a game recorded under another key). A game's total is the sum
//! over its installs, read from every folder of the game.
//!
//! - The Steam wrapper records the time from game start to exit; the tray
//!   watcher records games started outside it.
//! - A session says where it was played: the platform, as save manifests
//!   name it, and the save line the game followed on this device. Sessions
//!   recorded before that have neither and stay as they are.
//! - The first time a device keeps playtime for a game linked to a Steam
//!   shortcut, the minutes Steam already counted (`localconfig.vdf`) are
//!   imported once as that device's baseline. After that Steam's own counter
//!   is ignored, so nothing is counted twice.
//! - Other installs' files are cached under `playtime/remote/` by change
//!   tag, so totals show offline and unchanged files are not downloaded
//!   again.

use crate::cloud::gamedata::{self, Folders};
use crate::cloud::store::{join, Store};
use crate::config::{AppConfig, GameEntry};
use crate::{paths, steam, util, vdf};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Sessions shorter than this are launch failures, not play.
const MIN_SESSION_SECS: i64 = 10;

/// The folder of a game's playtime files.
const KIND: &str = "playtime";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Via {
    /// Started from Steam through `ludonimbus-run` (Steam counted it too).
    Steam,
    /// Started outside the wrapper, seen by the tray watcher.
    Direct,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub secs: u64,
    pub via: Via,
    /// Where the game's saves come from, as save manifests say it: `pc`,
    /// `emu:<adapter>`, `recomp:<adapter>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    /// The save line the game followed on this device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamBaseline {
    pub minutes: u64,
    pub account: String,
    pub appid: u32,
    pub imported_at: DateTime<Utc>,
}

/// One device's playtime for one game (the file it owns in the cloud).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLog {
    pub version: u32,
    pub device: String,
    pub device_name: String,
    #[serde(default)]
    pub steam_baseline: Option<SteamBaseline>,
    #[serde(default)]
    pub sessions: Vec<Session>,
    pub updated_at: DateTime<Utc>,
}

impl DeviceLog {
    fn new(cfg: &AppConfig) -> Self {
        DeviceLog {
            version: 1,
            device: cfg.device.id.clone(),
            device_name: cfg.device.name.clone(),
            steam_baseline: None,
            sessions: Vec::new(),
            updated_at: Utc::now(),
        }
    }

    pub fn total_secs(&self) -> u64 {
        self.steam_baseline.as_ref().map_or(0, |b| b.minutes * 60) + self.sessions.iter().map(|s| s.secs).sum::<u64>()
    }

    pub fn last_played(&self) -> Option<DateTime<Utc>> {
        self.sessions.iter().map(|s| s.end).max()
    }

    /// Add sessions from another copy of this device's log (e.g. the cloud
    /// copy after the local file was lost), keeping each start time once.
    fn merge(&mut self, other: &DeviceLog) -> bool {
        let mut changed = false;
        if self.steam_baseline.is_none() && other.steam_baseline.is_some() {
            self.steam_baseline = other.steam_baseline.clone();
            changed = true;
        }
        for s in &other.sessions {
            if !self.sessions.iter().any(|x| x.start == s.start) {
                self.sessions.push(s.clone());
                changed = true;
            }
        }
        if changed {
            self.sessions.sort_by_key(|s| s.start);
        }
        changed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceTotal {
    pub device: String,
    pub device_name: String,
    pub secs: u64,
    pub last_played: Option<DateTime<Utc>>,
    pub this_device: bool,
    /// Minutes imported from Steam (played before Ludonimbus kept time).
    pub steam_minutes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformTotal {
    pub platform: String,
    pub secs: u64,
}

/// Total playtime of a game over all devices.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub total_secs: u64,
    pub last_played: Option<DateTime<Utc>>,
    pub devices: Vec<DeviceTotal>,
    /// Time per platform ([`Session::platform`]), most first. Sessions from
    /// before sessions said where they were played, and minutes imported
    /// from Steam, are only in the total.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<PlatformTotal>,
}

// ---------------------------------------------------------------------------
// Local files

fn dir() -> PathBuf {
    paths::data_dir().join("playtime")
}

fn local_file(key: &str) -> PathBuf {
    dir().join(format!("{key}.json"))
}

fn remote_dir(key: &str) -> PathBuf {
    dir().join("remote").join(key)
}

/// This device's log, if it has one.
pub fn load_local(key: &str) -> Option<DeviceLog> {
    read_json(&local_file(key))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn save_local(key: &str, log: &DeviceLog) -> Result<()> {
    util::atomic_write(&local_file(key), &serde_json::to_vec_pretty(log)?)
}

#[derive(Serialize, Deserialize)]
struct CachedRemote {
    tag: String,
    /// The game folder the file is in and its name there. Empty in what 0.6
    /// kept: the key's folder, and the name of the file kept.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    folder: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    log: DeviceLog,
}

/// Another install's log, and whose it is.
struct Remote {
    device: String,
    install: String,
    log: DeviceLog,
}

fn cached_remotes(key: &str) -> Vec<Remote> {
    let Ok(rd) = std::fs::read_dir(remote_dir(key)) else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| {
            let c = read_json::<CachedRemote>(&e.path())?;
            let kept = e.file_name().to_string_lossy().to_string();
            let folder = if c.folder.is_empty() { key } else { c.folder.as_str() };
            let name = if c.name.is_empty() { kept.as_str() } else { c.name.as_str() };
            let (device, install) = gamedata::owner(folder, name.strip_suffix(".json").unwrap_or(name));
            let (device, install) = (device.to_string(), install.to_string());
            // The device is what the file says, where it says one.
            let device = if c.log.device.is_empty() { device } else { c.log.device.clone() };
            Some(Remote { device, install, log: c.log })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Steam

/// Minutes Steam counted for a shortcut on this PC (`localconfig.vdf`, where
/// shortcuts are keyed by their app id as a signed 32-bit number).
pub fn steam_minutes(root: &Path, account: &str, appid: u32) -> Option<u64> {
    let file = root.join("userdata").join(account).join("config").join("localconfig.vdf");
    let text = std::fs::read_to_string(file).ok()?;
    steam_minutes_in(&text, appid)
}

fn steam_minutes_in(text: &str, appid: u32) -> Option<u64> {
    let doc = vdf::parse_text(text).ok()?;
    let apps = doc
        .get("UserLocalConfigStore")?
        .get("Software")?
        .get("Valve")?
        .get("Steam")?
        .get("apps")?;
    let signed = (appid as i32).to_string();
    let entry = apps.get(&signed).or_else(|| apps.get(&appid.to_string()))?;
    entry.get("Playtime")?.as_str()?.trim().parse().ok()
}

/// This device's log, created on first use. Imports Steam's minutes once,
/// while no session started from Steam was recorded yet (those minutes would
/// otherwise be counted twice).
pub fn ensure(cfg: &AppConfig, game: &GameEntry) -> Result<DeviceLog> {
    let existing = load_local(&game.key);
    let mut log = existing.clone().unwrap_or_else(|| DeviceLog::new(cfg));
    log.device = cfg.device.id.clone();
    log.device_name = cfg.device.name.clone();
    let can_import = log.steam_baseline.is_none() && !log.sessions.iter().any(|s| s.via == Via::Steam);
    if can_import {
        if let (Some(link), Some(root)) = (game.steam.as_ref(), steam::root(cfg)) {
            if let Some(minutes) = steam_minutes(&root, &link.account, link.appid) {
                log.steam_baseline = Some(SteamBaseline {
                    minutes,
                    account: link.account.clone(),
                    appid: link.appid,
                    imported_at: Utc::now(),
                });
            }
        }
    }
    if existing.as_ref() != Some(&log) {
        log.updated_at = Utc::now();
        save_local(&game.key, &log)?;
    }
    Ok(log)
}

/// Record a finished session. Returns `false` when it was too short to count.
pub fn record(cfg: &AppConfig, game: &GameEntry, start: DateTime<Utc>, end: DateTime<Utc>, via: Via) -> Result<bool> {
    let secs = (end - start).num_seconds();
    if secs < MIN_SESSION_SECS {
        return Ok(false);
    }
    let mut log = ensure(cfg, game)?;
    if log.sessions.iter().any(|s| s.start == start) {
        return Ok(false);
    }
    log.sessions.push(Session {
        start,
        end,
        secs: secs as u64,
        via,
        platform: Some(crate::sync::platform(game)),
        line: Some(crate::cloud::wanted_line(cfg, game, Some(&crate::state::game(&game.key)))),
    });
    log.sessions.sort_by_key(|s| s.start);
    log.updated_at = Utc::now();
    save_local(&game.key, &log)?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// Totals and cloud

/// The total of the game `key` from this install's log and the other
/// installs': the files of one install are merged (it may have written in
/// two places), installs are added up, then grouped by device.
fn summarize(own: Option<&DeviceLog>, others: &[Remote], own_id: &str, key: &str) -> Summary {
    let mut installs: BTreeMap<(&str, &str), DeviceLog> = BTreeMap::new();
    // This install's own files are in `own` already.
    for r in others.iter().filter(|r| (r.device.as_str(), r.install.as_str()) != (own_id, key)) {
        match installs.get_mut(&(r.device.as_str(), r.install.as_str())) {
            Some(log) => {
                log.merge(&r.log);
            }
            None => {
                installs.insert((r.device.as_str(), r.install.as_str()), r.log.clone());
            }
        }
    }
    let logs: Vec<(&str, &DeviceLog)> = own.map(|l| (own_id, l)).into_iter().chain(installs.iter().map(|((d, _), l)| (*d, l))).collect();

    let mut devices: Vec<DeviceTotal> = Vec::new();
    for (device, log) in &logs {
        let i = match devices.iter().position(|d| d.device == *device) {
            Some(i) => i,
            None => {
                devices.push(DeviceTotal {
                    device: device.to_string(),
                    device_name: log.device_name.clone(),
                    secs: 0,
                    last_played: None,
                    this_device: *device == own_id,
                    steam_minutes: None,
                });
                devices.len() - 1
            }
        };
        let d = &mut devices[i];
        d.secs += log.total_secs();
        d.last_played = d.last_played.max(log.last_played());
        if let Some(b) = &log.steam_baseline {
            d.steam_minutes = Some(d.steam_minutes.unwrap_or(0) + b.minutes);
        }
    }
    devices.retain(|d| d.secs > 0);
    devices.sort_by(|a, b| b.this_device.cmp(&a.this_device).then(b.secs.cmp(&a.secs)));

    let mut by_platform: BTreeMap<&str, u64> = BTreeMap::new();
    for s in logs.iter().flat_map(|(_, l)| &l.sessions) {
        if let Some(p) = s.platform.as_deref().filter(|p| !p.is_empty()) {
            *by_platform.entry(p).or_default() += s.secs;
        }
    }
    let mut platforms: Vec<PlatformTotal> = by_platform.into_iter().map(|(p, secs)| PlatformTotal { platform: p.to_string(), secs }).collect();
    platforms.sort_by(|a, b| b.secs.cmp(&a.secs).then(a.platform.cmp(&b.platform)));

    Summary {
        total_secs: devices.iter().map(|d| d.secs).sum(),
        last_played: devices.iter().filter_map(|d| d.last_played).max(),
        devices,
        platforms,
    }
}

/// Total over all devices from local data (this device + cached copies of
/// the others), without touching the network.
pub fn summary(cfg: &AppConfig, key: &str) -> Summary {
    summarize(load_local(key).as_ref(), &cached_remotes(key), &cfg.device.id, key)
}

/// This install's file in the cloud: where it writes and under which name.
fn own_path(cfg: &AppConfig, at: &Folders) -> String {
    join(&[&at.write_dir(KIND), &format!("{}.json", at.own_name(&cfg.device.id))])
}

/// Upload this install's log (when it has one) of the game `at`.
pub fn upload(store: &dyn Store, cfg: &AppConfig, at: &Folders) -> Result<()> {
    let Some(log) = load_local(at.key()).filter(|l| l.total_secs() > 0) else {
        return Ok(());
    };
    store.write(&own_path(cfg, at), &serde_json::to_vec_pretty(&log)?)
}

/// Fetch the other installs' logs that changed, from every folder of the
/// game `at`; merge back this install's cloud copy when the local file lost
/// sessions; upload this install's log when the cloud copy is behind; and
/// return the new total.
pub fn sync(store: &dyn Store, cfg: &AppConfig, at: &Folders) -> Result<Summary> {
    let key = at.key();
    let device = cfg.device.id.as_str();
    let own = own_path(cfg, at);
    let rdir = remote_dir(key);
    let mut seen = Vec::new();
    let mut own_in_cloud: Option<DeviceLog> = None;
    // Files this install wrote elsewhere, before the game's folders changed.
    let mut own_elsewhere: Vec<DeviceLog> = Vec::new();
    for (folder, dir) in at.read_dirs(KIND) {
        for e in store.list(&dir)?.iter().filter(|e| !e.is_dir) {
            let Some(stem) = e.name.strip_suffix(".json") else { continue };
            let path = join(&[&dir, &e.name]);
            if at.is_own(folder, stem, device) {
                let log = store.read(&path)?.and_then(|b| serde_json::from_slice::<DeviceLog>(&b).ok());
                match path == own {
                    true => own_in_cloud = log,
                    false => own_elsewhere.extend(log),
                }
                continue;
            }
            let name = at.local_name(folder, &e.name);
            let cache = rdir.join(&name);
            seen.push(name);
            if read_json::<CachedRemote>(&cache).is_some_and(|c| !e.tag.is_empty() && c.tag == e.tag) {
                continue;
            }
            if let Some(log) = store.read(&path)?.and_then(|b| serde_json::from_slice::<DeviceLog>(&b).ok()) {
                let cached = CachedRemote { tag: e.tag.clone(), folder: folder.to_string(), name: e.name.clone(), log };
                util::atomic_write(&cache, &serde_json::to_vec(&cached)?)?;
            }
        }
    }
    // Installs whose file is gone from the cloud (or another cloud now).
    if let Ok(rd) = std::fs::read_dir(&rdir) {
        for f in rd.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if !seen.contains(&name) {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }

    let mut local = load_local(key);
    let mut changed = false;
    for cloud in own_in_cloud.iter().chain(&own_elsewhere) {
        match &mut local {
            Some(l) => changed |= l.merge(cloud),
            None => {
                local = Some(cloud.clone());
                changed = true;
            }
        }
    }
    if let Some(l) = local.as_ref().filter(|_| changed) {
        save_local(key, l)?;
    }
    if local.as_ref().is_some_and(|l| l.total_secs() > 0) && local != own_in_cloud {
        upload(store, cfg, at)?;
    }
    Ok(summary(cfg, key))
}

/// Best-effort upload after a session; failures only go to the log.
pub fn upload_quietly(cfg: &AppConfig, key: &str, log: impl Fn(String)) {
    match crate::cloud::open(cfg) {
        Ok(Some(cloud)) => {
            // Where it goes is worked out now that the cloud can be asked.
            if let Err(e) = Folders::of(&cloud, key).and_then(|at| upload(cloud.files(), cfg, &at)) {
                log(format!("{key}: playtime upload failed: {e:#}"));
            }
        }
        Ok(None) => {}
        Err(e) => log(format!("{key}: playtime upload skipped: {e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCALCONFIG: &str = r#""UserLocalConfigStore"
{
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"apps"
				{
					"4079546474"
					{
						"BadgeData"		"020000000816"
					}
					"-215420822"
					{
						"Playtime2wks"		"988"
						"Playtime"		"988"
					}
				}
			}
		}
	}
}
"#;

    #[test]
    fn reads_shortcut_minutes_by_signed_appid() {
        assert_eq!(steam_minutes_in(LOCALCONFIG, 4079546474), Some(988));
        assert_eq!(steam_minutes_in(LOCALCONFIG, 123), None);
    }

    fn log(device: &str, baseline: Option<u64>, secs: &[u64]) -> DeviceLog {
        let t0 = DateTime::parse_from_rfc3339("2026-11-01T00:00:00Z").unwrap().with_timezone(&Utc);
        DeviceLog {
            version: 1,
            device: device.into(),
            device_name: device.to_uppercase(),
            steam_baseline: baseline.map(|m| SteamBaseline {
                minutes: m,
                account: "1".into(),
                appid: 1,
                imported_at: t0,
            }),
            sessions: secs
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let start = t0 + chrono::Duration::hours(i as i64);
                    Session {
                        start,
                        end: start + chrono::Duration::seconds(*s as i64),
                        secs: *s,
                        via: Via::Steam,
                        platform: None,
                        line: None,
                    }
                })
                .collect(),
            updated_at: t0,
        }
    }

    fn remote(install: &str, log: DeviceLog) -> Remote {
        Remote { device: log.device.clone(), install: install.into(), log }
    }

    #[test]
    fn totals_add_up_over_devices() {
        let own = log("a", Some(10), &[60, 120]);
        let other = log("b", None, &[3600]);
        // A stale copy of this install's own file is not counted again.
        let s = summarize(Some(&own), &[remote("g", other), remote("g", log("a", None, &[999]))], "a", "g");
        assert_eq!(s.total_secs, 600 + 180 + 3600);
        assert_eq!(s.devices.len(), 2);
        assert!(s.devices[0].this_device);
        assert_eq!(s.devices[0].steam_minutes, Some(10));
        assert!(s.platforms.is_empty(), "sessions from before say no platform");
    }

    #[test]
    fn installs_add_up_and_show_under_their_device() {
        let own = log("a", None, &[60]);
        let mut steam = log("b", Some(1), &[100]);
        steam.sessions[0].platform = Some("pc".into());
        let mut emu = log("b", None, &[1000, 2000]);
        for s in &mut emu.sessions {
            s.platform = Some("emu:ryujinx".into());
        }
        // The same install's file in two folders: its sessions count once.
        let again = emu.clone();
        let others = [remote("g", steam), remote("g-switch", emu), remote("g-switch", again), remote("g-gog", log("a", None, &[5]))];
        let s = summarize(Some(&own), &others, "a", "g");
        assert_eq!(s.total_secs, 60 + 5 + 60 + 100 + 3000);
        assert_eq!(s.devices.len(), 2);
        // This device's other install of the game is this device's time too.
        assert_eq!((s.devices[0].device.as_str(), s.devices[0].secs, s.devices[0].this_device), ("a", 65, true));
        assert_eq!((s.devices[1].device.as_str(), s.devices[1].secs, s.devices[1].steam_minutes), ("b", 3160, Some(1)));
        assert_eq!(
            s.platforms,
            vec![PlatformTotal { platform: "emu:ryujinx".into(), secs: 3000 }, PlatformTotal { platform: "pc".into(), secs: 100 }]
        );
    }

    #[test]
    fn sessions_from_before_read_and_stay_as_they_were() {
        let old = r#"{"start":"2026-11-01T00:00:00Z","end":"2026-11-01T00:10:00Z","secs":600,"via":"steam"}"#;
        let s: Session = serde_json::from_str(old).unwrap();
        assert_eq!((s.platform.as_deref(), s.line.as_deref()), (None, None));
        assert_eq!(serde_json::to_string(&s).unwrap(), old);
    }

    #[test]
    fn merge_keeps_each_session_once() {
        let mut a = log("a", None, &[60, 120]);
        let b = log("a", Some(5), &[60, 120, 30]);
        assert!(a.merge(&b));
        assert_eq!(a.sessions.len(), 3);
        assert_eq!(a.steam_baseline.as_ref().map(|b| b.minutes), Some(5));
        assert!(!a.merge(&b));
    }
}
