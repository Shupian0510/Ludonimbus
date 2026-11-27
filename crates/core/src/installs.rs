//! Versions of games installed on this device that aren't in the library:
//! games Steam installed (left to Steam Cloud until the user adds them,
//! docs/plan-v2.md section 8) and games the Xbox app installed. A game's
//! page offers the ones that are the same game ([`same_game`]): added, they
//! are versions like any other, so a save can be sent to them
//! ([`crate::transfer`]). Any of them can be linked to a game by hand.
//!
//! A game Steam installed is listed only when Ludusavi's database knows it
//! (by its app id): that is where its saves are found, and it leaves out
//! Steam's tools (Proton, redistributables). An Xbox app game is looked up
//! in the database when it is added; one the database doesn't know keeps
//! its saves in its package's containers.

use crate::cloud::CloudGame;
use crate::config::{AppConfig, GameEntry, StoreLink};
use crate::games::FoundChoice;
use crate::{discover, games, ludusavi, savelib, steam, steamcloud, util, xbox};
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

/// A version installed here that isn't in the library.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Install {
    /// `steam:<app id>` or `xbox:<folder>`.
    pub id: String,
    pub name: String,
    /// `steam` or `xbox`.
    pub store: String,
    /// The entry it would be, for its ids and its program's logo.
    #[serde(skip)]
    pub entry: GameEntry,
}

/// Versions installed here that aren't in the library, by name.
pub fn list(cfg: &AppConfig) -> Vec<Install> {
    let exes: Vec<String> = cfg.games.iter().filter_map(|g| g.exe.as_deref()).map(norm).collect();
    let mut out = steam_apps(cfg, &exes);
    out.extend(xbox_games(&exes));
    out.sort_by_key(|i| i.name.to_lowercase());
    out
}

fn norm(p: &Path) -> String {
    util::norm_path(&p.to_string_lossy())
}

/// Whether one of `exes` (normalized) is in `dir`.
fn has_program_in(exes: &[String], dir: &Path) -> bool {
    let d = format!("{}{}", norm(dir), std::path::MAIN_SEPARATOR);
    exes.iter().any(|e| e.starts_with(&d))
}

/// Games Steam installed for the account in use that no entry is for: not
/// added through Steam, and no game in the library runs a program in its
/// folder (added by hand before Ludonimbus knew Steam's games).
fn steam_apps(cfg: &AppConfig, exes: &[String]) -> Vec<Install> {
    let Ok((root, account)) = steamcloud::steam_account(cfg) else { return Vec::new() };
    let apps: Vec<steam::InstalledApp> = steam::installed_apps(&root)
        .into_iter()
        .filter(|a| !cfg.games.iter().any(|g| g.store.as_ref().is_some_and(|s| s.appid == a.appid && s.account == account)))
        .filter(|a| !has_program_in(exes, &a.install_dir))
        .collect();
    let ids: HashSet<u32> = apps.iter().map(|a| a.appid).collect();
    let mut titles = ludusavi::titles_by_steam_id(&ids);
    apps.into_iter()
        .filter_map(|a| {
            let title = titles.remove(&a.appid)?;
            let entry = GameEntry {
                name: a.name.clone(),
                ludusavi_title: Some(title),
                store: Some(StoreLink { account: account.clone(), appid: a.appid, ..StoreLink::default() }),
                ..Default::default()
            };
            Some(Install { id: format!("steam:{}", a.appid), name: a.name, store: "steam".into(), entry })
        })
        .collect()
}

/// Games the Xbox app installed that no entry runs.
fn xbox_games(exes: &[String]) -> Vec<Install> {
    discover::xbox_games()
        .into_iter()
        .filter(|f| !f.exe.as_deref().is_some_and(|e| exes.contains(&util::norm_path(e))))
        .filter(|f| !f.install.as_deref().is_some_and(|i| has_program_in(exes, Path::new(i))))
        .map(|f| {
            let entry = GameEntry { name: f.name.clone(), exe: f.exe.as_deref().map(Into::into), ..Default::default() };
            Install { id: f.id, name: f.name, store: "xbox".into(), entry }
        })
        .collect()
}

/// The installs in `list` that are versions of the game `key` belongs to:
/// they share an id ([`savelib::ids`]: Ludusavi title, Xbox package,
/// name) with one of its versions, here or in the cloud (`cloud`). None
/// for a game the user keeps apart from all others.
pub fn same_game<'a>(cfg: &AppConfig, cloud: &[CloudGame], key: &str, list: &'a [Install]) -> Vec<&'a Install> {
    if cfg.game(key).is_some_and(|g| g.game.as_deref().is_some_and(|l| l.starts_with(savelib::SOLO))) {
        return Vec::new();
    }
    let families = savelib::index(cfg, cloud);
    let Some(fam) = families.get(key) else { return Vec::new() };
    let mine = |k: &str| families.get(k) == Some(fam);
    let mut ids: HashSet<String> = HashSet::new();
    for g in cfg.games.iter().filter(|g| mine(&g.key)) {
        ids.extend(savelib::ids(g));
    }
    for c in cloud.iter().filter(|c| cfg.game(&c.key).is_none() && mine(&c.key)) {
        ids.extend(savelib::cloud_ids(c));
    }
    list.iter().filter(|i| savelib::ids(&i.entry).iter().any(|id| ids.contains(id))).collect()
}

/// Add the install `id` (from [`list`]) to the library. A game Steam
/// installed comes in as Steam's own adding does ([`steamcloud::add`]:
/// backup only while Steam Cloud keeps it, so nothing is written into its
/// saves by itself). An Xbox app game's saves are found by its title in
/// Ludusavi's database, else they are its package's containers.
pub fn add(cfg: &AppConfig, id: &str) -> Result<GameEntry> {
    if let Some(appid) = id.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok()) {
        let (_, account) = steamcloud::steam_account(cfg)?;
        return steamcloud::add(cfg, &account, appid, FoundChoice::default(), false);
    }
    if id.starts_with("xbox:") {
        let found = discover::xbox_games()
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| anyhow!(crate::tr!("Xbox 应用里没有装这个游戏了", "The Xbox app no longer has this game installed")))?;
        let title = ludusavi::find_titles(std::slice::from_ref(&found.name)).ok().and_then(|mut t| t.pop().flatten());
        let mut choice = FoundChoice::default();
        if title.is_none() && found.adapter.is_none() {
            choice.saves = vec![containers_pattern(&found).ok_or_else(|| {
                anyhow!(crate::tr!(
                    "还不知道「{}」的存档在哪里：先在 Xbox 应用里玩一次，再加入",
                    "It's not known yet where \"{}\" keeps its saves: play it once in the Xbox app, then add it",
                    found.name
                ))
            })?];
        }
        return games::add_found(&discover::Found { ludusavi_title: title, ..found }, choice);
    }
    bail!(crate::tr!("不认识的安装：{id}", "Unknown install: {id}"))
}

/// The save pattern of an Xbox app game's containers, once the Xbox app
/// made its package's data folder.
fn containers_pattern(found: &discover::Found) -> Option<String> {
    let package = xbox::package_name(Path::new(found.exe.as_deref()?))?;
    let dir = xbox::package_data_dir(&dirs::data_local_dir()?, &package)?;
    let folder = dir.file_name()?.to_string_lossy().into_owned();
    Some(format!("{{localappdata}}/Packages/{folder}/SystemAppData/wgs/**"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, name: &str, title: Option<&str>) -> GameEntry {
        GameEntry { key: key.into(), name: name.into(), ludusavi_title: title.map(str::to_string), ..Default::default() }
    }

    fn install(id: &str, name: &str, title: Option<&str>) -> Install {
        Install { id: id.into(), name: name.into(), store: "steam".into(), entry: GameEntry { store: Some(StoreLink::default()), ..entry("", name, title) } }
    }

    #[test]
    fn offers_the_installs_that_are_the_same_game() {
        let cfg = AppConfig { games: vec![entry("densh", "Denshattack!", Some("Denshattack!")), entry("other", "Hades", Some("Hades"))], ..Default::default() };
        let list = vec![
            install("steam:1", "Denshattack!", Some("Denshattack!")),
            // Another name in this store, the same title in Ludusavi's database.
            install("steam:2", "電車アタック", Some("Denshattack!")),
            install("steam:3", "Celeste", Some("Celeste")),
        ];
        let ids: Vec<&str> = same_game(&cfg, &[], "densh", &list).iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["steam:1", "steam:2"]);
        assert!(same_game(&cfg, &[], "other", &list).is_empty());
        assert!(same_game(&cfg, &[], "missing", &list).is_empty());

        // Kept apart from every other version: nothing offered.
        let mut solo = cfg.clone();
        solo.games[0].game = Some(format!("{}densh", savelib::SOLO));
        assert!(same_game(&solo, &[], "densh", &list).is_empty());
    }

    #[test]
    fn tells_programs_in_a_folder() {
        let exes = vec![norm(Path::new("C:/Games/Steam/common/Game/bin/game.exe"))];
        assert!(has_program_in(&exes, Path::new("C:/Games/Steam/common/Game")));
        assert!(!has_program_in(&exes, Path::new("C:/Games/Steam/common/Gam")));
    }
}
