//! One-click setup of the Steam plugin: installs Millennium from its official
//! GitHub release when needed, then copies and enables the Ludonimbus plugin
//! (`plugin-millennium/`, bundled as a resource) so the cloud save status
//! shows up on game pages inside Steam.

use anyhow::{anyhow, bail, Result};
use ludonimbus_core::millennium::{self, Install};
use ludonimbus_core::{config, logger, paths, steam};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};
use ts_rs::TS;

type CmdResult<T> = std::result::Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?.map_err(|e| format!("{e:#}"))
}

fn log(msg: impl AsRef<str>) {
    logger::log("steam-plugin", msg);
}

/// The packed plugin shipped with this app (`LUDONIMBUS_PLUGIN` overrides it in development).
pub fn bundled(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LUDONIMBUS_PLUGIN") {
        if !p.trim().is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let p = app.path().resource_dir().ok()?.join("millennium").join(millennium::PLUGIN_FILE);
    p.is_file().then_some(p)
}

/// At startup: replace an installed plugin from an older app version. Takes
/// effect the next time Steam starts.
pub fn refresh_installed(app: &AppHandle) {
    let (Ok(cfg), Some(src)) = (config::load(), bundled(app)) else {
        return;
    };
    let Some(root) = steam::root(&cfg) else { return };
    let st = millennium::status(&root, Some(&src));
    if st.plugin_installed && !st.plugin_current {
        match millennium::install_plugin(&root, &src) {
            Ok(()) => log("updated the installed Steam plugin"),
            Err(e) => log(format!("could not update the Steam plugin: {e:#}")),
        }
    }
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SteamPluginStatus")]
pub struct PluginView {
    steam_root: Option<String>,
    steam_running: bool,
    /// This build ships the plugin.
    bundled: bool,
    millennium: Option<Install>,
    plugin_installed: bool,
    plugin_enabled: bool,
    plugin_current: bool,
}

#[tauri::command]
pub async fn steam_plugin_status(app: AppHandle) -> CmdResult<PluginView> {
    blocking(move || {
        let cfg = config::load()?;
        let root = steam::root(&cfg);
        let src = bundled(&app);
        let st = root.as_deref().map(|r| millennium::status(r, src.as_deref()));
        Ok(PluginView {
            steam_root: root.map(|r| r.display().to_string()),
            steam_running: steam::is_running(),
            bundled: src.is_some(),
            millennium: st.as_ref().map(|s| s.millennium),
            plugin_installed: st.as_ref().is_some_and(|s| s.plugin_installed),
            plugin_enabled: st.as_ref().is_some_and(|s| s.plugin_enabled),
            plugin_current: st.as_ref().is_some_and(|s| s.plugin_current),
        })
    })
    .await
}

#[derive(Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SteamPluginProgress")]
pub(crate) struct Progress {
    /// "download" | "install"
    #[ts(type = r#""download" | "install""#)]
    stage: &'static str,
    done: u64,
    total: u64,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SteamPluginInstall")]
pub struct InstallResult {
    /// Millennium version installed by this run, if any.
    millennium_installed: Option<String>,
    steam_restarted: bool,
}

/// Close Steam (when allowed), run `f`, and start Steam again if it was running.
fn with_steam_closed<T>(root: &Path, f: impl FnOnce() -> Result<T>) -> Result<(T, bool)> {
    let was_running = steam::is_running();
    if was_running {
        steam::shutdown(root)?;
    }
    let out = f();
    if was_running {
        if let Err(e) = steam::launch(root) {
            log(format!("could not restart Steam: {e}"));
        }
    }
    out.map(|v| (v, was_running))
}

#[tauri::command]
pub async fn steam_plugin_install(app: AppHandle, close_steam: bool) -> CmdResult<InstallResult> {
    blocking(move || {
        let cfg = config::load()?;
        let root = steam::root(&cfg).ok_or_else(|| anyhow!(ludonimbus_core::tr!("没有找到 Steam", "Steam not found")))?;
        let src = bundled(&app).ok_or_else(|| anyhow!(ludonimbus_core::tr!("这个版本的 Ludonimbus 没有附带 Steam 插件", "This build of Ludonimbus doesn't include the Steam plugin")))?;
        let need_millennium = match millennium::detect(&root) {
            Install::Installed => false,
            Install::Missing => true,
            Install::Legacy => bail!(ludonimbus_core::tr!(
                "检测到旧版 Millennium（2.x），它不支持这个插件。请先用 Millennium 官方安装程序更新到最新版，再回来启用。",
                "An old Millennium (2.x) was found; it doesn't support this plugin. Update to the latest version with Millennium's official installer, then come back and enable it."
            )),
        };
        // Ask before downloading anything, so Steam is closed only briefly.
        if steam::is_running() && !close_steam {
            bail!("STEAM_RUNNING");
        }

        let mut downloaded: Option<(String, PathBuf)> = None;
        if need_millennium {
            let emit = |done: u64, total: u64| {
                let _ = app.emit("steam-plugin-progress", Progress { stage: "download", done, total });
            };
            emit(0, 0);
            let release = millennium::latest_release()?;
            log(format!("downloading Millennium {} from {}", release.tag, release.zip_url));
            let zip = millennium::download(&release, &paths::staging_dir().join("millennium"), &emit)?;
            log(format!("Millennium {} downloaded and verified", release.tag));
            downloaded = Some((release.tag, zip));
        }

        let _ = app.emit("steam-plugin-progress", Progress { stage: "install", done: 0, total: 0 });
        let result = with_steam_closed(&root, || {
            if let Some((tag, zip)) = &downloaded {
                millennium::extract(zip, &root)?;
                log(format!("Millennium {tag} installed into {}", root.display()));
            }
            millennium::install_plugin(&root, &src)?;
            millennium::set_enabled(&root, true)?;
            log("Steam plugin installed and enabled");
            Ok(())
        });
        if let Some((_, zip)) = &downloaded {
            let _ = std::fs::remove_file(zip);
        }
        let ((), restarted) = result?;
        Ok(InstallResult {
            millennium_installed: downloaded.map(|(tag, _)| tag),
            steam_restarted: restarted,
        })
    })
    .await
}

#[tauri::command]
pub async fn steam_plugin_remove(close_steam: bool) -> CmdResult<()> {
    blocking(move || {
        let cfg = config::load()?;
        let root = steam::root(&cfg).ok_or_else(|| anyhow!(ludonimbus_core::tr!("没有找到 Steam", "Steam not found")))?;
        if steam::is_running() && !close_steam {
            bail!("STEAM_RUNNING");
        }
        with_steam_closed(&root, || millennium::remove_plugin(&root))?;
        log("Steam plugin removed");
        Ok(())
    })
    .await
}
