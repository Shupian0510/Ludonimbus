//! Applying saves from one version of a game to another: the save library's
//! "apply to…" (docs/plan-phase5.md §5.3 V3 and §5.5).
//!
//! Any save version in the cloud (any device, any version of the game) can
//! be applied to a version on this device:
//!
//! - **Same layout** (the same save rule, or one Switch game in two
//!   emulators): the version's files as they are, like a restore.
//! - **Same files on another platform** (a rule in `cross-saves.yaml`, e.g.
//!   Stardew Valley on Switch and PC): the saves go into the rule's folder
//!   on the target, file for file ([`crosssaves`]); the game versions that
//!   wrote both sides' saves are compared.
//! - **Files**: the source's save files (relative to their common folder)
//!   are placed among the target's save files where the most of them land
//!   on a file the target already has ([`align`]). Files the target doesn't
//!   have are added; nothing is deleted.
//! - **From an Xbox version**: the containers are read and named the way
//!   the game's other versions name its files ([`crate::xboxsaves`]).
//! - **To an Xbox version** (experimental): only save slots the Xbox
//!   version already has are replaced, written the way the Xbox app does
//!   ([`crate::wgs::replace`]).
//!
//! The target's current saves are kept in the cloud first, and the result
//! becomes its newest version ([`sync::apply_external`]).
//!
//! A Switch version only in the cloud (the console itself, which syncs
//! through the Switch client) can be a target too, for games a
//! cross-platform rule covers: the saves go on top of its newest version,
//! uploaded as its new newest version; the Switch downloads it at its next
//! sync ([`cloud_build`]).

use crate::cloud::CloudGame;
use crate::config::GameEntry;
use crate::snapshot::{self, SnapshotMeta, Trigger};
use crate::sync::{self, Ctx, Outcome};
use crate::{adapters, crosssaves, games, paths, savelib, spec, switch, wgs, xboxsaves};
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// The source file (relative path).
    pub from: String,
    /// Where it goes: a path on this PC, or `container / file` for Xbox.
    pub to: String,
    pub size: u64,
    /// Overwrites a file the target already has.
    pub replaces: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// Same save layout: applied like a restore.
    pub same: bool,
    pub from_xbox: bool,
    pub to_xbox: bool,
    /// The game's rule in `xbox-saves.yaml` (`None`: the generic rule).
    pub xbox_rule: Option<String>,
    pub items: Vec<Item>,
    /// Source files with no place in the target (Xbox: no such save slot).
    pub unmatched: Vec<String>,
    /// The target folder the files go into.
    pub base: Option<String>,
    /// How many files land on a file the target already has.
    pub matched: usize,
    /// The target has no saves yet: play it once and save, or pick its save
    /// folder.
    pub needs_folder: bool,
    /// The two versions have different save formats ([`savelib::save_scope`]):
    /// the files are rarely usable as they are. Only versions the user linked
    /// by hand get here.
    pub cross_platform: bool,
    /// The rule both versions follow (`adapters/cross-saves.yaml`): the saves
    /// go into the rule's folder file for file, whatever the platforms.
    pub format_rule: Option<String>,
    /// Source files that aren't saves under that rule (settings…): left out.
    pub skipped: Vec<String>,
    /// The game version that wrote the source's saves, and the target's
    /// current saves (where the rule says how to read it).
    pub source_version: Option<String>,
    pub target_version: Option<String>,
    /// The source's saves are from a newer game version than the target's,
    /// and the game can't load saves of a newer version: the target may need
    /// updating first.
    pub too_new: bool,
    /// The target is a Switch version only in the cloud (the console itself):
    /// the result is uploaded as its newest version, and the Switch gets it
    /// at its next sync.
    pub cloud_target: bool,
}

/// What [`apply`] writes.
enum Work {
    Same,
    Files(Vec<(PathBuf, Vec<u8>)>),
    Xbox(PathBuf, Vec<(String, String, Vec<u8>)>),
}

/// A source file, relative to its version's save folder.
struct Src {
    rel: String,
    data: Vec<u8>,
}

/// The definition of the source version: on this device or in the cloud.
fn source_def(ctx: &Ctx, key: &str) -> Result<CloudGame> {
    if let Some(g) = ctx.cfg.game(key) {
        return Ok(CloudGame::from(g));
    }
    if let Some(c) = savelib::cached(&ctx.cfg).games.into_iter().find(|c| c.key == key) {
        return Ok(c);
    }
    ctx.cloud()?
        .list_games()?
        .into_iter()
        .find(|c| c.key == key)
        .ok_or_else(|| anyhow!(crate::tr!("云端找不到这个游戏", "This game wasn't found in the cloud")))
}

fn zip_path(key: &str, id: &str) -> PathBuf {
    paths::staging_dir().join(format!("transfer-{key}-{id}.zip"))
}

/// Download a save version once (kept until it's applied).
fn fetch(ctx: &Ctx, key: &str, id: &str) -> Result<(SnapshotMeta, PathBuf)> {
    let zip = zip_path(key, id);
    if !zip.is_file() {
        let part = zip.with_extension("part");
        ctx.cloud()?.download(key, id, &part)?;
        std::fs::rename(&part, &zip)?;
    }
    Ok((snapshot::read_meta(&zip)?, zip))
}

/// Logical path → contents of every file of a save version.
fn read_files(zip: &Path, meta: &SnapshotMeta) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip)?)?;
    let mut out = BTreeMap::new();
    for f in &meta.files {
        let mut e = archive.by_name(&format!("files/{}", f.path)).with_context(|| f.path.clone())?;
        let mut data = Vec::with_capacity(f.size as usize);
        e.read_to_end(&mut data)?;
        out.insert(f.path.clone(), data);
    }
    Ok(out)
}

/// Whether a path (either slash) is an Xbox container index: a
/// `containers.index` under `wgs`, or in a `<user id>_<title id>` folder.
fn is_wgs_index(path: &str) -> bool {
    let l = path.replace('\\', "/").to_lowercase();
    let Some(dir) = l.strip_suffix(&format!("/{}", wgs::INDEX)) else { return false };
    let folder = dir.rsplit('/').next().unwrap_or("");
    let hex = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit());
    folder.split_once('_').is_some_and(|(a, b)| hex(a) && hex(b)) || l.contains("/wgs/")
}

/// Where a version's Xbox save containers are (the wgs user folder) among
/// its file paths (either slash): a container index by its place, or one
/// next to `<GUID>/container.N` files.
fn wgs_dir<'a>(paths: impl Iterator<Item = &'a str>) -> Option<String> {
    let all: Vec<String> = paths.map(|p| p.replace('\\', "/")).collect();
    let lower: Vec<String> = all.iter().map(|p| p.to_lowercase()).collect();
    let suffix = format!("/{}", wgs::INDEX);
    all.iter().zip(&lower).find_map(|(p, l)| {
        let dir = l.strip_suffix(&suffix)?;
        let beside = || {
            lower.iter().any(|q| {
                let rest = q.strip_prefix(dir).and_then(|r| r.strip_prefix('/'));
                rest.and_then(|r| r.split_once('/'))
                    .is_some_and(|(folder, file)| folder.len() == 32 && folder.chars().all(|c| c.is_ascii_hexdigit()) && file.starts_with("container."))
            })
        };
        (is_wgs_index(p) || beside()).then(|| p[..p.len() - suffix.len()].to_string())
    })
}

/// Paths relative to the folder they all share.
fn strip_common(paths: &[&str]) -> Vec<String> {
    let dirs: Vec<Vec<&str>> = paths
        .iter()
        .map(|p| {
            let mut c: Vec<&str> = p.split('/').collect();
            c.pop();
            c
        })
        .collect();
    let mut n = 0;
    while let Some(first) = dirs.first().and_then(|d| d.get(n)) {
        if dirs.iter().all(|d| d.get(n).is_some_and(|x| x.eq_ignore_ascii_case(first))) {
            n += 1;
        } else {
            break;
        }
    }
    paths.iter().map(|p| p.split('/').skip(n).collect::<Vec<_>>().join("/")).collect()
}

/// The source's files, named the way the game's non-Xbox versions name them.
fn source_files(files: &BTreeMap<String, Vec<u8>>) -> Result<(Vec<Src>, Option<String>, Option<String>)> {
    if let Some(dir) = wgs_dir(files.keys().map(String::as_str)) {
        let read = |rel: &str| files.get(&format!("{dir}/{rel}")).cloned();
        let (idx, containers) = wgs::read_all(&read)?;
        let rule = xboxsaves::rule_for(&idx.package).map(|r| r.name.clone());
        let named = xboxsaves::named(&idx.package, &containers);
        let src = named.into_iter().map(|n| Src { rel: n.path, data: n.data }).collect();
        return Ok((src, Some(idx.package), rule));
    }
    let keys: Vec<&str> = files.keys().map(String::as_str).collect();
    let rels = strip_common(&keys);
    let src = rels.into_iter().zip(files.values()).map(|(rel, data)| Src { rel, data: data.clone() }).collect();
    Ok((src, None, None))
}

fn safe_rel(rel: &str) -> bool {
    !rel.is_empty() && !rel.contains(['\\', ':']) && rel.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase()
}

fn join_key(base: &str, rest: &str) -> String {
    if base.is_empty() {
        rest.to_lowercase()
    } else {
        format!("{base}/{}", rest.to_lowercase())
    }
}

fn strip(rel: &str, k: usize) -> String {
    rel.split('/').skip(k).collect::<Vec<_>>().join("/")
}

/// Where to put `sources` (relative paths) among `targets` (the files the
/// target already has): the folder, how many leading folders to drop from
/// each source path, and how many sources then land on an existing file.
/// The most matches win, then dropping fewer folders, then the deeper
/// folder. `None` when no source file matches any target file.
pub fn align(sources: &[String], targets: &[PathBuf]) -> Option<(PathBuf, usize, usize)> {
    let set: HashSet<String> = targets.iter().map(|t| norm(t)).collect();
    let mut bases: Vec<PathBuf> = Vec::new();
    let mut seen = HashSet::new();
    for t in targets {
        for a in t.ancestors().skip(1).take(8) {
            if seen.insert(norm(a)) {
                bases.push(a.to_path_buf());
            }
        }
    }
    let max_k = sources.iter().map(|s| s.split('/').count()).min().unwrap_or(1).saturating_sub(1);
    let mut best: Option<(PathBuf, usize, usize)> = None;
    for b in &bases {
        let bk = norm(b);
        for k in 0..=max_k {
            let score = sources.iter().filter(|s| set.contains(&join_key(&bk, &strip(s, k)))).count();
            if score == 0 {
                continue;
            }
            let better = match &best {
                None => true,
                Some((bb, bk2, bs)) => score > *bs || (score == *bs && (k < *bk2 || (k == *bk2 && b.components().count() > bb.components().count()))),
            };
            if better {
                best = Some((b.clone(), k, score));
            }
        }
    }
    best
}

/// The deepest folder all `paths` are in.
fn common_dir(paths: &[PathBuf]) -> Option<PathBuf> {
    let mut it = paths.iter().filter_map(|p| p.parent());
    let mut common = it.next()?.to_path_buf();
    for p in it {
        while !p.starts_with(&common) {
            common = common.parent()?.to_path_buf();
        }
    }
    Some(common)
}

/// Whether the two versions keep their saves the same way (so a version's
/// files apply as they are).
fn same_layout(src: &CloudGame, tgt: &GameEntry) -> bool {
    let (a, b) = (spec::effective(&savelib::entry_of(src)), spec::effective(tgt));
    if a.ludusavi_title.is_some() || b.ludusavi_title.is_some() || a.saves.is_empty() || a.saves != b.saves {
        return false;
    }
    match (src.adapter.as_deref().and_then(adapters::find), tgt.adapter.as_deref().and_then(adapters::find)) {
        (Some(x), Some(y)) => x.key == y.key || x.shares_game_saves(y),
        (None, None) => true,
        _ => false,
    }
}

/// The source's saves under a cross-platform rule (relative to the rule's
/// folder), with the plan's rule, left-out files and source version filled.
fn rule_saves<'a>(rule: &crosssaves::Rule, src: &GameEntry, meta: &SnapshotMeta, files: &'a BTreeMap<String, Vec<u8>>, plan: &mut Plan) -> Vec<(String, &'a Vec<u8>)> {
    plan.format_rule = Some(rule.name.clone());
    let saves = crosssaves::saves_of(rule, &savelib::save_scope(src), files.iter().map(|(p, d)| (p.as_str(), (p, d))), &meta.roots);
    let kept: HashSet<&String> = saves.iter().map(|(_, (p, _))| *p).collect();
    plan.skipped = files.keys().filter(|p| !kept.contains(p)).cloned().collect();
    plan.source_version = crosssaves::newest_version(rule, &saves, |(_, d)| Some((*d).clone()));
    saves.into_iter().map(|(rel, (_, d))| (rel, d)).collect()
}

/// Whether the source's saves are from a game version the target can't load.
fn too_new(rule: &crosssaves::Rule, plan: &Plan) -> bool {
    rule.newer_only
        && matches!((&plan.source_version, &plan.target_version), (Some(s), Some(t)) if crosssaves::compare(s, t) == Some(std::cmp::Ordering::Greater))
}

/// What applying to a Switch version only in the cloud uploads.
struct CloudWork {
    /// Its newest version, which the new one follows.
    parent: Option<String>,
    /// That version's files with the saves on top.
    files: BTreeMap<String, Vec<u8>>,
    version: Option<String>,
    /// The newest version as downloaded (removed after applying).
    head_zip: Option<PathBuf>,
}

/// A Switch version only in the cloud (the console itself, synced through
/// the Switch client) that a cross-platform rule covers: the source's saves
/// go into its save data (`nxsave-<title id>/…`) on top of its newest
/// version.
fn cloud_build(ctx: &Ctx, src_def: &CloudGame, meta: &SnapshotMeta, files: &BTreeMap<String, Vec<u8>>, target: &str) -> Result<(Plan, CloudWork)> {
    let tdef = source_def(ctx, target)?;
    let tgt = savelib::entry_of(&tdef);
    let src = savelib::entry_of(src_def);
    let to = savelib::save_scope(&tgt);
    let rule = crosssaves::shared(&src, &tgt).filter(|r| r.folders.get(&to).is_some_and(|f| f == crosssaves::NXSAVE));
    let (Some(rule), Some(title)) = (rule, games::switch_title(&tgt.saves)) else {
        bail!(crate::tr!(
            "「{}」不在本机。存档只能应用到本机的游戏，或者存档规则支持的 Switch 版本",
            "\"{}\" isn't on this PC. Saves can be applied to games on this PC, or to a Switch version a save rule covers",
            tdef.name
        ));
    };
    let root = switch::root_name(switch::SaveKind::Account, title);
    let head = sync::mainline_head(&ctx.cloud()?.list_snapshots(target)?);
    let (mut current, head_roots, head_zip) = match &head {
        Some(h) => {
            let (m, z) = fetch(ctx, target, &h.id)?;
            (read_files(&z, &m)?, m.roots.clone(), Some(z))
        }
        None => (BTreeMap::new(), BTreeMap::new(), None),
    };
    let mut plan = Plan { cloud_target: true, base: Some(root.clone()), ..Plan::default() };
    let saves = rule_saves(rule, &src, meta, files, &mut plan);
    let version_of = |files: &BTreeMap<String, Vec<u8>>| {
        let saves = crosssaves::saves_of(rule, &to, files.iter().map(|(p, d)| (p.as_str(), d)), &head_roots);
        crosssaves::newest_version(rule, &saves, |d| Some((*d).clone()))
    };
    plan.target_version = version_of(&current);
    plan.too_new = too_new(rule, &plan);
    for (rel, data) in saves {
        if !safe_rel(&rel) {
            plan.unmatched.push(rel);
            continue;
        }
        let logical = format!("{root}/{rel}");
        plan.items.push(Item { from: rel, to: logical.clone(), size: data.len() as u64, replaces: current.contains_key(&logical) });
        current.insert(logical, data.clone());
    }
    plan.matched = plan.items.iter().filter(|i| i.replaces).count();
    let version = version_of(&current);
    Ok((plan, CloudWork { parent: head.map(|h| h.id), files: current, version, head_zip }))
}

/// Saves of two versions a cross-platform rule covers: the source's saves
/// (relative to the rule's folder there) go into the rule's folder on the
/// target, file for file; its other files (settings) are left out.
#[allow(clippy::too_many_arguments)]
fn by_rule(
    rule: &crosssaves::Rule,
    src: &GameEntry,
    meta: &SnapshotMeta,
    files: &BTreeMap<String, Vec<u8>>,
    target: &GameEntry,
    current: &[snapshot::LocalFile],
    roots: &BTreeMap<String, PathBuf>,
    base: Option<&Path>,
    mut plan: Plan,
) -> (Plan, Work) {
    let saves = rule_saves(rule, src, meta, files, &mut plan);
    plan.target_version = crosssaves::local_version(target, current, roots);
    plan.too_new = too_new(rule, &plan);
    let to = savelib::save_scope(target);
    let Some(dir) = base.map(Path::to_path_buf).or_else(|| crosssaves::folder_on_device(rule, &to, current, roots)) else {
        plan.needs_folder = true;
        return (plan, Work::Files(Vec::new()));
    };
    let mut writes = Vec::new();
    for (rel, data) in saves {
        if !safe_rel(&rel) {
            plan.unmatched.push(rel);
            continue;
        }
        let dest = rel.split('/').fold(dir.clone(), |p, c| p.join(c));
        plan.items.push(Item { from: rel.clone(), to: dest.display().to_string(), size: data.len() as u64, replaces: dest.is_file() });
        writes.push((dest, data.clone()));
    }
    plan.matched = plan.items.iter().filter(|i| i.replaces).count();
    plan.base = Some(dir.display().to_string());
    (plan, Work::Files(writes))
}

fn build(src_def: &CloudGame, meta: &SnapshotMeta, files: &BTreeMap<String, Vec<u8>>, target: &GameEntry, base: Option<&Path>) -> Result<(Plan, Work)> {
    let src = savelib::entry_of(src_def);
    let cross = crosssaves::shared(&src, target);
    let mut plan = Plan { cross_platform: cross.is_none() && savelib::save_scope(&src) != savelib::save_scope(target), ..Plan::default() };
    let (current, roots) = snapshot::scan_own(target).unwrap_or_default();
    let has_wgs_source = wgs_dir(files.keys().map(String::as_str)).is_some();
    let abs: Vec<String> = current.iter().map(|f| f.abs.to_string_lossy().into_owned()).collect();
    let target_wgs = wgs_dir(abs.iter().map(String::as_str)).map(PathBuf::from);
    // Xbox containers are never copied as they are: their GUIDs and index
    // belong to one PC's Xbox app.
    if !has_wgs_source && target_wgs.is_none() && same_layout(src_def, target) {
        plan.same = true;
        for f in &meta.files {
            let to = snapshot::resolve_logical(&roots, &f.path).map(|p| p.display().to_string()).unwrap_or_else(|_| f.path.clone());
            let replaces = Path::new(&to).is_file();
            plan.items.push(Item { from: f.path.clone(), to, size: f.size, replaces });
        }
        plan.matched = plan.items.iter().filter(|i| i.replaces).count();
        return Ok((plan, Work::Same));
    }
    // The same files on another platform (or in another place).
    if let Some(rule) = cross.filter(|_| !has_wgs_source && target_wgs.is_none()) {
        return Ok(by_rule(rule, &src, meta, files, target, &current, &roots, base, plan));
    }

    let (srcs, package, rule) = source_files(files)?;
    plan.from_xbox = package.is_some();
    plan.xbox_rule = rule;
    let (srcs, bad): (Vec<Src>, Vec<Src>) = srcs.into_iter().partition(|s| safe_rel(&s.rel));
    plan.unmatched.extend(bad.into_iter().map(|s| s.rel));
    let rels: Vec<String> = srcs.iter().map(|s| s.rel.clone()).collect();

    // Into an Xbox version: replace the save slots it has.
    if let Some(dir) = target_wgs {
        plan.to_xbox = true;
        let (idx, containers) = wgs::read_dir(&dir)?;
        let slots: Vec<xboxsaves::Named> = xboxsaves::named(&idx.package, &containers).into_iter().filter(|n| n.origin.is_some()).collect();
        let paths: Vec<PathBuf> = slots.iter().map(|n| PathBuf::from(&n.path)).collect();
        let by_path: HashMap<String, &xboxsaves::Named> = slots.iter().map(|n| (norm(Path::new(&n.path)), n)).collect();
        let mut changes = Vec::new();
        if let Some((b, k, _)) = align(&rels, &paths) {
            let bk = norm(&b);
            for s in &srcs {
                match by_path.get(&join_key(&bk, &strip(&s.rel, k))).and_then(|n| n.origin.clone()) {
                    Some((c, f)) => {
                        plan.items.push(Item { from: s.rel.clone(), to: format!("{c} / {f}"), size: s.data.len() as u64, replaces: true });
                        changes.push((c, f, s.data.clone()));
                    }
                    None => plan.unmatched.push(s.rel.clone()),
                }
            }
        } else {
            plan.unmatched.extend(rels.iter().cloned());
        }
        plan.matched = changes.len();
        plan.base = Some(dir.display().to_string());
        return Ok((plan, Work::Xbox(dir, changes)));
    }

    // Plain files.
    let targets: Vec<PathBuf> = match base {
        Some(b) => current.iter().map(|f| f.abs.clone()).filter(|p| p.starts_with(b)).collect(),
        None => current.iter().map(|f| f.abs.clone()).collect(),
    };
    let placed = align(&rels, &targets).or_else(|| match base {
        Some(b) => Some((b.to_path_buf(), 0, 0)),
        None => common_dir(&targets).map(|d| (d, 0, 0)),
    });
    let Some((dir, k, matched)) = placed else {
        plan.needs_folder = true;
        return Ok((plan, Work::Files(Vec::new())));
    };
    let mut writes = Vec::new();
    for s in srcs {
        let rest = strip(&s.rel, k);
        let dest = rest.split('/').fold(dir.clone(), |p, c| p.join(c));
        plan.items.push(Item { from: s.rel.clone(), to: dest.display().to_string(), size: s.data.len() as u64, replaces: dest.is_file() });
        writes.push((dest, s.data));
    }
    plan.matched = matched;
    plan.base = Some(dir.display().to_string());
    Ok((plan, Work::Files(writes)))
}

/// What applying save version `id` of `source` to `target` (a game on this
/// device) would do. `base` overrides the target folder.
pub fn plan(ctx: &Ctx, source: &str, id: &str, target: &str, base: Option<&Path>) -> Result<Plan> {
    let def = source_def(ctx, source)?;
    let (meta, zip) = fetch(ctx, source, id)?;
    let files = read_files(&zip, &meta)?;
    if ctx.cfg.game(target).is_none() {
        return Ok(cloud_build(ctx, &def, &meta, &files, target)?.0);
    }
    let tgt = ctx.game(target)?;
    Ok(build(&def, &meta, &files, &tgt, base)?.0)
}

fn applied_comment(def: &CloudGame, meta: &SnapshotMeta) -> String {
    crate::tr!(
        "应用自「{}」在 {} 的版本（{}）",
        "Applied from \"{}\", the version saved at {} ({})",
        def.name,
        meta.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M"),
        meta.device_name
    )
}

/// Apply to a Switch version only in the cloud: its new newest version.
fn apply_to_cloud(ctx: &Ctx, def: &CloudGame, meta: &SnapshotMeta, zip: &Path, files: &BTreeMap<String, Vec<u8>>, target: &str) -> Result<Outcome> {
    let (plan, work) = cloud_build(ctx, def, meta, files, target)?;
    if plan.items.is_empty() {
        bail!(crate::tr!("没有可以应用的文件", "There are no files to apply"));
    }
    let list: Vec<(String, Vec<u8>)> = work.files.into_iter().collect();
    let device = (ctx.cfg.device.id.as_str(), ctx.cfg.device.name.as_str());
    let cloud = ctx.cloud()?;
    let line = cloud.open(target)?;
    let origin = cloud.place(&def.key).ok().map(|from| crate::cloud::v2::Origin {
        game: from.game,
        line: from.line,
        snapshot: meta.id.clone(),
        how: "copy".to_string(),
        extra: Default::default(),
    });
    let new = sync::queue_files(
        &line,
        &sync::Foreign {
            key: target,
            files: &list,
            trigger: Trigger::Restore,
            parent: work.parent,
            over_all: true,
            device,
            platform: "",
            comment: &applied_comment(def, meta),
            game_version: work.version,
            origin,
        },
    )?;
    // Nothing uploads this game's queue later (it isn't on this PC), so it
    // is now or not at all.
    let uploaded = sync::upload_pending(cloud, &new);
    let _ = sync::drop_pending(&new);
    uploaded?;
    let _ = std::fs::remove_file(zip);
    if let Some(z) = work.head_zip {
        let _ = std::fs::remove_file(z);
    }
    Ok(Outcome::Uploaded { id: new.id })
}

/// Apply save version `id` of `source` to `target` (see [`plan`]).
pub fn apply(ctx: &Ctx, source: &str, id: &str, target: &str, base: Option<&Path>) -> Result<Outcome> {
    let def = source_def(ctx, source)?;
    let (meta, zip) = fetch(ctx, source, id)?;
    let files = read_files(&zip, &meta)?;
    if ctx.cfg.game(target).is_none() {
        return apply_to_cloud(ctx, &def, &meta, &zip, &files, target);
    }
    let tgt = ctx.game(target)?;
    let (plan, work) = build(&def, &meta, &files, &tgt, base)?;
    if plan.needs_folder {
        bail!(crate::tr!(
            "「{}」在本机还没有存档：先在这个版本里开一次游戏、存一次档，或者选择它的存档文件夹",
            "\"{}\" has no saves on this PC yet: play this version once and save, or choose its save folder",
            tgt.name
        ));
    }
    if plan.items.is_empty() {
        bail!(crate::tr!("没有可以应用的文件", "There are no files to apply"));
    }
    let comment = applied_comment(&def, &meta);
    let out = sync::apply_external(ctx, &tgt, &comment, || match work {
        Work::Same => snapshot::apply(&zip, &meta, &tgt),
        Work::Xbox(dir, changes) => wgs::replace(&dir, &changes).map(|_| ()),
        Work::Files(writes) => {
            for (dest, data) in &writes {
                crate::util::atomic_write(dest, data)?;
            }
            // Files written outside the target's save locations wouldn't sync.
            let (now, _) = snapshot::scan(&tgt)?;
            let seen: HashSet<String> = now.iter().map(|f| norm(&f.abs)).collect();
            if !writes.iter().any(|(d, _)| seen.contains(&norm(d))) {
                bail!(crate::tr!(
                    "文件已写入 {}，但不在「{}」的存档位置里，所以没有生成新版本。请确认目标文件夹",
                    "The files were written to {}, but that isn't among the save locations of \"{}\", so no new version was made. Check the target folder",
                    plan.base.clone().unwrap_or_default(),
                    tgt.name
                ));
            }
            Ok(())
        }
    })?;
    let _ = std::fs::remove_file(&zip);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_shared_folder() {
        assert_eq!(strip_common(&["{appdata}/Game/Saves/a.sav", "{appdata}/Game/Saves/b/c.sav"]), vec!["a.sav", "b/c.sav"]);
        assert_eq!(strip_common(&["{x}/One/save.dat"]), vec!["save.dat"]);
    }

    #[test]
    fn aligns_by_file_names() {
        let t = |p: &str| PathBuf::from(p);
        // The Steam version's folder holds the same files deeper down.
        let targets = vec![t("C:/Steam/userdata/1/2/remote/Saves/slot1.sav"), t("C:/Steam/userdata/1/2/remote/Saves/slot2.sav"), t("C:/Steam/userdata/1/2/remote/config.ini")];
        let sources = vec!["Saves/slot1.sav".to_string(), "Saves/slot2.sav".into(), "Saves/slot3.sav".into()];
        let (base, k, n) = align(&sources, &targets).unwrap();
        assert_eq!((norm(&base), k, n), ("c:/steam/userdata/1/2/remote".to_string(), 0, 2));
        // Sources one folder deeper than the target's.
        let sources = vec!["GameName/slot1.sav".to_string()];
        let (base, k, _) = align(&sources, &targets).unwrap();
        assert_eq!((norm(&base), k), ("c:/steam/userdata/1/2/remote/saves".to_string(), 1));
        assert!(align(&["other.bin".to_string()], &targets).is_none());
        // Relative targets (an Xbox version's save slots).
        let (base, k, n) = align(&["0123/Level.sav".to_string()], &[t("0123/Level.sav")]).unwrap();
        assert_eq!((norm(&base), k, n), (String::new(), 0, 1));
    }

    #[test]
    fn finds_xbox_containers() {
        let p = ["{localappdata}/Packages/X_y/SystemAppData/wgs/0009_00AB/containers.index", "{localappdata}/Packages/X_y/SystemAppData/wgs/0009_00AB/ABC/container.1"];
        assert_eq!(wgs_dir(p.iter().copied()).as_deref(), Some("{localappdata}/Packages/X_y/SystemAppData/wgs/0009_00AB"));
        assert_eq!(wgs_dir(["{appdata}/Game/containers.index"].iter().copied()), None);
        assert!(is_wgs_index("{pk}/000901F2A1B2C3D4_000000000000000000000000646E3F8B/containers.index"));
        assert!(is_wgs_index(r"C:\Users\me\AppData\Local\Packages\X_y\SystemAppData\wgs\t\containers.index"));
    }
}
