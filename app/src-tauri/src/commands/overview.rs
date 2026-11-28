//! The library page: what each game's card shows (also for the games only
//! the cloud has), how platforms and playtime are named, and the
//! collection's shelves and totals.

use super::{blocking, CmdResult};
use crate::{art, watcher};
use ludonimbus_core::cloud::{self, CloudGame};
use ludonimbus_core::config::{self, AppConfig, CloudConfig, CloudKind, GameEntry};
use ludonimbus_core::state::{self, ConflictInfo};
use ludonimbus_core::sync::{self, Ctx};
use ludonimbus_core::{adapters, games, localart, logos, pause, playtime, process, savelib, spec, steam, steamcloud};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use ts_rs::TS;

#[derive(Serialize, Default, TS)]
#[serde(rename_all = "camelCase")]
pub struct ArtView {
    cover: Option<String>,
    wide: Option<String>,
    hero: Option<String>,
    logo: Option<String>,
    icon: Option<String>,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SteamView {
    account: String,
    appid: u32,
    enabled: bool,
    /// Created by Ludonimbus ("Add to Steam"), so it can be removed again.
    created: bool,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    key: String,
    name: String,
    adapter: Option<String>,
    adapter_name: Option<String>,
    platform: Option<String>,
    exe: Option<String>,
    exe_exists: bool,
    /// The game itself is on this PC; otherwise the library shows it grey,
    /// as saves only (see `games::playable`).
    playable: bool,
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
    /// The last check of the game, as `check_game` returned it (kept as JSON
    /// in the game's state, `saves::cache_check`).
    #[ts(as = "Option<sync::StatusReport>")]
    last_check: Option<serde_json::Value>,
    /// Playtime over all devices (from local data; refreshed by every sync).
    playtime: PlaytimeView,
    /// Emulator / recomp / PC game…, for the library's badges and groups.
    source: games::Source,
    /// Which game this is a version of: the key of the first of its
    /// versions in the save library (see `savelib::families`).
    family: String,
    /// The user's "same game" link (`solo:…` = kept apart).
    link: Option<String>,
    /// It shares the save line of another version through matched folders
    /// (`join`), which the user can take back.
    joined: bool,
    /// It reads and writes that line through the Xbox app's save containers
    /// (`bind::Codec::Wgs`): writing into them is experimental, and the app
    /// says so before it does.
    xbox_containers: bool,
    /// The last time saves of the cloud replaced the ones here, while that
    /// can still be undone (`sync::undo`).
    replaced: Option<ReplacedView>,
    /// For a game Steam installed: who keeps its saves.
    store: Option<StoreView>,
    /// The collection's shelves its card is on (`finished`, `archived`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<_>", optional)]
    shelves: Vec<String>,
    /// Newer saves of its own line come here by themselves (through Steam,
    /// or Steam Cloud): a newer version in the cloud needs nothing from the
    /// user ([`auto_pull`]).
    auto_pull: bool,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StoreView {
    appid: u32,
    mode: steamcloud::State,
    /// Its launch options are meant to go through the wrapper.
    wrapper: bool,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReplacedView {
    /// Whose saves they were replaced with.
    device: String,
    at: chrono::DateTime<chrono::Utc>,
}

fn game_view(cfg: &AppConfig, g: &GameEntry, procs: &[String], families: &HashMap<String, String>, shelves: &BTreeMap<String, Vec<String>>) -> GameView {
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
    let art = ArtView { cover: pick("cover"), wide: pick("wide"), hero: pick("hero"), logo: pick("logo"), icon: pick("icon") };
    let names = watcher::watch_names(g);
    GameView {
        key: g.key.clone(),
        name: g.name.clone(),
        adapter: g.adapter.clone(),
        adapter_name: adapter.map(|a| a.name.clone()),
        platform: adapter.and_then(|a| a.platform_label()),
        exe: g.exe.as_ref().map(|p| p.display().to_string()),
        exe_exists: g.exe.as_ref().is_some_and(|p| p.is_file()),
        playable: games::playable(g),
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
        steam: g.steam.as_ref().map(|l| SteamView { account: l.account.clone(), appid: l.appid, enabled: l.enabled, created: l.created }),
        health: steam::health(cfg, g),
        art,
        running: watcher::runner_active(&g.key) || names.iter().any(|n| procs.contains(n)),
        replaced: st
            .replaced
            .as_ref()
            .filter(|r| st.last_synced_id.as_deref() == Some(r.now.as_str()))
            .map(|r| ReplacedView { device: r.device.clone(), at: r.at }),
        last_sync_at: st.last_sync_at,
        last_synced_device: st.last_synced_device,
        conflict: st.conflict,
        last_error: st.last_error,
        pending: st.pending.len(),
        last_check: st.last_check,
        playtime: playtime_view(cfg, playtime::summary(cfg, &g.key)),
        source: games::Source { logo: logos::for_game(g, adapter).and_then(|f| art::logo_url(&f)), ..games::source(g) },
        family: families.get(&g.key).cloned().unwrap_or_else(|| g.key.clone()),
        shelves: ludonimbus_core::shelf::of(cfg, shelves, &g.key),
        link: g.game.clone(),
        joined: !g.bind.is_empty(),
        xbox_containers: ludonimbus_core::bind::has_wgs(&g.bind),
        store: g.store.as_ref().map(|s| StoreView { appid: s.appid, mode: steamcloud::state(g), wrapper: s.wrapper }),
        auto_pull: auto_pull(g),
    }
}

/// Newer saves of the game's own line come here without the user: through
/// Steam ([`sync::pulls_by_itself`]), or through Steam Cloud for a game
/// kept backup only. Any other game waits for the user to take them.
pub(crate) fn auto_pull(g: &GameEntry) -> bool {
    sync::pulls_by_itself(g) || (g.auto_sync && g.backup_only())
}

/// A game the cloud has but this PC doesn't: shown grey in the library.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CloudOnlyView {
    key: String,
    name: String,
    adapter: Option<String>,
    adapter_name: Option<String>,
    platform: Option<String>,
    ludusavi_title: Option<String>,
    saves: Vec<String>,
    source: games::Source,
    /// Artwork downloaded from the cloud (see `artsync`).
    art: ArtView,
    /// From this PC's copy of the other devices' playtime.
    playtime: PlaytimeView,
    family: String,
    link: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<_>", optional)]
    shelves: Vec<String>,
}

pub(crate) fn cloud_only_views(
    cfg: &AppConfig,
    cloud: &[CloudGame],
    families: &HashMap<String, String>,
    shelves: &BTreeMap<String, Vec<String>>,
) -> Vec<CloudOnlyView> {
    cloud
        .iter()
        .filter(|c| cfg.game(&c.key).is_none())
        .map(|c| {
            let g = savelib::entry_of(c);
            let adapter = c.adapter.as_deref().and_then(adapters::find);
            let pick = |kind: &str| localart::file(&c.key, kind).map(|f| art::local_url(&c.key, kind, &f));
            CloudOnlyView {
                key: c.key.clone(),
                name: c.name.clone(),
                adapter: c.adapter.clone(),
                adapter_name: adapter.map(|a| a.name.clone()),
                platform: adapter.and_then(|a| a.platform_label()),
                ludusavi_title: c.ludusavi_title.clone(),
                saves: c.saves.clone(),
                source: games::Source { logo: logos::for_game(&g, adapter).and_then(|f| art::logo_url(&f)), ..games::source(&g) },
                art: ArtView { cover: pick("cover"), wide: pick("wide"), hero: pick("hero"), logo: pick("logo"), icon: pick("icon") },
                playtime: playtime_view(cfg, playtime::summary(cfg, &c.key)),
                family: families.get(&c.key).cloned().unwrap_or_else(|| c.key.clone()),
                link: c.game.clone(),
                shelves: ludonimbus_core::shelf::of(cfg, shelves, &c.key),
            }
        })
        .collect()
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CloudView {
    kind: CloudKind,
    label: Option<String>,
    error: Option<String>,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    device_id: String,
    device_name: String,
    cloud: CloudView,
    steam_found: bool,
    steam_running: bool,
    games: Vec<GameView>,
    /// Games the cloud has but this PC doesn't, from the copy of the cloud's
    /// list kept on this PC (refreshed by `refresh_cloud_library`).
    cloud_only: Vec<CloudOnlyView>,
    cloud_fetched_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Syncing is paused on this device (from the Steam Deck's panel or the
    /// local API, see `ludonimbus_core::pause`).
    paused: Option<pause::Pause>,
}

fn cloud_view(cfg: &AppConfig) -> CloudView {
    let label = match &cfg.cloud {
        CloudConfig::None => None,
        CloudConfig::LocalFolder { path } => Some(path.display().to_string()),
        CloudConfig::GoogleDrive { account, .. } => account.clone().or(Some("Google Drive".into())),
        CloudConfig::WebDav { url, folder, .. } => Some(format!("{}/{}", url.trim_end_matches('/'), folder)),
        CloudConfig::S3 { bucket, prefix, .. } => Some(format!("{bucket}/{prefix}")),
        CloudConfig::BaiduPan { account, .. } => account.clone().or(Some(ludonimbus_core::tr!("百度网盘", "Baidu Netdisk"))),
        CloudConfig::Quark { folder } => Some(ludonimbus_core::tr!("夸克网盘 · {folder}", "Quark · {folder}")),
    };
    let error = match cloud::open(cfg) {
        Ok(_) => None,
        Err(e) => Some(format!("{e:#}")),
    };
    CloudView { kind: cfg.cloud.kind(), label, error }
}

#[tauri::command]
pub async fn get_overview() -> CmdResult<Overview> {
    blocking(|| {
        let cfg = config::load()?;
        let procs = process::process_names();
        let cached = savelib::cached(&cfg);
        let families = savelib::index(&cfg, &cached.games);
        Ok(Overview {
            device_id: cfg.device.id.clone(),
            device_name: cfg.device.name.clone(),
            cloud: cloud_view(&cfg),
            steam_found: steam::root(&cfg).is_some(),
            steam_running: procs.iter().any(|p| p == steam::PROCESS),
            // An entry for Ludonimbus itself (added before scans left it out) stays hidden.
            games: cfg.games.iter().filter(|g| !games::is_ludonimbus(g)).map(|g| game_view(&cfg, g, &procs, &families, &cached.tags)).collect(),
            cloud_only: cloud_only_views(&cfg, &cached.games, &families, &cached.tags),
            cloud_fetched_at: cached.fetched_at,
            paused: pause::get(),
        })
    })
    .await
}

/// Pause syncing on this device, or go on (the backlog then goes up, see
/// the watcher).
#[tauri::command]
pub async fn set_sync_paused(paused: bool) -> CmdResult<Option<pause::Pause>> {
    blocking(move || {
        if paused {
            pause::pause()?;
        } else {
            pause::resume()?;
        }
        Ok(pause::get())
    })
    .await
}

/// How a version's platform ([`sync::platform`]) is shown: a name, and the
/// logo of the program when a game of this device runs on the same thing
/// (logos come from the programs installed here, never from Ludonimbus).
#[derive(Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlatformMark {
    pub(crate) label: String,
    logo: Option<String>,
}

fn platform_mark(cfg: &config::AppConfig, platform: &str) -> PlatformMark {
    let (kind, rule) = platform.split_once(':').unwrap_or((platform, ""));
    let label = match (kind, rule) {
        ("emu" | "recomp", r) => adapters::find(r).map(|a| a.name.clone()).unwrap_or_else(|| r.to_string()),
        ("console", "switch") => "Switch".to_string(),
        ("pc", "xbox") => "Xbox".to_string(),
        ("pc", "steam") => "Steam".to_string(),
        ("pc", _) => "PC".to_string(),
        _ => platform.to_string(),
    };
    let logo = cfg
        .games
        .iter()
        .filter(|g| sync::platform(g) == platform)
        .find_map(|g| logos::for_game(g, g.adapter.as_deref().and_then(adapters::find)))
        .and_then(|f| art::logo_url(&f));
    PlatformMark { label, logo }
}

/// A game's playtime as the app shows it: the summary, and what each
/// platform it was played on is called.
#[derive(Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "PlaytimeSummary")]
pub struct PlaytimeView {
    #[serde(flatten)]
    summary: playtime::Summary,
    /// Platform ([`playtime::PlatformTotal::platform`]) → its name.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[ts(as = "Option<_>", optional)]
    platform_names: BTreeMap<String, String>,
}

pub(crate) fn playtime_view(cfg: &config::AppConfig, summary: playtime::Summary) -> PlaytimeView {
    let platform_names = summary.platforms.iter().map(|p| (p.platform.clone(), platform_mark(cfg, &p.platform).label)).collect();
    PlaytimeView { summary, platform_names }
}

pub(crate) fn platform_marks<'a>(cfg: &config::AppConfig, platforms: impl IntoIterator<Item = &'a str>) -> BTreeMap<String, PlatformMark> {
    let mut out = BTreeMap::new();
    for p in platforms {
        if !p.is_empty() && !out.contains_key(p) {
            out.insert(p.to_string(), platform_mark(cfg, p));
        }
    }
    out
}

/// Put the versions `keys` of one game on the collection's shelf `tag`
/// (`finished`, `archived`), or take them off it.
#[tauri::command]
pub async fn set_shelf(keys: Vec<String>, tag: String, on: bool) -> CmdResult<()> {
    blocking(move || {
        let ctx = Ctx::load()?;
        ludonimbus_core::shelf::set(&ctx, &keys, &tag, on)?;
        Ok(())
    })
    .await
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectionStats {
    /// Achievements unlocked, and how many the games have, over the games
    /// of this PC whose achievements are known.
    achievements_unlocked: usize,
    achievements_total: usize,
}

/// What the collection page adds up that the library's views don't carry.
/// From this PC's files; no request.
#[tauri::command]
pub async fn collection_stats() -> CmdResult<CollectionStats> {
    blocking(|| {
        let cfg = config::load()?;
        let (mut unlocked, mut total) = (0, 0);
        for g in &cfg.games {
            let db = ludonimbus_core::achievements::load_db(&g.key);
            let Some(schema) = db.schema.as_ref().filter(|s| !s.achievements.is_empty()) else { continue };
            total += schema.achievements.len();
            unlocked += schema.achievements.iter().filter(|a| db.unlocks.contains_key(&a.id)).count();
        }
        Ok(CollectionStats { achievements_unlocked: unlocked, achievements_total: total })
    })
    .await
}
