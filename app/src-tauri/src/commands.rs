//! Commands invoked by the web UI. All heavy work runs on blocking threads.

use crate::{art, watcher};
use anyhow::{anyhow, bail, Result};
use ludonimbus_core::cloud::{self, oauth, AccountInfo, CloudGame, DeviceInfo};
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry, Retention};
use ludonimbus_core::snapshot::{self, SnapshotMeta, SnapshotSummary, Trigger};
use ludonimbus_core::state::{self, ConflictInfo};
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution, StatusReport};
use ludonimbus_core::{adapters, games, ludusavi, paths, process, relocate, spec, steam};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type CmdResult<T> = std::result::Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> CmdResult<T> {
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
}

fn game_view(cfg: &AppConfig, g: &GameEntry, procs: &[String]) -> GameView {
    let adapter = g.adapter.as_deref().and_then(adapters::find);
    let eff = spec::effective(g);
    let st = state::game(&g.key);
    let root = steam::root(cfg);
    let art = match (&root, &g.steam) {
        (Some(r), Some(l)) => {
            let a = steam::art(r, &l.account, l.appid);
            let u = |p: &Option<PathBuf>, kind: &str| p.as_ref().map(|_| art::url(&l.account, l.appid, kind));
            ArtView {
                cover: u(&a.cover, "cover"),
                hero: u(&a.hero, "hero"),
                logo: u(&a.logo, "logo"),
                icon: u(&a.icon, "icon"),
            }
        }
        _ => ArtView::default(),
    };
    let names = watcher::watch_names(g);
    GameView {
        key: g.key.clone(),
        name: g.name.clone(),
        adapter: g.adapter.clone(),
        adapter_name: adapter.map(|a| a.name.clone()),
        platform: adapter.and_then(|a| a.platform.clone()),
        exe: g.exe.as_ref().map(|p| p.display().to_string()),
        exe_exists: g.exe.as_ref().is_some_and(|p| p.is_file()),
        install: spec::install_dir(g).map(|p| p.display().to_string()),
        args: g.args.clone(),
        saves: eff.saves.clone(),
        exclude: g.exclude.clone(),
        wait_for: g.wait_for.clone(),
        custom_saves: !g.saves.is_empty(),
        default_saves: adapter.map(|a| a.saves.clone()).unwrap_or_default(),
        ludusavi_title: g.ludusavi_title.clone(),
        auto_sync: g.auto_sync,
        steam: g.steam.as_ref().map(|l| SteamView {
            account: l.account.clone(),
            appid: l.appid,
            enabled: l.enabled,
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

fn cache_check(key: &str, report: &StatusReport) {
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
            bail!("游戏正在运行，退出游戏后会自动同步");
        }
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
            bail!("游戏正在运行，请先退出游戏再恢复存档");
        }
        with_game(&key, |ctx, g| {
            if process::is_running(&watcher::watch_names(g)) {
                bail!("游戏正在运行，请先退出游戏再恢复存档");
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
            .set_title("导出存档版本")
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
        bail!("还没有找到存档文件夹")
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
        let root = steam::root(&cfg).ok_or_else(|| anyhow!("没有找到 Steam"))?;
        Ok(games::shortcut_candidates(&cfg, &account)?
            .into_iter()
            .map(|c| {
                let a = steam::art(&root, &account, c.appid);
                ShortcutView {
                    cover: a.cover.map(|_| art::url(&account, c.appid, "cover")),
                    icon: a.icon.map(|_| art::url(&account, c.appid, "icon")),
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
        let g = games::add_from_shortcut(&account, appid, games::NewGame { name, adapter, saves, key })?;
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
            .ok_or_else(|| anyhow!("云端找不到这个游戏"))?;
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
                    bail!("至少需要一个存档位置");
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
        Ok(relocate::find_candidate(g).map(|p| p.display().to_string()))
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
            .set_title("选择游戏程序")
            .add_filter("程序", &["exe", "bat", "cmd", "lnk"]);
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
        let mut d = rfd::FileDialog::new().set_title(title.unwrap_or_else(|| "选择文件夹".into()));
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

fn with_steam_closed<T>(close_steam: bool, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let cfg = config::load()?;
    let root = steam::root(&cfg).ok_or_else(|| anyhow!("没有找到 Steam"))?;
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
    steam_account: Option<String>,
    cloud: CloudConfig,
    google_builtin: bool,
    data_dir: String,
    runner_path: String,
    runner_exists: bool,
    ludusavi: Option<String>,
    version: String,
}

#[tauri::command]
pub async fn get_settings() -> CmdResult<SettingsView> {
    blocking(|| {
        let cfg = config::load()?;
        let root = steam::root(&cfg);
        let mut cloud = cfg.cloud.clone();
        if let CloudConfig::GoogleDrive { client_secret, .. } = &mut cloud {
            if !client_secret.is_empty() {
                *client_secret = "••••••••".into();
            }
        }
        let runner = paths::runner_exe();
        Ok(SettingsView {
            device_id: cfg.device.id.clone(),
            device_name: cfg.device.name.clone(),
            retention: cfg.retention.clone(),
            network_timeout_secs: cfg.network_timeout_secs,
            steam_account: root.as_deref().and_then(|r| steam::default_account(&cfg, r)),
            steam_root: root.map(|r| r.display().to_string()),
            cloud,
            google_builtin: oauth::GoogleClient::has_builtin(),
            data_dir: paths::data_dir().display().to_string(),
            runner_exists: runner.is_file(),
            runner_path: runner.display().to_string(),
            ludusavi: ludusavi::version(),
            version: ludonimbus_core::APP_VERSION.to_string(),
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

#[tauri::command]
pub async fn cloud_use_folder(path: String) -> CmdResult<()> {
    blocking(move || {
        let p = PathBuf::from(&path);
        std::fs::create_dir_all(&p)?;
        config::update(|c| {
            c.cloud = CloudConfig::LocalFolder { path: p.clone() };
            Ok(())
        })?;
        reset_cloud_markers()
    })
    .await
}

fn reset_cloud_markers() -> Result<()> {
    state::update(|s| {
        for g in s.games.values_mut() {
            g.cloud_def_uploaded = false;
            g.last_check = None;
        }
    })
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
            .ok_or_else(|| anyhow!("请先填写 Google OAuth Client ID"))?;
        let auth = oauth::authorize(&client, std::time::Duration::from_secs(300))?;
        oauth::store_refresh_token(&auth.refresh_token)?;
        let label = auth.email.clone().unwrap_or_else(|| "Google Drive".into());
        config::update(|c| {
            c.cloud = CloudConfig::GoogleDrive {
                client_id: client_id.trim().to_string(),
                client_secret: secret.trim().to_string(),
                folder: "Ludonimbus".into(),
                account: auth.email.clone(),
            };
            Ok(())
        })?;
        reset_cloud_markers()?;
        Ok(label)
    })
    .await
}

#[tauri::command]
pub async fn cloud_disconnect() -> CmdResult<()> {
    blocking(|| {
        let _ = oauth::clear_refresh_token();
        config::update(|c| {
            c.cloud = CloudConfig::None;
            Ok(())
        })?;
        reset_cloud_markers()
    })
    .await
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
