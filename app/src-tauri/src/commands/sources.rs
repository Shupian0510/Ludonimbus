//! A game's saves from elsewhere (`core::elsewhere`): the other sources of
//! saves on its card, for the Saves tab's groups, and the newer progress one
//! of them has, for the hint row on the game page. One read for both.

use super::overview::{platform_marks, PlatformMark};
use super::saves::too_new_ids;
use super::{blocking, with_game, CmdResult};
use ludonimbus_core::{elsewhere, logger, sync};
use serde::Serialize;
use std::collections::BTreeMap;
use ts_rs::TS;

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SaveSources {
    sources: Vec<elsewhere::Source>,
    /// How to show each platform the sources and their saves name, this
    /// version's own too.
    platforms: BTreeMap<String, PlatformMark>,
    /// As [`SnapshotList::too_new`](super::saves::SnapshotList::too_new).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<_>", optional)]
    too_new: Vec<String>,
    /// The newest progress another version has, when it is newer than the
    /// save here and the user hasn't waved that save off.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    newer: Option<elsewhere::Newer>,
    /// What this version is called and runs on, for the group of the saves
    /// here.
    here_name: String,
    here_platform: String,
}

/// The other sources of the game's saves, and the newer progress among them.
#[tauri::command]
pub async fn save_sources(key: String) -> CmdResult<SaveSources> {
    blocking(move || {
        with_game(&key, |ctx, g| {
            let sources = elsewhere::sources(ctx, g)?;
            // The groups stand without the hint.
            let newer = elsewhere::newer_among(ctx, g, &sources).unwrap_or_else(|e| {
                logger::log("app", format!("{key}: newer progress elsewhere not known: {e:#}"));
                None
            });
            let here_platform = sync::platform(g);
            let listed = sources.iter().flat_map(|s| std::iter::once(s.platform.as_str()).chain(s.snapshots.iter().map(|x| x.platform.as_str())));
            let platforms = platform_marks(&ctx.cfg, listed.chain(std::iter::once(here_platform.as_str())));
            let too_new = too_new_ids(&ctx.cfg, &key, sources.iter().flat_map(|s| &s.snapshots));
            Ok(SaveSources { sources, platforms, too_new, newer, here_name: elsewhere::version_name(g), here_platform })
        })
    })
    .await
}
