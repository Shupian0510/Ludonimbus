//! Screenshots: a game's gallery over all devices, and fetching, syncing and
//! showing them.

use super::{blocking, CmdResult};
use crate::{art, watcher};
use anyhow::anyhow;
use ludonimbus_core::cloud::gamedata::{self, Folders};
use ludonimbus_core::config::{self, AppConfig, GameEntry};
use ludonimbus_core::screenshots;
use ludonimbus_core::sync::Ctx;
use serde::Serialize;
use std::collections::BTreeMap;
use ts_rs::TS;

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "Shot")]
pub struct ShotView {
    #[serde(flatten)]
    shot: screenshots::Shot,
    /// Thumbnail (or image) already on this PC, as a `ludoart://` URL.
    thumb_url: Option<String>,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "Gallery")]
pub struct GalleryView {
    shots: Vec<ShotView>,
    mode: config::ScreenshotMode,
    linked: bool,
    folder: Option<String>,
    /// Space the screenshots take in the cloud (images only).
    cloud_bytes: u64,
    /// The cloud couldn't be listed; only this PC's screenshots are shown.
    cloud_error: Option<String>,
}

#[tauri::command]
pub async fn screenshot_gallery(key: String) -> CmdResult<GalleryView> {
    blocking(move || {
        let ctx = Ctx::load()?;
        let g = ctx.game(&key)?;
        let cloud = ctx.cloud().ok();
        // Every folder of the game in the cloud: what this device last learnt
        // when the cloud can't be asked.
        let at = cloud.and_then(|c| Folders::of(c, &key).ok()).unwrap_or_else(|| Folders::remembered(&g));
        let (mut shots, cloud_error) = match screenshots::gallery(cloud.map(|c| c.files()), &ctx.cfg, &at, &g) {
            Ok(s) => (s, None),
            Err(e) => (screenshots::gallery(None, &ctx.cfg, &at, &g)?, Some(format!("{e:#}"))),
        };
        let mut names: BTreeMap<String, String> = cloud.and_then(|c| c.list_devices().ok()).unwrap_or_default().into_iter().map(|d| (d.id, d.name)).collect();
        names.insert(ctx.cfg.device.id.clone(), ctx.cfg.device.name.clone());
        for s in &mut shots {
            s.device_name = names.get(gamedata::device_of(&s.device)).cloned();
        }
        let cloud_bytes = shots.iter().filter(|s| s.in_cloud).map(|s| s.size).sum();
        Ok(GalleryView {
            shots: shots.into_iter().map(|s| ShotView { thumb_url: shot_link(&ctx.cfg, &g, &s.device, &s.name, true), shot: s }).collect(),
            mode: ctx.cfg.screenshots,
            linked: g.steam.is_some(),
            folder: screenshots::steam_dir(&ctx.cfg, &g).filter(|d| d.is_dir()).map(|d| d.display().to_string()),
            cloud_bytes,
            cloud_error,
        })
    })
    .await
}

/// Make a screenshot (or its thumbnail) available on this PC and return its URL.
#[tauri::command]
pub async fn screenshot_fetch(key: String, device: String, name: String, thumb: bool) -> CmdResult<String> {
    blocking(move || {
        let ctx = Ctx::load()?;
        let g = ctx.game(&key)?;
        if screenshots::local_file(&ctx.cfg, &g, &device, &name, thumb).is_none() {
            let cloud = ctx.cloud()?;
            // A device that uploaded no thumbnail: the full image comes instead.
            screenshots::fetch(cloud.files(), &ctx.cfg, &Folders::of(cloud, &key)?, &g, &device, &name, thumb)?;
        }
        shot_link(&ctx.cfg, &g, &device, &name, thumb).ok_or_else(|| anyhow!(ludonimbus_core::tr!("这张截图不在本机", "This screenshot isn't on this PC")))
    })
    .await
}

/// The `ludoart://` URL of a screenshot on this PC: of its thumbnail when
/// `thumb` and there is one, else of the full image. It names what the
/// protocol serves for it, so it never answers "not found" for a thumbnail
/// that isn't there. `None` when neither is on this PC.
fn shot_link(cfg: &AppConfig, g: &GameEntry, device: &str, name: &str, thumb: bool) -> Option<String> {
    let sizes: &[bool] = if thumb { &[true, false] } else { &[false] };
    sizes.iter().find_map(|&t| screenshots::local_file(cfg, g, device, name, t).map(|f| art::shot_url(&g.key, device, name, t, &f)))
}

#[tauri::command]
pub async fn screenshot_sync(app: tauri::AppHandle, key: String) -> CmdResult<screenshots::Report> {
    tauri::async_runtime::spawn_blocking(move || watcher::sync_screenshots(&app, Some(key))).await.map_err(|e| e.to_string())?
}

/// Show a screenshot in Explorer (this PC's Steam folder or the cache).
#[tauri::command]
pub async fn screenshot_reveal(key: String, device: String, name: String) -> CmdResult<()> {
    blocking(move || {
        let ctx = Ctx::load()?;
        let g = ctx.game(&key)?;
        let file = screenshots::local_file(&ctx.cfg, &g, &device, &name, false)
            .ok_or_else(|| anyhow!(ludonimbus_core::tr!("这张截图不在本机", "This screenshot isn't on this PC")))?;
        opener::reveal(&file)?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn without_version(url: &str) -> &str {
        url.split('?').next().unwrap()
    }

    #[test]
    fn a_screenshot_link_names_what_is_served() {
        // A scratch data folder (the one the other tests of the app use),
        // never the real one.
        std::env::set_var("LUDONIMBUS_HOME", std::env::temp_dir().join(format!("ludonimbus-app-test-{}", std::process::id())));
        let cfg = AppConfig::default();
        let g = GameEntry { key: "shot-link-test".into(), name: "Game".into(), ..Default::default() };
        let (device, name) = ("d-other", "20261127100000_1.jpg");
        let full = screenshots::cache_file(&g.key, device, name, false);
        let _ = std::fs::remove_dir_all(full.parent().unwrap());
        assert_eq!(shot_link(&cfg, &g, device, name, true), None);
        // Only the full image (its device uploaded no thumbnail): the
        // thumbnail's link is the full image's, which is served.
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, b"jpg").unwrap();
        let link = shot_link(&cfg, &g, device, name, true).unwrap();
        assert!(link.contains("/f/"), "{link}");
        assert_eq!(without_version(&link), without_version(&shot_link(&cfg, &g, device, name, false).unwrap()));
        // With the thumbnail there, the thumbnail.
        let thumb = screenshots::cache_file(&g.key, device, name, true);
        std::fs::create_dir_all(thumb.parent().unwrap()).unwrap();
        std::fs::write(&thumb, b"jpg").unwrap();
        assert!(shot_link(&cfg, &g, device, name, true).unwrap().contains("/t/"));
        assert!(shot_link(&cfg, &g, device, name, false).unwrap().contains("/f/"));
        let _ = std::fs::remove_dir_all(full.parent().unwrap());
    }
}
