//! Save lines: which line a game follows on this device, sharing a line with
//! a version that keeps the same saves in other folders (`join`), and taking
//! the progress of another line.

use super::overview::{platform_marks, PlatformMark};
use super::saves::too_new_ids;
use super::{blocking, restoring_while_running, with_free_game, with_game, CmdResult};
use anyhow::bail;
use ludonimbus_core::join;
use ludonimbus_core::sync::{self, Outcome};
use serde::Serialize;
use std::collections::BTreeMap;
use ts_rs::TS;

/// The save lines the game can follow on this device.
#[tauri::command]
pub async fn game_lines(key: String) -> CmdResult<Vec<sync::LineInfo>> {
    blocking(move || with_game(&key, sync::lines)).await
}

/// Make the game follow another save line on this device: `kind` is
/// `shared`, `independent` (with a `name` for the line, or none) or `follow`
/// (with the `line` another device keeps).
#[tauri::command]
pub async fn use_game_line(key: String, kind: String, line: Option<String>, name: Option<String>) -> CmdResult<Outcome> {
    blocking(move || {
        let choice = match (kind.as_str(), line) {
            ("independent", _) => sync::LineChoice::Independent { name },
            ("follow", Some(line)) => sync::LineChoice::Follow { line },
            _ => sync::LineChoice::Shared,
        };
        let running = ludonimbus_core::tr!("游戏正在运行，退出游戏后再改", "The game is running; change this after you quit");
        with_free_game(&key, running, |ctx, g| sync::use_line(ctx, g, choice))
    })
    .await
}

/// The save line the game could share with a version that keeps the same
/// saves in other folders, and which folder is which. Nothing changes.
#[tauri::command]
pub async fn join_suggestion(key: String) -> CmdResult<Option<join::Suggestion>> {
    blocking(move || with_game(&key, join::suggestion)).await
}

/// Another version's line the game can share (「共用进度」, an advanced
/// choice in the version bar's menu), with how that version is called.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShareOffer {
    suggestion: join::Suggestion,
    /// The version whose line it is ("Xbox", "Steam", "Ryujinx").
    label: String,
}

/// [`join_suggestion`] when it fits: the two versions' saves are the same
/// files, or a rule says so. `None` otherwise; nothing changes.
#[tauri::command]
pub async fn share_offer(key: String) -> CmdResult<Option<ShareOffer>> {
    blocking(move || {
        with_game(&key, |ctx, g| {
            let Some(s) = join::suggestion(ctx, g)?.filter(|s| s.fit == join::Fit::Ready) else { return Ok(None) };
            let label = match platform_marks(&ctx.cfg, [s.platform.as_str()]).remove(&s.platform) {
                Some(mark) => mark.label,
                None if !s.from.is_empty() => s.from.clone(),
                None => s.device.clone(),
            };
            Ok(Some(ShareOffer { suggestion: s, label }))
        })
    })
    .await
}

/// What the user answered to [`join_suggestion`]: `accept` (with the
/// suggestion) shares the line and syncs with it, `decline` (with the
/// suggestion) never suggests it again, `leave` stops sharing.
#[tauri::command]
pub async fn join_answer(key: String, answer: String, suggestion: Option<join::Suggestion>) -> CmdResult<Option<Outcome>> {
    blocking(move || {
        if answer == "decline" {
            let Some(s) = suggestion else { bail!("no suggestion") };
            join::decline(&key, &s.def)?;
            return Ok(None);
        }
        let running = ludonimbus_core::tr!("游戏正在运行，退出游戏后再改", "The game is running; change this after you quit");
        with_free_game(&key, running, |ctx, g| match (answer.as_str(), &suggestion) {
            ("accept", Some(s)) => join::accept(ctx, g, s),
            ("leave", _) => join::leave(ctx, g),
            _ => bail!("unknown answer"),
        })
        .map(Some)
    })
    .await
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LineList {
    lines: Vec<sync::LineInfo>,
    platforms: BTreeMap<String, PlatformMark>,
}

/// The game's save lines, who is on each with what, and how to show that.
#[tauri::command]
pub async fn game_line_members(key: String) -> CmdResult<LineList> {
    blocking(move || {
        with_game(&key, |ctx, g| {
            let lines = sync::lines(ctx, g)?;
            let platforms = platform_marks(&ctx.cfg, lines.iter().flat_map(|l| l.members.iter().map(|m| m.platform.as_str())));
            Ok(LineList { lines, platforms })
        })
    })
    .await
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OtherSaves {
    saves: Vec<sync::OtherSave>,
    platforms: BTreeMap<String, PlatformMark>,
    /// As [`SnapshotList::too_new`](super::saves::SnapshotList::too_new).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<_>", optional)]
    too_new: Vec<String>,
}

/// The newest version of each save line of the game this device doesn't
/// follow.
#[tauri::command]
pub async fn other_saves(key: String) -> CmdResult<OtherSaves> {
    blocking(move || {
        with_game(&key, |ctx, g| {
            let saves = sync::other_saves(ctx, g)?;
            let platforms = platform_marks(&ctx.cfg, saves.iter().map(|s| s.snapshot.platform.as_str()));
            let too_new = too_new_ids(&ctx.cfg, &key, saves.iter().map(|s| &s.snapshot));
            Ok(OtherSaves { saves, platforms, too_new })
        })
    })
    .await
}

/// Make a version of another save line of the game the progress here.
#[tauri::command]
pub async fn take_save(key: String, line: String, id: String) -> CmdResult<Outcome> {
    blocking(move || with_free_game(&key, restoring_while_running(), |ctx, g| sync::take_from(ctx, g, &line, &id))).await
}
