//! Steam screenshots of linked shortcuts, kept in the cloud per device.
//!
//! ```text
//! Steam:  userdata/<account>/760/remote/<gameid>/screenshots/<name>.jpg
//!                                                 screenshots/thumbnails/<name>.jpg
//!         <gameid> = (appid << 32) | 0x02000000 for non-Steam shortcuts
//! Cloud:  games/<key>/screenshots/<device-id>/<name>.jpg
//!         games/<key>/screenshots/<device-id>/thumbnails/<name>.jpg
//! ```
//! A device uploads only the screenshots taken on it; screenshots of other
//! devices are downloaded into this PC's folder of the same shortcut (its app
//! id may differ per PC). `%APPDATA%\Ludonimbus\screenshots\<key>.json`
//! remembers what was uploaded and downloaded, so a screenshot deleted in
//! Steam is not downloaded again and a downloaded one is not uploaded back
//! as this device's.
//!
//! Steam owns `760/screenshots.vdf` and rewrites it while it runs, so only
//! the files are placed; Steam picks them up when it scans the folder.

use crate::cloud::store::{join, Store};
use crate::config::{AppConfig, GameEntry, ScreenshotMode};
use crate::{paths, steam, util};
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Steam's game id of a non-Steam shortcut (also used in `steam://rungameid/`).
pub fn shortcut_gameid(appid: u32) -> u64 {
    ((appid as u64) << 32) | 0x0200_0000
}

/// This PC's Steam screenshot folder of a linked game.
pub fn steam_dir(cfg: &AppConfig, game: &GameEntry) -> Option<PathBuf> {
    let link = game.steam.as_ref()?;
    let root = steam::root(cfg)?;
    Some(
        root.join("userdata")
            .join(&link.account)
            .join("760")
            .join("remote")
            .join(shortcut_gameid(link.appid).to_string())
            .join("screenshots"),
    )
}

fn is_image(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    (n.ends_with(".jpg") || n.ends_with(".jpeg") || n.ends_with(".png")) && !n.ends_with(".lntmp")
}

/// Screenshot file names must stay plain names (they become paths).
fn safe_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', ':']) && !name.starts_with('.') && is_image(name)
}

/// When a screenshot was taken, from Steam's `YYYYMMDDhhmmss_N` name (local time).
pub fn taken_at(name: &str) -> Option<DateTime<Utc>> {
    let stamp = name.get(..14)?;
    let naive = NaiveDateTime::parse_from_str(stamp, "%Y%m%d%H%M%S").ok()?;
    chrono::Local.from_local_datetime(&naive).single().map(|t| t.with_timezone(&Utc))
}

// ---------------------------------------------------------------------------
// Local record

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Record {
    uploaded: BTreeSet<String>,
    /// `<device-id>/<name>` of screenshots downloaded from other devices.
    downloaded: BTreeSet<String>,
}

fn record_file(key: &str) -> PathBuf {
    paths::data_dir().join("screenshots").join(format!("{key}.json"))
}

fn load_record(key: &str) -> Record {
    std::fs::read(record_file(key))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_record(key: &str, r: &Record) -> Result<()> {
    util::atomic_write(&record_file(key), &serde_json::to_vec_pretty(r)?)
}

/// Where screenshots of other devices are cached for viewing in Ludonimbus.
pub fn cache_file(key: &str, device: &str, name: &str, thumb: bool) -> PathBuf {
    let dir = paths::data_dir().join("screenshot-cache").join(key).join(device);
    if thumb {
        dir.join("thumbnails").join(name)
    } else {
        dir.join(name)
    }
}

fn cloud_dir(key: &str) -> String {
    join(&["games", key, "screenshots"])
}

// ---------------------------------------------------------------------------
// Sync

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub uploaded: usize,
    pub downloaded: usize,
    pub failed: usize,
}

/// Local screenshot names (not thumbnails), oldest first.
fn local_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| safe_name(n))
        .collect();
    names.sort();
    names
}

/// Download to a temporary name first so Steam never sees half a file.
fn download_to(store: &dyn Store, path: &str, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_file_name(format!(
        "{}.lntmp",
        dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    ));
    store.download(path, &tmp)?;
    std::fs::rename(&tmp, dest).with_context(|| crate::tr!("无法写入 {}", "Can't write {}", dest.display()))?;
    Ok(())
}

/// Upload this PC's new screenshots and, in [`ScreenshotMode::Sync`],
/// download the other devices' into this PC's Steam folder.
pub fn sync(store: &dyn Store, cfg: &AppConfig, game: &GameEntry, log: &dyn Fn(String)) -> Result<Report> {
    let mut report = Report::default();
    if cfg.screenshots == ScreenshotMode::Off {
        return Ok(report);
    }
    let Some(dir) = steam_dir(cfg, game) else {
        return Ok(report);
    };
    let key = &game.key;
    let own = &cfg.device.id;
    let mut record = load_record(key);

    // Upload what was taken here.
    let own_dir = join(&[&cloud_dir(key), own]);
    let in_cloud: BTreeSet<String> = store
        .list(&own_dir)?
        .into_iter()
        .filter(|e| !e.is_dir)
        .map(|e| e.name)
        .collect();
    let came_from_elsewhere: BTreeSet<&str> = record
        .downloaded
        .iter()
        .filter_map(|id| id.split_once('/').map(|(_, n)| n))
        .collect();
    let mut uploaded_now = Vec::new();
    for name in local_names(&dir) {
        if in_cloud.contains(&name) || record.uploaded.contains(&name) || came_from_elsewhere.contains(name.as_str()) {
            continue;
        }
        let thumb = dir.join("thumbnails").join(&name);
        let result = (|| -> Result<()> {
            if thumb.is_file() {
                store.upload(&join(&[&own_dir, "thumbnails", &name]), &thumb)?;
            }
            // The image goes last: it marks the screenshot as complete.
            store.upload(&join(&[&own_dir, &name]), &dir.join(&name))
        })();
        match result {
            Ok(()) => {
                report.uploaded += 1;
                uploaded_now.push(name);
            }
            Err(e) => {
                report.failed += 1;
                log(format!("{key}: screenshot {name} upload failed: {e:#}"));
                if crate::cloud::is_unavailable(&e) {
                    break;
                }
            }
        }
    }
    if !uploaded_now.is_empty() {
        record.uploaded.extend(uploaded_now);
        save_record(key, &record)?;
    }

    if cfg.screenshots != ScreenshotMode::Sync {
        return Ok(report);
    }

    // Download the other devices' screenshots this PC has never had.
    let local: BTreeSet<String> = local_names(&dir).into_iter().collect();
    for dev in store.list(&cloud_dir(key))?.into_iter().filter(|e| e.is_dir && &e.name != own) {
        let dev_dir = join(&[&cloud_dir(key), &dev.name]);
        let entries = store.list(&dev_dir)?;
        let has_thumbs = entries.iter().any(|e| e.is_dir && e.name == "thumbnails");
        let thumbs: BTreeSet<String> = if has_thumbs {
            store
                .list(&join(&[&dev_dir, "thumbnails"]))?
                .into_iter()
                .map(|e| e.name)
                .collect()
        } else {
            BTreeSet::new()
        };
        for e in entries.iter().filter(|e| !e.is_dir && safe_name(&e.name)) {
            let id = format!("{}/{}", dev.name, e.name);
            if record.downloaded.contains(&id) {
                continue;
            }
            if local.contains(&e.name) {
                // Same name already here (e.g. copied by hand): don't overwrite.
                record.downloaded.insert(id);
                continue;
            }
            let result = (|| -> Result<()> {
                let thumb_dest = dir.join("thumbnails").join(&e.name);
                if thumbs.contains(&e.name) {
                    download_to(store, &join(&[&dev_dir, "thumbnails", &e.name]), &thumb_dest)?;
                }
                download_to(store, &join(&[&dev_dir, &e.name]), &dir.join(&e.name))?;
                if !thumb_dest.is_file() {
                    std::fs::copy(dir.join(&e.name), &thumb_dest)?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    report.downloaded += 1;
                    record.downloaded.insert(id);
                    save_record(key, &record)?;
                }
                Err(err) => {
                    report.failed += 1;
                    log(format!("{key}: screenshot {id} download failed: {err:#}"));
                    if crate::cloud::is_unavailable(&err) {
                        return Ok(report);
                    }
                }
            }
        }
    }
    save_record(key, &record)?;
    Ok(report)
}

// ---------------------------------------------------------------------------
// Gallery

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shot {
    pub name: String,
    /// Device that took it.
    pub device: String,
    pub device_name: Option<String>,
    pub size: u64,
    pub taken_at: Option<DateTime<Utc>>,
    /// The image is in this PC's Steam folder.
    pub on_this_pc: bool,
    /// Uploaded to the cloud.
    pub in_cloud: bool,
    pub has_thumb: bool,
}

/// Screenshots on this PC and in the cloud (when reachable), newest first.
pub fn gallery(store: Option<&dyn Store>, cfg: &AppConfig, game: &GameEntry) -> Result<Vec<Shot>> {
    let key = &game.key;
    let own = cfg.device.id.clone();
    let record = load_record(key);
    let dir = steam_dir(cfg, game);
    let mut shots: Vec<Shot> = Vec::new();
    if let Some(dir) = &dir {
        for name in local_names(dir) {
            // Downloaded from another device: listed under that device.
            let device = record
                .downloaded
                .iter()
                .find_map(|id| id.split_once('/').filter(|(_, n)| *n == name).map(|(d, _)| d.to_string()))
                .unwrap_or_else(|| own.clone());
            shots.push(Shot {
                size: std::fs::metadata(dir.join(&name)).map(|m| m.len()).unwrap_or(0),
                taken_at: taken_at(&name),
                on_this_pc: true,
                in_cloud: false,
                has_thumb: dir.join("thumbnails").join(&name).is_file(),
                device,
                device_name: None,
                name,
            });
        }
    }
    if let Some(store) = store {
        for dev in store.list(&cloud_dir(key))?.into_iter().filter(|e| e.is_dir) {
            let dev_dir = join(&[&cloud_dir(key), &dev.name]);
            let entries = store.list(&dev_dir)?;
            let thumbs: BTreeSet<String> = if entries.iter().any(|e| e.is_dir && e.name == "thumbnails") {
                store
                    .list(&join(&[&dev_dir, "thumbnails"]))?
                    .into_iter()
                    .map(|e| e.name)
                    .collect()
            } else {
                BTreeSet::new()
            };
            for e in entries.into_iter().filter(|e| !e.is_dir && safe_name(&e.name)) {
                if let Some(s) = shots.iter_mut().find(|s| s.name == e.name && s.device == dev.name) {
                    s.in_cloud = true;
                    continue;
                }
                shots.push(Shot {
                    taken_at: taken_at(&e.name),
                    on_this_pc: false,
                    in_cloud: true,
                    has_thumb: thumbs.contains(&e.name) || cache_file(key, &dev.name, &e.name, true).is_file(),
                    device: dev.name.clone(),
                    device_name: None,
                    size: e.size,
                    name: e.name,
                });
            }
        }
    }
    shots.sort_by(|a, b| b.taken_at.cmp(&a.taken_at).then(b.name.cmp(&a.name)));
    Ok(shots)
}

/// A screenshot's file on this PC (Steam folder or cache), if present.
pub fn local_file(cfg: &AppConfig, game: &GameEntry, device: &str, name: &str, thumb: bool) -> Option<PathBuf> {
    if !safe_name(name) || device.contains(['/', '\\', '.']) {
        return None;
    }
    let in_steam = steam_dir(cfg, game).map(|d| if thumb { d.join("thumbnails").join(name) } else { d.join(name) });
    in_steam
        .filter(|p| p.is_file())
        .or_else(|| Some(cache_file(&game.key, device, name, thumb)).filter(|p| p.is_file()))
}

/// Download a cloud screenshot (or its thumbnail) into the cache. Falls back
/// to the full image when the device uploaded no thumbnail.
pub fn fetch(store: &dyn Store, cfg: &AppConfig, game: &GameEntry, device: &str, name: &str, thumb: bool) -> Result<PathBuf> {
    if let Some(p) = local_file(cfg, game, device, name, thumb) {
        return Ok(p);
    }
    anyhow::ensure!(safe_name(name) && !device.contains(['/', '\\', '.']), "bad screenshot name");
    let dev_dir = join(&[&cloud_dir(&game.key), device]);
    let dest = cache_file(&game.key, device, name, thumb);
    if thumb {
        if download_to(store, &join(&[&dev_dir, "thumbnails", name]), &dest).is_ok() {
            return Ok(dest);
        }
        let full = fetch(store, cfg, game, device, name, false)?;
        return Ok(full);
    }
    download_to(store, &join(&[&dev_dir, name]), &dest)?;
    Ok(dest)
}

/// Space the game's screenshots use in the cloud.
pub fn cloud_usage(store: &dyn Store, key: &str) -> Result<u64> {
    let mut total = 0;
    for dev in store.list(&cloud_dir(key))?.into_iter().filter(|e| e.is_dir) {
        let dev_dir = join(&[&cloud_dir(key), &dev.name]);
        for e in store.list(&dev_dir)? {
            if e.is_dir {
                total += store.list(&join(&[&dev_dir, &e.name]))?.iter().map(|x| x.size).sum::<u64>();
            } else {
                total += e.size;
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_gameid_matches_steam() {
        // Measured on a real Steam install: appid 4079546474.
        assert_eq!(shortcut_gameid(4079546474), 17521518688375668736);
    }

    #[test]
    fn names() {
        assert!(safe_name("20261122223831_1.jpg"));
        assert!(!safe_name("../x.jpg"));
        assert!(!safe_name("a.jpg.lntmp"));
        assert!(!safe_name("thumbnails"));
        assert!(taken_at("20261122223831_1.jpg").is_some());
        assert!(taken_at("shot.jpg").is_none());
    }
}
