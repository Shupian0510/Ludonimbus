//! Playtime: every device records its own play sessions and uploads them to
//! `games/<key>/playtime/<device-id>.json`; the total is the sum over devices.
//!
//! - The Steam wrapper records the time from game start to exit; the tray
//!   watcher records games started outside it.
//! - The first time a device keeps playtime for a game linked to a Steam
//!   shortcut, the minutes Steam already counted (`localconfig.vdf`) are
//!   imported once as that device's baseline. After that Steam's own counter
//!   is ignored, so nothing is counted twice.
//! - Other devices' files are cached under `playtime/remote/` by change tag,
//!   so totals show offline and unchanged files are not downloaded again.

use crate::cloud::store::{join, Store};
use crate::config::{AppConfig, GameEntry};
use crate::{paths, steam, util, vdf};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Sessions shorter than this are launch failures, not play.
const MIN_SESSION_SECS: i64 = 10;

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

/// Total playtime of a game over all devices.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub total_secs: u64,
    pub last_played: Option<DateTime<Utc>>,
    pub devices: Vec<DeviceTotal>,
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

fn cloud_dir(key: &str) -> String {
    join(&["games", key, "playtime"])
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
    log: DeviceLog,
}

fn cached_remotes(key: &str) -> Vec<DeviceLog> {
    let Ok(rd) = std::fs::read_dir(remote_dir(key)) else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| read_json::<CachedRemote>(&e.path()))
        .map(|c| c.log)
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
    });
    log.sessions.sort_by_key(|s| s.start);
    log.updated_at = Utc::now();
    save_local(&game.key, &log)?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// Totals and cloud

fn summarize(own: Option<&DeviceLog>, others: &[DeviceLog], own_id: &str) -> Summary {
    let mut devices: Vec<DeviceTotal> = own
        .into_iter()
        .chain(others.iter().filter(|l| l.device != own_id))
        .map(|l| DeviceTotal {
            device: l.device.clone(),
            device_name: l.device_name.clone(),
            secs: l.total_secs(),
            last_played: l.last_played(),
            this_device: l.device == own_id,
            steam_minutes: l.steam_baseline.as_ref().map(|b| b.minutes),
        })
        .filter(|d| d.secs > 0)
        .collect();
    devices.sort_by(|a, b| b.this_device.cmp(&a.this_device).then(b.secs.cmp(&a.secs)));
    Summary {
        total_secs: devices.iter().map(|d| d.secs).sum(),
        last_played: devices.iter().filter_map(|d| d.last_played).max(),
        devices,
    }
}

/// Total over all devices from local data (this device + cached copies of
/// the others), without touching the network.
pub fn summary(cfg: &AppConfig, key: &str) -> Summary {
    summarize(load_local(key).as_ref(), &cached_remotes(key), &cfg.device.id)
}

/// Upload this device's log (when it has one).
pub fn upload(store: &dyn Store, cfg: &AppConfig, key: &str) -> Result<()> {
    let Some(log) = load_local(key).filter(|l| l.total_secs() > 0) else {
        return Ok(());
    };
    store.write(
        &join(&[&cloud_dir(key), &format!("{}.json", cfg.device.id)]),
        &serde_json::to_vec_pretty(&log)?,
    )
}

/// Fetch the other devices' logs that changed, merge back this device's
/// cloud copy when the local file lost sessions, upload this device's log
/// when the cloud copy is behind, and return the new total.
pub fn sync(store: &dyn Store, cfg: &AppConfig, key: &str) -> Result<Summary> {
    let entries = store.list(&cloud_dir(key))?;
    let own_name = format!("{}.json", cfg.device.id);
    let rdir = remote_dir(key);
    let mut seen = Vec::new();
    let mut own_in_cloud: Option<DeviceLog> = None;
    for e in entries.iter().filter(|e| !e.is_dir && e.name.ends_with(".json")) {
        let path = join(&[&cloud_dir(key), &e.name]);
        if e.name == own_name {
            own_in_cloud = store.read(&path)?.and_then(|b| serde_json::from_slice(&b).ok());
            continue;
        }
        seen.push(e.name.clone());
        let cache = rdir.join(&e.name);
        if read_json::<CachedRemote>(&cache).is_some_and(|c| !e.tag.is_empty() && c.tag == e.tag) {
            continue;
        }
        if let Some(log) = store.read(&path)?.and_then(|b| serde_json::from_slice::<DeviceLog>(&b).ok()) {
            let cached = CachedRemote { tag: e.tag.clone(), log };
            util::atomic_write(&cache, &serde_json::to_vec(&cached)?)?;
        }
    }
    // Devices whose file is gone from the cloud (or another cloud now).
    if let Ok(rd) = std::fs::read_dir(&rdir) {
        for f in rd.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if !seen.contains(&name) {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }

    let mut local = load_local(key);
    if let Some(cloud) = &own_in_cloud {
        match &mut local {
            Some(l) => {
                if l.merge(cloud) {
                    save_local(key, l)?;
                }
            }
            None => {
                save_local(key, cloud)?;
                local = Some(cloud.clone());
            }
        }
    }
    if local.as_ref().is_some_and(|l| l.total_secs() > 0) && local != own_in_cloud {
        upload(store, cfg, key)?;
    }
    Ok(summary(cfg, key))
}

/// Best-effort upload after a session; failures only go to the log.
pub fn upload_quietly(cfg: &AppConfig, key: &str, log: impl Fn(String)) {
    match crate::cloud::open(cfg) {
        Ok(Some(cloud)) => {
            if let Err(e) = upload(cloud.files(), cfg, key) {
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
                    }
                })
                .collect(),
            updated_at: t0,
        }
    }

    #[test]
    fn totals_add_up_over_devices() {
        let own = log("a", Some(10), &[60, 120]);
        let other = log("b", None, &[3600]);
        let s = summarize(Some(&own), &[other, log("a", None, &[999])], "a");
        assert_eq!(s.total_secs, 600 + 180 + 3600);
        assert_eq!(s.devices.len(), 2);
        assert!(s.devices[0].this_device);
        assert_eq!(s.devices[0].steam_minutes, Some(10));
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
