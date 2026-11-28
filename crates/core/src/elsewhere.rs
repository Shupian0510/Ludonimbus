//! A game's saves from elsewhere (docs/plan-positioning.md, section 7: S1
//! and S2): every other source of saves on the game's card besides the save
//! line this install follows, each with its newest save and the ones kept
//! there, and how it can be brought here ([`sources`]). Of those, the newest
//! progress another version of the game has ([`newer`]) is what the game
//! page's hint row and the launch wrapper's question offer.
//!
//! A source is one of:
//!
//! - another save line of the card that holds the same files
//!   ([`Cloud::other_lines`]): a line another device keeps to itself
//!   ([`Kind::Device`]), the shared line while this device keeps its own
//!   ([`Kind::Shared`]), or the line of another version that keeps the same
//!   files (a Switch game on the console and in an emulator). Taken as it is
//!   ([`sync::take_from`]).
//! - another version of the game in the library (the families of
//!   [`savelib`]: another store's, the Xbox app's, another platform's), with
//!   the line it follows. Brought here by [`transfer::apply`], which writes
//!   the saves the way this version keeps them; a pair no rule says can read
//!   each other's saves can't be ([`Take::Never`]).
//!
//! Only other versions count as newer progress. A line another device
//! keeps to itself is kept apart on purpose, and so is the shared line by
//! a device that keeps its own: they are listed, never offered.
//!
//! What it costs (docs/plan-v2.md, section 5.6): the game records (at most
//! once a minute) and, for each source, one listing of its line (and its
//! index when that changed). That is what the list of other lines and the
//! offer of newer saves in another version read between them before; the
//! game page reads it once for both. Deciding whether one is newer than the
//! save here reads nothing more unless there is a candidate: then what this
//! device remembers of its own line, and the saves here (no hashing).

use crate::cloud::library::{self, MAIN_LINE};
use crate::cloud::line::LineView;
use crate::cloud::{self, upgrade, v2, Cloud};
use crate::config::GameEntry;
use crate::snapshot::{self, SnapshotSummary};
use crate::sync::{self, Ctx, Outcome};
use crate::{crosssaves, games, logger, savelib, state, transfer};
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Datelike, Local, Utc};
use serde::Serialize;
use ts_rs::TS;

/// How many kept saves a source lists besides its newest one.
const KEPT_SHOWN: usize = 5;

/// What a source of saves is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SaveSourceKind")]
pub enum Kind {
    /// Another version of the game: another store's, the Xbox app's, the
    /// Switch itself, an emulator's.
    Version,
    /// A line another device keeps to itself (docs/plan-v2.md, section 7).
    Device,
    /// The line the game's devices share, while this device keeps its own.
    Shared,
}

/// Whether, and how, a source's saves can be brought here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(rename = "SaveTake")]
pub enum Take {
    /// The same files: taken as they are.
    Copy,
    /// Written the way this version keeps them: `xbox` (out of or into the
    /// Xbox app's containers), `rule` (by the game's cross-platform rule),
    /// `files` (placed by file name).
    Convert { how: String },
    /// Can't be brought here; `reason` says why.
    Never { reason: String },
}

/// One source of saves of a game besides the line this install follows.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SaveSource")]
pub struct Source {
    /// Unique among a game's sources: `line:<line id>` or `version:<key>`.
    pub id: String,
    pub kind: Kind,
    /// What it is called, in the interface language: "Xbox 版", "Ryujinx",
    /// "孩子的电脑"…
    pub name: String,
    /// What its newest save was made on ([`sync::platform`]): which
    /// program's icon its group shows.
    pub platform: String,
    /// The line to take from ([`sync::take_from`]), for a line of the card
    /// that holds the same files.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub line: Option<String>,
    /// The version to bring saves from ([`transfer::apply`]), for another
    /// version of the game.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub key: Option<String>,
    /// The devices its listed saves were made on, the newest first.
    pub devices: Vec<String>,
    /// Its newest save first (`head`, when it has one that isn't missing a
    /// pack), then the ones kept there, newest first.
    pub snapshots: Vec<SnapshotSummary>,
    pub take: Take,
    /// Which files its line holds ([`v2::Line::def`]), where that is known:
    /// a line that doesn't say is never offered to share
    /// ([`crate::join::suggestion`]).
    #[serde(skip)]
    pub def: Option<String>,
    /// Where its line is ([`v2::Line::path`]): to tell whether the line a
    /// suggestion offers is this one ([`offers`]).
    #[serde(skip)]
    pub dir: String,
}

impl Source {
    /// Its newest save, when it has one that can be taken.
    pub fn newest(&self) -> Option<&SnapshotSummary> {
        self.snapshots.first().filter(|s| s.head && !s.incomplete)
    }
}

/// Newer progress in another version of the game than the save here.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "NewerElsewhere")]
pub struct Newer {
    pub source: Source,
    /// Its newest save: the progress offered.
    pub snapshot: SnapshotSummary,
    /// The save this device is at, as far as it knows; `None` without one.
    pub here: Option<SnapshotSummary>,
    /// What this version is called ([`version_name`]).
    pub here_name: String,
}

fn log(msg: impl AsRef<str>) {
    logger::log("elsewhere", msg);
}

/// How a platform a version names ([`sync::platform`]) is called.
pub fn platform_name(platform: &str) -> String {
    let (kind, rest) = platform.split_once(':').unwrap_or((platform, ""));
    match (kind, rest) {
        ("emu" | "recomp", r) => crate::adapters::find(r).map(|a| a.name.clone()).unwrap_or_else(|| r.to_string()),
        ("console", "switch") => "Switch".to_string(),
        ("pc", "xbox") => crate::tr!("Xbox 版", "Xbox version"),
        ("pc", "steam") => crate::tr!("Steam 版", "Steam version"),
        ("pc", _) => crate::tr!("PC 版", "PC version"),
        ("", _) => crate::tr!("另一个版本", "Another version"),
        _ => platform.to_string(),
    }
}

/// How a kind of install a line says began it ([`v2::Line::kind`]) is
/// called: "Steam 版", "GOG 版", "Xbox 版", "PC 版", "Switch", an emulator.
pub fn kind_name(kind: &str) -> String {
    let store = |name: &str| crate::tr!("{} 版", "{} version", name);
    match kind.split_once(':') {
        Some(("emu" | "recomp", r)) => crate::adapters::find(r).map(|a| a.name.clone()).unwrap_or_else(|| r.to_string()),
        _ => match kind {
            "steam" => store("Steam"),
            "epic" => store("Epic"),
            "gog" => store("GOG"),
            "xbox" => store("Xbox"),
            "switch" => "Switch".to_string(),
            _ => store("PC"),
        },
    }
}

/// What a version of a game is called where its saves are compared: the
/// group the user gave it, else what kind of install it is
/// ([`upgrade::line_kind`]: its store, the Switch, its emulator).
pub fn version_name(g: &GameEntry) -> String {
    games::source(g).label.unwrap_or_else(|| kind_name(&upgrade::line_kind(g)))
}

/// [`version_name`] of another version: what the kind of its line says
/// (`kind`, written by the device that began it) or what its saves were
/// made on (`platform`) where that says more than this device can tell. A
/// version only in the cloud has no program here to tell its store by.
fn name_of(g: &GameEntry, kind: Option<&str>, platform: &str) -> String {
    let s = games::source(g);
    match (s.label, kind, platform) {
        (Some(label), _, _) => label,
        (None, Some(kind), _) => kind_name(kind),
        (None, None, "pc:xbox" | "pc:steam") => platform_name(platform),
        _ => version_name(g),
    }
}

/// Whether the line `s` offers to share is the line of `source`: the
/// record it names, whose shared line it is, is that line now. The game
/// records were read for the suggestion (not again within a minute).
pub fn offers(ctx: &Ctx, source: &Source, s: &crate::join::Suggestion) -> bool {
    let Ok(library) = ctx.cloud().and_then(|c| c.library()) else { return false };
    library::resolve_line(&library.records, &s.record, MAIN_LINE).is_some_and(|(_, l)| l.path == source.dir)
}

/// The other versions of `game` in the library: on this device, then only
/// in the cloud (as this device last listed the cloud's games).
fn other_versions(ctx: &Ctx, game: &GameEntry) -> Vec<GameEntry> {
    let cached = savelib::cached(&ctx.cfg);
    let fam = savelib::index(&ctx.cfg, &cached.games);
    let mine = fam.get(&game.key).cloned().unwrap_or_else(|| game.key.clone());
    let same = |k: &str| k != game.key && fam.get(k) == Some(&mine);
    ctx.cfg
        .games
        .iter()
        .filter(|g| same(&g.key))
        .cloned()
        .chain(cached.games.iter().filter(|c| same(&c.key) && ctx.cfg.game(&c.key).is_none()).map(savelib::entry_of))
        .collect()
}

/// Whether, and how, saves of the version `src` (its newest made on
/// `platform`) can be brought to `tgt`, worked out from what the two are
/// without reading the saves ([`transfer::plan`] says it exactly).
pub fn fit(src: &GameEntry, platform: &str, tgt: &GameEntry, name: &str) -> Take {
    let rule = crosssaves::shared(src, tgt);
    if rule.is_none() && savelib::save_scope(src) != savelib::save_scope(tgt) {
        return Take::Never {
            reason: crate::tr!(
                "{}和这个版本的存档格式不同，也没有规则说明它们能互相读取",
                "{} keeps its saves in another format than this version, and no rule says they can read each other's",
                name
            ),
        };
    }
    let xbox = platform == "pc:xbox" || upgrade::line_kind(tgt) == "xbox";
    match () {
        _ if xbox => Take::Convert { how: "xbox".to_string() },
        _ if transfer::same_files(src, tgt) => Take::Copy,
        _ if rule.is_some() => Take::Convert { how: "rule".to_string() },
        _ => Take::Convert { how: "files".to_string() },
    }
}

/// What a line lists as a source: its newest save that isn't missing a
/// pack (the newest of its heads when it has forked), then the ones kept.
fn listed(view: &LineView) -> Vec<SnapshotSummary> {
    let usable = view.usable();
    let newest = |a: &&v2::IndexEntry, b: &&v2::IndexEntry| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id));
    let head = usable.head_entries().max_by(newest);
    let mut out: Vec<SnapshotSummary> = head.map(|h| SnapshotSummary::of(h, true)).into_iter().collect();
    let mut kept: Vec<&v2::IndexEntry> = view.snapshots.iter().filter(|s| s.pin.is_some() && head.is_none_or(|h| h.id != s.id)).collect();
    kept.sort_by(|a, b| newest(b, a));
    out.extend(kept.into_iter().take(KEPT_SHOWN).map(|s| SnapshotSummary::of(s, false)));
    out
}

fn devices_of(list: &[SnapshotSummary]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in list {
        if !s.device_name.is_empty() && !out.contains(&s.device_name) {
            out.push(s.device_name.clone());
        }
    }
    out
}

/// The line of the card that reads the folder `dir`.
fn line_at<'a>(records: &'a [v2::GameRecord], dir: &str) -> Option<&'a v2::Line> {
    library::live(records).flat_map(|r| r.lines.iter()).find(|l| l.path == dir)
}

/// Every other source of saves of `game` (see the module's notes): the
/// other versions first, then the shared line, then the lines devices keep
/// to themselves, each group newest first. A source with no saves isn't
/// listed. A source that can't be read for another reason than the cloud
/// being out of reach is left out (and logged); out of reach is an error.
pub fn sources(ctx: &Ctx, game: &GameEntry) -> Result<Vec<Source>> {
    let cloud = ctx.cloud()?;
    let mine = cloud.place(&game.key)?;
    let independent_here = cloud.following(&game.key) != MAIN_LINE;
    let mut seen: Vec<String> = vec![mine.dir().to_string()];
    let mut out: Vec<Source> = Vec::new();

    for (line, place) in cloud.other_lines(&game.key)? {
        seen.push(place.dir().to_string());
        let snapshots = listed(&cloud.peek(&place)?);
        let Some(first) = snapshots.first() else { continue };
        let platform = first.platform.clone();
        let kind = match () {
            _ if line.is_independent() => Kind::Device,
            _ if independent_here && line.id == MAIN_LINE => Kind::Shared,
            _ => Kind::Version,
        };
        let name = match kind {
            Kind::Device => line.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| first.device_name.clone()),
            Kind::Shared => crate::tr!("共用的存档", "Shared saves"),
            Kind::Version => line.kind.as_deref().map(kind_name).unwrap_or_else(|| platform_name(&platform)),
        };
        out.push(Source {
            id: format!("line:{}", line.id),
            kind,
            name,
            platform,
            line: Some(line.id.clone()),
            key: None,
            devices: devices_of(&snapshots),
            snapshots,
            take: Take::Copy,
            def: line.def.clone(),
            dir: place.dir().to_string(),
        });
    }

    let records = match cloud.is_legacy() {
        Ok(false) => cloud.library().map(|l| l.records.clone()).unwrap_or_default(),
        _ => Vec::new(),
    };
    for o in other_versions(ctx, game) {
        let line = match cloud.open(&o.key) {
            Ok(l) => l,
            Err(e) if cloud::is_unavailable(&e) => return Err(e),
            Err(e) => {
                log(format!("{}: the saves of {} can't be read ({e:#}); not listed", game.key, o.key));
                continue;
            }
        };
        let dir = line.place.dir().to_string();
        if seen.contains(&dir) {
            continue;
        }
        seen.push(dir.clone());
        let snapshots = listed(&line.view);
        let Some(first) = snapshots.first() else { continue };
        let platform = Some(first.platform.clone()).filter(|p| !p.is_empty()).unwrap_or_else(|| sync::platform(&o));
        let found = line_at(&records, &dir);
        let name = name_of(&o, found.and_then(|l| l.kind.as_deref()), &platform);
        out.push(Source {
            id: format!("version:{}", o.key),
            kind: Kind::Version,
            // The line's kind knows an Xbox app version where its saves may not say.
            take: fit(&o, if found.and_then(|l| l.kind.as_deref()) == Some("xbox") { "pc:xbox" } else { &platform }, game, &name),
            name,
            platform,
            line: None,
            key: Some(o.key.clone()),
            devices: devices_of(&snapshots),
            snapshots,
            def: found.and_then(|l| l.def.clone()),
            dir,
        });
    }

    let rank = |k: Kind| match k {
        Kind::Version => 0,
        Kind::Shared => 1,
        Kind::Device => 2,
    };
    out.sort_by(|a, b| {
        let at = |s: &Source| s.snapshots.first().map(|x| x.created_at);
        rank(a.kind).cmp(&rank(b.kind)).then_with(|| at(b).cmp(&at(a)))
    });
    Ok(out)
}

/// Which source has newer progress than the save here, if any: the source
/// whose newest save is the newest of all among the other versions that can
/// be brought here, when that save was made after the save here (`here`, or
/// the saves here changed later: `touched`) and isn't the same saves. A save
/// the user said no to (`dismissed`) isn't offered; nor, then, an older one
/// of another version.
pub fn pick<'a>(here: Option<&SnapshotSummary>, touched: Option<DateTime<Utc>>, sources: &'a [Source], dismissed: Option<&str>) -> Option<&'a Source> {
    let since = here.map(|h| h.created_at).max(touched);
    let (source, newest) = sources
        .iter()
        .filter(|s| s.kind == Kind::Version && !matches!(s.take, Take::Never { .. }))
        .filter_map(|s| s.newest().map(|n| (s, n)))
        .filter(|(_, n)| since.is_none_or(|t| n.created_at > t))
        .filter(|(_, n)| here.is_none_or(|h| h.content_hash != n.content_hash))
        .max_by(|(_, a), (_, b)| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)))?;
    (dismissed != Some(newest.id.as_str())).then_some(source)
}

/// The save this device is at on the line it follows: what it remembers of
/// the line (no request), a version still waiting for the cloud, or else
/// the line read once more.
fn here(cloud: &Cloud, game: &GameEntry) -> Option<SnapshotSummary> {
    let id = state::game(&game.key).last_synced_id?;
    if let Some(e) = cloud.remembered(&game.key, &id) {
        return Some(SnapshotSummary::of(&e, false));
    }
    if let Some(m) = sync::pending_metas(&game.key).iter().find(|m| m.id == id) {
        return Some(SnapshotSummary::from(m));
    }
    let line = cloud.open(&game.key).ok()?;
    line.view.get(&id).map(|e| SnapshotSummary::of(e, false))
}

/// When the saves here last changed: their newest file.
fn touched(game: &GameEntry) -> Option<DateTime<Utc>> {
    let (files, _) = snapshot::scan(game).ok()?;
    files.iter().map(|f| f.mtime).max().and_then(|s| DateTime::<Utc>::from_timestamp(s, 0))
}

/// [`pick`] among `list` (from [`sources`]) for `game`, with what the save
/// here is: read only when some other version has a save that could be
/// newer at all.
pub fn newer_among(ctx: &Ctx, game: &GameEntry, list: &[Source]) -> Result<Option<Newer>> {
    if pick(None, None, list, None).is_none() {
        return Ok(None);
    }
    let here = here(ctx.cloud()?, game);
    let dismissed = state::game(&game.key).newer_dismissed;
    let Some(source) = pick(here.as_ref(), touched(game), list, dismissed.as_deref()) else { return Ok(None) };
    let snapshot = source.newest().cloned().ok_or_else(|| anyhow!("no newest save"))?;
    Ok(Some(Newer { source: source.clone(), snapshot, here, here_name: version_name(game) }))
}

/// Newer progress in another version of the game than the save here, which
/// the user hasn't said no to ([`dismiss`]).
pub fn newer(ctx: &Ctx, game: &GameEntry) -> Result<Option<Newer>> {
    newer_among(ctx, game, &sources(ctx, game)?)
}

/// Don't offer this save of another version again.
pub fn dismiss(key: &str, snapshot_id: &str) -> Result<()> {
    savelib::dismiss_newer(key, snapshot_id)
}

/// Bring save `id` of `source` here ("取回到这里"): the saves here are kept
/// first, and the result is the newest version of the line this install
/// follows, which can be taken back ([`sync::undo`]). A save of another
/// version is written the way this one keeps its saves; into the Xbox app's
/// containers only as the user's own choice, as here.
pub fn take(ctx: &Ctx, game: &GameEntry, source: &Source, id: &str) -> Result<Outcome> {
    if let Take::Never { reason } = &source.take {
        bail!("{reason}");
    }
    match (&source.line, &source.key) {
        (Some(line), _) => sync::take_from(ctx, game, line, id),
        (None, Some(key)) => transfer::apply(ctx, key, id, &game.key, None),
        (None, None) => bail!(crate::tr!("不知道这份存档从哪里取", "Where to take this save from isn't known")),
    }
}

/// [`take`] of the newer progress [`newer`] offers now, if `id` is still it.
pub fn take_newer(ctx: &Ctx, game: &GameEntry, id: &str) -> Result<Outcome> {
    let n = newer(ctx, game)?
        .filter(|n| n.snapshot.id == id)
        .ok_or_else(|| anyhow!(crate::tr!("没有这份更新的存档了", "This newer save isn't offered any more")))?;
    take(ctx, game, &n.source, id)
}

// ------------------------------------------------------------ the question

/// The launch wrapper's question about newer progress elsewhere
/// (docs/plan-positioning.md, section 7, S2), in the interface language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub title: String,
    /// The two saves, one line each (there, then here), and what the user
    /// needs to know before choosing.
    pub text: String,
    /// Take the progress from there.
    pub take: String,
    /// The small line under it.
    pub take_note: String,
    /// Keep playing with the save here (and don't ask about that save again).
    pub keep: String,
    /// Share the progress from now on ("共用进度", [`crate::join::accept`]).
    pub join: String,
}

/// A time as a dialog says it: today and yesterday by name, else the date.
pub fn when(t: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let (t, now) = (t.with_timezone(&Local), now.with_timezone(&Local));
    let clock = t.format("%H:%M");
    let days = (now.date_naive() - t.date_naive()).num_days();
    match days {
        0 => crate::tr!("今天 {}", "today {}", clock),
        1 => crate::tr!("昨天 {}", "yesterday {}", clock),
        _ if t.year() == now.year() => t.format("%m-%d %H:%M").to_string(),
        _ => t.format("%Y-%m-%d %H:%M").to_string(),
    }
}

/// A playtime difference: "2 小时 10 分" / "2 h 10 min".
pub fn duration(secs: u64) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    match (h, m) {
        (0, m) => crate::tr!("{} 分钟", "{} min", m.max(1)),
        (h, 0) => crate::tr!("{} 小时", "{} h", h),
        (h, m) => crate::tr!("{} 小时 {} 分", "{} h {} min", h, m),
    }
}

/// How much more (or less) the game had been played when `there` was made
/// than when `here` was, where both say and it is at least a minute.
pub fn played_more(there: &SnapshotSummary, here: Option<&SnapshotSummary>) -> Option<String> {
    let (a, b) = (there.playtime?, here?.playtime?);
    match a.cmp(&b) {
        std::cmp::Ordering::Greater if a - b >= 60 => Some(crate::tr!("多玩了 {}", "{} more play", duration(a - b))),
        std::cmp::Ordering::Less if b - a >= 60 => Some(crate::tr!("少玩了 {}", "{} less play", duration(b - a))),
        _ => None,
    }
}

/// Chinese puts a space between Chinese and Latin text.
fn zh_spaced(name: &str) -> String {
    let latin = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    let lead = if latin(name.chars().next()) { " " } else { "" };
    let trail = if latin(name.chars().last()) { " " } else { "" };
    format!("{lead}{name}{trail}")
}

/// "the Xbox version", "Ryujinx", "Kid's PC" in an English sentence.
fn en_named(kind: Kind, name: &str) -> String {
    match kind {
        Kind::Version if name.ends_with(" version") => format!("the {name}"),
        _ => name.to_string(),
    }
}

/// The question for `n` about `game`. `joinable`: the two versions keep the
/// same saves and can share one progress from now on. `notes`: what else the
/// user needs to know first (writing into the Xbox app's containers, a save
/// made by a newer game), one line each.
pub fn question(game: &GameEntry, n: &Newer, joinable: bool, notes: &[String], now: DateTime<Utc>) -> Question {
    let there = &n.snapshot;
    let en = crate::i18n::is_en();
    let title = match (en, n.source.kind) {
        (false, _) => format!("《{}》在{}有更新的进度", game.name, zh_spaced(&n.source.name)),
        (true, Kind::Device) => format!("{} has newer progress on {}", game.name, n.source.name),
        (true, kind) => format!("{} has newer progress in {}", game.name, en_named(kind, &n.source.name)),
    };
    let mut first = vec![n.source.name.clone(), there.device_name.clone(), when(there.created_at, now)];
    first.extend(played_more(there, n.here.as_ref()));
    let here_name = crate::tr!("{}（这里）", "{} (here)", n.here_name);
    let second = match &n.here {
        Some(h) => [here_name, h.device_name.clone(), when(h.created_at, now)].join(" · "),
        None => format!("{here_name} · {}", crate::tr!("还没有存档", "no saves yet")),
    };
    // A full-width bracket has its own space.
    let mut text = format!("{}\n{}", first.join(" · "), second.replace("） · ", "）· "));
    if let Take::Convert { .. } = n.source.take {
        text.push_str("\n\n");
        text.push_str(&crate::tr!(
            "两个版本的存档放法不同，会转换成这个版本的。",
            "The two versions keep their saves differently; they are converted for this one."
        ));
    }
    for note in notes {
        text.push_str("\n\n");
        text.push_str(note);
    }
    let take = match en {
        false => format!("用{}的进度", zh_spaced(&n.source.name)),
        true => format!("Use {}'s progress", en_named(n.source.kind, &n.source.name)),
    };
    let keep = match en {
        false => format!("继续用{}的", zh_spaced(&n.here_name)),
        true => format!("Keep {}'s", en_named(Kind::Version, &n.here_name)),
    };
    Question {
        title,
        text,
        take: take.trim().to_string(),
        take_note: crate::tr!("这里现在的存档会先保留，可以撤销", "The saves here are kept first; you can undo it"),
        keep: keep.trim().to_string(),
        join: if joinable { crate::tr!("以后自动接上", "Always continue from there") } else { String::new() },
    }
}
