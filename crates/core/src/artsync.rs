//! Game artwork in the cloud, so every device shows the same cover for a
//! game, including games it only knows from the cloud (docs/plan-phase5.md
//! §5, V2): `games/<folder>/art/<kind>.<ext>` plus `games/<folder>/art/art.json`
//! with each image's file name and hash.
//!
//! A game has one set of artwork, wherever its folders are
//! ([`crate::cloud::gamedata`]): each kind is taken from the first of the
//! game's folders, in its record's order, whose `art.json` has it. A device
//! uploads into the folder it writes to.
//!
//! A device uploads the artwork it shows (Steam's for a game in Steam, else
//! the one chosen in Ludonimbus). It downloads the cloud's artwork only for
//! kinds it has no image of its own for, into Ludonimbus's artwork folder.

use crate::cloud::gamedata::Folders;
use crate::cloud::store::{join, Store};
use crate::config::{AppConfig, GameEntry};
use crate::{localart, paths, state, steam};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The kinds the library and the game page show.
pub const KINDS: [&str; 4] = ["cover", "hero", "logo", "icon"];
const INDEX: &str = "art.json";
/// The folder of a game's artwork.
const KIND: &str = "art";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    files: BTreeMap<String, Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Item {
    file: String,
    hash: String,
}

fn hash(p: &Path) -> Option<String> {
    std::fs::read(p).ok().map(|b| blake3::hash(&b).to_hex().to_string())
}

/// The image of one kind this device shows for a game: Steam's for a game
/// in Steam, else the one chosen in Ludonimbus.
pub fn shown(cfg: &AppConfig, g: &GameEntry, kind: &str) -> Option<PathBuf> {
    if let (Some(root), Some(l)) = (steam::root(cfg), &g.steam) {
        if let Some(f) = steam::art_file(&root, &l.account, l.appid, kind) {
            return Some(f);
        }
    }
    localart::file(&g.key, kind)
}

fn read_index(store: &dyn Store, dir: &str) -> Result<Option<Index>> {
    Ok(store.read(&join(&[dir, INDEX]))?.and_then(|b| serde_json::from_slice(&b).ok()))
}

/// The game's artwork in the cloud: each kind, with the folder it is in,
/// from the first of the game's folders whose index has it. One read per
/// folder looked in; once every kind is found, no further folder is.
fn find(store: &dyn Store, at: &Folders) -> Result<BTreeMap<String, (String, Item)>> {
    let mut found: BTreeMap<String, (String, Item)> = BTreeMap::new();
    for (_, dir) in at.read_dirs(KIND) {
        let Some(idx) = read_index(store, &dir)? else { continue };
        for (kind, item) in idx.files {
            if KINDS.contains(&kind.as_str()) && valid_name(&item.file) && !found.contains_key(&kind) {
                found.insert(kind, (dir.clone(), item));
            }
        }
        if KINDS.iter().all(|k| found.contains_key(*k)) {
            break;
        }
    }
    Ok(found)
}

/// Upload the artwork this device shows for a game where it differs from
/// what was uploaded before, into the folder of the game `at` this device
/// writes to. Images that came from the cloud aren't sent back. Returns
/// whether anything was uploaded.
pub fn push(store: &dyn Store, cfg: &AppConfig, at: &Folders, g: &GameEntry) -> Result<bool> {
    let st = state::game(&g.key);
    let mine: Vec<(&str, PathBuf, String)> = KINDS
        .iter()
        .filter_map(|k| {
            let f = shown(cfg, g, k)?;
            let h = hash(&f)?;
            Some((*k, f, h))
        })
        .filter(|(k, _, h)| st.art_pushed.get(*k) != Some(h) && st.art_pulled.get(*k) != Some(h))
        .collect();
    if mine.is_empty() {
        return Ok(false);
    }
    let dir = at.write_dir(KIND);
    let mut idx = read_index(store, &dir)?.unwrap_or_default();
    let mut sent = Vec::new();
    for (kind, file, h) in &mine {
        if idx.files.get(*kind).is_some_and(|i| &i.hash == h) {
            sent.push((kind.to_string(), h.clone()));
            continue;
        }
        let name = format!("{kind}.{}", steam::image_ext(file));
        store.upload(&join(&[&dir, &name]), file)?;
        if let Some(old) = idx.files.get(*kind).filter(|i| i.file != name) {
            let _ = store.remove(&join(&[&dir, &old.file]));
        }
        idx.files.insert(kind.to_string(), Item { file: name, hash: h.clone() });
        sent.push((kind.to_string(), h.clone()));
    }
    store.write(&join(&[&dir, INDEX]), &serde_json::to_vec_pretty(&idx)?)?;
    state::update_game(&g.key, |s| {
        for (k, h) in sent {
            s.art_pushed.insert(k, h);
        }
    })?;
    Ok(true)
}

/// Download the cloud's artwork of the game `at` (`game`: its entry on this
/// device, `None` for a game only the cloud has) for every kind this device
/// shows no image of its own for. An image downloaded earlier is replaced
/// when the cloud's changes. Returns whether anything was downloaded.
pub fn pull(store: &dyn Store, cfg: &AppConfig, at: &Folders, game: Option<&GameEntry>) -> Result<bool> {
    let key = at.key();
    let found = find(store, at)?;
    let st = state::game(key);
    let mut got = Vec::new();
    for (kind, (dir, item)) in &found {
        let own = match game {
            Some(g) => shown(cfg, g, kind),
            None => localart::file(key, kind),
        };
        if let Some(f) = own {
            let h = hash(&f);
            // Already this image, or one of this device's own.
            if h.as_deref() == Some(item.hash.as_str()) || h.as_ref() != st.art_pulled.get(kind) {
                continue;
            }
        }
        let tmp = paths::staging_dir().join(format!("art-{key}-{}", item.file));
        store.download(&join(&[dir, &item.file]), &tmp)?;
        let res = localart::set(key, kind, &tmp);
        let _ = std::fs::remove_file(&tmp);
        res?;
        got.push((kind.clone(), item.hash.clone()));
    }
    if got.is_empty() {
        return Ok(false);
    }
    state::update_game(key, |s| {
        for (k, h) in got {
            s.art_pulled.insert(k, h);
        }
    })?;
    Ok(true)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_') && !name.starts_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::memory::Memory;

    fn index(kinds: &[&str]) -> Vec<u8> {
        let files = kinds.iter().map(|k| (k.to_string(), Item { file: format!("{k}.png"), hash: format!("h-{k}") })).collect();
        serde_json::to_vec(&Index { files }).unwrap()
    }

    #[test]
    fn each_kind_comes_from_the_first_folder_that_has_it() {
        let store = Memory::new();
        store.put("games/b/art/art.json", &index(&["cover", "hero"]));
        store.put("games/c/art/art.json", &index(&["cover", "logo", "icon"]));
        let at = Folders::new("b", &["a".to_string(), "b".to_string(), "c".to_string()]);
        let found = find(&store, &at).unwrap();
        let from: Vec<(&str, &str)> = found.iter().map(|(k, (dir, _))| (k.as_str(), dir.as_str())).collect();
        assert_eq!(from, [("cover", "games/b/art"), ("hero", "games/b/art"), ("icon", "games/c/art"), ("logo", "games/c/art")]);
        assert_eq!(store.take().read, 3);
        // A set the first folder has whole costs one read.
        store.put("games/a/art/art.json", &index(&KINDS));
        assert!(find(&store, &at).unwrap().values().all(|(dir, _)| dir == "games/a/art"));
        assert_eq!(store.take().read, 1);
    }
}
