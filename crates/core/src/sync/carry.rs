//! Another platform's files on a line. A version of a game that shares
//! another platform's save line through a rule ([`crate::join::by_rule`])
//! has no place for that platform's files: a version it makes carries them
//! over from the version it is based on ([`carry`]), and the saves here are
//! compared with them along ([`local_here`]). And a version that platform's
//! newer game wrote may be one the game here can't load ([`too_new`]); a
//! sync nobody chose leaves it in the cloud (`run`).

use super::{base_file, log, remember_base};
use crate::cloud::line::LineView;
use crate::cloud::saves::{self, Place};
use crate::cloud::v2::{self, FileRef, Manifest};
use crate::cloud::{packs, Cloud, OpenLine};
use crate::config::GameEntry;
use crate::paths;
use crate::snapshot::{self, LocalState};
use crate::state::{self, GameState};
use anyhow::Result;
use std::path::PathBuf;

/// The manifest of the version this device is at on the game's line, when
/// this device kept it ([`remember_base`]).
fn known_base(key: &str, st: &GameState) -> Option<Manifest> {
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(base_file(key)).ok()?).ok()?;
    (Some(&manifest.id) == st.last_synced_id.as_ref()).then_some(manifest)
}

/// The version of the line a version that shares another platform's line
/// carries that platform's files over from: the one this device is at, else
/// the line's newest. At most one read (a manifest is read once and then
/// kept), none when this device kept the one it is at.
fn carry_source(cloud: Option<&Cloud>, game: &GameEntry, st: &GameState, line: Option<(&Place, &LineView)>) -> Result<Option<Manifest>> {
    if let Some(m) = known_base(&game.key, st) {
        return Ok(Some(m));
    }
    let (Some(cloud), Some((place, view))) = (cloud, line) else { return Ok(None) };
    let at = st.last_synced_id.as_deref().filter(|id| view.get(id).is_some());
    let Some(id) = at.or_else(|| view.head_entries().last().map(|h| h.id.as_str())) else { return Ok(None) };
    let manifest = saves::manifest(cloud.files(), place, view, id)?;
    if at.is_some() {
        remember_base(&game.key, Some(&manifest));
    }
    Ok(Some(manifest))
}

fn carried_dir(key: &str) -> PathBuf {
    paths::data_dir().join("carried").join(key)
}

/// The files of `from` (a version of the line) that a version sharing
/// another platform's line through a rule has no place for, as a new version
/// of it carries them over: by reference to the pack that holds each
/// ([`FileRef::pack`]), and nothing of them uploaded.
///
/// A version of the layout of 0.6 keeps its files in a zip beside its
/// manifest, which no new version may point into (0.6 deletes those by its
/// own rules). Its files are fetched once (`fetch`; one download) and kept on
/// this device, and a new version packs them anew. Without them at hand they
/// are named without a pack: enough to compare, and a version made of them
/// can't be put together (the error says which file is missing) rather than
/// leave them out.
pub(super) fn carry(
    cloud: Option<&Cloud>,
    game: &GameEntry,
    line: Option<(&Place, &LineView)>,
    from: &Manifest,
    fetch: bool,
) -> Result<(Vec<FileRef>, Vec<packs::LocalFile>)> {
    let theirs: Vec<&FileRef> = from.files.iter().filter(|f| crate::bind::to_local(&game.bind, &f.path).is_none()).collect();
    if theirs.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    if from.v >= v2::LAYOUT && from.unsupported().is_empty() {
        let refs = theirs.into_iter().map(|f| FileRef { pack: Some(f.pack.clone().unwrap_or_else(|| from.id.clone())), ..f.clone() }).collect();
        return Ok((refs, Vec::new()));
    }
    let dir = carried_dir(&game.key).join(&from.id);
    let kept = |f: &FileRef| {
        let at = snapshot::resolve_in(&dir, &f.path).ok()?;
        packs::LocalFile::from_disk(f.path.clone(), at).ok().filter(|l| l.size == f.size && (f.hash.is_empty() || l.hash == f.hash))
    };
    let mut have: Vec<packs::LocalFile> = theirs.iter().filter_map(|f| kept(f)).collect();
    if have.len() < theirs.len() && fetch {
        if let (Some(cloud), Some((place, view))) = (cloud, line.filter(|(_, v)| v.get(&from.id).is_some())) {
            let work = paths::staging_dir().join(format!("carry-{}", ulid::Ulid::new()));
            let only: Vec<String> = theirs.iter().map(|f| f.path.clone()).collect();
            let got = saves::fetch(cloud.files(), place, view, from, Some(&only), &work.join("files"), &work.join("packs")).and_then(|_| {
                // Only the version at hand is kept.
                let _ = std::fs::remove_dir_all(carried_dir(&game.key));
                std::fs::create_dir_all(carried_dir(&game.key))?;
                std::fs::rename(work.join("files"), &dir)?;
                Ok(())
            });
            let _ = std::fs::remove_dir_all(&work);
            got?;
            log(format!("{}: fetched {} file(s) of the line's other platform from {} to carry over", game.key, theirs.len(), from.id));
            have = theirs.iter().filter_map(|f| kept(f)).collect();
        }
    }
    let refs = theirs.into_iter().filter(|f| !have.iter().any(|h| h.path == f.path)).map(|f| FileRef { pack: None, ..f.clone() }).collect();
    Ok((refs, have))
}

/// The saves of `game` on this device as the line compares them
/// ([`LocalState::hash`]). For a version that shares another platform's line
/// through a rule ([`crate::join::by_rule`]) that is its files and the other
/// platform's files of the version it is based on ([`carry`]), as a version
/// it uploads holds them: saves that differ from a version of the line only
/// in that platform's files are the same saves. Every other game: its files,
/// and no request.
///
/// `line` is the game's line as just read, `fetch` whether files of a
/// version of 0.6 may be downloaded to carry them over.
pub(super) fn local_here(cloud: Option<&Cloud>, game: &GameEntry, line: Option<(&Place, &LineView)>, fetch: bool) -> Result<LocalState> {
    with_carried(cloud, game, line, fetch, snapshot::local_state(game)?)
}

/// [`local_here`], for saves scanned already.
pub(super) fn with_carried(cloud: Option<&Cloud>, game: &GameEntry, line: Option<(&Place, &LineView)>, fetch: bool, local: LocalState) -> Result<LocalState> {
    if crate::join::by_rule(game).is_none() {
        return Ok(local);
    }
    let st = state::game(&game.key);
    let Some(from) = carry_source(cloud, game, &st, line)? else { return Ok(local) };
    let (refs, files) = carry(cloud, game, line, &from, fetch)?;
    Ok(local.carrying(refs, files))
}

/// For files handed to this device for a version of `game` that shares
/// another platform's line through a rule (a Switch's, over the local
/// network): the other platform's files of version `from` of the line (its
/// newest when `from` is none or not there) that a version of these files
/// takes over. By reference ([`queue_files_carrying`](super::queue_files_carrying)), and as content where
/// they have to be packed anew ([`carry`]). Nothing for any other game.
pub fn carried_for(cloud: &Cloud, game: &GameEntry, line: &OpenLine, from: Option<&str>) -> Result<(Vec<FileRef>, Vec<(String, Vec<u8>)>)> {
    if crate::join::by_rule(game).is_none() {
        return Ok((Vec::new(), Vec::new()));
    }
    let view = &line.view;
    let Some(id) = from.filter(|id| view.get(id).is_some()).or_else(|| view.head_entries().last().map(|h| h.id.as_str())) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let manifest = saves::manifest(cloud.files(), &line.place, view, id)?;
    let (refs, files) = carry(Some(cloud), game, Some((&line.place, view)), &manifest, true)?;
    let mut content = Vec::with_capacity(files.len());
    for f in files {
        content.push((f.path.clone(), std::fs::read(&f.source)?));
    }
    Ok((refs, content))
}

/// The save scope ([`crate::savelib::save_scope`]) of what a version says it
/// was made on ([`platform`](super::platform)); `None` when it doesn't say.
pub(super) fn scope_of_platform(platform: &str) -> Option<String> {
    let (kind, rest) = platform.split_once(':').unwrap_or((platform, ""));
    match kind {
        "pc" => Some("pc".to_string()),
        "console" if !rest.is_empty() => Some(rest.to_string()),
        "emu" | "recomp" => {
            crate::adapters::find(rest).map(|_| crate::savelib::save_scope(&GameEntry { adapter: Some(rest.to_string()), ..Default::default() }))
        }
        _ => None,
    }
}

/// The game version that wrote the saves of `game` on this device, when its
/// cross-platform rule says an older game can't load a save a newer one
/// wrote (`newerOnly`). `None` otherwise, or when it can't be read.
pub fn saves_version(game: &GameEntry) -> Option<String> {
    crate::crosssaves::rule_for(game).filter(|r| r.newer_only)?;
    let local = snapshot::local_state(game).ok()?;
    crate::crosssaves::local_version(game, &local.files, &local.roots)
}

/// Whether a version of the line (made on `platform` by game version
/// `version`) was saved on another platform by a newer game than the one
/// that wrote the saves here (`here`, [`saves_version`]), and the game's rule
/// says an older game can't load it. Unknown versions or platforms never
/// are. Versions of the same platform are not compared: they get the game's
/// updates the same way (a store updates them), and the saves here may be
/// older than the game installed.
pub fn too_new(game: &GameEntry, here: Option<&str>, platform: &str, version: Option<&str>) -> bool {
    let Some(rule) = crate::crosssaves::rule_for(game) else { return false };
    let other = scope_of_platform(platform).is_some_and(|s| s != crate::savelib::save_scope(game));
    other && crate::crosssaves::too_new(rule, version, here)
}
