//! The save library: games as the user thinks of them, whatever they run on
//! (docs/plan-phase5.md §5).
//!
//! A library entry is one version of a game (its Steam, Xbox or emulator
//! version…) with its own saves in the cloud (`games/<key>/`). Entries that
//! are the same game share an id ([`ids`]) or a link the user made
//! ([`GameEntry::game`]): the library shows them as one game, and saves can
//! be applied from one version to another ([`crate::transfer`]). Games the
//! cloud has but this device doesn't come from a local copy of the cloud's
//! game list ([`cached`]), so the library can show them offline too.

use crate::cloud::{Cloud, CloudGame};
use crate::config::{self, AppConfig, CloudConfig, GameEntry};
use crate::{adapters, crosssaves, discover, games, paths, state, util, xbox};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A [`GameEntry::game`] value that keeps an entry apart from all others.
pub const SOLO: &str = "solo:";

/// Which versions can use each other's saves: only versions with the same
/// save format are one game in the library. PC versions (Steam, Epic, GOG,
/// the Xbox app, hand-added games) are `pc`; an emulator's games go by the
/// console it emulates (Ryujinx, Eden… and the Switch itself are all
/// `switch`); a recompilation or port is alone. A game's Wii U and Switch
/// versions are two games here: their saves don't carry over. Games whose
/// saves are the same on several platforms have a rule in
/// `rules/games/` ([`crosssaves`]), which joins their versions
/// across scopes.
pub fn save_scope(g: &GameEntry) -> String {
    match g.adapter.as_deref().and_then(adapters::find) {
        // `rom`: RetroArch, many consoles under one rule.
        Some(a) if a.is_emulator() => match a.game_id.as_deref() {
            Some(id) if id != "rom" => id.to_string(),
            _ => a.key.clone(),
        },
        Some(a) => a.key.clone(),
        None if games::switch_console(&g.saves) => "switch".into(),
        None => "pc".into(),
    }
}

/// What identifies a game among versions with the same save format
/// ([`save_scope`]): its Switch title id, its title in Ludusavi's database
/// (the same for its Steam, Epic, GOG and Xbox versions), its Xbox package,
/// and its name. Entries sharing one are versions of the same game. A
/// version a [`crosssaves`] rule covers also gets `fmt:<rule>`, which its
/// versions on other platforms share.
pub fn ids(g: &GameEntry) -> Vec<String> {
    let mut out = Vec::new();
    let scope = save_scope(g);
    push_common(&mut out, &g.saves, g.ludusavi_title.as_deref(), &g.name, &scope);
    if let Some(p) = g.exe.as_deref().and_then(xbox_package) {
        out.push(format!("xbox:{}", p.to_lowercase()));
    }
    push_format(&mut out, &scope);
    out
}

/// [`ids`] of a game known from the cloud: the ones its device worked out
/// (an Xbox package can only be read there), with the rest worked out again
/// from the definition (another version of Ludonimbus may have made them
/// differently, or known other rules).
pub fn cloud_ids(cg: &CloudGame) -> Vec<String> {
    let mut out: Vec<String> = cg.ids.iter().filter(|i| !i.starts_with("name:") && !i.starts_with("fmt:")).cloned().collect();
    let mut again = Vec::new();
    let scope = save_scope(&entry_of(cg));
    push_common(&mut again, &cg.saves, cg.ludusavi_title.as_deref(), &cg.name, &scope);
    for id in again {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    push_format(&mut out, &scope);
    out
}

/// `fmt:<rule>` when a cross-platform rule covers a version with these ids.
fn push_format(out: &mut Vec<String>, scope: &str) {
    if let Some(r) = crosssaves::rule_for_ids(out, scope) {
        out.push(format!("fmt:{}", r.id));
    }
}

fn push_common(out: &mut Vec<String>, saves: &[String], ludusavi: Option<&str>, name: &str, scope: &str) {
    if let Some(t) = games::switch_title(saves) {
        out.push(format!("nx:{t:016X}"));
    }
    if let Some(t) = ludusavi.map(str::trim).filter(|t| !t.is_empty() && scope == "pc") {
        out.push(format!("ludusavi:{}", t.to_lowercase()));
    }
    // Short names ("Ys", "Rez") match too much.
    let k = discover::match_key(name);
    if k.chars().count() >= 3 {
        out.push(format!("name:{scope}:{k}"));
    }
}

/// The package name of an Xbox app game (`BethesdaSoftworks.ProjectGold`),
/// from the `appxmanifest.xml` next to its program.
fn xbox_package(exe: &Path) -> Option<String> {
    let dir = xbox::content_dir(exe)?;
    let text = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case("appxmanifest.xml"))
        .and_then(|e| std::fs::read_to_string(e.path()).ok())?;
    xbox::xml_attr(&text, "Identity", "Name").filter(|n| !n.is_empty())
}

/// One library entry for [`families`]: a game on this device or one only
/// the cloud has.
pub struct Member<'a> {
    pub key: &'a str,
    pub ids: Vec<String>,
    pub game: Option<&'a str>,
}

impl<'a> Member<'a> {
    pub fn local(g: &'a GameEntry) -> Self {
        Member { key: &g.key, ids: ids(g), game: g.game.as_deref() }
    }
    pub fn cloud(cg: &'a CloudGame) -> Self {
        Member { key: &cg.key, ids: cloud_ids(cg), game: cg.game.as_deref() }
    }
}

/// Which game each entry is a version of: key → the smallest key among the
/// versions of that game. Entries that share an id or a link are the same
/// game; an entry the user kept apart (`solo:…`) is only itself.
pub fn families(members: &[Member]) -> HashMap<String, String> {
    let mut parent: Vec<usize> = (0..members.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut i = i;
        while p[i] != r {
            let next = p[i];
            p[i] = r;
            i = next;
        }
        r
    }
    let mut first: HashMap<String, usize> = HashMap::new();
    for (i, m) in members.iter().enumerate() {
        let solo = m.game.is_some_and(|g| g.starts_with(SOLO));
        let keys: Vec<String> = match m.game {
            Some(g) if solo => vec![format!("game:{g}")],
            Some(g) => std::iter::once(format!("game:{g}")).chain(m.ids.iter().cloned()).collect(),
            None => m.ids.clone(),
        };
        for k in keys {
            match first.get(&k) {
                Some(&j) => {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    parent[a] = b;
                }
                None => {
                    first.insert(k, i);
                }
            }
        }
    }
    let mut root_key: HashMap<usize, &str> = HashMap::new();
    for (i, m) in members.iter().enumerate() {
        let r = find(&mut parent, i);
        root_key.entry(r).and_modify(|cur| *cur = (*cur).min(m.key)).or_insert(m.key);
    }
    (0..members.len())
        .map(|i| {
            let r = find(&mut parent, i);
            (members[i].key.to_string(), root_key[&r].to_string())
        })
        .collect()
}

/// Families of everything in the library: this device's games and the
/// cloud's games this device doesn't have.
pub fn index(cfg: &AppConfig, cloud: &[CloudGame]) -> HashMap<String, String> {
    let mut members: Vec<Member> = cfg.games.iter().map(Member::local).collect();
    members.extend(cloud.iter().filter(|c| cfg.game(&c.key).is_none()).map(Member::cloud));
    families(&members)
}

// ---------------------------------------------------------------------------
// The cloud's game list, kept on this device

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Cached {
    pub fetched_at: Option<DateTime<Utc>>,
    /// The cloud the list is from; a list from another cloud is ignored.
    pub scope: String,
    pub games: Vec<CloudGame>,
}

fn cache_file() -> PathBuf {
    paths::data_dir().join("cloud-games.json")
}

fn scope(cloud: &CloudConfig) -> String {
    serde_json::to_string(cloud).unwrap_or_default()
}

/// The cloud's game list as last fetched (empty without a cloud or when it
/// was fetched from another cloud).
pub fn cached(cfg: &AppConfig) -> Cached {
    if cfg.cloud == CloudConfig::None {
        return Cached::default();
    }
    std::fs::read(cache_file())
        .ok()
        .and_then(|b| serde_json::from_slice::<Cached>(&b).ok())
        .filter(|c| c.scope == scope(&cfg.cloud))
        .unwrap_or_default()
}

fn save_cache(c: &Cached) -> Result<()> {
    util::atomic_write(&cache_file(), &serde_json::to_vec(c)?)
}

/// Add a game this device just defined in the cloud to the copy of the
/// list, so it's known before the next fetch.
pub fn remember(cfg: &AppConfig, game: &CloudGame) {
    let mut c = cached(cfg);
    if c.fetched_at.is_none() {
        return;
    }
    c.games.retain(|g| g.key != game.key);
    c.games.push(game.clone());
    let _ = save_cache(&c);
}

/// Forget the cached list (another cloud was connected).
pub fn forget() {
    let _ = std::fs::remove_file(cache_file());
}

/// Fetch the cloud's game list and keep a copy. Links the user made on
/// another device ([`GameEntry::game`]) are taken over for games on this
/// device that have none.
pub fn refresh(cfg: &AppConfig, cloud: &Cloud) -> Result<Cached> {
    let games = cloud.list_games()?;
    let c = Cached { fetched_at: Some(Utc::now()), scope: scope(&cfg.cloud), games };
    save_cache(&c)?;
    let adopt: Vec<(String, String)> = c
        .games
        .iter()
        .filter_map(|cg| {
            let link = cg.game.clone()?;
            cfg.game(&cg.key).filter(|g| g.game.is_none()).map(|_| (cg.key.clone(), link))
        })
        .collect();
    if !adopt.is_empty() {
        config::update(|cfg| {
            for (key, link) in &adopt {
                if let Some(g) = cfg.game_mut(key) {
                    g.game = Some(link.clone());
                }
            }
            Ok(())
        })?;
    }
    Ok(c)
}

/// A game only the cloud has, as a library entry without a program (for
/// [`games::source`], logos and the like).
pub fn entry_of(cg: &CloudGame) -> GameEntry {
    GameEntry {
        key: cg.key.clone(),
        name: cg.name.clone(),
        adapter: cg.adapter.clone(),
        saves: cg.saves.clone(),
        exclude: cg.exclude.clone(),
        wait_for: cg.wait_for.clone(),
        roots: cg.roots.clone(),
        ludusavi_title: cg.ludusavi_title.clone(),
        source: cg.source.clone(),
        game: cg.game.clone(),
        auto_sync: true,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Newer saves in another version

/// What a version runs on, for questions and lists: its emulator, "Xbox",
/// "Steam", the user's group…
pub fn version_label(g: &GameEntry) -> String {
    let s = games::source(g);
    if let Some(l) = s.label {
        return l;
    }
    match s.kind {
        games::SourceKind::Emulator | games::SourceKind::Console => s.name.unwrap_or_else(|| g.name.clone()),
        games::SourceKind::Xbox => "Xbox".into(),
        games::SourceKind::Pc if g.steam.is_some() => "Steam".into(),
        games::SourceKind::Pc => crate::tr!("PC 版", "PC version"),
        games::SourceKind::Recomp => s.detail.unwrap_or_else(|| g.name.clone()),
        games::SourceKind::Custom => g.name.clone(),
    }
}

/// Another version of a game whose newest saves are newer than anything
/// this version has.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Newer {
    pub key: String,
    pub name: String,
    /// What it runs on ([`version_label`]).
    pub label: String,
    pub snapshot: crate::snapshot::SnapshotSummary,
}

/// The newest save version among the other versions of `game` (on this
/// device or only in the cloud), when it is newer than this version's
/// newest version in the cloud and its saves on this device, and the user
/// hasn't waved it off. This is how playing on another device in another
/// version (the Xbox version on the PC, the Steam version on the Deck)
/// carries over.
pub fn newer_version(ctx: &crate::sync::Ctx, game: &GameEntry) -> Result<Option<Newer>> {
    let cached = cached(&ctx.cfg);
    let fam = index(&ctx.cfg, &cached.games);
    let mine = fam.get(&game.key).cloned().unwrap_or_else(|| game.key.clone());
    let same = |k: &str| k != game.key && fam.get(k) == Some(&mine);
    let others: Vec<GameEntry> = ctx
        .cfg
        .games
        .iter()
        .filter(|g| same(&g.key))
        .cloned()
        .chain(cached.games.iter().filter(|c| same(&c.key) && ctx.cfg.game(&c.key).is_none()).map(entry_of))
        .collect();
    if others.is_empty() {
        return Ok(None);
    }
    let cloud = ctx.cloud()?;
    let own = cloud.list_snapshots(&game.key)?;
    let mut newest = crate::sync::mainline_head(&own).map(|h| h.created_at);
    if let Ok(local) = crate::snapshot::local_state(game) {
        let touched = local.files.iter().map(|f| f.mtime).max().and_then(|s| DateTime::<Utc>::from_timestamp(s, 0));
        newest = newest.max(touched);
    }
    let mut best: Option<Newer> = None;
    for o in &others {
        let Some(head) = crate::sync::mainline_head(&cloud.list_snapshots(&o.key)?) else { continue };
        let newer_than_mine = newest.is_none_or(|n| head.created_at > n);
        let newest_so_far = best.as_ref().is_none_or(|b| head.created_at > b.snapshot.created_at);
        if newer_than_mine && newest_so_far {
            best = Some(Newer { key: o.key.clone(), name: o.name.clone(), label: version_label(o), snapshot: head });
        }
    }
    let dismissed = state::game(&game.key).newer_dismissed;
    Ok(best.filter(|b| dismissed.as_deref() != Some(b.snapshot.id.as_str())))
}

/// Don't offer this save version of another version again.
pub fn dismiss_newer(key: &str, snapshot_id: &str) -> Result<()> {
    state::update_game(key, |s| s.newer_dismissed = Some(snapshot_id.to_string()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Linking versions by hand

/// Write a game's definition to the cloud now (after its link changed).
fn push_def(cloud: Option<&Cloud>, g: &GameEntry) {
    let _ = state::update_game(&g.key, |s| s.cloud_def_uploaded = false);
    if let Some(c) = cloud {
        if c.put_game(&CloudGame::from(g)).is_ok() {
            let _ = state::update_game(&g.key, |s| s.cloud_def_uploaded = true);
        }
    }
}

/// Set the link of `keys` (games on this device or only in the cloud) to
/// `value`, on this device and in the cloud.
fn set_links(cloud: Option<&Cloud>, keys: &[String], value: &str) -> Result<()> {
    let changed = config::update(|cfg| {
        let mut out = Vec::new();
        for k in keys {
            if let Some(g) = cfg.game_mut(k) {
                g.game = Some(value.to_string());
                out.push(g.clone());
            }
        }
        Ok(out)
    })?;
    for g in &changed {
        push_def(cloud, g);
    }
    let rest: Vec<&String> = keys.iter().filter(|k| !changed.iter().any(|g| &g.key == *k)).collect();
    if rest.is_empty() {
        return Ok(());
    }
    let cloud = cloud.ok_or_else(|| anyhow!(crate::tr!("没有连接云盘", "No cloud connected")))?;
    let mut list = cloud.list_games()?;
    for cg in list.iter_mut().filter(|c| rest.contains(&&c.key)) {
        cg.game = Some(value.to_string());
        cloud.put_game(cg)?;
    }
    let cfg = config::load()?;
    save_cache(&Cached { fetched_at: Some(Utc::now()), scope: scope(&cfg.cloud), games: list })
}

/// Mark `keys` as versions of one game. Keeps the link one of them already
/// has, so a game linked on another device stays one game.
pub fn link(cloud: Option<&Cloud>, keys: &[String]) -> Result<String> {
    let cfg = config::load()?;
    let cached = cached(&cfg);
    let existing = keys.iter().find_map(|k| {
        let link = cfg.game(k).and_then(|g| g.game.clone()).or_else(|| cached.games.iter().find(|c| &c.key == k).and_then(|c| c.game.clone()));
        link.filter(|l| !l.starts_with(SOLO))
    });
    let first = keys.first().ok_or_else(|| anyhow!("no games"))?;
    let value = existing.unwrap_or_else(|| format!("link:{first}"));
    set_links(cloud, keys, &value)?;
    Ok(value)
}

/// Keep `key` apart from the other games it was shown with.
pub fn unlink(cloud: Option<&Cloud>, key: &str) -> Result<()> {
    set_links(cloud, &[key.to_string()], &format!("{SOLO}{key}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, name: &str, saves: &[&str], ludusavi: Option<&str>, game: Option<&str>) -> GameEntry {
        GameEntry {
            key: key.into(),
            name: name.into(),
            saves: saves.iter().map(|s| s.to_string()).collect(),
            ludusavi_title: ludusavi.map(str::to_string),
            game: game.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn ids_of_games() {
        let g = entry("mh", "Monster Hunter Rise", &["{nxsave-0100B04011742000}/**"], None, None);
        assert_eq!(ids(&g), vec!["nx:0100B04011742000".to_string(), "name:switch:monsterhunterrise".into()]);
        let g = entry("hades", "Hades™", &[], Some("Hades"), None);
        assert_eq!(ids(&g), vec!["ludusavi:hades".to_string(), "name:pc:hades".into()]);
        // Too short to tell games apart.
        assert_eq!(ids(&entry("ys", "Ys", &[], None, None)), Vec::<String>::new());
    }

    #[test]
    fn families_by_ids_and_links() {
        let steam = entry("hades", "Hades", &[], Some("Hades"), None);
        let xbox = entry("hades-xbox", "Hades (PC)", &[], Some("Hades"), None);
        let celeste = entry("celeste", "Celeste", &[], Some("Celeste"), None);
        let oot = entry("oot-longshot", "塞尔达传说：时之笛", &[], None, None);
        let oot_n64 = entry("oot-n64", "The Legend of Zelda: Ocarina of Time", &[], None, None);
        let mut cloud = CloudGame::from(&entry("celeste-gog", "Celeste", &[], None, None));
        cloud.ids.clear(); // uploaded by an older version
        let list = [&steam, &xbox, &celeste, &oot, &oot_n64];
        let mut members: Vec<Member> = list.iter().map(|g| Member::local(g)).collect();
        members.push(Member::cloud(&cloud));
        let f = families(&members);
        assert_eq!(f["hades-xbox"], "hades");
        assert_eq!(f["hades"], "hades");
        assert_eq!(f["celeste-gog"], "celeste");
        assert_eq!(f["oot-longshot"], "oot-longshot");
        assert_eq!(f["oot-n64"], "oot-n64");

        // Linked by hand; kept apart by hand.
        let oot = entry("oot-longshot", "塞尔达传说：时之笛", &[], None, Some("link:oot-longshot"));
        let oot_n64 = entry("oot-n64", "The Legend of Zelda: Ocarina of Time", &[], None, Some("link:oot-longshot"));
        let xbox = entry("hades-xbox", "Hades", &[], Some("Hades"), Some("solo:hades-xbox"));
        let list = [&steam, &xbox, &oot, &oot_n64];
        let f = families(&list.iter().map(|g| Member::local(g)).collect::<Vec<_>>());
        assert_eq!(f["oot-n64"], "oot-longshot");
        assert_eq!(f["hades-xbox"], "hades-xbox");
        assert_eq!(f["hades"], "hades");
    }

    #[test]
    fn one_game_only_where_saves_carry_over() {
        let with = |mut g: GameEntry, adapter: &str| {
            g.adapter = Some(adapter.into());
            g
        };
        // Breath of the Wild: Wii U in Cemu, the Switch itself, Switch in
        // Ryujinx. The Switch and Ryujinx share saves (one title id); the
        // Wii U version doesn't, even with the same name.
        let wiiu = with(entry("botw", "The Legend of Zelda - Breath of the Wild", &["{mlc}/usr/save/00050000/101c9300/**"], None, None), "cemu");
        let switch = entry(
            "nx-01007ef00011e000",
            "塞尔达传说 旷野之息",
            &["{nxsave-01007ef00011e000}/**", "{nxdevice-01007ef00011e000}/**"],
            None,
            None,
        );
        let ryujinx = with(entry("botw-ryujinx", "The Legend of Zelda: Breath of the Wild", &["{nxsave-01007ef00011e000}/**"], None, None), "ryujinx");
        assert_eq!((save_scope(&wiiu).as_str(), save_scope(&switch).as_str(), save_scope(&ryujinx).as_str()), ("wiiu", "switch", "switch"));
        let mut cloud = CloudGame::from(&switch);
        cloud.ids = vec!["nx:01007EF00011E000".into()]; // as the PC defines a Switch upload
        let members = vec![Member::local(&wiiu), Member::local(&ryujinx), Member::cloud(&cloud)];
        let f = families(&members);
        assert_eq!(f["nx-01007ef00011e000"], "botw-ryujinx");
        assert_eq!(f["botw"], "botw", "the Wii U version is a game of its own");
        // Without Ryujinx, the names alone don't join Wii U and Switch.
        let f = families(&[Member::local(&wiiu), Member::cloud(&cloud)]);
        assert_ne!(f["botw"], f["nx-01007ef00011e000"]);

        // PC versions still join by name; an emulator and a recomp don't.
        let steam = entry("celeste", "Celeste", &[], Some("Celeste"), None);
        let custom = entry("celeste-itch", "Celeste", &["{home}/Celeste/Saves/**"], None, None);
        let recomp = with(entry("oot-longshot", "Ocarina of Time", &[], None, None), "zelda-oot-longshot");
        let n64 = with(entry("oot-rom", "Ocarina of Time", &[], None, None), "retroarch");
        let f = families(&[Member::local(&steam), Member::local(&custom), Member::local(&recomp), Member::local(&n64)]);
        assert_eq!(f["celeste-itch"], "celeste");
        assert_ne!(f["oot-longshot"], f["oot-rom"]);

        // Stardew Valley's saves are the same files on PC and Switch
        // (rules/games/): one game across the two platforms.
        let sdv_steam = entry("sdv", "Stardew Valley", &[], Some("Stardew Valley"), None);
        let sdv_ryujinx = with(entry("sdv-ryujinx", "星露谷物语", &["{nxsave-0100e65002bb8000}/**"], None, None), "ryujinx");
        let mut sdv_switch = CloudGame::from(&entry("nx-0100e65002bb8000", "Stardew Valley", &["{nxsave-0100e65002bb8000}/**"], None, None));
        sdv_switch.ids = vec!["nx:0100E65002BB8000".into()]; // from an older version, without `fmt:`
        let f = families(&[Member::local(&sdv_steam), Member::local(&sdv_ryujinx), Member::cloud(&sdv_switch), Member::local(&steam)]);
        assert_eq!((&f["sdv"], &f["sdv-ryujinx"]), (&f["nx-0100e65002bb8000"], &f["nx-0100e65002bb8000"]));
        assert_eq!(f["celeste"], "celeste");

        // The Switch itself is its own source in the library.
        assert_eq!(games::source(&switch).kind, games::SourceKind::Console);
        assert_eq!(games::source(&switch).name.as_deref(), Some("Switch"));
    }
}
