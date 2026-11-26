//! Game records: `library/<game id>.json`, one per card in the library
//! (docs/plan-v2.md, section 3.3).
//!
//! A record is a small file that any device may rewrite, on clouds that
//! can't say "write only if nobody else did". So nothing here needs a write
//! to win: when two versions of a record meet, their lists are joined
//! ([`merge`]); when two records turn out to be one game, both files stay
//! and one points at the other ([`dedupe`]); and a save line keeps its
//! folder whichever record it ends up in, so nothing is ever moved or
//! deleted in storage.
//!
//! - **Load** ([`load`]): one listing of `library/`, then one read per
//!   record whose tag differs from the [`LibraryCache`]. Files that are not
//!   records are skipped and reported, never an error.
//! - **Live view** ([`live`], [`resolve`], [`resolve_line`]): a record with
//!   `mergedInto` is not a card; following it gives the record that
//!   replaced it.
//! - **Save** ([`save`]): read the record again, merge when another device
//!   wrote it in between, add one to `rev`, write. Creating a record is the
//!   same call.
//! - **Identity** ([`is_strong`], [`name_id`], [`find`]): a shared strong id
//!   (`steam:…`, `nx:…`, `rule:…`) is the same game; a shared name is only a
//!   suggestion.
//! - **New records** ([`new_record`]): the id is a readable slug of the name
//!   plus four random characters (`stardew-valley-7k3f`), so a device never
//!   has to ask the cloud whether an id is free.
//! - **Duplicates** ([`dedupe`]): two devices created the same game. The
//!   record made first stays and takes over the ids, folders, tags and lines
//!   of the other, which gets `mergedInto`.
//! - **Joining** ([`line_to_join`], [`same_saves`]): a game the library has
//!   no record of under its own id continues the saves of the card it is a
//!   version of, when those are the very files it keeps itself.
//!
//! | operation | requests |
//! |---|---|
//! | refresh, nothing changed | 1 list |
//! | refresh | 1 list + 1 read per record that changed |
//! | save or create a record | 1 read + 1 write |
//! | save, nothing to change | 1 read |

use super::store::Store;
use super::v2::{self, Extra, GameRecord, Line, SCOPE_SHARED};
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The folder of the game records.
pub const DIR: &str = "library";
/// The line every new record starts with.
pub const MAIN_LINE: &str = "main";
pub const NAME_PREFIX: &str = "name:";

// ----------------------------------------------------------------------- load

/// What a device remembers about the library between refreshes. Keep one
/// per cloud.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryCache {
    /// File name in `library/` to the record it held when its tag was this.
    #[serde(default)]
    pub records: BTreeMap<String, CachedRecord>,
    /// File name to tag of files named like a record that can't be
    /// understood. Remembered so that a damaged file doesn't cost a request
    /// on every refresh.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unreadable: BTreeMap<String, String>,
    /// The build that found them unreadable: a newer one looks again.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub app_version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedRecord {
    pub tag: String,
    pub record: GameRecord,
}

/// The library as it is in the cloud right now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Library {
    /// Every record, sorted by id. The ones merged into another are here
    /// too; [`live`] leaves them out.
    pub records: Vec<GameRecord>,
    /// Files in `library/` that are not a record this build understands.
    pub skipped: Vec<String>,
}

/// The game id a file in `library/` is named after.
fn record_id(file: &str) -> Option<&str> {
    file.strip_suffix(".json").filter(|id| !id.is_empty())
}

/// Read every record, using and updating `cache`.
pub fn load(store: &dyn Store, cache: &mut LibraryCache) -> Result<Library> {
    if cache.app_version != crate::APP_VERSION {
        cache.unreadable.clear();
        cache.app_version = crate::APP_VERSION.to_string();
    }

    // What exists: the listing, nothing else.
    let mut listed: BTreeMap<String, String> = BTreeMap::new();
    let mut skipped = Vec::new();
    for e in store.list(DIR)? {
        if e.is_dir {
            continue;
        }
        if record_id(&e.name).is_some() {
            listed.insert(e.name, e.tag);
        } else {
            skipped.push(e.name);
        }
    }
    cache.records.retain(|file, _| listed.contains_key(file));
    cache.unreadable.retain(|file, _| listed.contains_key(file));

    for (file, tag) in &listed {
        // A backend that can't tell whether a file changed gives no tag:
        // such a file is read every time.
        let known = !tag.is_empty()
            && (cache.records.get(file).is_some_and(|c| c.tag == *tag) || cache.unreadable.get(file) == Some(tag));
        if known {
            continue;
        }
        let id = record_id(file).unwrap_or_default();
        let bytes = store.read(&v2::game_record_path(id))?;
        cache.records.remove(file);
        cache.unreadable.remove(file);
        // `None`: deleted since the listing was made.
        let Some(bytes) = bytes else { continue };
        match serde_json::from_slice::<GameRecord>(&bytes) {
            Ok(record) if record.id == id => {
                cache.records.insert(file.clone(), CachedRecord { tag: tag.clone(), record });
            }
            _ => {
                cache.unreadable.insert(file.clone(), tag.clone());
            }
        }
    }

    skipped.extend(cache.unreadable.keys().cloned());
    skipped.sort();
    let mut records: Vec<GameRecord> = cache.records.values().map(|c| c.record.clone()).collect();
    records.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Library { records, skipped })
}

/// One record, straight from the cloud. `None` when there is none; a file
/// that can't be understood is an error, so that [`save`] never replaces
/// what this build can't read.
pub fn read(store: &dyn Store, id: &str) -> Result<Option<GameRecord>> {
    let Some(bytes) = store.read(&v2::game_record_path(id))? else { return Ok(None) };
    match serde_json::from_slice::<GameRecord>(&bytes) {
        Ok(record) if record.id == id => Ok(Some(record)),
        _ => Err(anyhow!(crate::tr!("云端的游戏记录 {id} 读不懂", "Can't understand the game record {id} in the cloud"))),
    }
}

// ------------------------------------------------------------------ live view

/// The records that are cards: all but those merged into another.
pub fn live(records: &[GameRecord]) -> impl Iterator<Item = &GameRecord> + '_ {
    records.iter().filter(|r| r.merged_into.is_none())
}

/// The record with this id, merged into another or not.
pub fn get<'a>(records: &'a [GameRecord], id: &str) -> Option<&'a GameRecord> {
    records.iter().find(|r| r.id == id)
}

/// The record that stands for game `id` now: itself, or the one at the end
/// of its `mergedInto` chain. `None` when the id is unknown or the chain
/// ends nowhere (a record that is gone, or a circle).
pub fn resolve<'a>(records: &'a [GameRecord], id: &str) -> Option<&'a GameRecord> {
    let mut at = get(records, id)?;
    // A chain is never longer than the library: one that is goes in a circle.
    for _ in 0..records.len() {
        match &at.merged_into {
            None => return Some(at),
            Some(next) => at = get(records, next)?,
        }
    }
    None
}

/// Where line `line` of game `game` is now: the live record and the line
/// that reads its folder. A line that moved to another record, or became
/// part of one of its lines, is found by its folder, which never changes.
pub fn resolve_line<'a>(records: &'a [GameRecord], game: &str, line: &str) -> Option<(&'a GameRecord, &'a Line)> {
    let from = get(records, game)?;
    let old = from.lines.iter().find(|l| l.id == line)?;
    let now = resolve(records, game)?;
    if now.id == from.id {
        return Some((now, old));
    }
    let found = now
        .lines
        .iter()
        .find(|l| l.path == old.path)
        .or_else(|| now.lines.iter().find(|l| reads(l, &old.path)))?;
    Some((now, found))
}

// ------------------------------------------------------------------- identity

/// A strong id names one game: a store's or a console's id for it, its
/// Ludusavi title, a rule. Everything of the form `<kind>:<value>` is one,
/// except `name:…`.
pub fn is_strong(id: &str) -> bool {
    match id.split_once(':') {
        Some((kind, value)) => !kind.is_empty() && !value.trim().is_empty() && !kind.eq_ignore_ascii_case("name"),
        None => false,
    }
}

fn is_name(id: &str) -> bool {
    id.len() > NAME_PREFIX.len() && id.get(..NAME_PREFIX.len()).is_some_and(|p| p.eq_ignore_ascii_case(NAME_PREFIX))
}

/// The weak id of a name: `name:` and its letters and digits in lower case.
/// Letters of every script are kept, punctuation and spaces of every script
/// are dropped ("《原神》" and "原神" are the same name). `None` when nothing
/// is left.
pub fn name_id(name: &str) -> Option<String> {
    let letters: String = name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    (!letters.is_empty()).then(|| format!("{NAME_PREFIX}{letters}"))
}

/// Ids are compared without regard to letter case: package names and the
/// hex digits of title ids get written both ways.
fn key(id: &str) -> String {
    id.chars().flat_map(char::to_lowercase).collect()
}

/// Which of two records of the same game stays: the one made first. A
/// record that doesn't say when it was made counts as the oldest (it is
/// from before records said so); the smaller id settles a tie.
fn seniority(a: &GameRecord, b: &GameRecord) -> Ordering {
    a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id))
}

/// What the library knows about a game with certain ids.
#[derive(Debug, Clone, PartialEq)]
pub enum Match<'a> {
    /// A record shares a strong id: this is the game.
    Record(&'a GameRecord),
    /// Only the name is the same. For the user to choose from, never to be
    /// used without asking: different games do share names.
    Suggestions(Vec<&'a GameRecord>),
    None,
}

/// The record for a game known by `ids` (strong ids, and `name:` ids from
/// [`name_id`]). Only live records are returned: what a merged record knew
/// belongs to the record that replaced it. When several live records share
/// a strong id (duplicates nobody has merged yet), the answer is the one
/// [`dedupe`] will keep.
pub fn find<'a>(records: &'a [GameRecord], ids: &[String]) -> Match<'a> {
    let strong: HashSet<String> = ids.iter().filter(|id| is_strong(id)).map(|id| key(id)).collect();
    let names: HashSet<String> = ids.iter().filter(|id| is_name(id)).map(|id| key(id)).collect();
    let mut sure: Vec<&GameRecord> = Vec::new();
    let mut maybe: Vec<&GameRecord> = Vec::new();
    for r in records {
        let same_game = r.ids.iter().any(|id| is_strong(id) && strong.contains(&key(id)));
        // A record's own name counts even where its ids don't list it.
        let same_name = r.ids.iter().any(|id| is_name(id) && names.contains(&key(id)))
            || name_id(&r.name).is_some_and(|n| names.contains(&n));
        if !same_game && !same_name {
            continue;
        }
        let Some(card) = resolve(records, &r.id) else { continue };
        let list = if same_game { &mut sure } else { &mut maybe };
        if !list.iter().any(|x| x.id == card.id) {
            list.push(card);
        }
    }
    if let Some(card) = sure.into_iter().min_by(|a, b| seniority(a, b)) {
        return Match::Record(card);
    }
    if maybe.is_empty() {
        Match::None
    } else {
        Match::Suggestions(maybe)
    }
}

// -------------------------------------------------------------------- joining

/// Two lines hold the same files: one format, and the same answer to which
/// files those are ([`Line::def`]). A line that doesn't say is the same as
/// none. What [`dedupe_with`] is given to decide which shared lines become
/// one history, on every device alike.
pub fn same_saves(a: &Line, b: &Line) -> bool {
    a.format == b.format && a.def.is_some() && a.def == b.def
}

/// The record whose shared line a game continues, when the library has no
/// record of it under its own id.
///
/// `ids` say which game it is: only a strong id finds a card ([`find`]); a
/// name never does. `format` and `def` say which files its saves are. Of the
/// records that are that card now (the card itself, then the ones merged
/// into it, oldest first), the answer is the first whose line `main` is
/// shared and holds those files: the game follows `main` of that record,
/// wherever [`resolve_line`] finds it.
///
/// `None`: the game is not in the library, or none of its lines there holds
/// the files this version keeps. Such a game gets a record of its own, which
/// [`dedupe_with`] then puts on the card as one more line.
pub fn line_to_join<'a>(records: &'a [GameRecord], ids: &[String], format: &str, def: &str) -> Option<&'a GameRecord> {
    let Match::Record(card) = find(records, ids) else { return None };
    let mut of_card: Vec<&GameRecord> =
        records.iter().filter(|r| resolve(records, &r.id).is_some_and(|now| now.id == card.id)).collect();
    of_card.sort_by(|a, b| (a.id != card.id).cmp(&(b.id != card.id)).then_with(|| seniority(a, b)));
    of_card.into_iter().find(|r| {
        r.lines
            .iter()
            .any(|l| l.id == MAIN_LINE && l.scope == SCOPE_SHARED && l.format == format && l.def.as_deref() == Some(def))
    })
}

// ---------------------------------------------------------------- new records

/// Crockford's base 32, as in snapshot ids: no i, l, o or u, so an id is
/// easy to read out and rarely spells anything.
const BASE32: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
/// Ids are folder names on every backend, some of which limit the length of
/// a whole path.
const SLUG_MAX: usize = 40;

/// The ASCII letters an accented Latin letter is written with in an id, so
/// that "Pokémon" doesn't become `pok-mon`.
fn unaccent(c: char) -> &'static str {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => "a",
        'ç' | 'ć' | 'č' => "c",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => "e",
        'ì' | 'í' | 'î' | 'ï' | 'ī' => "i",
        'ñ' | 'ń' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => "o",
        'ù' | 'ú' | 'û' | 'ü' | 'ū' => "u",
        'ý' | 'ÿ' => "y",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        _ => "",
    }
}

/// A name as lower-case ASCII words joined by `-`; empty when it has none.
fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !unaccent(c).is_empty() {
            s.push_str(unaccent(c));
        } else if matches!(c, '\'' | '’' | '\u{300}'..='\u{36f}') {
            // Apostrophes and combining accents are inside a word.
        } else if !s.is_empty() && !s.ends_with('-') {
            s.push('-');
        }
    }
    let mut s = s.trim_end_matches('-').to_string();
    if s.len() > SLUG_MAX {
        // At the end of a word, when that keeps most of the name.
        let cut = match s[..=SLUG_MAX].rfind('-') {
            Some(i) if i >= SLUG_MAX / 2 => i,
            _ => SLUG_MAX,
        };
        s.truncate(cut);
    }
    s
}

/// An id for a new game: the slug of its name (`game` when it has no ASCII
/// letters or digits) and four random characters. Lower-case letters,
/// digits and `-` only, so it is a valid file and folder name everywhere.
/// The random part is what lets two devices make ids without asking the
/// cloud: a million possibilities per name.
pub fn new_id(name: &str) -> String {
    let mut rng = rand::thread_rng();
    let tail: String = (0..4).map(|_| BASE32[rng.gen_range(0..BASE32.len())] as char).collect();
    match slug(name).as_str() {
        "" => format!("game-{tail}"),
        slug => format!("{slug}-{tail}"),
    }
}

/// A record for a game the library doesn't have yet. It is not in the cloud
/// until [`save`]d (`rev` 0; the first save makes it 1). `format` is the
/// format of its one shared line, `main`.
pub fn new_record(name: &str, ids: &[String], format: &str, device_id: &str, now: DateTime<Utc>) -> GameRecord {
    let id = new_id(name);
    let mut own = Vec::new();
    add_ids(&mut own, ids);
    GameRecord {
        name: name.trim().to_string(),
        ids: own,
        folders: vec![id.clone()],
        lines: vec![Line {
            id: MAIN_LINE.to_string(),
            format: format.to_string(),
            scope: SCOPE_SHARED.to_string(),
            path: v2::line_dir(&id, MAIN_LINE),
            ..Default::default()
        }],
        created_at: Some(now),
        updated_at: Some(now),
        updated_by: device_id.to_string(),
        id,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------- merge

/// Append what `more` has and `list` lacks, in the order it comes.
fn add_new(list: &mut Vec<String>, more: &[String]) {
    for item in more {
        if !list.contains(item) {
            list.push(item.clone());
        }
    }
}

/// Append the ids `list` lacks, whatever their letter case.
pub fn add_ids(list: &mut Vec<String>, more: &[String]) {
    let mut have: HashSet<String> = list.iter().map(|id| key(id)).collect();
    for id in more {
        if have.insert(key(id)) {
            list.push(id.clone());
        }
    }
}

fn add_extra(extra: &mut Extra, more: &Extra) {
    for (k, v) in more {
        if !extra.contains_key(k) {
            extra.insert(k.clone(), v.clone());
        }
    }
}

/// `line` reads the snapshots in `dir`.
fn reads(line: &Line, dir: &str) -> bool {
    line.path == dir || line.also.iter().any(|p| p == dir)
}

/// Make `line` read these folders too.
fn add_places<'a>(line: &mut Line, dirs: impl IntoIterator<Item = &'a String>) {
    for dir in dirs {
        if !reads(line, dir) {
            line.also.push(dir.clone());
        }
    }
}

/// Which of two versions of a record is the newer: the higher `rev`, then
/// the one written later. The rest only makes the answer the same whichever
/// way round the two are given.
fn version_order(a: &GameRecord, b: &GameRecord) -> Ordering {
    a.rev
        .cmp(&b.rev)
        .then_with(|| a.updated_at.cmp(&b.updated_at))
        .then_with(|| a.updated_by.cmp(&b.updated_by))
        .then_with(|| serde_json::to_string(a).unwrap_or_default().cmp(&serde_json::to_string(b).unwrap_or_default()))
}

/// Two versions of the same record as one that has what either has.
///
/// - `ids`, `folders` and `tags`: the newer version's, then what only the
///   older has.
/// - `lines`: the same, by line id. A line both have reads every folder
///   either reads; its other fields are the newer version's.
/// - `name`, `notes`, `rule`, `mergedInto`: the newer version's. This is
///   where a change can be lost when two devices write at once.
/// - `createdAt`: the earliest.
/// - Fields this build doesn't know, on the record and on its lines: kept
///   from both, the newer version's where both have one.
///
/// The order of the arguments makes no difference, and merging a version
/// in a second time changes nothing.
pub fn merge(a: &GameRecord, b: &GameRecord) -> GameRecord {
    let (new, old) = if version_order(a, b) == Ordering::Less { (b, a) } else { (a, b) };
    let mut out = new.clone();
    add_ids(&mut out.ids, &old.ids);
    add_new(&mut out.folders, &old.folders);
    add_new(&mut out.tags, &old.tags);
    for line in &old.lines {
        match out.lines.iter_mut().find(|l| l.id == line.id) {
            Some(mine) => {
                // Also `path`, should the two differ: a folder with
                // snapshots in it must never drop out of sight.
                add_places(mine, std::iter::once(&line.path).chain(&line.also));
                add_extra(&mut mine.extra, &line.extra);
            }
            None => out.lines.push(line.clone()),
        }
    }
    out.created_at = match (a.created_at, b.created_at) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, y) => x.or(y),
    };
    add_extra(&mut out.extra, &old.extra);
    out
}

// ----------------------------------------------------------------------- save

/// `cloud` is the very version `mine` was made from: nobody wrote since.
/// A version says which one it is by `rev`, `updatedAt` and `updatedBy`; a
/// record that was never stamped can't be told from another like it.
fn same_version(cloud: &GameRecord, mine: &GameRecord) -> bool {
    cloud.updated_at.is_some()
        && cloud.rev == mine.rev
        && cloud.updated_at == mine.updated_at
        && cloud.updated_by == mine.updated_by
}

/// Write `record`: a new one, or one read earlier and changed since (leave
/// its `rev`, `updatedAt` and `updatedBy` as they were read). Returns what
/// is in the cloud afterwards.
///
/// There is no conditional write, so the record is read again first:
///
/// - Nothing there, or still the version `record` was made from: `record`
///   is written as it is. This is the only way to take something out of a
///   list (a tag, an id, a line).
/// - Another device wrote in between: the two are [`merge`]d. Nothing is
///   lost from the lists; of `name`, `notes`, `rule` and `mergedInto` the
///   cloud's stay, being the later revision (with equal revisions, the
///   caller's). So a rename made on a copy that was out of date can be lost
///   once, and so can a removal: the returned record tells. It also tells
///   when the game became part of another card meanwhile (`mergedInto` is
///   set and wasn't in `record`): what was added belongs on that card.
/// - The result is what the cloud already has: nothing is written, so
///   other devices don't read the record again for nothing.
///
/// Two devices writing within the same moment can still overwrite each
/// other; whichever saves next brings its own lists back.
pub fn save(store: &dyn Store, record: &GameRecord, device_id: &str, now: DateTime<Utc>) -> Result<GameRecord> {
    if record.id.is_empty() || record.id.contains(['/', '\\']) {
        bail!(crate::tr!("游戏记录的 id 不能用：{:?}", "A game record can't have the id {:?}", record.id));
    }
    let cloud = read(store, &record.id)?;
    let mut next = record.clone();
    let mut rev = record.rev;
    if let Some(cloud) = &cloud {
        rev = rev.max(cloud.rev);
        if !same_version(cloud, record) {
            // Being written now, it is the later of two with the same `rev`.
            next.updated_at = Some(now);
            next.updated_by = device_id.to_string();
            next = merge(cloud, &next);
        }
        next.rev = cloud.rev;
        next.updated_at = cloud.updated_at;
        next.updated_by = cloud.updated_by.clone();
        if next == *cloud {
            return Ok(next);
        }
    }
    next.rev = rev + 1;
    next.updated_at = Some(now);
    next.updated_by = device_id.to_string();
    store.write(&v2::game_record_path(&next.id), &serde_json::to_vec(&next)?)?;
    Ok(next)
}

// ----------------------------------------------------------------- duplicates

/// A game that has more than one record, and what to write so that it has
/// one card. Write `survivor` first: a record pointing at a survivor that
/// hasn't taken over its lines yet would hide them.
#[derive(Debug, Clone, PartialEq)]
pub struct Duplicate {
    /// The record made first, with what the others had.
    pub survivor: GameRecord,
    /// The others, unchanged but for `mergedInto`.
    pub losers: Vec<GameRecord>,
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// An id for a line that moves to a record where its own is taken. Made
/// from where the line comes from, not at random: every device that merges
/// the same records must arrive at the same id.
fn free_line_id(lines: &[Line], line: &str, from: &str) -> String {
    let mut id = format!("{line}-{from}");
    let mut n = 1;
    while lines.iter().any(|l| l.id == id) {
        n += 1;
        id = format!("{line}-{from}-{n}");
    }
    id
}

/// Give `survivor` what `other`, a record of the same game, has. `join`
/// says whether two shared lines of one format may become one history.
fn take_over(survivor: &mut GameRecord, other: &GameRecord, join: &dyn Fn(&Line, &Line) -> bool) {
    add_ids(&mut survivor.ids, &other.ids);
    add_new(&mut survivor.folders, &other.folders);
    add_new(&mut survivor.tags, &other.tags);
    if survivor.rule.is_none() {
        survivor.rule = other.rule.clone();
    }
    if survivor.notes.is_empty() {
        survivor.notes = other.notes.clone();
    }
    let shared = |l: &Line| l.scope == SCOPE_SHARED;
    for line in &other.lines {
        let lines = &mut survivor.lines;
        // Its folder is read here already: this merge was begun before and
        // the other record never got its `mergedInto`.
        if let Some(i) = lines.iter().position(|l| reads(l, &line.path)) {
            add_places(&mut lines[i], &line.also);
            continue;
        }
        // Two shared lines of one format are one history, when the caller
        // agrees. Where both have progress, reading the line shows a fork
        // like any other.
        if shared(line) {
            if let Some(i) = lines.iter().position(|l| shared(l) && l.format == line.format && join(l, line)) {
                add_places(&mut lines[i], std::iter::once(&line.path).chain(&line.also));
                continue;
            }
        }
        // Anything else stays a line of its own, in the folder it has.
        let mut moved = line.clone();
        if lines.iter().any(|l| l.id == moved.id) {
            moved.id = free_line_id(lines, &line.id, &other.id);
        }
        lines.push(moved);
    }
}

/// Find the games that have several live records and say how to make them
/// one. Records are the same game when they share a strong id, also through
/// a third (A and B share one, B and C another: one game). Records that
/// only share a name never are.
///
/// In each group the record made first stays ([`Duplicate::survivor`]):
///
/// - the others' `ids`, `folders` and `tags` are added after its own, and
///   their `rule` and `notes` are used where it has none;
/// - a shared line of a format it already has a shared line for becomes
///   part of that line, which now also reads the other's folders (`also`);
/// - every other line moves over as it is, under a new id when its own is
///   taken, and keeps its folder.
///
/// Only records change: no file is moved or deleted. Doing it again on the
/// result, or after only the survivor got written, changes nothing more.
pub fn dedupe(records: &[GameRecord]) -> Vec<Duplicate> {
    dedupe_with(records, &|_, _| true)
}

/// [`dedupe`], with the caller deciding which lines become one history.
///
/// `join(kept, other)` is asked for two shared lines of the same format,
/// `kept` already on the surviving record: `true` makes `other`'s folders
/// part of `kept`, `false` moves `other` over as a line of its own.
///
/// A format names a set of paths, but two versions of a game for PCs don't
/// always save in the same place (a store's own folder, an Xbox container).
/// One history for both would put one version's files where the other never
/// looks, and take the other's away. So whoever joins lines has to know the
/// saves are the same ones; "never" is always safe, and leaves one card with
/// two lines. The answer must not depend on the device that asks: every
/// device that merges the same records has to arrive at the same result.
pub fn dedupe_with(records: &[GameRecord], join: &dyn Fn(&Line, &Line) -> bool) -> Vec<Duplicate> {
    let cards: Vec<&GameRecord> = live(records).collect();
    let mut parent: Vec<usize> = (0..cards.len()).collect();
    let mut first: HashMap<String, usize> = HashMap::new();
    for (i, r) in cards.iter().enumerate() {
        for id in r.ids.iter().filter(|id| is_strong(id)) {
            match first.get(&key(id)) {
                Some(&j) => {
                    let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                    parent[a] = b;
                }
                None => {
                    first.insert(key(id), i);
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<&GameRecord>> = BTreeMap::new();
    for (i, r) in cards.iter().enumerate() {
        groups.entry(root(&mut parent, i)).or_default().push(r);
    }
    let mut out: Vec<Duplicate> = groups
        .into_values()
        .filter(|group| group.len() > 1)
        .map(|mut group| {
            group.sort_by(|a, b| seniority(a, b));
            let mut survivor = group[0].clone();
            let mut losers = Vec::new();
            for other in &group[1..] {
                take_over(&mut survivor, other, join);
                losers.push(GameRecord { merged_into: Some(survivor.id.clone()), ..(**other).clone() });
            }
            Duplicate { survivor, losers }
        })
        .collect();
    out.sort_by(|a, b| a.survivor.id.cmp(&b.survivor.id));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::v2::SCOPE_INDEPENDENT;
    use chrono::TimeZone;
    use serde_json::json;

    fn at(minutes: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_780_000_000 + minutes * 60, 0).unwrap()
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn line(id: &str, format: &str, scope: &str, path: &str) -> Line {
        Line { id: id.into(), format: format.into(), scope: scope.into(), path: path.into(), ..Default::default() }
    }

    /// A saved record with one shared `pc` line, made at minute `created`.
    fn record(id: &str, ids: &[&str], created: Option<i64>) -> GameRecord {
        GameRecord {
            id: id.into(),
            name: id.into(),
            ids: strings(ids),
            folders: vec![id.into()],
            lines: vec![line(MAIN_LINE, "pc", SCOPE_SHARED, &v2::line_dir(id, MAIN_LINE))],
            created_at: created.map(at),
            rev: 1,
            updated_at: created.map(at),
            updated_by: "d-a".into(),
            ..Default::default()
        }
    }

    /// `record` as written by `device` at `minute`, as revision `rev`.
    fn version(record: &GameRecord, rev: u64, minute: i64, device: &str) -> GameRecord {
        GameRecord { rev, updated_at: Some(at(minute)), updated_by: device.into(), ..record.clone() }
    }

    fn merged_into(record: &GameRecord, id: &str) -> GameRecord {
        GameRecord { merged_into: Some(id.into()), ..record.clone() }
    }

    // ----------------------------------------------------------- identity

    #[test]
    fn everything_but_a_name_is_a_strong_id() {
        for id in [
            "steam:413150",
            "nx:0100E65002BB8000",
            "xbox:ConcernedApe.StardewValleyPC",
            "gog:1453375253",
            "epic:d5241c76f178492ea1540fce45616757",
            "ludusavi:stardew valley",
            "ra:1234",
            "wiiu:00050000101C9400",
            "ps4:CUSA00572",
            "rule:stardew-valley",
        ] {
            assert!(is_strong(id), "{id}");
        }
        for id in ["name:stardewvalley", "name:pc:hades", "Name:hades", "", "steam:", "steam: ", ":413150", "stardew"] {
            assert!(!is_strong(id), "{id:?}");
        }
    }

    #[test]
    fn a_name_id_keeps_the_letters_and_digits_of_any_script() {
        assert_eq!(name_id("Stardew Valley").unwrap(), "name:stardewvalley");
        assert_eq!(name_id("NieR:Automata™"), name_id("nier automata"));
        assert_eq!(name_id("Persona 5 Royal").unwrap(), "name:persona5royal");
        assert_eq!(name_id("塞尔达传说：旷野之息").unwrap(), "name:塞尔达传说旷野之息");
        assert_eq!(name_id("《原神》"), name_id("原神"));
        assert_eq!(name_id("ゼルダの伝説　ブレス オブ ザ ワイルド").unwrap(), "name:ゼルダの伝説ブレスオブザワイルド");
        assert_eq!(name_id("Ведьмак 3: Дикая Охота").unwrap(), "name:ведьмак3дикаяохота");
        assert_ne!(name_id("原神"), name_id("崩坏"));
        assert_eq!(name_id("?!… —"), None);
        assert!(!is_strong(&name_id("原神").unwrap()));
    }

    #[test]
    fn a_strong_id_finds_the_game_and_a_name_only_suggests() {
        let stardew = GameRecord {
            name: "Stardew Valley".into(),
            ..record("stardew-valley-7k3f", &["steam:413150", "xbox:ConcernedApe.StardewValleyPC"], Some(0))
        };
        let hades = GameRecord { name: "Hades".into(), ..record("hades-2b9d", &["steam:1145360", "name:hades"], Some(1)) };
        let other_hades = GameRecord { name: "HADES!".into(), ..record("hades-8m1x", &["gog:1"], Some(2)) };
        let genshin = GameRecord { name: "原神".into(), ..record("game-4t6v", &["rule:genshin"], Some(3)) };
        let records = vec![stardew.clone(), hades.clone(), other_hades.clone(), genshin.clone()];

        // Strong: whatever the letter case, and whatever else is asked for.
        assert_eq!(find(&records, &strings(&["nx:1", "XBOX:concernedape.stardewvalleypc"])), Match::Record(&stardew));
        assert_eq!(find(&records, &strings(&["steam:1145360", "name:stardewvalley"])), Match::Record(&hades));

        // Weak: every record of that name, by the ids it lists or by its
        // name itself.
        assert_eq!(find(&records, &strings(&["epic:9", "name:hades"])), Match::Suggestions(vec![&hades, &other_hades]));
        assert_eq!(find(&records, &[name_id("《原神》").unwrap()]), Match::Suggestions(vec![&genshin]));

        // Nothing.
        assert_eq!(find(&records, &strings(&["steam:1", "name:celeste"])), Match::None);
        assert_eq!(find(&records, &[]), Match::None);
        // The kind is part of the id.
        assert_eq!(find(&records, &strings(&["gog:413150"])), Match::None);
    }

    #[test]
    fn what_a_merged_record_knew_finds_the_record_that_replaced_it() {
        let old = record("stardew-valley-7k3f", &["steam:413150"], Some(0));
        let gone = merged_into(&record("stardew-x2m9", &["nx:0100E65002BB8000"], Some(5)), &old.id);
        let records = vec![old.clone(), gone];
        assert_eq!(find(&records, &strings(&["nx:0100e65002bb8000"])), Match::Record(&old));
        assert_eq!(find(&records, &strings(&["name:stardewx2m9"])), Match::Suggestions(vec![&old]));

        // Two cards share ids with the game and nobody merged them yet: the
        // one that will stay.
        let a = record("zelda-9z9z", &["wiiu:00050000101C9400"], Some(7));
        let b = record("zelda-1a1a", &["nx:01007EF00011E000"], Some(3));
        let records = vec![a, b.clone()];
        assert_eq!(find(&records, &strings(&["wiiu:00050000101C9400", "nx:01007EF00011E000"])), Match::Record(&b));
    }

    // ------------------------------------------------------------ joining

    #[test]
    fn a_game_continues_the_line_that_holds_its_files() {
        let with_def = |r: GameRecord, def: &str| {
            let mut r = r;
            r.lines[0].def = Some(def.into());
            r
        };
        let steam = with_def(record("hades-2b9d", &["ludusavi:hades", "name:hades"], Some(0)), "aaaa");
        let other = with_def(record("celeste", &["ludusavi:celeste"], Some(1)), "aaaa");
        let records = vec![steam.clone(), other];
        let ids = strings(&["LUDUSAVI:Hades", "name:hadesgoty"]);

        // The same game, the same files: its line.
        assert_eq!(line_to_join(&records, &ids, "pc", "aaaa"), Some(&steam));
        // Other files (another store's folder), or another format: no.
        assert_eq!(line_to_join(&records, &ids, "pc", "bbbb"), None);
        assert_eq!(line_to_join(&records, &ids, "nx", "aaaa"), None);
        // A name is never enough, and neither is nothing at all.
        assert_eq!(line_to_join(&records, &strings(&["name:hades"]), "pc", "aaaa"), None);
        assert_eq!(line_to_join(&records, &[], "pc", "aaaa"), None);
        // A line that doesn't say which files it holds is not assumed to
        // hold these.
        let silent = vec![record("hades-2b9d", &["ludusavi:hades"], Some(0))];
        assert_eq!(line_to_join(&silent, &ids, "pc", "aaaa"), None);
        // A line a device keeps to itself is not the one to share.
        let mut solo = steam.clone();
        solo.lines[0].scope = SCOPE_INDEPENDENT.into();
        assert_eq!(line_to_join(&[solo], &ids, "pc", "aaaa"), None);

        // A card made of two records: each kept its own line. The game
        // continues the one with its files, found through the record that
        // line came with.
        let xbox = with_def(record("hades-xbox", &["ludusavi:hades", "xbox:supergiantgames.hades"], Some(5)), "bbbb");
        let merged = dedupe_with(&[steam.clone(), xbox.clone()], &same_saves).remove(0);
        assert_eq!(merged.survivor.lines.len(), 2);
        let records = vec![merged.survivor.clone(), merged.losers[0].clone()];
        assert_eq!(line_to_join(&records, &ids, "pc", "aaaa").map(|r| r.id.as_str()), Some("hades-2b9d"));
        let joined = line_to_join(&records, &ids, "pc", "bbbb").unwrap();
        assert_eq!(joined.id, "hades-xbox");
        let (card, line) = resolve_line(&records, &joined.id, MAIN_LINE).unwrap();
        assert_eq!((card.id.as_str(), line.id.as_str()), ("hades-2b9d", "main-hades-xbox"));
        assert_eq!(line_to_join(&records, &ids, "pc", "cccc"), None);
    }

    #[test]
    fn lines_with_the_same_files_become_one_history() {
        let mut a = record("stardew", &["ludusavi:stardew valley"], Some(0));
        let mut b = record("stardew-valley", &["ludusavi:stardew valley"], Some(1));
        let mut c = record("stardew-xbox", &["ludusavi:stardew valley"], Some(2));
        a.lines[0].def = Some("aaaa".into());
        b.lines[0].def = Some("aaaa".into());
        c.lines[0].def = Some("bbbb".into());
        assert!(same_saves(&a.lines[0], &b.lines[0]));
        assert!(!same_saves(&a.lines[0], &c.lines[0]));
        // Nothing known is not "the same".
        let unknown = record("x", &[], Some(0));
        assert!(!same_saves(&unknown.lines[0], &unknown.lines[0]));
        let mut nx = a.lines[0].clone();
        nx.format = "nx".into();
        assert!(!same_saves(&a.lines[0], &nx));

        let out = dedupe_with(&[a.clone(), b, c], &same_saves).remove(0);
        assert_eq!(out.survivor.id, "stardew");
        assert_eq!(out.survivor.lines.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["main", "main-stardew-xbox"]);
        assert_eq!(out.survivor.lines[0].also, strings(&["games/stardew-valley/saves/main"]));
        assert_eq!(out.survivor.lines[0].def.as_deref(), Some("aaaa"));
        assert_eq!(out.survivor.lines[1].def.as_deref(), Some("bbbb"));
    }

    // -------------------------------------------------------- new records

    #[test]
    fn slugs_are_ascii_words() {
        assert_eq!(slug("Stardew Valley"), "stardew-valley");
        assert_eq!(slug("  The Witcher® 3: Wild Hunt — GOTY  "), "the-witcher-3-wild-hunt-goty");
        assert_eq!(slug("Baldur's Gate 3"), "baldurs-gate-3");
        assert_eq!(slug("Pokémon Legends: Arceus"), "pokemon-legends-arceus");
        assert_eq!(slug("Poke\u{301}mon"), "pokemon");
        assert_eq!(slug("塞尔达传说 Breath of the Wild"), "breath-of-the-wild");
        assert_eq!(slug("東方Project紅魔郷EoSD"), "project-eosd");
        assert_eq!(slug("原神"), "");
        assert_eq!(slug("?!"), "");

        // Long names end at the end of a word, or at the limit.
        assert_eq!(
            slug("The Legend of Heroes: Trails of Cold Steel IV -The End of Saga-"),
            "the-legend-of-heroes-trails-of-cold"
        );
        assert_eq!(slug(&"a".repeat(60)), "a".repeat(SLUG_MAX));
        assert_eq!(slug(&format!("a {}", "b".repeat(60))), format!("a-{}", "b".repeat(SLUG_MAX - 2)));
    }

    /// `<slug>-<four base-32 characters>`.
    fn tail_of<'a>(id: &'a str, slug: &str) -> Option<&'a str> {
        id.strip_prefix(slug)?
            .strip_prefix('-')
            .filter(|tail| tail.len() == 4 && tail.bytes().all(|b| BASE32.contains(&b)))
    }

    #[test]
    fn new_ids_are_file_names_and_differ() {
        assert!(tail_of(&new_id("Stardew Valley"), "stardew-valley").is_some());
        assert!(tail_of(&new_id("原神"), "game").is_some());
        assert!(tail_of(&new_id(""), "game").is_some());
        // Names Windows keeps for itself are no problem with the tail on.
        assert!(tail_of(&new_id("CON"), "con").is_some());

        let long = "long name ".repeat(30);
        for name in ["Stardew Valley", "塞尔达传说：旷野之息", "A/B\\C:D*E?F\"G<H>I|J.", long.as_str()] {
            let id = new_id(name);
            assert!(id.len() <= SLUG_MAX + 5, "{id}");
            assert!(id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'), "{id}");
            assert!(!id.starts_with('-') && !id.ends_with('-') && !id.contains("--"), "{id}");
        }

        // A million possibilities: a hundred ids for one name may collide
        // once in a while, never mostly.
        let ids: HashSet<String> = (0..100).map(|_| new_id("Stardew Valley")).collect();
        assert!(ids.len() > 90, "{} different ids out of 100", ids.len());
    }

    #[test]
    fn a_new_record_has_one_folder_and_one_shared_line() {
        let ids = strings(&["steam:413150", "rule:stardew-valley", "STEAM:413150"]);
        let r = new_record(" Stardew Valley ", &ids, "rule:stardew-valley", "d-a", at(0));
        assert!(tail_of(&r.id, "stardew-valley").is_some());
        assert_eq!(r.name, "Stardew Valley");
        assert_eq!(r.ids, strings(&["steam:413150", "rule:stardew-valley"]));
        assert_eq!(r.folders, vec![r.id.clone()]);
        assert_eq!(
            r.lines,
            vec![line("main", "rule:stardew-valley", "shared", &format!("games/{}/saves/main", r.id))]
        );
        assert_eq!((r.rev, r.created_at, r.merged_into.clone()), (0, Some(at(0)), None));
        assert_eq!(r.updated_by, "d-a");
    }

    // -------------------------------------------------------------- merge

    #[test]
    fn merging_two_versions_keeps_what_either_has() {
        let base = record("stardew-7k3f", &["steam:413150"], Some(0));

        let mut a = version(&base, 2, 10, "d-a");
        a.name = "Stardew Valley".into();
        a.rule = Some("stardew-valley".into());
        a.ids.push("nx:0100E65002BB8000".into());
        a.tags.push("finished".into());
        a.lines.push(line("solo-9d2k", "pc", SCOPE_INDEPENDENT, "games/stardew-7k3f/saves/solo-9d2k"));

        let mut b = version(&base, 3, 5, "d-b");
        b.name = "Stardew".into();
        b.notes = "co-op farm".into();
        b.ids.extend(strings(&["xbox:ConcernedApe.StardewValleyPC", "nx:0100e65002bb8000"]));
        b.folders.push("stardew".into());
        b.tags.push("co-op".into());
        b.lines[0].also.push("games/stardew/snapshots".into());

        let m = merge(&a, &b);
        assert_eq!(m, merge(&b, &a), "the order of the arguments makes no difference");

        // Lists: the newer version's, then what only the older has. An id
        // written in another letter case is the same id.
        assert_eq!(m.ids, strings(&["steam:413150", "xbox:ConcernedApe.StardewValleyPC", "nx:0100e65002bb8000"]));
        assert_eq!(m.folders, strings(&["stardew-7k3f", "stardew"]));
        assert_eq!(m.tags, strings(&["co-op", "finished"]));
        assert_eq!(m.lines.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["main", "solo-9d2k"]);
        assert_eq!(m.lines[0].also, strings(&["games/stardew/snapshots"]));
        assert_eq!(m.lines[1], a.lines[1]);

        // Single values: the higher `rev`, even where it was written
        // earlier by the clock and even where it has none.
        assert_eq!((m.name.as_str(), m.notes.as_str(), m.rule.clone()), ("Stardew", "co-op farm", None));
        assert_eq!((m.rev, m.updated_at, m.updated_by.as_str()), (3, Some(at(5)), "d-b"));
        assert_eq!(m.created_at, Some(at(0)));

        // Merging a version in again changes nothing.
        assert_eq!(merge(&m, &a), m);
        assert_eq!(merge(&b, &m), m);
        assert_eq!(merge(&m, &m), m);
        assert_eq!(merge(&a, &a), a);
    }

    #[test]
    fn fields_this_build_does_not_know_are_kept_from_both_versions() {
        let base = record("stardew-7k3f", &["steam:413150"], Some(0));
        let mut a = version(&base, 4, 1, "d-a");
        a.extra.insert("collection".into(), json!({ "shelf": 2 }));
        a.extra.insert("rating".into(), json!(3));
        a.lines[0].extra.insert("owner".into(), json!("family"));
        a.lines[0].extra.insert("color".into(), json!("green"));
        let mut b = version(&base, 5, 2, "d-b");
        b.extra.insert("hidden".into(), json!(true));
        b.extra.insert("rating".into(), json!(5));
        b.lines[0].extra.insert("color".into(), json!("red"));
        b.lines[0].extra.insert("icon".into(), json!("deck"));

        let m = merge(&a, &b);
        assert_eq!(m, merge(&b, &a));
        assert_eq!(
            serde_json::Value::Object(m.extra.clone()),
            json!({ "collection": { "shelf": 2 }, "hidden": true, "rating": 5 })
        );
        assert_eq!(
            serde_json::Value::Object(m.lines[0].extra.clone()),
            json!({ "owner": "family", "color": "red", "icon": "deck" })
        );
        // And they are written back where they were found.
        let out = serde_json::to_value(&m).unwrap();
        assert_eq!(out["collection"]["shelf"], 2);
        assert_eq!(out["lines"][0]["owner"], "family");
        assert_eq!(merge(&m, &a), m);
    }

    #[test]
    fn equal_revisions_go_by_the_time_and_then_by_a_fixed_rule() {
        let base = record("hades-2b9d", &["steam:1145360"], Some(0));
        let early = GameRecord { name: "Hades (early)".into(), ..version(&base, 2, 10, "d-b") };
        let late = GameRecord { name: "Hades (late)".into(), ..version(&base, 2, 20, "d-a") };
        assert_eq!(merge(&early, &late).name, "Hades (late)");
        assert_eq!(merge(&late, &early).name, "Hades (late)");

        // Same revision, same moment: the answer is the same both ways.
        let x = GameRecord { name: "X".into(), ..version(&base, 2, 10, "d-a") };
        let y = GameRecord { name: "Y".into(), ..version(&base, 2, 10, "d-a") };
        assert_eq!(merge(&x, &y), merge(&y, &x));
    }

    #[test]
    fn the_earliest_creation_time_is_kept() {
        let base = record("hades-2b9d", &[], Some(9));
        let newer = GameRecord { created_at: Some(at(4)), ..version(&base, 3, 30, "d-a") };
        assert_eq!(merge(&base, &newer).created_at, Some(at(4)));
        let newer = GameRecord { created_at: None, ..version(&base, 3, 30, "d-a") };
        assert_eq!(merge(&newer, &base).created_at, Some(at(9)));
    }

    #[test]
    fn a_line_both_versions_have_reads_every_folder_either_reads() {
        let base = record("hades-2b9d", &[], Some(0));
        let mut a = version(&base, 2, 1, "d-a");
        a.lines[0].also = strings(&["games/hades/snapshots"]);
        let mut b = version(&base, 3, 2, "d-b");
        b.lines[0].path = "games/hades-2b9d/saves/main-2".into();
        b.lines[0].name = Some("Main".into());
        b.lines[0].also = strings(&["games/other/saves/main"]);

        let m = merge(&a, &b);
        assert_eq!(m.lines.len(), 1);
        assert_eq!(m.lines[0].path, "games/hades-2b9d/saves/main-2");
        assert_eq!(m.lines[0].name.as_deref(), Some("Main"));
        assert_eq!(
            m.lines[0].also,
            strings(&["games/other/saves/main", "games/hades-2b9d/saves/main", "games/hades/snapshots"])
        );
        assert_eq!(merge(&m, &a), m);
    }

    // ---------------------------------------------------------- live view

    #[test]
    fn merged_records_are_followed_and_a_circle_does_not_hang() {
        let a = record("a", &[], Some(0));
        let b = merged_into(&record("b", &[], Some(1)), "a");
        let c = merged_into(&record("c", &[], Some(2)), "b");
        // Broken by hand or by a bug: a circle, a record pointing at itself,
        // one pointing at a record that is gone.
        let x = merged_into(&record("x", &[], Some(3)), "y");
        let y = merged_into(&record("y", &[], Some(4)), "x");
        let own = merged_into(&record("own", &[], Some(5)), "own");
        let lost = merged_into(&record("lost", &[], Some(6)), "deleted");
        let records = vec![a.clone(), b, c, x, y, own, lost];

        assert_eq!(live(&records).map(|r| r.id.as_str()).collect::<Vec<_>>(), ["a"]);
        for id in ["a", "b", "c"] {
            assert_eq!(resolve(&records, id), Some(&a), "{id}");
        }
        for id in ["x", "y", "own", "lost", "never-existed"] {
            assert_eq!(resolve(&records, id), None, "{id}");
        }
        assert_eq!(find(&records, &strings(&["name:x"])), Match::None);
        assert!(dedupe(&records).is_empty());
    }

    // --------------------------------------------------------- duplicates

    #[test]
    fn two_records_of_one_game_become_one() {
        // Made offline on two devices: the Steam version here, the Switch
        // version there. The rule both recognised is what they share.
        let mut old = record("stardew-valley-7k3f", &["steam:413150", "rule:stardew-valley"], Some(0));
        let mut new = record("stardew-valley-x2m9", &["nx:0100E65002BB8000", "RULE:Stardew-Valley"], Some(5));
        for r in [&mut old, &mut new] {
            r.lines[0].format = "rule:stardew-valley".into();
        }
        old.lines[0].extra.insert("owner".into(), json!("family"));
        new.rule = Some("stardew-valley".into());
        new.notes = "co-op farm".into();
        new.tags = strings(&["finished"]);
        let hades = record("hades-2b9d", &["steam:1145360"], Some(1));

        let out = dedupe(&[new.clone(), hades.clone(), old.clone()]);
        assert_eq!(out.len(), 1);
        let Duplicate { survivor, losers } = &out[0];

        // The one made first stays and has what the other had.
        let mut expected = old.clone();
        expected.ids = strings(&["steam:413150", "rule:stardew-valley", "nx:0100E65002BB8000"]);
        expected.folders = strings(&["stardew-valley-7k3f", "stardew-valley-x2m9"]);
        expected.tags = strings(&["finished"]);
        expected.rule = Some("stardew-valley".into());
        expected.notes = "co-op farm".into();
        // Two shared lines of one format: one line that reads both folders.
        expected.lines[0].also = strings(&["games/stardew-valley-x2m9/saves/main"]);
        assert_eq!(*survivor, expected);
        assert_eq!(survivor.lines.len(), 1);

        // The other is as it was, and points at it.
        assert_eq!(*losers, vec![merged_into(&new, &old.id)]);

        // Once written there is nothing left to do.
        let after = vec![survivor.clone(), losers[0].clone(), hades];
        assert!(dedupe(&after).is_empty());
        assert_eq!(resolve(&after, &new.id), Some(survivor));
        let (card, main) = resolve_line(&after, &new.id, MAIN_LINE).unwrap();
        assert_eq!((card, main), (survivor, &survivor.lines[0]));
        assert_eq!(resolve_line(&after, &new.id, "no-such-line"), None);
    }

    #[test]
    fn records_joined_through_a_third_are_one_game() {
        // The Steam version and the Switch version share nothing; the third
        // record knows both ids.
        let steam = record("stardew-steam", &["steam:413150"], Some(3));
        let mut nx = record("stardew-nx", &["nx:0100E65002BB8000"], Some(1));
        nx.lines[0].also = strings(&["games/stardew-nx/snapshots"]);
        let both = record("stardew-both", &["nx:0100e65002bb8000", "steam:413150", "name:stardewvalley"], Some(2));
        let lookalike = record("stardew-fan-game", &["name:stardewvalley"], Some(0));

        let out = dedupe(&[steam.clone(), both.clone(), lookalike, nx.clone()]);
        assert_eq!(out.len(), 1);
        let Duplicate { survivor, losers } = &out[0];
        assert_eq!(survivor.id, "stardew-nx");
        // The others in the order they were made.
        assert_eq!(*losers, vec![merged_into(&both, "stardew-nx"), merged_into(&steam, "stardew-nx")]);
        assert_eq!(survivor.ids, strings(&["nx:0100E65002BB8000", "steam:413150", "name:stardewvalley"]));
        assert_eq!(survivor.folders, strings(&["stardew-nx", "stardew-both", "stardew-steam"]));
        assert_eq!(survivor.lines.len(), 1);
        assert_eq!(
            survivor.lines[0].also,
            strings(&["games/stardew-nx/snapshots", "games/stardew-both/saves/main", "games/stardew-steam/saves/main"])
        );
    }

    #[test]
    fn records_that_only_share_a_name_are_never_merged() {
        let a = record("hades-2b9d", &["steam:1145360", "name:hades"], Some(0));
        let b = record("hades-8m1x", &["gog:1", "name:hades"], Some(1));
        // Not ids at all.
        let c = record("c", &["", "hades"], Some(2));
        let d = record("d", &["", "hades"], Some(3));
        assert!(dedupe(&[a, b, c, d]).is_empty());
    }

    #[test]
    fn lines_that_cannot_be_one_move_over_as_they_are() {
        // Breath of the Wild on Switch and on Wii U: one card, saves that
        // don't read each other.
        let mut nx = record("zelda-botw-1a1a", &["nx:01007EF00011E000", "rule:zelda-botw"], Some(0));
        nx.lines = vec![
            line("main", "nx", SCOPE_SHARED, "games/zelda-botw-1a1a/saves/main"),
            line("solo-9d2k", "nx", SCOPE_INDEPENDENT, "games/zelda-botw-1a1a/saves/solo-9d2k"),
        ];
        let mut wiiu = record("zelda-botw-9z9z", &["wiiu:00050000101C9400", "rule:zelda-botw"], Some(1));
        let mut kid = line("solo-9d2k", "nx", SCOPE_INDEPENDENT, "games/zelda-botw-9z9z/saves/solo-9d2k");
        kid.name = Some("Kid's PC".into());
        kid.extra.insert("icon".into(), json!("deck"));
        wiiu.lines = vec![
            line("main", "wiiu", SCOPE_SHARED, "games/zelda-botw-9z9z/saves/main"),
            kid.clone(),
            line("emu", "nx", SCOPE_INDEPENDENT, "games/zelda-botw-9z9z/saves/emu"),
            // A scope from a later version is not known to be shared.
            line("later", "nx", "group:family", "games/zelda-botw-9z9z/saves/later"),
        ];

        let out = dedupe(&[nx.clone(), wiiu.clone()]);
        let survivor = &out[0].survivor;
        assert_eq!(survivor.id, nx.id);
        assert_eq!(survivor.lines[..2], nx.lines[..]);
        // A taken id gets the name of where the line is from; the folder
        // and everything else stay.
        assert_eq!(
            survivor.lines[2..],
            [
                Line { id: "main-zelda-botw-9z9z".into(), ..wiiu.lines[0].clone() },
                Line { id: "solo-9d2k-zelda-botw-9z9z".into(), ..kid },
                wiiu.lines[2].clone(),
                wiiu.lines[3].clone(),
            ]
        );

        // The line is found again from what a device remembers of it.
        let after = vec![survivor.clone(), out[0].losers[0].clone()];
        let (card, line) = resolve_line(&after, &wiiu.id, "main").unwrap();
        assert_eq!((card.id.as_str(), line.id.as_str()), (nx.id.as_str(), "main-zelda-botw-9z9z"));
        assert_eq!(line.path, "games/zelda-botw-9z9z/saves/main");
        let (_, line) = resolve_line(&after, &nx.id, "main").unwrap();
        assert_eq!(line.path, "games/zelda-botw-1a1a/saves/main");
        assert!(dedupe(&after).is_empty());
    }

    #[test]
    fn the_record_made_first_stays() {
        let ids = ["steam:1145360"];
        // By the time it was made, not by its id.
        let out = dedupe(&[record("hades-a", &ids, Some(9)), record("hades-b", &ids, Some(3))]);
        assert_eq!(out[0].survivor.id, "hades-b");
        // A record that doesn't say (from before the migration) is older
        // than any that does.
        let out = dedupe(&[record("hades-a", &ids, Some(0)), record("hades-b", &ids, None)]);
        assert_eq!(out[0].survivor.id, "hades-b");
        // Made at the same moment: the smaller id, on every device.
        let out = dedupe(&[record("hades-b", &ids, Some(3)), record("hades-a", &ids, Some(3))]);
        assert_eq!(out[0].survivor.id, "hades-a");
        assert_eq!(out[0].losers[0].merged_into.as_deref(), Some("hades-a"));

        // Each game on its own, in the order of the survivors.
        let out = dedupe(&[
            record("z-2", &["steam:2"], Some(1)),
            record("a-1", &["steam:1"], Some(1)),
            record("z-1", &["steam:2"], Some(0)),
            record("a-2", &["steam:1"], Some(0)),
        ]);
        let pairs: Vec<(&str, &str)> = out.iter().map(|d| (d.survivor.id.as_str(), d.losers[0].id.as_str())).collect();
        assert_eq!(pairs, [("a-2", "a-1"), ("z-1", "z-2")]);
    }

    #[test]
    fn a_merge_that_was_cut_off_is_finished_without_doubling_anything() {
        let old = record("stardew-7k3f", &["steam:413150"], Some(0));
        let mut new = record("stardew-x2m9", &["steam:413150"], Some(5));
        new.lines.push(line("solo-9d2k", "pc", SCOPE_INDEPENDENT, "games/stardew-x2m9/saves/solo-9d2k"));
        new.lines.push(line("main-2", "nx", SCOPE_SHARED, "games/stardew-x2m9/saves/main-2"));
        let first = dedupe(&[old.clone(), new.clone()]).remove(0);
        assert_eq!(first.survivor.lines.len(), 3);

        // The survivor got written, the other record did not.
        let again = dedupe(&[first.survivor.clone(), new.clone()]).remove(0);
        assert_eq!(again, first);

        // Meanwhile the other record's device has pointed its line at one
        // more folder: that is picked up.
        new.lines[0].also = strings(&["games/stardew/snapshots"]);
        let again = dedupe(&[first.survivor.clone(), new]).remove(0);
        assert_eq!(
            again.survivor.lines[0].also,
            strings(&["games/stardew-x2m9/saves/main", "games/stardew/snapshots"])
        );
        assert_eq!(again.survivor.lines[1..], first.survivor.lines[1..]);
    }
}
