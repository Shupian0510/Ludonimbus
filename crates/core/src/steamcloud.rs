//! Games installed through Steam itself (docs/plan-v2.md, section 8).
//!
//! Such a game is in the library in one of three states:
//!
//! | state | what Ludonimbus does |
//! |---|---|
//! | Steam's (the default; `auto_sync` off) | nothing |
//! | backup only ([`StoreMode::Backup`]) | Steam Cloud stays on. After the game a version of its saves is stored, and nothing is ever written into them by itself (the rule for [`GameEntry::backup_only`] in [`crate::sync`]) |
//! | managed ([`StoreMode::Managed`]) | the user turned Steam Cloud off for the game, and it syncs like any other: before playing through its launch options `"<runner>" --game <key> -- %command%`, or in the background for a game that can't start through the wrapper ([`poll`]) |
//!
//! A game Steam Cloud doesn't keep at all (Steam has noted no cloud sync of
//! it on this device, [`steam::AppCloud::used`]) is managed from the moment
//! it is added: there is nothing to take over from. Should Steam start
//! syncing it later, it goes back to backup only like any other.
//!
//! Ludonimbus never flips a Steam Cloud switch. It takes a game over only
//! once Steam's files show the switch off and Steam's last sync of the game
//! complete ([`take_over`]), and goes back to backup only by itself as soon
//! as the switch reads on again ([`recheck`]: before every sync of a managed
//! game, before each launch, and in the background).
//!
//! Steam's own cloud folder is `{steamuser}` in save paths
//! ([`crate::spec::steam_user_dir`]): each device finds it in its own Steam
//! folder.

use crate::config::{self, AppConfig, GameEntry, StoreLink, StoreMode};
use crate::games::{self, FoundChoice};
use crate::snapshot::{self, Trigger};
use crate::steam::{self, InstalledApp, WrapperHealth};
use crate::sync::{self, Ctx, Outcome, Resolution, Status};
use crate::{catchup, discover, logger, ludusavi, paths, process};
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn log(msg: impl AsRef<str>) {
    logger::log("steamcloud", msg);
}

/// Which of the three states a game installed through Steam is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Steam Cloud keeps the saves and Ludonimbus does nothing.
    Steam,
    /// Steam Cloud keeps the saves; Ludonimbus stores versions of them.
    Backup,
    /// Ludonimbus keeps the saves; Steam Cloud is off for the game.
    Managed,
}

pub fn state(game: &GameEntry) -> State {
    match &game.store {
        Some(s) if s.mode == StoreMode::Managed => State::Managed,
        Some(_) if game.auto_sync => State::Backup,
        _ => State::Steam,
    }
}

fn store_link(game: &GameEntry) -> Result<&StoreLink> {
    game.store.as_ref().ok_or_else(|| anyhow!(crate::tr!("「{}」不是从 Steam 安装的游戏", "\"{}\" isn't a game installed through Steam", game.name)))
}

fn steam_root(cfg: &AppConfig) -> Result<PathBuf> {
    steam::root(cfg).ok_or_else(|| anyhow!(crate::tr!("没有找到 Steam", "Steam not found")))
}

/// Steam's folder and the account in use on this device.
pub fn steam_account(cfg: &AppConfig) -> Result<(PathBuf, String)> {
    let root = steam_root(cfg)?;
    let account = steam::default_account(cfg, &root).ok_or_else(|| anyhow!(crate::tr!("没有找到 Steam 账号", "No Steam account found")))?;
    Ok((root, account))
}

/// The library entry for the game `appid` of `account`.
fn entry_of<'a>(cfg: &'a AppConfig, account: &str, appid: u32) -> Option<&'a GameEntry> {
    cfg.games.iter().find(|g| g.store.as_ref().is_some_and(|s| s.appid == appid && s.account == account))
}

/// Why the game can't change hands now: it is being played or synced.
fn busy(game: &GameEntry) -> Option<String> {
    if catchup::runner_phase(&game.key).is_some() || process::is_running(&catchup::watch_names(game)) {
        return Some(crate::tr!("游戏正在运行，退出游戏后再改", "The game is running; change this after you quit"));
    }
    if sync::sync_running(&game.key) {
        return Some(crate::tr!("正在同步，请稍候", "Syncing; please wait"));
    }
    None
}

// ------------------------------------------------------------- what's here

/// A game Steam installed on this device.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreApp {
    pub appid: u32,
    pub name: String,
    /// Its library entry, once it was added.
    pub key: Option<String>,
    pub state: State,
    /// How Steam's last cloud sync of it went (`synchronized`, …).
    pub sync_state: Option<String>,
    /// Its own Steam Cloud switch.
    pub cloud_enabled: bool,
    /// Steam Cloud syncs it on this device (not: the game has none).
    pub cloud_used: bool,
}

/// The games Steam installed, for the account in use, by name.
pub fn apps(cfg: &AppConfig) -> Vec<StoreApp> {
    let Ok((root, account)) = steam_account(cfg) else { return Vec::new() };
    let mut out: Vec<StoreApp> = steam::installed_apps(&root)
        .into_iter()
        .map(|a| {
            let entry = entry_of(cfg, &account, a.appid);
            let cloud = steam::app_cloud(&root, &account, a.appid);
            StoreApp {
                appid: a.appid,
                name: a.name,
                key: entry.map(|g| g.key.clone()),
                state: entry.map_or(State::Steam, state),
                sync_state: cloud.sync_state,
                cloud_enabled: cloud.enabled,
                cloud_used: cloud.used,
            }
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// How a game Steam installed stands with Steam and Ludonimbus.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreStatus {
    pub mode: State,
    pub sync_state: Option<String>,
    /// Steam's last cloud sync of the game left nothing behind.
    pub synchronized: bool,
    pub cloud_enabled: bool,
    /// Steam Cloud syncs it on this device (not: the game has none).
    pub cloud_used: bool,
    /// The account's Steam Cloud switch.
    pub global_enabled: bool,
    pub steam_running: bool,
    /// Whether its launch options still go through the wrapper; `None` when
    /// they aren't meant to.
    pub wrapper: Option<WrapperHealth>,
}

pub fn status(cfg: &AppConfig, game: &GameEntry) -> Result<StoreStatus> {
    let link = store_link(game)?;
    let root = steam::root(cfg);
    let cloud = root.as_deref().map(|r| steam::app_cloud(r, &link.account, link.appid)).unwrap_or_default();
    let wrapper = link.wrapper.then(|| {
        let options = root.as_deref().and_then(|r| steam::launch_options(r, &link.account, link.appid).ok().flatten());
        steam::wrapper_health(options.as_deref(), &game.key, &paths::runner_exe())
    });
    Ok(StoreStatus {
        mode: state(game),
        synchronized: cloud.synchronized(),
        sync_state: cloud.sync_state,
        cloud_enabled: cloud.enabled,
        cloud_used: cloud.used,
        global_enabled: cloud.global,
        steam_running: steam::is_running(),
        wrapper,
    })
}

/// For the local API, in [`steam::Health`]'s words: whether the launch
/// options of a game Steam installed still go through the wrapper ("ok",
/// "missing", "changed", "runnerMoved"). "ok" when they aren't meant to.
pub fn wrapper_word(cfg: &AppConfig, game: &GameEntry) -> &'static str {
    let Some(link) = game.store.as_ref().filter(|l| l.wrapper) else { return "ok" };
    let options = steam::root(cfg).and_then(|r| steam::launch_options(&r, &link.account, link.appid).ok().flatten());
    match steam::wrapper_health(options.as_deref(), &game.key, &paths::runner_exe()) {
        steam::WrapperHealth::Ok => "ok",
        steam::WrapperHealth::Missing => "missing",
        steam::WrapperHealth::Changed => "changed",
        steam::WrapperHealth::RunnerMoved => "runnerMoved",
    }
}

// ------------------------------------------------------------------ adding

/// What a batch did for one game.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    pub appid: u32,
    /// Its library entry, when there is one.
    pub key: Option<String>,
    pub error: Option<String>,
}

impl Done {
    fn of(appid: u32, key: Option<String>, result: Result<GameEntry>) -> Done {
        match result {
            Ok(g) => Done { appid, key: Some(g.key), error: None },
            Err(e) => Done { appid, key, error: Some(format!("{e:#}")) },
        }
    }
}

/// The Ludusavi titles of games Steam installed, by app id: by the ids in
/// Ludusavi's manifest first, then by name (one run of Ludusavi for all).
fn titles(apps: &[&InstalledApp]) -> HashMap<u32, String> {
    let ids: HashSet<u32> = apps.iter().map(|a| a.appid).collect();
    let mut out = ludusavi::titles_by_steam_id(&ids);
    let rest: Vec<&&InstalledApp> = apps.iter().filter(|a| !out.contains_key(&a.appid)).collect();
    let names: Vec<String> = rest.iter().map(|a| a.name.clone()).collect();
    if let Ok(found) = ludusavi::find_titles(&names) {
        for (a, t) in rest.iter().zip(found) {
            if let Some(t) = t {
                out.insert(a.appid, t);
            }
        }
    }
    out
}

fn not_installed(appid: u32) -> anyhow::Error {
    anyhow!(crate::tr!("Steam 里没有装这个游戏（{appid}）", "This game ({appid}) isn't installed in Steam"))
}

fn installed(root: &std::path::Path, appid: u32) -> Result<InstalledApp> {
    steam::installed_apps(root).into_iter().find(|a| a.appid == appid).ok_or_else(|| not_installed(appid))
}

/// Add the game `appid` that Steam installed for `account` to the library:
/// backup only when Steam Cloud keeps it (Steam's files say so, or the
/// caller knows, `kept_by_steam`: a scan found its saves in Steam's cloud
/// folder), managed otherwise. Where its saves are: what the user chose
/// (`choice.saves`), else a built-in rule that knows its program, else
/// Ludusavi's database (`choice.ludusavi_title`, else by its app id, then
/// by its name). Fails when none of them knows.
pub fn add(cfg: &AppConfig, account: &str, appid: u32, choice: FoundChoice, kept_by_steam: bool) -> Result<GameEntry> {
    let root = steam_root(cfg)?;
    let app = installed(&root, appid)?;
    let title = match &choice.ludusavi_title {
        Some(t) => Some(t.clone()),
        None if choice.saves.is_empty() => titles(&[&app]).remove(&appid),
        None => None,
    };
    insert(account, &app, title, choice, mode_for(&root, account, appid, kept_by_steam)).map(games::added)
}

/// The state a game Steam installed is added in: Steam Cloud keeps its
/// saves unless Steam has noted no cloud sync of it and nothing else says
/// so. Then there is nothing to take over from, and it syncs like any game
/// that isn't Steam's.
fn mode_for(root: &std::path::Path, account: &str, appid: u32, kept_by_steam: bool) -> StoreMode {
    if kept_by_steam || steam::app_cloud(root, account, appid).used {
        StoreMode::Backup
    } else {
        StoreMode::Managed
    }
}

/// Make the library entry for a game Steam installed and put it in the
/// configuration. Its program is the one in its folder that looks most like
/// the game: the background knows it is running by its name.
fn insert(account: &str, app: &InstalledApp, title: Option<String>, choice: FoundChoice, mode: StoreMode) -> Result<GameEntry> {
    let name = Some(choice.name.trim()).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| app.name.clone());
    let exe = choice.exe.clone().or_else(|| discover::pick_exe(&app.install_dir));
    let rec = exe.as_deref().filter(|_| choice.saves.is_empty()).and_then(|e| games::recognize(e, "", None));
    let saves = if choice.saves.is_empty() { rec.as_ref().map(games::Recognized::saves).unwrap_or_default() } else { choice.saves.clone() };
    let title = title.filter(|_| rec.is_none() && saves.is_empty());
    if rec.is_none() && saves.is_empty() && title.is_none() {
        bail!(crate::tr!(
            "还不知道「{name}」的存档在哪里，请先指定存档位置",
            "It's not known yet where \"{name}\" keeps its saves. Choose the save location first"
        ));
    }
    let adapter = rec.as_ref().map(|r| r.adapter);
    config::update(|cfg| {
        if let Some(g) = entry_of(cfg, account, app.appid) {
            bail!(crate::tr!("「{}」已经添加过了", "\"{}\" is already added", g.name));
        }
        if let Some(a) = adapter {
            games::check_emulator_overlap(cfg, a, &saves)?;
        }
        let base = adapter.filter(|a| !a.is_emulator()).map(|a| a.key.clone()).unwrap_or_else(|| games::slugify(&name));
        let entry = GameEntry {
            key: games::unique_key(cfg, &base),
            name: name.clone(),
            adapter: adapter.map(|a| a.key.clone()),
            exe: exe.clone(),
            // A rule says what `{install}` is; otherwise the game's folder.
            install: match &rec {
                Some(r) => r.install.clone(),
                None => Some(app.install_dir.clone()),
            },
            saves: saves.clone(),
            ludusavi_title: title.clone(),
            store: Some(StoreLink { account: account.to_string(), appid: app.appid, mode, ..StoreLink::default() }),
            auto_sync: true,
            ..Default::default()
        };
        cfg.games.push(entry.clone());
        let how = if mode == StoreMode::Managed { "managed (Steam Cloud doesn't keep it)" } else { "backup only" };
        log(format!("{}: added the Steam game {} ({}), {how}", entry.key, app.appid, entry.name));
        Ok(entry)
    })
}

/// Add several games Steam installed for the account in use (as [`add`]
/// does), with one look at Ludusavi's database for all of them.
pub fn add_many(cfg: &AppConfig, appids: &[u32]) -> Vec<Done> {
    let (root, account) = match steam_account(cfg) {
        Ok(x) => x,
        Err(e) => return appids.iter().map(|&appid| Done { appid, key: None, error: Some(format!("{e:#}")) }).collect(),
    };
    let all = steam::installed_apps(&root);
    let wanted: Vec<&InstalledApp> = all.iter().filter(|a| appids.contains(&a.appid)).collect();
    let mut titles = titles(&wanted);
    let done = appids
        .iter()
        .map(|&appid| {
            let result = match all.iter().find(|a| a.appid == appid) {
                Some(app) => insert(&account, app, titles.remove(&appid), FoundChoice::default(), mode_for(&root, &account, appid, false)),
                None => Err(not_installed(appid)),
            };
            Done::of(appid, None, result)
        })
        .collect();
    // The save locations of all of them, with one read of Ludusavi's database.
    if let Err(e) = games::refresh_ludusavi_saves(None) {
        log(format!("no save locations from Ludusavi's database yet: {e:#}"));
    }
    done
}

/// Every game Steam installed that isn't in the library yet ("all backup
/// only": nothing can go wrong with it; the ones Steam Cloud doesn't keep
/// are managed, as in [`add`]). The ones whose save location isn't known
/// are reported.
pub fn backup_all(cfg: &AppConfig) -> Vec<Done> {
    let Ok((root, account)) = steam_account(cfg) else { return Vec::new() };
    let new: Vec<u32> = steam::installed_apps(&root).iter().filter(|a| entry_of(cfg, &account, a.appid).is_none()).map(|a| a.appid).collect();
    add_many(cfg, &new)
}

// ---------------------------------------------------------- taking over

/// What the kept version of the saves before a take-over is called.
fn before_take_over() -> String {
    crate::tr!("接管前", "Before taking over from Steam Cloud")
}

/// Step 4 of [`take_over`]: the saves as they are now are a kept version in
/// the cloud. One with the same content is kept instead of uploading again.
/// Fails unless the cloud has it; nothing is changed before the cloud was
/// reached.
fn keep_before(ctx: &Ctx, game: &GameEntry) -> Result<()> {
    let local = snapshot::local_state(game)?;
    if local.is_empty() {
        log(format!("{}: no saves yet, nothing to keep before taking over", game.key));
        return Ok(());
    }
    // Reached, or nothing is changed (a version made now would wait in the
    // queue instead).
    ctx.cloud()?.list_snapshots(&game.key)?;
    match sync::backup_now(ctx, game, &before_take_over())? {
        Outcome::Uploaded { id } => {
            log(format!("{}: kept {id} before taking over", game.key));
            Ok(())
        }
        Outcome::Queued { reason, .. } => bail!(crate::tr!(
            "接管前要先把现在的存档保存到云端，但现在连不上云盘：{reason}",
            "Before taking over, the saves as they are now have to be in the cloud, and it can't be reached: {reason}"
        )),
        other => bail!("{other:?}"),
    }
}

/// Let Ludonimbus keep the saves of a game Steam installed, once the user
/// turned Steam Cloud off for it. In this order, stopping at the first step
/// that fails, with nothing changed by then:
///
/// 1. With `wrapper`, Steam is closed (`STEAM_RUNNING:`); the game isn't
///    running and isn't being synced.
/// 2. Steam's last cloud sync of the game is complete
///    (`STEAM_NOT_SYNCED:`): otherwise this device may not have the
///    newest saves yet. Not asked of a game Steam Cloud doesn't keep.
/// 3. Steam Cloud is off for the game, by its own switch or the account's
///    (`STEAM_CLOUD_ON:`). Ludonimbus never flips it.
/// 4. The saves as they are now are a kept version in the cloud ("before
///    taking over"). No cloud, no take-over.
/// 5. With `wrapper`, the launch options go through the wrapper, with the
///    user's own behind `%command%`; theirs are remembered for the
///    hand-back. Without, the game is synced in the background ([`poll`]).
/// 6. The entry is managed and syncs by itself.
pub fn take_over(ctx: &Ctx, key: &str, wrapper: bool) -> Result<GameEntry> {
    let game = ctx.game(key)?;
    let link = store_link(&game)?.clone();
    let root = steam_root(&ctx.cfg)?;
    if wrapper {
        steam::ensure_closed_for_options(&root)?;
    }
    if let Some(reason) = busy(&game) {
        bail!(reason);
    }
    let cloud = steam::app_cloud(&root, &link.account, link.appid);
    if cloud.used && !cloud.synchronized() {
        bail!(crate::tr!(
            "STEAM_NOT_SYNCED: Steam 上次没有和云端同步完「{}」。请打开 Steam 等它同步完成（必要时启动一次游戏再退出），然后再接管",
            "STEAM_NOT_SYNCED: Steam didn't finish syncing \"{}\" with Steam Cloud last time. Start Steam and let it finish (start the game once and quit if needed), then take it over",
            game.name
        ));
    }
    if cloud.on() {
        bail!(crate::tr!(
            "STEAM_CLOUD_ON: 「{}」的 Steam 云存档还开着。请在 Steam 里右键这个游戏 → 属性 → 通用，关掉「在 Steam 云中保留游戏存档」，然后再接管",
            "STEAM_CLOUD_ON: Steam Cloud is still on for \"{}\". In Steam, right-click the game → Properties → General and turn off \"Keep game saves in the Steam Cloud\", then take it over",
            game.name
        ));
    }
    let runner = paths::runner_exe();
    let mut original = link.original_options.clone();
    if wrapper {
        if !runner.is_file() {
            bail!(crate::tr!("找不到启动包装器 {}，请重新安装 Ludonimbus", "The launch wrapper {} is missing. Please reinstall Ludonimbus", runner.display()));
        }
        steam::can_set_launch_options(&root, &link.account, link.appid)?;
        let now = steam::launch_options(&root, &link.account, link.appid)?;
        // Through the wrapper already (taken over before): the user's own
        // options are behind it.
        original = match now.as_deref().and_then(steam::parse_wrapped) {
            Some(w) => original.or_else(|| Some(w.original).filter(|o| !o.is_empty())),
            None => now,
        };
    }
    keep_before(ctx, &game)?;
    if wrapper {
        let options = steam::wrapper_options(&runner, key, original.as_deref().unwrap_or_default());
        steam::set_launch_options(&root, &link.account, link.appid, Some(&options))?;
    }
    let entry = games::update_game(key, |g| {
        if let Some(s) = g.store.as_mut() {
            s.mode = StoreMode::Managed;
            if wrapper {
                s.wrapper = true;
                s.original_options = original.clone();
            }
        }
        g.auto_sync = true;
        Ok(())
    })?;
    log(format!("{key}: taken over from Steam Cloud ({})", if wrapper { "through the wrapper" } else { "without the wrapper" }));
    Ok(entry)
}

/// Put back the launch options a game Steam installed had before the
/// wrapper went in, while they still go through it for this game (changed
/// in Steam since, they are the user's and stay). Needs Steam closed then
/// (`STEAM_RUNNING:`). Without Steam on this device there is nothing to put
/// back.
pub(crate) fn give_back_options(cfg: &AppConfig, game: &GameEntry) -> Result<()> {
    let Some(link) = game.store.as_ref().filter(|s| s.wrapper) else { return Ok(()) };
    let Some(root) = steam::root(cfg) else { return Ok(()) };
    let now = steam::launch_options(&root, &link.account, link.appid)?;
    let Some(w) = now.as_deref().and_then(steam::parse_wrapped).filter(|w| w.key == game.key) else { return Ok(()) };
    let before = link.original_options.clone().or_else(|| Some(w.original).filter(|o| !o.is_empty()));
    steam::set_launch_options(&root, &link.account, link.appid, before.as_deref())?;
    log(format!("{}: launch options given back", game.key));
    Ok(())
}

/// Give a game back to Steam Cloud: the launch options it had before the
/// wrapper (Steam closed), and backup only again. The user turns Steam
/// Cloud on in Steam; should Steam then ask which saves to keep, the ones on
/// this device are the current ones.
pub fn hand_back(ctx: &Ctx, key: &str) -> Result<GameEntry> {
    let game = ctx.game(key)?;
    store_link(&game)?;
    if let Some(reason) = busy(&game) {
        bail!(reason);
    }
    give_back_options(&ctx.cfg, &game)?;
    let entry = games::update_game(key, |g| {
        if let Some(s) = g.store.as_mut() {
            s.mode = StoreMode::Backup;
            s.wrapper = false;
            s.original_options = None;
        }
        Ok(())
    })?;
    log(format!("{key}: handed back to Steam Cloud"));
    Ok(entry)
}

/// Whether Steam Cloud is still off for a game Ludonimbus manages. When it
/// reads on again, the game goes back to backup only in the configuration
/// and the changed entry is returned: from then on nothing is written into
/// its saves by itself. The wrapper stays in the launch options; it does no
/// harm there. Steam's files that can't be found count as on.
pub fn recheck(cfg: &AppConfig, game: &GameEntry) -> Result<Option<GameEntry>> {
    let Some(link) = game.store.as_ref().filter(|s| s.mode == StoreMode::Managed) else { return Ok(None) };
    let cloud = steam::root(cfg).map(|r| steam::app_cloud(&r, &link.account, link.appid)).unwrap_or_default();
    if !cloud.on() {
        return Ok(None);
    }
    log(format!("{}: Steam Cloud is on again for this game; backup only from now on", game.key));
    let entry = games::update_game(&game.key, |g| {
        if let Some(s) = g.store.as_mut() {
            s.mode = StoreMode::Backup;
        }
        Ok(())
    })?;
    Ok(Some(entry))
}

/// The entry a sync about to run goes by, after [`recheck`]: with Steam
/// Cloud on again it is backup only, also when that couldn't be written
/// down.
pub(crate) fn rechecked(cfg: &AppConfig, mut game: GameEntry) -> GameEntry {
    match recheck(cfg, &game) {
        Ok(None) => {}
        Ok(Some(_)) | Err(_) => {
            if let Some(s) = game.store.as_mut() {
                s.mode = StoreMode::Backup;
            }
        }
    }
    game
}

/// Take over every game in backup only mode at once. Refused unless Steam
/// Cloud is off for the whole account (its switch for every game); each
/// game then goes through [`take_over`], and the ones whose last Steam sync
/// wasn't complete or whose saves couldn't be kept in the cloud are
/// reported and stay as they are.
pub fn take_over_all(ctx: &Ctx, wrapper: bool) -> Result<Vec<Done>> {
    let (root, account) = steam_account(&ctx.cfg)?;
    if steam::cloud_on_for_account(&root, &account) {
        bail!(crate::tr!(
            "STEAM_CLOUD_ON: 要全部接管，请先在 Steam 的设置 → 云里关掉 Steam 云（对所有游戏）",
            "STEAM_CLOUD_ON: To take over every game, first turn Steam Cloud off for all games in Steam's Settings → Cloud"
        ));
    }
    let backed_up: Vec<(u32, String)> = ctx
        .cfg
        .games
        .iter()
        .filter(|g| state(g) == State::Backup)
        .filter_map(|g| g.store.as_ref().filter(|s| s.account == account).map(|s| (s.appid, g.key.clone())))
        .collect();
    Ok(backed_up.into_iter().map(|(appid, key)| Done::of(appid, Some(key.clone()), take_over(ctx, &key, wrapper))).collect())
}

/// Give every game back to Steam Cloud, and every wrapper out of the
/// launch options (an uninstall does this).
pub fn hand_back_all(ctx: &Ctx) -> Vec<Done> {
    let ours: Vec<(u32, String)> =
        ctx.cfg.games.iter().filter_map(|g| g.store.as_ref().filter(|s| s.mode == StoreMode::Managed || s.wrapper).map(|s| (s.appid, g.key.clone()))).collect();
    ours.into_iter().map(|(appid, key)| Done::of(appid, Some(key.clone()), hand_back(ctx, &key))).collect()
}

// -------------------------------------------------------------- background

/// What the background found looking at a game taken over without the
/// wrapper ([`poll`]).
#[derive(Debug)]
pub enum Polled {
    /// Steam Cloud reads on again: backup only from now on.
    Demoted(GameEntry),
    Nothing,
    /// The cloud couldn't be reached.
    Offline,
    Synced(Outcome),
}

/// The background's look at a managed game that doesn't start through the
/// wrapper (anti-cheat, a launcher of its own): nothing syncs before it is
/// played, so a newer version in the cloud is pulled while it isn't
/// running, and saves changed meanwhile are uploaded. Steam Cloud is checked
/// again first. One listing of the save line when there is nothing to do;
/// a conflict is left for the user.
pub fn poll(ctx: &Ctx, game: &GameEntry) -> Result<Polled> {
    if let Some(entry) = recheck(&ctx.cfg, game)? {
        return Ok(Polled::Demoted(entry));
    }
    let unwrapped = game.store.as_ref().is_some_and(|s| s.mode == StoreMode::Managed && !s.wrapper);
    if !unwrapped || !game.auto_sync || busy(game).is_some() {
        return Ok(Polled::Nothing);
    }
    match sync::check(ctx, game).status {
        Status::Offline => Ok(Polled::Offline),
        Status::CloudNewer | Status::CanRestore | Status::LocalChanged | Status::NotUploaded => {
            Ok(Polled::Synced(sync::sync(ctx, game, Resolution::Auto, Trigger::Passive)?))
        }
        _ => Ok(Polled::Nothing),
    }
}
