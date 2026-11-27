//! The collection's shelves (docs/plan-v2.md, section 10.5): games the user
//! finished, and games put away ("archived": kept as a record of saves,
//! play time and achievements in the cloud, gone from the library, the
//! game itself free to be uninstalled).
//!
//! A shelf is a tag on the game's card, its record in the cloud
//! ([`crate::cloud::v2::GameRecord::tags`]), so every device sees it. The
//! tags are read with the cloud's game list ([`crate::savelib::refresh`])
//! and kept on this device with it: showing them costs no request. Setting
//! one reads the record and writes it (two requests), only when the user
//! asks.

use crate::cloud::library;
use crate::cloud::v2::Access;
use crate::config::AppConfig;
use crate::sync::Ctx;
use crate::{savelib, tr};
use anyhow::{bail, Result};
use chrono::Utc;
use std::collections::BTreeMap;

/// The game was played to the end.
pub const FINISHED: &str = "finished";
/// The game is put away: on the collection's shelf, not in the library.
pub const ARCHIVED: &str = "archived";

/// The record a game of this device belongs to: the card it joined, else
/// its own (same id as the game).
fn record_of(cfg: &AppConfig, key: &str) -> String {
    cfg.game(key).and_then(|g| g.record.clone()).unwrap_or_else(|| key.to_string())
}

/// The shelves of the game `key` (this device's or only in the cloud), from
/// the copy of the cloud's list kept on this device (`by_record`:
/// [`savelib::Cached::tags`]).
pub fn of(cfg: &AppConfig, by_record: &BTreeMap<String, Vec<String>>, key: &str) -> Vec<String> {
    by_record.get(&record_of(cfg, key)).cloned().unwrap_or_default()
}

/// Put the games `keys` (the versions of one game) on the shelf `tag`, or
/// take them off it. Each distinct card is read and written once; a card
/// another took over is followed there. Returns the cards' tags now, by
/// record id, as the copy of the list on this device has them too.
pub fn set(ctx: &Ctx, keys: &[String], tag: &str, on: bool) -> Result<BTreeMap<String, Vec<String>>> {
    if tag != FINISHED && tag != ARCHIVED {
        bail!("unknown shelf {tag:?}");
    }
    let cloud = ctx.cloud()?;
    if cloud.access()? != Access::ReadWrite {
        bail!(tr!(
            "云端还是旧版本的格式，升级完成后才能放上书架",
            "The cloud still has the older layout; shelves work once it is upgraded"
        ));
    }
    let store = cloud.files();
    let mut done: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for key in keys {
        // The card as the cloud has it now, past any that were merged.
        let mut id = record_of(&ctx.cfg, key);
        let mut record = None;
        for _ in 0..8 {
            let Some(r) = library::read(store, &id)? else { break };
            match r.merged_into.clone() {
                Some(next) if next != id => id = next,
                _ => {
                    record = Some(r);
                    break;
                }
            }
        }
        let Some(mut record) = record else { continue };
        if done.contains_key(&record.id) {
            continue;
        }
        record.tags.retain(|t| t != tag);
        if on {
            record.tags.push(tag.to_string());
        }
        let saved = library::save(store, &record, cloud.device_id(), Utc::now())?;
        done.insert(saved.id.clone(), saved.tags.clone());
        // The keys that pointed at a merged card read the survivor's tags.
        if saved.id != record_of(&ctx.cfg, key) {
            done.insert(record_of(&ctx.cfg, key), saved.tags);
        }
    }
    if done.is_empty() {
        bail!(tr!("这个游戏还没有上传过存档，云端没有它的记录", "This game hasn't uploaded any saves yet, so the cloud has no record of it"));
    }
    cloud.forget_records();
    savelib::remember_tags(&ctx.cfg, &done);
    Ok(done)
}
