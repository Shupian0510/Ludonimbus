//! Achievements in the cloud, append-only like `devices/`:
//!
//! ```text
//! games/<key>/achievements/<device-id>.json   this device's unlocks; only this device writes it
//! games/<key>/achievements/schema.json        definitions, without local icon paths
//! ```
//!
//! A device file only ever gains entries, so resetting trophies on one PC
//! never erases another's records. All device files are merged; the
//! earliest unlock of an achievement wins. Same style as playtime
//! (`games/<key>/playtime/<device-id>.json`). Everything goes through the
//! cloud's path-based [`Store`] (`CloudProvider::files()`); other devices'
//! files are only downloaded again when their change tag moved.

use super::{GameDb, Schema, Unlock};
use crate::cloud::store::{join, Store};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const SCHEMA: &str = "schema.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceFile {
    pub device: String,
    pub device_name: String,
    pub updated_at: Option<DateTime<Utc>>,
    pub unlocks: Vec<Unlock>,
}

fn dir(key: &str) -> String {
    join(&["games", key, "achievements"])
}

fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex()[..16].to_string()
}

/// Definitions as uploaded: local icon paths mean nothing on other PCs.
fn portable(schema: &Schema) -> Schema {
    let mut s = schema.clone();
    for a in &mut s.achievements {
        if a.icon.as_deref().is_some_and(|i| !super::is_web(i)) {
            a.icon = None;
        }
    }
    s
}

/// Read the other devices' files and merge them into the local database.
/// Returns how many unlocks were added or moved earlier.
pub fn pull(files: &dyn Store, key: &str, device: &str) -> Result<usize> {
    let d = dir(key);
    let entries = files.list(&d)?;
    let known = super::load_db(key).remote_tags;
    let mut read = Vec::new();
    let mut tags = Vec::new();
    for e in entries.iter().filter(|e| !e.is_dir && e.name.ends_with(".json") && e.name != SCHEMA) {
        if e.name.trim_end_matches(".json") == device {
            continue;
        }
        if !e.tag.is_empty() && known.get(&e.name) == Some(&e.tag) {
            continue;
        }
        if let Some(bytes) = files.read(&join(&[&d, &e.name]))? {
            if let Ok(f) = serde_json::from_slice::<DeviceFile>(&bytes) {
                read.push(f);
                tags.push((e.name.clone(), e.tag.clone()));
            }
        }
    }
    let need_schema = super::load_db(key).schema.is_none() && entries.iter().any(|e| e.name == SCHEMA);
    let schema = if need_schema {
        files.read(&join(&[&d, SCHEMA]))?.and_then(|b| serde_json::from_slice::<Schema>(&b).ok())
    } else {
        None
    };
    super::update_db(key, |db| {
        db.remote_tags.extend(tags);
        merge_files(db, &read, schema)
    })
}

fn merge_files(db: &mut GameDb, files: &[DeviceFile], schema: Option<Schema>) -> usize {
    if db.schema.is_none() {
        db.schema = schema;
    }
    let mut changed = 0;
    for f in files {
        for u in &f.unlocks {
            if db.merge(u, &f.device, &f.device_name) {
                changed += 1;
            }
        }
    }
    changed
}

/// Upload this device's unlocks (and the definitions) when they changed.
pub fn push(files: &dyn Store, key: &str, device: &str, device_name: &str) -> Result<bool> {
    let db = super::load_db(key);
    let mut wrote = false;
    if !db.mine.is_empty() {
        let file = DeviceFile {
            device: device.to_string(),
            device_name: device_name.to_string(),
            updated_at: None,
            unlocks: db.mine.values().cloned().collect(),
        };
        let h = hash(&serde_json::to_vec(&file)?);
        if db.uploaded.as_deref() != Some(h.as_str()) {
            // Never drop entries another run of this device uploaded before
            // (e.g. after the local database was deleted).
            let path = join(&[&dir(key), &format!("{device}.json")]);
            let mut out = file;
            if let Some(old) = files.read(&path)?.and_then(|b| serde_json::from_slice::<DeviceFile>(&b).ok()) {
                for u in old.unlocks {
                    if !out.unlocks.iter().any(|m| m.id == u.id) {
                        out.unlocks.push(u);
                    }
                }
            }
            out.updated_at = Some(Utc::now());
            files.write(&path, &serde_json::to_vec_pretty(&out)?)?;
            super::update_db(key, |db| db.uploaded = Some(h))?;
            wrote = true;
        }
    }
    if let Some(schema) = &db.schema {
        let body = serde_json::to_vec_pretty(&portable(schema))?;
        let h = hash(&body);
        if db.uploaded_schema.as_deref() != Some(h.as_str()) {
            files.write(&join(&[&dir(key), SCHEMA]), &body)?;
            super::update_db(key, |db| db.uploaded_schema = Some(h))?;
            wrote = true;
        }
    }
    Ok(wrote)
}

/// Pull, then push.
pub fn sync(files: &dyn Store, key: &str, device: &str, device_name: &str) -> Result<()> {
    pull(files, key, device)?;
    push(files, key, device, device_name)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::achievements::{Achievement, SourceKind};

    #[test]
    fn merging_device_files() {
        let t = |s: &str| s.parse::<DateTime<Utc>>().unwrap();
        let mut db = GameDb::default();
        db.merge(&Unlock { id: "1".into(), unlocked_at: t("2026-11-22T00:00:00Z"), hardcore: false }, "me", "Desktop");
        let other = DeviceFile {
            device: "laptop".into(),
            device_name: "Laptop".into(),
            updated_at: None,
            unlocks: vec![
                Unlock { id: "1".into(), unlocked_at: t("2026-11-21T00:00:00Z"), hardcore: false },
                Unlock { id: "2".into(), unlocked_at: t("2026-11-23T00:00:00Z"), hardcore: false },
            ],
        };
        assert_eq!(merge_files(&mut db, &[other.clone()], None), 2);
        assert_eq!(db.unlocks["1"].device_name, "Laptop");
        assert_eq!(db.unlocks.len(), 2);
        // Merging again changes nothing; other devices' unlocks never count as this device's.
        assert_eq!(merge_files(&mut db, &[other], None), 0);
        assert!(db.mine.is_empty());
    }

    #[test]
    fn local_icons_are_not_uploaded() {
        let s = Schema {
            source: SourceKind::Rpcs3,
            source_id: "NPWR00001_00".into(),
            title: None,
            achievements: vec![
                Achievement { id: "0".into(), name: "A".into(), description: String::new(), icon: Some("C:\\x\\TROP000.PNG".into()), hidden: false, points: None, grade: None },
                Achievement { id: "1".into(), name: "B".into(), description: String::new(), icon: Some("https://media.retroachievements.org/Badge/1.png".into()), hidden: false, points: None, grade: None },
            ],
        };
        let p = portable(&s);
        assert!(p.achievements[0].icon.is_none());
        assert!(p.achievements[1].icon.is_some());
    }
}
