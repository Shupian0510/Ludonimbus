//! Game artwork in the cloud, so every device shows the same cover for a
//! game, including games it only knows from the cloud (docs/plan-phase5.md
//! §5, V2): `games/<key>/art/<kind>.<ext>` plus `games/<key>/art/art.json`
//! with each image's file name and hash.
//!
//! A device uploads the artwork it shows (Steam's for a game in Steam, else
//! the one chosen in Ludonimbus). It downloads the cloud's artwork only for
//! kinds it has no image of its own for, into Ludonimbus's artwork folder.

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

fn dir(key: &str) -> String {
    join(&["games", key, "art"])
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

fn read_index(store: &dyn Store, key: &str) -> Result<Option<Index>> {
    Ok(store.read(&join(&[&dir(key), INDEX]))?.and_then(|b| serde_json::from_slice(&b).ok()))
}

/// Upload the artwork this device shows for a game where it differs from
/// what was uploaded before. Images that came from the cloud aren't sent
/// back. Returns whether anything was uploaded.
pub fn push(store: &dyn Store, cfg: &AppConfig, g: &GameEntry) -> Result<bool> {
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
    let mut idx = read_index(store, &g.key)?.unwrap_or_default();
    let mut sent = Vec::new();
    for (kind, file, h) in &mine {
        if idx.files.get(*kind).is_some_and(|i| &i.hash == h) {
            sent.push((kind.to_string(), h.clone()));
            continue;
        }
        let name = format!("{kind}.{}", steam::image_ext(file));
        store.upload(&join(&[&dir(&g.key), &name]), file)?;
        if let Some(old) = idx.files.get(*kind).filter(|i| i.file != name) {
            let _ = store.remove(&join(&[&dir(&g.key), &old.file]));
        }
        idx.files.insert(kind.to_string(), Item { file: name, hash: h.clone() });
        sent.push((kind.to_string(), h.clone()));
    }
    store.write(&join(&[&dir(&g.key), INDEX]), &serde_json::to_vec_pretty(&idx)?)?;
    state::update_game(&g.key, |s| {
        for (k, h) in sent {
            s.art_pushed.insert(k, h);
        }
    })?;
    Ok(true)
}

/// Download the cloud's artwork of a game (`game`: its entry on this device,
/// `None` for a game only the cloud has) for every kind this device shows no
/// image of its own for. An image downloaded earlier is replaced when the
/// cloud's changes. Returns whether anything was downloaded.
pub fn pull(store: &dyn Store, cfg: &AppConfig, key: &str, game: Option<&GameEntry>) -> Result<bool> {
    let Some(idx) = read_index(store, key)? else { return Ok(false) };
    let st = state::game(key);
    let mut got = Vec::new();
    for (kind, item) in &idx.files {
        if !KINDS.contains(&kind.as_str()) || !valid_name(&item.file) {
            continue;
        }
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
        store.download(&join(&[&dir(key), &item.file]), &tmp)?;
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

