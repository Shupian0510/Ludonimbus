//! Commands invoked by the web UI. All heavy work runs on blocking threads.

use crate::{art, busy, watcher};
use anyhow::{anyhow, bail, Result};
use ludonimbus_core::cloud::{self, oauth, AccountInfo, CloudGame, DeviceInfo};
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry, Retention};
use ludonimbus_core::snapshot::{self, SnapshotMeta, SnapshotSummary, Trigger};
use ludonimbus_core::state::{self, ConflictInfo};
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution, StatusReport};
use ludonimbus_core::{adapters, games, localart, ludusavi, paths, playtime, process, relocate, screenshots, spec, steam, util};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) type CmdResult<T> = std::result::Result<T, String>;

pub(crate) async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

// ---------------------------------------------------------------------------
// Views

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArtView {
    cover: Option<String>,
    wide: Option<String>,
    hero: Option<String>,
    logo: Option<String>,
    icon: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamView {
    account: String,
    appid: u32,
    enabled: bool,
    /// Created by Ludonimbus ("Add to Steam"), so it can be removed again.
    created: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    key: String,
    name: String,
    adapter: Option<String>,
    adapter_name: Option<String>,
    platform: Option<String>,
    exe: Option<String>,
    exe_exists: bool,
    install: Option<String>,
    args: String,
    saves: Vec<String>,
    exclude: Vec<String>,
    wait_for: Vec<String>,
    custom_saves: bool,
    default_saves: Vec<String>,
    default_wait_for: Vec<String>,
    /// Effective: upload when the game process exits, not when the launcher closes.
    upload_on_game_exit: bool,
    has_game_process: bool,
    ludusavi_title: Option<String>,
    auto_sync: bool,
    steam: Option<SteamView>,
    health: steam::Health,
    art: ArtView,
    running: bool,
    last_sync_at: Option<chrono::DateTime<chrono::Utc>>,
    last_synced_device: Option<String>,
    conflict: Option<ConflictInfo>,
    last_error: Option<String>,
    pending: usize,
    last_check: Option<serde_json::Value>,
    /// Playtime over all devices (from local data; refreshed by every sync).
    playtime: playtime::Summary,
    /// Emulator / recomp / PC game…, for the library's badges and groups.
    source: games::Source,
}

fn game_view(cfg: &AppConfig, g: &GameEntry, procs: &[String]) -> GameView {
    let adapter = g.adapter.as_deref().and_then(adapters::find);
    let eff = spec::effective(g);
    let st = state::game(&g.key);
    let root = steam::root(cfg);
    // Steam's artwork for linked games, else the artwork chosen in Ludonimbus.
    let pick = |kind: &str| -> Option<String> {
        if let (Some(r), Some(l)) = (&root, &g.steam) {
            if let Some(f) = steam::art_file(r, &l.account, l.appid, kind) {
                return Some(art::url(&l.account, l.appid, kind, &f));
            }
        }
        localart::file(&g.key, kind).map(|f| art::local_url(&g.key, kind, &f))
    };
    let art = ArtView {
        cover: pick("cover"),
        wide: pick("wide"),
        hero: pick("hero"),
        logo: pick("logo"),
        icon: pick("icon"),
    };
    let names = watcher::watch_names(g);
    GameView {
        key: g.key.clone(),
        name: g.name.clone(),
        adapter: g.adapter.clone(),
        adapter_name: adapter.map(|a| a.name.clone()),
        platform: adapter.and_then(|a| a.platform_label()),
        exe: g.exe.as_ref().map(|p| p.display().to_string()),
        exe_exists: g.exe.as_ref().is_some_and(|p| p.is_file()),
        install: spec::install_dir(g).map(|p| p.display().to_string()),
        args: g.args.clone(),
        saves: eff.saves.clone(),
        exclude: g.exclude.clone(),
        wait_for: g.wait_for.clone(),
        custom_saves: !g.saves.is_empty(),
        default_saves: adapter.map(|a| a.saves.clone()).unwrap_or_default(),
        default_wait_for: adapter.map(|a| a.wait_for.clone()).unwrap_or_default(),
        upload_on_game_exit: eff.upload_on_game_exit,
        has_game_process: !eff.wait_for.is_empty(),
        ludusavi_title: g.ludusavi_title.clone(),
        auto_sync: g.auto_sync,
        steam: g.steam.as_ref().map(|l| SteamView {
            account: l.account.clone(),
            appid: l.appid,
            enabled: l.enabled,
            created: l.created,
        }),
        health: steam::health(cfg, g),
        art,
        running: watcher::runner_active(&g.key) || names.iter().any(|n| procs.contains(n)),
        last_sync_at: st.last_sync_at,
        last_synced_device: st.last_synced_device,
        conflict: st.conflict,
        last_error: st.last_error,
        pending: st.pending.len(),
        last_check: st.last_check,
        playtime: playtime::summary(cfg, &g.key),
        source: games::source(g),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudView {
    kind: String,
    label: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    device_id: String,
    device_name: String,
    cloud: CloudView,
    steam_found: bool,
    steam_running: bool,
    games: Vec<GameView>,
}

fn cloud_view(cfg: &AppConfig) -> CloudView {
    let (kind, label) = match &cfg.cloud {
        CloudConfig::None => ("none", None),
        CloudConfig::LocalFolder { path } => ("localFolder", Some(path.display().to_string())),
        CloudConfig::GoogleDrive { account, .. } => ("googleDrive", account.clone().or(Some("Google Drive".into()))),
        CloudConfig::WebDav { url, folder, .. } => ("webDav", Some(format!("{}/{}", url.trim_end_matches('/'), folder))),
        CloudConfig::S3 { bucket, prefix, .. } => ("s3", Some(format!("{bucket}/{prefix}"))),
        CloudConfig::BaiduPan { account, .. } => ("baiduPan", account.clone().or(Some(ludonimbus_core::tr!("百度网盘", "Baidu Netdisk")))),
        CloudConfig::Quark { folder } => ("quark", Some(ludonimbus_core::tr!("夸克网盘 · {folder}", "Quark · {folder}"))),
    };
    let error = match cloud::open(cfg) {
        Ok(_) => None,
        Err(e) => Some(format!("{e:#}")),
    };
    CloudView {
        kind: kind.into(),
        label,
        error,
    }
}

#[tauri::command]
pub async fn get_overview() -> CmdResult<Overview> {
    blocking(|| {
        let cfg = config::load()?;
        let procs = process::process_names();
        Ok(Overview {
            device_id: cfg.device.id.clone(),
            device_name: cfg.device.name.clone(),
            cloud: cloud_view(&cfg),
            steam_found: steam::root(&cfg).is_some(),
            steam_running: procs.iter().any(|p| p == "steam.exe"),
            games: cfg.games.iter().map(|g| game_view(&cfg, g, &procs)).collect(),
        })
    })
    .await
}

// ---------------------------------------------------------------------------
// Sync

fn with_game<T>(key: &str, f: impl FnOnce(&Ctx, &GameEntry) -> Result<T>) -> Result<T> {
    let ctx = Ctx::load()?;
    let game = ctx.game(key)?;
    f(&ctx, &game)
}

pub(crate) fn cache_check(key: &str, report: &StatusReport) {
    let v = serde_json::to_value(report).ok();
    let _ = state::update_game(key, |g| g.last_check = v);
}

#[tauri::command]
pub async fn check_game(key: String) -> CmdResult<StatusReport> {
    blocking(move || {
        with_game(&key, |ctx, g| {
            let r = sync::check(ctx, g);
            cache_check(&key, &r);
            Ok(r)
        })
    })
    .await
}

#[tauri::command]
pub async fn sync_game(key: String, resolution: String) -> CmdResult<Outcome> {
    blocking(move || {
        if watcher::runner_active(&key) {
            bail!(ludonimbus_core::tr!("游戏正在运行，退出游戏后会自动同步", "The game is running; saves sync after you quit"));
        }
        let Some(_busy) = busy::begin(&key) else {
            bail!(ludonimbus_core::tr!("正在同步，请稍候", "Syncing; please wait"));
        };
        let res = match resolution.as_str() {
            "local" => Resolution::UseLocal,
            "cloud" => Resolution::UseCloud,
            _ => Resolution::Auto,
        };
        with_game(&key, |ctx, g| sync::sync(ctx, g, res, Trigger::SyncNow))
    })
    .await
}

#[tauri::command]
pub async fn backup_game(key: String, comment: String) -> CmdResult<Outcome> {
    blocking(move || with_game(&key, |ctx, g| sync::backup_now(ctx, g, comment.trim()))).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotList {
    snapshots: Vec<SnapshotSummary>,
    head_id: Option<String>,
    last_synced_id: Option<String>,
}

#[tauri::command]
pub async fn list_snapshots(key: String) -> CmdResult<SnapshotList> {
    blocking(move || {
        with_game(&key, |ctx, _| {
            let mut list = ctx.cloud()?.list_snapshots(&key)?;
            let head = sync::mainline_head(&list).map(|h| h.id);
            list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            Ok(SnapshotList {
                snapshots: list,
                head_id: head,
                last_synced_id: state::game(&key).last_synced_id,
            })
        })
    })
    .await
}

#[tauri::command]
pub async fn get_snapshot(key: String, id: String) -> CmdResult<SnapshotMeta> {
    blocking(move || with_game(&key, |ctx, _| ctx.cloud()?.get_meta(&key, &id))).await
}

#[tauri::command]
pub async fn restore_snapshot(key: String, id: String) -> CmdResult<Outcome> {
    blocking(move || {
        if watcher::runner_active(&key) {
            bail!(ludonimbus_core::tr!("游戏正在运行，请先退出游戏再恢复存档", "The game is running. Quit it before restoring saves"));
        }
        with_game(&key, |ctx, g| {
            if process::is_running(&watcher::watch_names(g)) {
                bail!(ludonimbus_core::tr!("游戏正在运行，请先退出游戏再恢复存档", "The game is running. Quit it before restoring saves"));
            }
            sync::restore(ctx, g, &id)
        })
    })
    .await
}

#[tauri::command]
pub async fn set_snapshot(key: String, id: String, locked: Option<bool>, comment: Option<String>) -> CmdResult<()> {
    blocking(move || with_game(&key, |ctx, g| sync::set_snapshot_flags(ctx, g, &id, locked, comment))).await
}

#[tauri::command]
pub async fn delete_snapshot(key: String, id: String) -> CmdResult<()> {
    blocking(move || with_game(&key, |ctx, g| sync::delete_snapshot(ctx, g, &id))).await
}

#[tauri::command]
pub async fn export_snapshot(key: String, id: String) -> CmdResult<Option<String>> {
    blocking(move || {
        let file_name = format!("{key}-{id}.zip");
        let Some(dest) = rfd::FileDialog::new()
            .set_title(ludonimbus_core::tr!("导出存档版本", "Export save version"))
            .set_file_name(&file_name)
            .add_filter("Zip", &["zip"])
            .save_file()
        else {
            return Ok(None);
        };
        with_game(&key, |ctx, g| sync::export_snapshot(ctx, g, &id, &dest))?;
        Ok(Some(dest.display().to_string()))
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalFiles {
    files: Vec<snapshot::LocalFile>,
    roots: BTreeMap<String, String>,
}

#[tauri::command]
pub async fn local_files(key: String) -> CmdResult<LocalFiles> {
    blocking(move || {
        let cfg = config::load()?;
        let g = cfg.require_game(&key)?;
        let (files, roots) = snapshot::scan(g)?;
        let used: std::collections::HashSet<&str> = files.iter().filter_map(|f| f.logical.split('/').next()).collect();
        let mut shown: BTreeMap<String, String> = BTreeMap::new();
        for (k, v) in &roots {
            if used.contains(k.as_str()) || k == "install" {
                shown.insert(k.clone(), v.display().to_string());
            }
        }
        Ok(LocalFiles { files, roots: shown })
    })
    .await
}

#[tauri::command]
pub async fn open_path(path: String) -> CmdResult<()> {
    blocking(move || {
        let p = PathBuf::from(&path);
        if p.is_file() {
            opener::reveal(&p)?;
        } else {
            opener::open(&p)?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn open_save_folder(key: String) -> CmdResult<()> {
    blocking(move || {
        let cfg = config::load()?;
        let g = cfg.require_game(&key)?;
        let eff = spec::effective(g);
        let roots = spec::resolve_roots(g, &eff)?;
        for pattern in &eff.saves {
            let (root, rest) = spec::split_pattern(pattern)?;
            let Some(base) = spec::resolve_root_name(&root, &roots) else { continue };
            let literal: PathBuf = rest
                .split('/')
                .take_while(|c| !c.contains(['*', '?', '[', '{']))
                .fold(base, |p, c| p.join(c));
            let target = literal.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf);
            if let Some(t) = target {
                opener::open(&t)?;
                return Ok(());
            }
        }
        if let Some(title) = &eff.ludusavi_title {
            if let Some(f) = ludusavi::resolve_files(title)?.first() {
                opener::reveal(f)?;
                return Ok(());
            }
        }
        bail!(ludonimbus_core::tr!("还没有找到存档文件夹", "No save folder found yet"))
    })
    .await
}

// ---------------------------------------------------------------------------
// Library management

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsView {
    accounts: Vec<steam::Account>,
    selected: Option<String>,
    root: Option<String>,
}

#[tauri::command]
pub async fn steam_accounts() -> CmdResult<AccountsView> {
    blocking(|| {
        let cfg = config::load()?;
        let root = steam::root(&cfg);
        let accounts = root.as_deref().map(steam::accounts).unwrap_or_default();
        let selected = root.as_deref().and_then(|r| steam::default_account(&cfg, r));
        Ok(AccountsView {
            accounts,
            selected,
            root: root.map(|r| r.display().to_string()),
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutView {
    #[serde(flatten)]
    candidate: games::ShortcutCandidate,
    cover: Option<String>,
    icon: Option<String>,
}

#[tauri::command]
pub async fn steam_shortcuts(account: String) -> CmdResult<Vec<ShortcutView>> {
    blocking(move || {
        let cfg = config::load()?;
        let root = steam::root(&cfg).ok_or_else(|| anyhow!(ludonimbus_core::tr!("没有找到 Steam", "Steam not found")))?;
        Ok(games::shortcut_candidates(&cfg, &account)?
            .into_iter()
            .map(|c| {
                let a = steam::art(&root, &account, c.appid);
                ShortcutView {
                    cover: a.cover.map(|f| art::url(&account, c.appid, "cover", &f)),
                    icon: a.icon.map(|f| art::url(&account, c.appid, "icon", &f)),
                    candidate: c,
                }
            })
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn add_from_shortcut(
    account: String,
    appid: u32,
    name: Option<String>,
    adapter: Option<String>,
    saves: Vec<String>,
    key: Option<String>,
) -> CmdResult<String> {
    blocking(move || {
        let g = games::add_from_shortcut(&account, appid, games::NewGame { name, adapter, saves, key, ludusavi_title: None })?;
        Ok(g.key)
    })
    .await
}

#[tauri::command]
pub async fn add_custom(name: String, exe: Option<String>, saves: Vec<String>, ludusavi_title: Option<String>) -> CmdResult<String> {
    blocking(move || {
        let exe = exe.filter(|s| !s.trim().is_empty()).map(PathBuf::from);
        Ok(games::add_custom(name.trim(), exe, saves, ludusavi_title, None)?.key)
    })
    .await
}

#[tauri::command]
pub async fn cloud_games() -> CmdResult<Vec<CloudGame>> {
    blocking(|| {
        let ctx = Ctx::load()?;
        let list = ctx.cloud()?.list_games()?;
        Ok(list.into_iter().filter(|g| ctx.cfg.game(&g.key).is_none()).collect())
    })
    .await
}

#[tauri::command]
pub async fn add_from_cloud(key: String, account: Option<String>, appid: Option<u32>, exe: Option<String>) -> CmdResult<String> {
    blocking(move || {
        let ctx = Ctx::load()?;
        let cg = ctx
            .cloud()?
            .list_games()?
            .into_iter()
            .find(|g| g.key == key)
            .ok_or_else(|| anyhow!(ludonimbus_core::tr!("云端找不到这个游戏", "This game wasn't found in the cloud")))?;
        let g = games::add_from_cloud(&cg, account.as_deref(), appid, exe.map(PathBuf::from))?;
        Ok(g.key)
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamePatch {
    name: Option<String>,
    saves: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    wait_for: Option<Vec<String>>,
    args: Option<String>,
    auto_sync: Option<bool>,
    upload_on_game_exit: Option<bool>,
    /// Library group; empty = the detected source.
    source: Option<String>,
}

fn clean(list: Vec<String>) -> Vec<String> {
    list.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

#[tauri::command]
pub async fn update_game(key: String, patch: GamePatch) -> CmdResult<()> {
    blocking(move || {
        let saves = patch.saves.map(clean);
        let exclude = patch.exclude.map(clean);
        for p in saves.iter().flatten().chain(exclude.iter().flatten()) {
            spec::split_pattern(p)?;
        }
        games::update_game(&key, |g| {
            if let Some(n) = patch.name.filter(|n| !n.trim().is_empty()) {
                g.name = n.trim().to_string();
            }
            if let Some(s) = saves {
                if s.is_empty() && g.adapter.is_none() && g.ludusavi_title.is_none() {
                    bail!(ludonimbus_core::tr!("至少需要一个存档位置", "At least one save location is needed"));
                }
                g.saves = s;
            }
            if let Some(e) = exclude {
                g.exclude = e;
            }
            if let Some(w) = patch.wait_for {
                g.wait_for = clean(w);
            }
            if let Some(a) = patch.args {
                g.args = a;
            }
            if let Some(a) = patch.auto_sync {
                g.auto_sync = a;
            }
            if let Some(v) = patch.upload_on_game_exit {
                g.upload_on_game_exit = Some(v);
            }
            if let Some(s) = patch.source {
                g.source = Some(s.trim().to_string()).filter(|s| !s.is_empty());
            }
            Ok(())
        })?;
        // Keep the cloud copy of the definition current.
        let _ = state::update_game(&key, |s| s.cloud_def_uploaded = false);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn remove_game(key: String) -> CmdResult<()> {
    blocking(move || games::remove(&key)).await
}

#[tauri::command]
pub async fn relocate_auto(key: String) -> CmdResult<Option<String>> {
    blocking(move || {
        let cfg = config::load()?;
        let g = cfg.require_game(&key)?;
        // No exe yet (added from its saves): look for where it is installed.
        let found = if g.exe.is_none() { ludonimbus_core::discover::locate_game(&cfg, g) } else { relocate::find_candidate(g) };
        Ok(found.map(|p| p.display().to_string()))
    })
    .await
}

#[tauri::command]
pub async fn set_game_exe(key: String, path: String) -> CmdResult<()> {
    blocking(move || games::set_exe(&key, Path::new(&path)).map(|_| ())).await
}

#[tauri::command]
pub async fn pick_exe(start: Option<String>) -> CmdResult<Option<String>> {
    blocking(move || {
        let mut d = rfd::FileDialog::new()
            .set_title(ludonimbus_core::tr!("选择游戏程序", "Choose the game's program"))
            .add_filter(ludonimbus_core::tr!("程序", "Programs"), &["exe", "bat", "cmd", "lnk"]);
        if let Some(s) = start.map(PathBuf::from).and_then(|p| p.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf)) {
            d = d.set_directory(s);
        }
        Ok(d.pick_file().map(|p| p.display().to_string()))
    })
    .await
}

#[tauri::command]
pub async fn pick_folder(title: Option<String>, start: Option<String>) -> CmdResult<Option<String>> {
    blocking(move || {
        let mut d = rfd::FileDialog::new().set_title(title.unwrap_or_else(|| ludonimbus_core::tr!("选择文件夹", "Choose a folder")));
        if let Some(s) = start.map(PathBuf::from).and_then(|p| p.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf)) {
            d = d.set_directory(s);
        }
        Ok(d.pick_folder().map(|p| p.display().to_string()))
    })
    .await
}

/// Turn a folder the user picked into a portable save pattern.
#[tauri::command]
pub async fn folder_to_pattern(path: String, exe: Option<String>) -> CmdResult<String> {
    blocking(move || {
        let install = exe.as_deref().map(Path::new).and_then(Path::parent).map(Path::to_path_buf);
        let roots = spec::builtin_roots(install.as_deref());
        let logical = spec::tokenize(Path::new(&path), &roots);
        let (root, rest) = logical.split_once('/').unwrap_or((&logical, ""));
        Ok(if rest.is_empty() {
            format!("{{{root}}}/**")
        } else {
            format!("{{{root}}}/{rest}/**")
        })
    })
    .await
}

pub(crate) fn with_steam_closed<T>(close_steam: bool, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let cfg = config::load()?;
    let root = steam::root(&cfg).ok_or_else(|| anyhow!(ludonimbus_core::tr!("没有找到 Steam", "Steam not found")))?;
    let was_running = steam::is_running();
    if was_running {
        if !close_steam {
            bail!("STEAM_RUNNING");
        }
        steam::shutdown(&root)?;
    }
    let out = f();
    if was_running {
        let _ = std::process::Command::new(root.join("steam.exe")).spawn();
    }
    out
}

#[tauri::command]
pub async fn steam_set_enabled(key: String, enabled: bool, close_steam: bool) -> CmdResult<()> {
    blocking(move || {
        with_steam_closed(close_steam, || {
            if enabled {
                games::steam_enable(&key)
            } else {
                games::steam_disable(&key)
            }
        })
    })
    .await
}

#[tauri::command]
pub async fn steam_repair(key: String, close_steam: bool) -> CmdResult<()> {
    blocking(move || with_steam_closed(close_steam, || games::steam_repair(&key))).await
}

// ---------------------------------------------------------------------------
// Settings & cloud

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    device_id: String,
    device_name: String,
    retention: Retention,
    network_timeout_secs: u64,
    steam_root: Option<String>,
    /// The Steam folder was picked in Settings (not auto-detected).
    steam_root_custom: bool,
    steam_account: Option<String>,
    cloud: CloudConfig,
    /// Cloud used before the last switch whose data can still be copied.
    previous_cloud: Option<PreviousCloud>,
    google_builtin: bool,
    /// A Baidu Netdisk app is built into this release.
    baidu_builtin: bool,
    /// App name (= folder under 「我的应用数据」) of the built-in Baidu app.
    baidu_app_name: String,
    data_dir: String,
    runner_path: String,
    runner_exists: bool,
    ludusavi: Option<String>,
    version: String,
    screenshots: config::ScreenshotMode,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviousCloud {
    kind: String,
    name: String,
}

#[tauri::command]
pub async fn get_settings() -> CmdResult<SettingsView> {
    blocking(|| {
        let cfg = config::load()?;
        let root = steam::root(&cfg);
        let mut cloud = cfg.cloud.clone();
        match &mut cloud {
            CloudConfig::GoogleDrive { client_secret, .. } | CloudConfig::BaiduPan { secret_key: client_secret, .. }
                if !client_secret.is_empty() =>
            {
                *client_secret = "••••••••".into();
            }
            _ => {}
        }
        let runner = paths::runner_exe();
        Ok(SettingsView {
            device_id: cfg.device.id.clone(),
            device_name: cfg.device.name.clone(),
            retention: cfg.retention.clone(),
            network_timeout_secs: cfg.network_timeout_secs,
            steam_account: root.as_deref().and_then(|r| steam::default_account(&cfg, r)),
            steam_root_custom: cfg.steam.root.as_deref().is_some_and(|r| root.as_deref() == Some(r)),
            steam_root: root.map(|r| r.display().to_string()),
            previous_cloud: cfg.previous_cloud.as_ref().map(|p| PreviousCloud {
                kind: cloud::kind_id(p).to_string(),
                name: previous_name(p),
            }),
            cloud,
            google_builtin: oauth::GoogleClient::has_builtin(),
            baidu_builtin: cloud::baidu::BaiduClient::has_builtin(),
            baidu_app_name: cloud::baidu::BaiduClient::builtin_app_name(),
            data_dir: paths::data_dir().display().to_string(),
            runner_exists: runner.is_file(),
            runner_path: runner.display().to_string(),
            ludusavi: ludusavi::version(),
            version: ludonimbus_core::APP_VERSION.to_string(),
            screenshots: cfg.screenshots,
        })
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    device_name: Option<String>,
    keep_recent: Option<usize>,
    keep_daily_days: Option<i64>,
    network_timeout_secs: Option<u64>,
    steam_account: Option<String>,
    screenshots: Option<config::ScreenshotMode>,
}

/// First-run guide state.
#[tauri::command]
pub async fn get_onboarding() -> CmdResult<config::Onboarding> {
    blocking(|| Ok(config::load()?.onboarding)).await
}

/// The guide was finished or closed; `usage` is what the user said they use Ludonimbus for.
#[tauri::command]
pub async fn finish_onboarding(usage: Vec<String>) -> CmdResult<()> {
    blocking(move || {
        config::update(|c| {
            c.onboarding.done_at = Some(chrono::Utc::now());
            c.onboarding.usage = usage;
            Ok(())
        })
    })
    .await
}

/// The interface language picked in Settings ("system", "zh-CN" or "en").
#[tauri::command]
pub async fn get_language() -> CmdResult<String> {
    blocking(|| Ok(config::load()?.language)).await
}

#[tauri::command]
pub async fn set_language(app: tauri::AppHandle, language: String) -> CmdResult<()> {
    let language = match language.as_str() {
        "zh-CN" | "en" => language,
        _ => "system".to_string(),
    };
    blocking(move || {
        config::update(|c| {
            c.language = language.clone();
            Ok(())
        })?;
        ludonimbus_core::i18n::set(&language);
        Ok(())
    })
    .await?;
    crate::relabel_tray(&app);
    Ok(())
}

#[tauri::command]
pub async fn save_settings(patch: SettingsPatch) -> CmdResult<()> {
    blocking(move || {
        let renamed = patch.device_name.is_some();
        config::update(|c| {
            if let Some(n) = patch.device_name.filter(|n| !n.trim().is_empty()) {
                c.device.name = n.trim().to_string();
            }
            if let Some(v) = patch.keep_recent {
                c.retention.keep_recent = v.clamp(1, 500);
            }
            if let Some(v) = patch.keep_daily_days {
                c.retention.keep_daily_days = v.clamp(0, 3650);
            }
            if let Some(v) = patch.network_timeout_secs {
                c.network_timeout_secs = v.clamp(3, 120);
            }
            if let Some(a) = patch.steam_account {
                c.steam.account = Some(a);
            }
            if let Some(m) = patch.screenshots {
                c.screenshots = m;
            }
            Ok(())
        })?;
        if renamed {
            let ctx = Ctx::load()?;
            if let Ok(c) = ctx.cloud() {
                let _ = c.put_device(&cloud::device_info(&ctx.cfg));
            }
        }
        Ok(())
    })
    .await
}

fn previous_name(c: &CloudConfig) -> String {
    let detail = match c {
        CloudConfig::LocalFolder { path } => path.display().to_string(),
        CloudConfig::GoogleDrive { account, .. } => account.clone().unwrap_or_default(),
        CloudConfig::WebDav { url, .. } => url.clone(),
        CloudConfig::S3 { bucket, .. } => bucket.clone(),
        CloudConfig::BaiduPan { account, .. } => account.clone().unwrap_or_default(),
        CloudConfig::Quark { folder } => folder.clone(),
        CloudConfig::None => String::new(),
    };
    if detail.is_empty() {
        cloud::kind_name(c).to_string()
    } else {
        ludonimbus_core::tr!("{}（{detail}）", "{} ({detail})", cloud::kind_name(c))
    }
}

/// Steam install folders found on this PC.
#[tauri::command]
pub async fn steam_find_roots() -> CmdResult<Vec<String>> {
    blocking(|| Ok(steam::find_roots().iter().map(|p| p.display().to_string()).collect())).await
}

/// Use the Steam install a picked folder is in; `None` goes back to
/// detecting it. Returns the folder now in use.
#[tauri::command]
pub async fn steam_set_root(path: Option<String>) -> CmdResult<Option<String>> {
    blocking(move || {
        let root = match path.filter(|p| !p.trim().is_empty()) {
            None => None,
            Some(p) => Some(steam::root_of(Path::new(p.trim())).ok_or_else(|| {
                anyhow!(ludonimbus_core::tr!(
                    "这个文件夹里没有 Steam（找不到 steam.exe）：{p}",
                    "Steam isn't in this folder (no steam.exe): {p}"
                ))
            })?),
        };
        // The folder Steam itself registered needs no pinning: keep following it.
        let detected = steam::detect_root();
        let pinned = root.filter(|r| {
            !detected.as_ref().is_some_and(|d| util::same_path(&d.to_string_lossy(), &r.to_string_lossy()))
        });
        config::update(|c| {
            c.steam.root = pinned;
            Ok(())
        })?;
        Ok(steam::root(&config::load()?).map(|r| r.display().to_string()))
    })
    .await
}

/// Folders kept in sync by OneDrive, Dropbox, Google Drive for desktop and
/// the like, for the "local folder" cloud.
#[tauri::command]
pub async fn cloud_folder_suggestions() -> CmdResult<Vec<cloud::local::SyncFolder>> {
    blocking(|| Ok(cloud::local::sync_folders())).await
}

#[tauri::command]
pub async fn cloud_use_folder(path: String) -> CmdResult<()> {
    blocking(move || {
        let p = PathBuf::from(&path);
        std::fs::create_dir_all(&p)?;
        cloud::switch_to(CloudConfig::LocalFolder { path: p }, None)
    })
    .await
}

/// Use the saved secret when the form leaves it empty for the cloud that is
/// already connected.
fn secret_for(new: &CloudConfig, secret: &str) -> Result<Option<String>> {
    if !secret.is_empty() {
        return Ok(Some(secret.to_string()));
    }
    match cloud::secrets::load_for(new)? {
        Some(s) => Ok(Some(s)),
        None => match new {
            CloudConfig::WebDav { .. } => bail!(ludonimbus_core::tr!("请填写 WebDAV 密码", "Enter the WebDAV password")),
            CloudConfig::S3 { .. } => bail!(ludonimbus_core::tr!("请填写 Secret Key", "Enter the Secret Key")),
            _ => Ok(None),
        },
    }
}

fn check_store_config(c: &CloudConfig) -> Result<()> {
    match c {
        CloudConfig::WebDav { .. } | CloudConfig::S3 { .. } => Ok(()),
        _ => bail!(ludonimbus_core::tr!("这里只能连接 WebDAV 或 S3 兼容存储", "Only WebDAV or S3-compatible storage can be connected here")),
    }
}

/// Try WebDAV / S3 settings without saving them.
#[tauri::command]
pub async fn cloud_test(cloud: CloudConfig, secret: String) -> CmdResult<AccountInfo> {
    blocking(move || {
        check_store_config(&cloud)?;
        let secret = secret_for(&cloud, &secret)?;
        let timeout = cloud::timeout(&config::load()?).max(std::time::Duration::from_secs(15));
        cloud::probe(&cloud, secret.as_deref(), timeout)
    })
    .await
}

/// Test WebDAV / S3 settings, then switch to them.
#[tauri::command]
pub async fn cloud_connect(cloud: CloudConfig, secret: String) -> CmdResult<String> {
    blocking(move || {
        check_store_config(&cloud)?;
        let secret = secret_for(&cloud, &secret)?;
        let timeout = cloud::timeout(&config::load()?).max(std::time::Duration::from_secs(15));
        let account = cloud::probe(&cloud, secret.as_deref(), timeout)?;
        cloud::switch_to(cloud, secret.as_deref())?;
        Ok(account.label)
    })
    .await
}

/// Copy all saves from the previous cloud into the current one.
#[tauri::command]
pub async fn cloud_migrate(app: tauri::AppHandle) -> CmdResult<cloud::migrate::Report> {
    use tauri::Emitter;
    blocking(move || {
        cloud::migrate_previous(|p| {
            let _ = app.emit("cloud-migrate-progress", p.clone());
        })
    })
    .await
}

static LOGIN_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Sign in to Baidu Netdisk (device code). The code and QR code are sent to
/// the UI as `baidu-device-code` and the approval page opens in the browser.
#[tauri::command]
pub async fn cloud_connect_baidu(app: tauri::AppHandle, app_key: String, secret_key: String, app_name: String) -> CmdResult<String> {
    use std::sync::atomic::Ordering;
    use tauri::Emitter;
    LOGIN_CANCEL.store(false, Ordering::SeqCst);
    blocking(move || {
        // Keep the stored secret when the UI shows the masked placeholder.
        let secret = if secret_key.starts_with('•') {
            match config::load()?.cloud {
                CloudConfig::BaiduPan { secret_key, .. } => secret_key,
                _ => String::new(),
            }
        } else {
            secret_key
        };
        cloud::connect_baidu(
            &app_key,
            &secret,
            &app_name,
            |code| {
                let _ = app.emit("baidu-device-code", code.clone());
                let _ = opener::open_browser(code.browser_url());
            },
            &LOGIN_CANCEL,
        )
    })
    .await
}

/// What the Quark option shows before downloading OpenList.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenListInfo {
    installed: bool,
    version: &'static str,
    size: u64,
    url: String,
    asset: &'static str,
    project: &'static str,
    license: &'static str,
}

#[tauri::command]
pub async fn openlist_status() -> CmdResult<OpenListInfo> {
    use ludonimbus_core::openlist;
    Ok(OpenListInfo {
        installed: openlist::installed(),
        version: openlist::VERSION,
        size: openlist::SIZE,
        url: openlist::download_url(),
        asset: openlist::ASSET,
        project: openlist::PROJECT_URL,
        license: openlist::LICENSE,
    })
}

/// Download OpenList from GitHub (progress as `openlist-progress`), or
/// install it from a zip the user downloaded.
#[tauri::command]
pub async fn openlist_install(app: tauri::AppHandle, zip: Option<String>) -> CmdResult<()> {
    use tauri::Emitter;
    blocking(move || {
        let emit = |done: u64, total: u64| {
            let _ = app.emit("openlist-progress", serde_json::json!({ "done": done, "total": total }));
        };
        ludonimbus_core::openlist::install(zip.as_deref().map(Path::new), &emit)
    })
    .await
}

#[tauri::command]
pub async fn openlist_pick_zip() -> CmdResult<Option<String>> {
    blocking(|| {
        Ok(rfd::FileDialog::new()
            .set_title(ludonimbus_core::tr!("选择下载好的 OpenList 安装包", "Choose the downloaded OpenList package"))
            .add_filter("zip", &["zip"])
            .pick_file()
            .map(|p| p.display().to_string()))
    })
    .await
}

/// Sign in to Quark in a private window, mount it in OpenList and switch to
/// it (also used to sign in again when the sign-in expired).
#[tauri::command]
pub async fn cloud_connect_quark(app: tauri::AppHandle) -> CmdResult<String> {
    use std::sync::atomic::Ordering;
    LOGIN_CANCEL.store(false, Ordering::SeqCst);
    let cookie = crate::quark_login::sign_in(&app, &LOGIN_CANCEL).await.map_err(|e| format!("{e:#}"))?;
    blocking(move || cloud::connect_quark(&cookie)).await
}

#[tauri::command]
pub async fn cloud_cancel_login() -> CmdResult<()> {
    LOGIN_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn cloud_dismiss_previous() -> CmdResult<()> {
    blocking(cloud::dismiss_previous).await
}

#[tauri::command]
pub async fn cloud_connect_google(client_id: String, client_secret: String) -> CmdResult<String> {
    blocking(move || {
        // Keep the stored secret when the UI shows the masked placeholder.
        let current = config::load()?.cloud;
        let secret = if client_secret.starts_with('•') {
            match current {
                CloudConfig::GoogleDrive { client_secret, .. } => client_secret,
                _ => String::new(),
            }
        } else {
            client_secret
        };
        let client = oauth::GoogleClient::resolve(&client_id, &secret)
            .ok_or_else(|| anyhow!(ludonimbus_core::tr!("请先填写 Google OAuth Client ID", "Enter the Google OAuth Client ID first")))?;
        let auth = oauth::authorize(&client, std::time::Duration::from_secs(300))?;
        oauth::store_refresh_token(&auth.refresh_token)?;
        let label = auth.email.clone().unwrap_or_else(|| "Google Drive".into());
        cloud::switch_to(
            CloudConfig::GoogleDrive {
                client_id: client_id.trim().to_string(),
                client_secret: secret.trim().to_string(),
                folder: "Ludonimbus".into(),
                account: auth.email.clone(),
            },
            None,
        )?;
        Ok(label)
    })
    .await
}

#[tauri::command]
pub async fn cloud_disconnect() -> CmdResult<()> {
    blocking(cloud::disconnect).await
}

#[tauri::command]
pub async fn cloud_account() -> CmdResult<AccountInfo> {
    blocking(|| Ctx::load()?.cloud()?.account()).await
}

#[tauri::command]
pub async fn list_devices() -> CmdResult<Vec<DeviceInfo>> {
    blocking(|| {
        let ctx = Ctx::load()?;
        let c = ctx.cloud()?;
        let _ = c.put_device(&cloud::device_info(&ctx.cfg));
        let mut list = c.list_devices()?;
        list.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        Ok(list)
    })
    .await
}

#[tauri::command]
pub async fn ludusavi_search(query: String) -> CmdResult<Vec<String>> {
    blocking(move || ludusavi::search(query.trim())).await
}

#[tauri::command]
pub async fn ludusavi_preview(title: String) -> CmdResult<Vec<String>> {
    blocking(move || Ok(ludusavi::resolve_files(&title)?.iter().map(|p| p.display().to_string()).collect())).await
}

// ---------------------------------------------------------------------------
// Screenshots

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotView {
    #[serde(flatten)]
    shot: screenshots::Shot,
    /// Thumbnail (or image) already on this PC, as a `ludoart://` URL.
    thumb_url: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
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
        let (mut shots, cloud_error) = match screenshots::gallery(cloud.map(|c| c.files()), &ctx.cfg, &g) {
            Ok(s) => (s, None),
            Err(e) => (screenshots::gallery(None, &ctx.cfg, &g)?, Some(format!("{e:#}"))),
        };
        let mut names: BTreeMap<String, String> = cloud
            .and_then(|c| c.list_devices().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|d| (d.id, d.name))
            .collect();
        names.insert(ctx.cfg.device.id.clone(), ctx.cfg.device.name.clone());
        for s in &mut shots {
            s.device_name = names.get(&s.device).cloned();
        }
        let cloud_bytes = shots.iter().filter(|s| s.in_cloud).map(|s| s.size).sum();
        Ok(GalleryView {
            shots: shots
                .into_iter()
                .map(|s| ShotView {
                    thumb_url: screenshots::local_file(&ctx.cfg, &g, &s.device, &s.name, true)
                        .or_else(|| screenshots::local_file(&ctx.cfg, &g, &s.device, &s.name, false))
                        .map(|f| art::shot_url(&key, &s.device, &s.name, true, &f)),
                    shot: s,
                })
                .collect(),
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
        let file = match screenshots::local_file(&ctx.cfg, &g, &device, &name, thumb) {
            Some(f) => f,
            None => screenshots::fetch(ctx.cloud()?.files(), &ctx.cfg, &g, &device, &name, thumb)?,
        };
        Ok(art::shot_url(&key, &device, &name, thumb, &file))
    })
    .await
}

#[tauri::command]
pub async fn screenshot_sync(app: tauri::AppHandle, key: String) -> CmdResult<screenshots::Report> {
    tauri::async_runtime::spawn_blocking(move || watcher::sync_screenshots(&app, Some(key)))
        .await
        .map_err(|e| e.to_string())?
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
