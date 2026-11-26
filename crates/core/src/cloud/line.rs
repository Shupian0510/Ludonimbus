//! Reading and writing the folder of one save line
//! (docs/plan-v2.md, sections 3.4, 3.7 and 3.8).
//!
//! The folder holds one manifest per snapshot (`<id>.json`), a pin file for
//! each snapshot the user keeps (`<id>.pin.json`) and `index.json`. What
//! exists is decided by the folder listing alone; the index and the local
//! [`LineCache`] only save requests:
//!
//! | situation | requests |
//! |---|---|
//! | nothing new since the last read | 1 list |
//! | new snapshots, index up to date | 1 list + 1 read |
//! | a device that has never seen the line | 1 list + 1 read |
//! | index missing, damaged or behind | 1 list + 1 read per snapshot the index lacks |
//!
//! A line can live in several folders ([`load_all`]): the one it is written
//! to, plus folders that are only read (the 0.6 folder from before the
//! migration, a line that was merged into this one). Each of those costs one
//! more listing.

use super::store::Store;
use super::v2::{self, IndexEntry, LineIndex, Manifest, Pin, PinRef};
use crate::vv::{self, Vv};
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What a device remembers about a line between reads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineCache {
    /// Tag of the `index.json` read last.
    #[serde(default)]
    pub index_tag: String,
    /// The same for the indexes of folders that are only read, by folder.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub also_index_tags: BTreeMap<String, String>,
    #[serde(default)]
    pub snapshots: BTreeMap<String, IndexEntry>,
}

impl LineCache {
    /// Remember a snapshot this device just wrote, so that reading the line
    /// again doesn't fetch it.
    pub fn insert(&mut self, manifest: &Manifest) {
        self.snapshots.insert(manifest.id.clone(), manifest.summary());
    }
}

/// A save line as it is in the cloud right now.
#[derive(Debug, Clone, Default)]
pub struct LineView {
    /// Every snapshot, oldest first. Snapshots of the 0.6 layout carry the
    /// vector `{legacy: n}` here (they have none in the cloud).
    pub snapshots: Vec<IndexEntry>,
    /// Positions in `snapshots` of the versions nothing else contains: one
    /// normally, several when the line has forked, none when it is empty.
    pub heads: Vec<usize>,
    /// The index in the cloud didn't cover the listing; write a new one.
    pub needs_index: bool,
    /// Manifests that exist but can't be understood.
    pub unreadable: Vec<String>,
    /// For each of `snapshots`, which of the folders given to [`load_all`]
    /// holds its manifest (always 0 for a line in one folder).
    pub places: Vec<usize>,
}

impl LineView {
    /// The newest version; `None` when the line is empty or has forked.
    pub fn head(&self) -> Option<&IndexEntry> {
        match self.heads.as_slice() {
            [i] => self.snapshots.get(*i),
            _ => None,
        }
    }

    pub fn head_entries(&self) -> impl Iterator<Item = &IndexEntry> + '_ {
        self.heads.iter().filter_map(|&i| self.snapshots.get(i))
    }

    pub fn is_forked(&self) -> bool {
        self.heads.len() > 1
    }

    pub fn get(&self, id: &str) -> Option<&IndexEntry> {
        self.snapshots.iter().find(|s| s.id == id)
    }

    /// Which folder (its position in the list given to [`load_all`]) holds
    /// the manifest of `id`.
    pub fn place(&self, id: &str) -> Option<usize> {
        let i = self.snapshots.iter().position(|s| s.id == id)?;
        self.places.get(i).copied()
    }
}

/// Read the line in `dir`, using and updating `cache`.
pub fn load(store: &dyn Store, dir: &str, cache: &mut LineCache) -> Result<LineView> {
    load_all(store, &[dir], cache)
}

/// Take what an index knows about the snapshots that exist.
fn merge_index(cache: &mut LineCache, index: LineIndex, ids: &BTreeMap<String, usize>, pins: &BTreeMap<String, (String, usize)>) {
    for mut entry in index.snapshots {
        if !ids.contains_key(&entry.id) {
            continue;
        }
        if entry.pin.as_ref().map(|p| &p.tag) != pins.get(&entry.id).map(|(tag, _)| tag) {
            entry.pin = None;
        }
        match cache.snapshots.get_mut(&entry.id) {
            Some(known) => {
                if known.pin.is_none() {
                    known.pin = entry.pin;
                }
            }
            None => {
                cache.snapshots.insert(entry.id.clone(), entry);
            }
        }
    }
}

/// Read a line that lives in several folders. `dirs[0]` is the one the line
/// is written to (new manifests, pins, the index that covers everything);
/// the others are only read. A snapshot found in two folders counts where it
/// comes first.
pub fn load_all(store: &dyn Store, dirs: &[&str], cache: &mut LineCache) -> Result<LineView> {
    // What exists: the listings, nothing else.
    let mut ids: BTreeMap<String, usize> = BTreeMap::new();
    let mut pins: BTreeMap<String, (String, usize)> = BTreeMap::new();
    let mut index_tags: Vec<Option<String>> = Vec::with_capacity(dirs.len());
    for (place, dir) in dirs.iter().enumerate() {
        let mut index_tag = None;
        for e in store.list(dir)? {
            if e.is_dir {
                continue;
            }
            if e.name == v2::INDEX_FILE {
                index_tag = Some(e.tag);
            } else if let Some(id) = e.name.strip_suffix(v2::PIN_SUFFIX) {
                if v2::is_snapshot_id(id) {
                    pins.entry(id.to_string()).or_insert((e.tag, place));
                }
            } else if let Some(id) = e.name.strip_suffix(".json") {
                if v2::is_snapshot_id(id) {
                    ids.entry(id.to_string()).or_insert(place);
                }
            }
        }
        index_tags.push(index_tag);
    }
    pins.retain(|id, _| ids.contains_key(id));

    // Forget what is gone or has changed.
    cache.snapshots.retain(|id, _| ids.contains_key(id));
    for (id, entry) in cache.snapshots.iter_mut() {
        if entry.pin.as_ref().map(|p| &p.tag) != pins.get(id).map(|(tag, _)| tag) {
            entry.pin = None;
        }
    }
    cache.also_index_tags.retain(|dir, _| dirs.iter().skip(1).any(|d| d == dir));

    // The indexes, when they have changed and something is missing here. The
    // first folder's covers the whole line; the others' only help right after
    // a merge, before the first folder's index has caught up.
    let incomplete = |cache: &LineCache| {
        ids.keys().any(|id| !cache.snapshots.contains_key(id))
            || pins.keys().any(|id| cache.snapshots.get(id).is_some_and(|e| e.pin.is_none()))
    };
    let mut needs_index = index_tags.first().is_some_and(|t| t.is_none()) && !ids.is_empty();
    for (place, dir) in dirs.iter().enumerate() {
        let Some(tag) = &index_tags[place] else { continue };
        let known = if place == 0 { Some(&cache.index_tag) } else { cache.also_index_tags.get(*dir) };
        if known == Some(tag) || !incomplete(cache) {
            continue;
        }
        match read_index(store, dir)? {
            Some(index) => {
                let before = cache.snapshots.len();
                merge_index(cache, index, &ids, &pins);
                if place == 0 {
                    cache.index_tag = tag.clone();
                } else {
                    cache.also_index_tags.insert(dir.to_string(), tag.clone());
                    // The line's own index didn't have these.
                    needs_index |= cache.snapshots.len() > before;
                }
            }
            None if place == 0 => needs_index = true,
            None => {}
        }
    }

    // Whatever the indexes didn't have: straight from the manifests and pins.
    let mut unreadable = Vec::new();
    let missing: Vec<(&String, usize)> = ids
        .iter()
        .filter(|(id, _)| !cache.snapshots.contains_key(*id))
        .map(|(id, place)| (id, *place))
        .collect();
    for (id, place) in missing {
        needs_index = true;
        // `None`: deleted since the listing was made.
        if let Some(bytes) = store.read(&v2::manifest_path(dirs[place], id))? {
            match serde_json::from_slice::<Manifest>(&bytes) {
                Ok(m) if m.id == *id => cache.insert(&m),
                _ => unreadable.push(id.clone()),
            }
        }
    }
    for (id, (tag, place)) in &pins {
        let Some(entry) = cache.snapshots.get_mut(id) else { continue };
        if entry.pin.is_some() {
            continue;
        }
        needs_index = true;
        entry.pin = store.read(&v2::pin_path(dirs[*place], id))?.map(|bytes| {
            // A pin that can't be understood still keeps its snapshot.
            let pin: Pin = serde_json::from_slice(&bytes).unwrap_or_default();
            PinRef { name: pin.name, tag: tag.clone() }
        });
    }

    let mut snapshots: Vec<IndexEntry> = cache.snapshots.values().cloned().collect();
    snapshots.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
    // Snapshots of the 0.6 layout: numbered in the order they were made.
    let mut legacy = 0;
    for s in snapshots.iter_mut() {
        if s.vv.is_empty() && !s.is_side() {
            legacy += 1;
            s.vv = Vv::legacy(legacy);
        }
    }
    let heads = {
        let main: Vec<(usize, &IndexEntry)> = snapshots.iter().enumerate().filter(|(_, s)| !s.is_side()).collect();
        vv::heads(&main, |(_, s)| &s.vv).into_iter().map(|k| main[k].0).collect()
    };
    let places = snapshots.iter().map(|s| ids.get(&s.id).copied().unwrap_or(0)).collect();
    Ok(LineView { snapshots, heads, needs_index, unreadable, places })
}

/// `None` when there is no index or it can't be understood: both mean "read
/// the manifests".
pub fn read_index(store: &dyn Store, dir: &str) -> Result<Option<LineIndex>> {
    Ok(store.read(&v2::index_path(dir))?.and_then(|bytes| serde_json::from_slice(&bytes).ok()))
}

/// Write the index from what `cache` knows (call after [`load`], and after
/// writing a manifest or a pin).
pub fn write_index(store: &dyn Store, dir: &str, cache: &LineCache, device_id: &str) -> Result<()> {
    let index = LineIndex {
        v: 1,
        updated_at: Some(Utc::now()),
        updated_by: device_id.to_string(),
        snapshots: cache.snapshots.values().cloned().collect(),
        extra: v2::Extra::new(),
    };
    store.write(&v2::index_path(dir), &serde_json::to_vec(&index)?)
}

pub fn read_manifest(store: &dyn Store, dir: &str, id: &str) -> Result<Option<Manifest>> {
    match store.read(&v2::manifest_path(dir, id))? {
        None => Ok(None),
        Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
    }
}

/// Writing the manifest is what makes a snapshot exist: its packs must be in
/// the cloud before this is called.
pub fn write_manifest(store: &dyn Store, dir: &str, manifest: &Manifest) -> Result<()> {
    store.write(&v2::manifest_path(dir, &manifest.id), &serde_json::to_vec(manifest)?)
}

pub fn write_pin(store: &dyn Store, dir: &str, id: &str, pin: &Pin) -> Result<()> {
    store.write(&v2::pin_path(dir, id), &serde_json::to_vec(pin)?)
}

pub fn remove_pin(store: &dyn Store, dir: &str, id: &str) -> Result<()> {
    store.remove(&v2::pin_path(dir, id))
}
