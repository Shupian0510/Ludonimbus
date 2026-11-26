//! Bindings: how a version of a game that keeps its saves in other folders
//! takes part in a save line (docs/plan-v2.md, sections 4.2 and 6).
//!
//! A save line holds files under the paths of the version that uploaded to
//! it first. Another version of the game (another store's, a copy in
//! another place) may keep the very same files in other folders. A binding
//! says which folder of the line is which folder of that version; with a
//! set of them the version reads and writes the line as if the files were
//! its own. Only folders are matched, file contents are never changed.
//!
//! Paths here are the logical ones of [`crate::snapshot`]
//! (`appdata/Game/saves/slot1.sav`): a root name and a path below it, with
//! `/`. They are compared without regard to letter case, as everywhere.
//!
//! Files of the version outside every binding are not the line's: they are
//! not uploaded and never touched. Files of the line outside every binding
//! have no place on the device; a version only joins a line all of whose
//! files it has a folder for ([`Proposal::usable`]), and stops when that is
//! no longer so ([`uncovered`]).
//!
//! A binding also says how the version keeps the files ([`Codec`]): as
//! they are, or inside the Xbox app's save containers. With the containers
//! the folder `local` is the container folder (or one below it), and the
//! files in it are the containers' files under the names the game's other
//! versions use ([`crate::xboxsaves::named`]); [`crate::snapshot`] reads and
//! writes them.
//!
//! A binding can be narrower than its folder ([`Bind::files`]): only the
//! files a wildcard names (relative to the folder) belong to it, the way a
//! cross-platform rule names which files in its folder are saves
//! (`adapters/cross-saves.yaml`). The other files in the folder are outside
//! every binding, like files in another folder.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// How a version keeps the files of a binding.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Codec {
    /// Plain files, under the bound folder as they are.
    #[default]
    Files,
    /// The Xbox app's save containers ([`crate::wgs`]): the bound folder is
    /// the container folder or one below it, and its files are the
    /// containers' files as the other versions name them.
    Wgs,
    /// A codec a newer Ludonimbus knows: kept as it is, never used here.
    Other(String),
}

impl Codec {
    pub fn is_files(&self) -> bool {
        *self == Codec::Files
    }
}

impl From<String> for Codec {
    fn from(s: String) -> Codec {
        match s.as_str() {
            "files" => Codec::Files,
            "wgs" => Codec::Wgs,
            _ => Codec::Other(s),
        }
    }
}

impl From<Codec> for String {
    fn from(c: Codec) -> String {
        match c {
            Codec::Files => "files".to_string(),
            Codec::Wgs => "wgs".to_string(),
            Codec::Other(s) => s,
        }
    }
}

/// One folder of a save line, and the folder that holds the same files in
/// a version of the game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bind {
    /// The folder in the line's files.
    pub canon: String,
    /// The folder in the version's own files.
    pub local: String,
    /// How the version keeps them. Plain files unless it says otherwise,
    /// and then not written, so bindings of plain files read as before.
    #[serde(default, skip_serializing_if = "Codec::is_files")]
    pub codec: Codec,
    /// Which files in the folder belong to the binding: a wildcard relative
    /// to it, matched as a cross-platform rule's `files` are (any letter
    /// case, `*` doesn't cross `/`). None = every file in it, and then not
    /// written, so bindings without one read and write as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<String>,
}

impl Bind {
    /// A binding of plain files.
    pub fn files(canon: impl Into<String>, local: impl Into<String>) -> Bind {
        Bind { canon: canon.into(), local: local.into(), codec: Codec::Files, files: None }
    }

    /// A binding of the plain files in the folders that `files` names.
    pub fn some(canon: impl Into<String>, local: impl Into<String>, files: impl Into<String>) -> Bind {
        Bind { files: Some(files.into()), ..Bind::files(canon, local) }
    }

    /// Whether a file at `rest` below the bound folder (what follows the
    /// folder, with its leading `/`) belongs to the binding.
    fn holds(&self, rest: &str) -> bool {
        match self.files.as_deref() {
            None => true,
            Some(pattern) => rest.strip_prefix('/').is_some_and(|rel| crate::crosssaves::matches(pattern, rel)),
        }
    }
}

/// Some binding reads its files out of the Xbox app's containers.
pub fn has_wgs(binds: &[Bind]) -> bool {
    binds.iter().any(|b| b.codec == Codec::Wgs)
}

/// Bindings this build can use: none with a codec only a newer Ludonimbus
/// knows.
pub fn known(binds: &[Bind]) -> bool {
    binds.iter().all(|b| !matches!(b.codec, Codec::Other(_)))
}

/// `path` below the folder `dir` (or `dir` itself): what follows `dir`,
/// with its leading `/`.
fn below<'a>(dir: &str, path: &'a str) -> Option<&'a str> {
    let head = path.get(..dir.len())?;
    let rest = &path[dir.len()..];
    (same(head, dir) && (rest.is_empty() || rest.starts_with('/'))).then_some(rest)
}

/// The bindings whose folder on the side `from` holds `path`, with what
/// follows the folder; a binding that names its files only when it names
/// this one.
fn holding<'a, 'p>(binds: &'a [Bind], path: &'p str, from: impl Fn(&Bind) -> &str) -> impl Iterator<Item = (&'a Bind, &'p str)> {
    binds.iter().filter_map(move |b| below(from(b), path).filter(|rest| b.holds(rest)).map(|rest| (b, rest)))
}

fn mapped(binds: &[Bind], path: &str, from: impl Fn(&Bind) -> &str, to: impl Fn(&Bind) -> &str) -> Option<String> {
    holding(binds, path, &from)
        // The deepest folder that holds it.
        .max_by_key(|(b, _)| from(b).len())
        .map(|(b, rest)| format!("{}{rest}", to(b)))
}

/// The line's path of the version's file `local`; `None` for a file outside
/// every binding.
pub fn to_canon(binds: &[Bind], local: &str) -> Option<String> {
    mapped(binds, local, |b| &b.local, |b| &b.canon)
}

/// The version's path of the line's file `canon`; `None` for a file the
/// version has no folder for.
pub fn to_local(binds: &[Bind], canon: &str) -> Option<String> {
    mapped(binds, canon, |b| &b.canon, |b| &b.local)
}

/// The line's path of the version's file `local` that it keeps the way
/// `codec` says: only bindings of that codec count.
pub fn to_canon_as(binds: &[Bind], local: &str, codec: &Codec) -> Option<String> {
    let of: Vec<Bind> = binds.iter().filter(|b| &b.codec == codec).cloned().collect();
    to_canon(&of, local)
}

/// [`to_local`], with the codec of the binding it goes through.
pub fn to_local_with(binds: &[Bind], canon: &str) -> Option<(String, Codec)> {
    holding(binds, canon, |b| &b.canon)
        .max_by_key(|(b, _)| b.canon.len())
        .map(|(b, rest)| (format!("{}{rest}", b.local), b.codec.clone()))
}

/// Whether `path` is the folder `dir` or below it.
pub fn is_within(dir: &str, path: &str) -> bool {
    below(dir, path).is_some()
}

/// Bindings worked out from files among which the folders `containers`
/// hold the Xbox app's containers (as their files under the other
/// versions' names): a binding in such a folder reads them out of the
/// containers. `None` when a binding holds a container folder together with
/// other files: those can't be read one way.
pub fn with_containers(binds: &[Bind], containers: &[String]) -> Option<Vec<Bind>> {
    let mut out = Vec::with_capacity(binds.len());
    for b in binds {
        if containers.iter().any(|c| is_within(c, &b.local)) {
            out.push(Bind { codec: Codec::Wgs, ..b.clone() });
        } else if containers.iter().any(|c| is_within(&b.local, c)) {
            return None;
        } else {
            out.push(b.clone());
        }
    }
    Some(out)
}

/// Bindings into the Xbox app's containers made where the containers are in
/// another folder, used with the container folders of this device.
///
/// The container folder is named after the Xbox account
/// (`…/wgs/<user>_<title>`): another device, or another account here, has
/// another one next to it. A binding whose folder is no container folder
/// here is moved into the one here when there is exactly one, beside where
/// it was. Everything else stays as it is.
pub fn rebase(binds: &[Bind], containers: &[String]) -> Vec<Bind> {
    binds
        .iter()
        .map(|b| {
            if b.codec != Codec::Wgs || containers.iter().any(|c| is_within(c, &b.local)) {
                return b.clone();
            }
            let [here] = containers else { return b.clone() };
            let Some((parent, _)) = here.rsplit_once('/') else { return b.clone() };
            // `/<their folder>` and what is below it.
            let Some(rest) = below(parent, &b.local).and_then(|r| r.strip_prefix('/')) else { return b.clone() };
            let under = rest.split_once('/').map(|(_, r)| format!("/{r}")).unwrap_or_default();
            Bind { local: format!("{here}{under}"), ..b.clone() }
        })
        .collect()
}

/// The files of a line (`paths`) the version has no folder for.
pub fn uncovered<'a>(binds: &[Bind], paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    paths.into_iter().filter(|p| to_local(binds, p).is_none()).map(str::to_string).collect()
}

/// Bindings worked out from the files on both sides ([`propose`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub binds: Vec<Bind>,
    /// Files both sides have, under the bindings.
    pub matched: usize,
    /// Files of the line in a bound folder that the version doesn't have
    /// (it gets them with the line's saves).
    pub only_in_line: usize,
    /// Files of the version in a bound folder that the line doesn't have.
    pub only_here: usize,
    /// Files of the line no folder of the version holds.
    pub uncovered: Vec<String>,
    /// Files of the version outside every binding: they stay as they are,
    /// and are not part of the line.
    pub outside: Vec<String>,
}

impl Proposal {
    /// Whether the version can take part in the line with these bindings:
    /// every file of the line has a folder, and at least one file is on
    /// both sides (nothing else says the folders are the same ones).
    pub fn usable(&self) -> bool {
        !self.binds.is_empty() && self.matched > 0 && self.uncovered.is_empty()
    }
}

/// What a set of bindings does with the files of a line (`canon`) and of a
/// version of the game (`local`).
pub fn measure(binds: &[Bind], canon: &[String], local: &[String]) -> Proposal {
    let mut out = Proposal { binds: binds.to_vec(), ..Proposal::default() };
    let here: HashSet<String> = local.iter().map(|l| l.to_lowercase()).collect();
    let line: HashSet<String> = canon.iter().map(|c| c.to_lowercase()).collect();
    for c in canon {
        match to_local(binds, c) {
            None => out.uncovered.push(c.clone()),
            Some(l) if here.contains(&l.to_lowercase()) => out.matched += 1,
            Some(_) => out.only_in_line += 1,
        }
    }
    for l in local {
        match to_canon(binds, l) {
            None => out.outside.push(l.clone()),
            Some(c) if !line.contains(&c.to_lowercase()) => out.only_here += 1,
            Some(_) => {}
        }
    }
    out
}

fn parts(path: &str) -> Vec<&str> {
    path.split('/').collect()
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// One folder is the other, or above or below it.
fn nested(a: &str, b: &str) -> bool {
    below(a, b).is_some() || below(b, a).is_some()
}

/// A file name with many files of that name on the other side is compared
/// with this many of them.
const SAME_NAME_LIMIT: usize = 64;

/// Which folders of a line (`canon`: the paths of its files) are which
/// folders of a version of the game (`local`: the paths of the files it
/// has now).
///
/// Two files with the same name and the same folders above it, as far as
/// those go, say that what remains of the two paths is the same folder. Of
/// all pairs of folders found that way, the one under which the most files
/// are on both sides is taken; among equals the deepest, so that files next
/// to the saves that only one side has (settings, a store's own files) stay
/// outside. The files it holds are set aside and the rest is looked at
/// again, until nothing matches. A pair that would contain an earlier one
/// is left out: one folder is bound once.
pub fn propose(canon: &[String], local: &[String]) -> Proposal {
    let mut canon_left: Vec<&str> = canon.iter().map(String::as_str).collect();
    let mut local_left: Vec<&str> = local.iter().map(String::as_str).collect();
    let mut binds: Vec<Bind> = Vec::new();
    loop {
        let mut by_name: HashMap<String, Vec<&str>> = HashMap::new();
        for l in &local_left {
            by_name.entry(l.rsplit('/').next().unwrap_or(l).to_lowercase()).or_default().push(l);
        }
        // (folder of the line, folder of the version), lower case → files on
        // both sides, and the pair as written.
        let mut pairs: HashMap<(String, String), (usize, String, String)> = HashMap::new();
        for c in &canon_left {
            let cp = parts(c);
            let Some(named) = by_name.get(&cp[cp.len() - 1].to_lowercase()) else { continue };
            for l in named.iter().take(SAME_NAME_LIMIT) {
                let lp = parts(l);
                // Each side keeps at least its root.
                let most = cp.len().min(lp.len()) - 1;
                for k in 1..=most {
                    if !same(cp[cp.len() - k], lp[lp.len() - k]) {
                        break;
                    }
                    let (cd, ld) = (cp[..cp.len() - k].join("/"), lp[..lp.len() - k].join("/"));
                    if binds.iter().any(|b| nested(&b.canon, &cd) || nested(&b.local, &ld)) {
                        continue;
                    }
                    pairs.entry((cd.to_lowercase(), ld.to_lowercase())).or_insert((0, cd, ld)).0 += 1;
                }
            }
        }
        let depth = |s: &str| s.split('/').count();
        let best = pairs.into_iter().max_by(|(ka, a), (kb, b)| {
            a.0.cmp(&b.0)
                .then_with(|| depth(&a.1).cmp(&depth(&b.1)))
                .then_with(|| depth(&a.2).cmp(&depth(&b.2)))
                // Any order, as long as it is the same every time.
                .then_with(|| kb.cmp(ka))
        });
        let Some((_, (_, cd, ld))) = best else { break };
        canon_left.retain(|c| below(&cd, c).is_none());
        local_left.retain(|l| below(&ld, l).is_none());
        binds.push(Bind::files(cd, ld));
    }
    measure(&binds, canon, local)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    fn b(canon: &str, local: &str) -> Bind {
        Bind::files(canon, local)
    }

    fn wgs(canon: &str, local: &str) -> Bind {
        Bind { codec: Codec::Wgs, ..b(canon, local) }
    }

    #[test]
    fn the_codec_is_written_only_when_it_is_not_plain_files() {
        // As written before there were codecs: plain files.
        let old: Bind = serde_json::from_str(r#"{"canon":"appdata/Game","local":"documents/Game"}"#).unwrap();
        assert_eq!(old, b("appdata/Game", "documents/Game"));
        assert_eq!(serde_json::to_string(&old).unwrap(), r#"{"canon":"appdata/Game","local":"documents/Game"}"#);
        let x = wgs("appdata/Game", "data");
        let text = serde_json::to_string(&x).unwrap();
        assert_eq!(text, r#"{"canon":"appdata/Game","local":"data","codec":"wgs"}"#);
        assert_eq!(serde_json::from_str::<Bind>(&text).unwrap(), x);
        // One a newer version knows is kept as it is, and not used.
        let newer: Bind = serde_json::from_str(r#"{"canon":"a","local":"b","codec":"nx"}"#).unwrap();
        assert_eq!(newer.codec, Codec::Other("nx".into()));
        assert!(serde_json::to_string(&newer).unwrap().contains(r#""codec":"nx""#));
        assert!(!known(&[newer.clone()]) && known(&[x.clone(), old.clone()]));
        assert!(has_wgs(&[old.clone(), x]) && !has_wgs(&[old, newer]));
    }

    #[test]
    fn the_files_of_a_binding_are_written_only_when_it_names_them() {
        // As before there was a wildcard: not written, read back the same.
        let old: Bind = serde_json::from_str(r#"{"canon":"appdata/Game","local":"documents/Game"}"#).unwrap();
        assert_eq!(old.files, None);
        assert_eq!(serde_json::to_string(&old).unwrap(), r#"{"canon":"appdata/Game","local":"documents/Game"}"#);
        let some = Bind::some("nxsave-0100e65002bb8000", "appdata/StardewValley/Saves", "*/*");
        let text = serde_json::to_string(&some).unwrap();
        assert_eq!(text, r#"{"canon":"nxsave-0100e65002bb8000","local":"appdata/StardewValley/Saves","files":"*/*"}"#);
        assert_eq!(serde_json::from_str::<Bind>(&text).unwrap(), some);
    }

    #[test]
    fn only_the_files_a_binding_names_belong_to_it() {
        let binds = [Bind::some("nxsave-0100e65002bb8000", "appdata/StardewValley/Saves", "*/*")];
        // A farm's files, both ways, in any letter case.
        assert_eq!(to_canon(&binds, "appdata/StardewValley/Saves/Farm_1/Farm_1").as_deref(), Some("nxsave-0100e65002bb8000/Farm_1/Farm_1"));
        assert_eq!(to_canon(&binds, "AppData/stardewvalley/saves/Farm_1/SaveGameInfo").as_deref(), Some("nxsave-0100e65002bb8000/Farm_1/SaveGameInfo"));
        assert_eq!(to_local(&binds, "NXSAVE-0100E65002BB8000/farm_2/FARM_2").as_deref(), Some("appdata/StardewValley/Saves/farm_2/FARM_2"));
        assert_eq!(to_local_with(&binds, "nxsave-0100e65002bb8000/Farm_1/Farm_1").map(|(_, c)| c), Some(Codec::Files));
        // A file in the folder itself, or deeper than the wildcard goes: in
        // the folder, but not the binding's.
        assert_eq!(to_local(&binds, "nxsave-0100e65002bb8000/startup_preferences"), None);
        assert_eq!(to_canon(&binds, "appdata/StardewValley/Saves/Farm_1/ErrorLogs/log.txt"), None);
        assert_eq!(to_canon(&binds, "appdata/StardewValley/Saves"), None);
        // Another root altogether, as ever.
        assert_eq!(to_local(&binds, "nxdevice-0100e65002bb8000/x/y"), None);
        assert_eq!(
            uncovered(&binds, ["nxsave-0100e65002bb8000/Farm_1/Farm_1", "nxsave-0100e65002bb8000/startup_preferences", "nxdevice-0100e65002bb8000/a/b"]),
            v(&["nxsave-0100e65002bb8000/startup_preferences", "nxdevice-0100e65002bb8000/a/b"])
        );
        // By codec too.
        assert_eq!(to_canon_as(&binds, "appdata/StardewValley/Saves/Farm_1/Farm_1", &Codec::Files).as_deref(), Some("nxsave-0100e65002bb8000/Farm_1/Farm_1"));
        assert_eq!(to_canon_as(&binds, "appdata/StardewValley/Saves/default_options", &Codec::Files), None);

        // What it does with two sides' files: the line's other files are
        // uncovered, the version's other files outside.
        let line = v(&["nxsave-0100e65002bb8000/Farm_1/Farm_1", "nxsave-0100e65002bb8000/Farm_1/SaveGameInfo", "nxsave-0100e65002bb8000/startup_preferences"]);
        let here = v(&["appdata/StardewValley/Saves/Farm_1/Farm_1", "appdata/StardewValley/Saves/Farm_2/Farm_2", "appdata/StardewValley/default_options"]);
        let p = measure(&binds, &line, &here);
        assert_eq!((p.matched, p.only_in_line, p.only_here), (1, 1, 1));
        assert_eq!(p.uncovered, v(&["nxsave-0100e65002bb8000/startup_preferences"]));
        assert_eq!(p.outside, v(&["appdata/StardewValley/default_options"]));

        // A narrower binding in a wider one: a file goes through the deepest
        // folder that holds it and names it.
        let both = [b("appdata/Game", "documents/Game"), Bind::some("appdata/Game/saves", "documents/Game/slots", "*.sav")];
        assert_eq!(to_canon(&both, "documents/Game/slots/a.sav").as_deref(), Some("appdata/Game/saves/a.sav"));
        assert_eq!(to_canon(&both, "documents/Game/slots/a.ini").as_deref(), Some("appdata/Game/slots/a.ini"));
        // Kept by the moves for other accounts' containers.
        let w = "localappdata/Packages/G/SystemAppData/wgs/0009_00AB";
        let narrow = [Bind { codec: Codec::Wgs, ..Bind::some("appdata/Game", w, "*.sav") }];
        assert_eq!(rebase(&narrow, &["localappdata/Packages/G/SystemAppData/wgs/0007_00AB".to_string()])[0].files.as_deref(), Some("*.sav"));
    }

    #[test]
    fn bindings_in_container_folders_read_the_containers() {
        let w = "localappdata/Packages/G/SystemAppData/wgs/0009_00AB";
        let binds = [b("appdata/Game/Saves", w), b("appdata/Game/cfg", "documents/Game")];
        assert_eq!(with_containers(&binds, &[w.to_string()]), Some(vec![wgs("appdata/Game/Saves", w), b("appdata/Game/cfg", "documents/Game")]));
        // Deeper in the container folder too.
        let deeper = [b("appdata/Game/Saves/world1", &format!("{w}/world1"))];
        assert_eq!(with_containers(&deeper, &[w.to_string()]).unwrap()[0].codec, Codec::Wgs);
        // A folder that holds the containers and other files: no way to read it.
        assert_eq!(with_containers(&[b("appdata", "localappdata/Packages/G")], &[w.to_string()]), None);
        // Mapping goes by codec one way, by the deepest folder the other.
        let both = [wgs("appdata/Game/Saves", w), b("appdata/Game/Saves/cfg", "documents/Game")];
        assert_eq!(to_canon_as(&both, &format!("{w}/0.sav"), &Codec::Wgs).as_deref(), Some("appdata/Game/Saves/0.sav"));
        assert_eq!(to_canon_as(&both, &format!("{w}/0.sav"), &Codec::Files), None);
        assert_eq!(to_local_with(&both, "appdata/Game/Saves/0.sav"), Some((format!("{w}/0.sav"), Codec::Wgs)));
        assert_eq!(to_local_with(&both, "appdata/Game/Saves/cfg/a.ini"), Some(("documents/Game/a.ini".to_string(), Codec::Files)));
        assert_eq!(to_local_with(&both, "appdata/Other/x"), None);
    }

    #[test]
    fn container_folders_of_another_account_are_moved_to_the_one_here() {
        let theirs = "localappdata/Packages/G/SystemAppData/wgs/0009_00AB";
        let mine = "localappdata/Packages/G/SystemAppData/wgs/0007_00AB".to_string();
        let binds = [wgs("appdata/Game/Saves", theirs), wgs("appdata/Game/Worlds/1", &format!("{theirs}/1")), b("appdata/Game/cfg", "documents/Game")];
        assert_eq!(
            rebase(&binds, std::slice::from_ref(&mine)),
            vec![wgs("appdata/Game/Saves", &mine), wgs("appdata/Game/Worlds/1", &format!("{mine}/1")), b("appdata/Game/cfg", "documents/Game")]
        );
        // The folder is here: as it is. Two here, or none: as it is.
        assert_eq!(rebase(&binds, &[theirs.to_string(), mine.clone()]), binds.to_vec());
        assert_eq!(rebase(&binds, &[]), binds.to_vec());
        // Not beside it: another game's folder.
        assert_eq!(rebase(&binds, &["localappdata/Packages/H/SystemAppData/wgs/0007_00AB".to_string()]), binds.to_vec());
    }

    #[test]
    fn paths_go_through_the_deepest_folder_that_holds_them() {
        let binds = [b("appdata/Game/Steam/123", "documents/My Games/Game"), b("appdata/Game/Steam/123/cfg", "localappdata/Game")];
        assert_eq!(to_canon(&binds, "documents/My Games/Game/slot1.sav").as_deref(), Some("appdata/Game/Steam/123/slot1.sav"));
        assert_eq!(to_canon(&binds, "DOCUMENTS/my games/game/Sub/A.sav").as_deref(), Some("appdata/Game/Steam/123/Sub/A.sav"));
        assert_eq!(to_canon(&binds, "localappdata/Game/a.ini").as_deref(), Some("appdata/Game/Steam/123/cfg/a.ini"));
        assert_eq!(to_canon(&binds, "documents/My Games/Game2/slot1.sav"), None);
        assert_eq!(to_canon(&binds, "documents/My Games"), None);
        assert_eq!(to_local(&binds, "appdata/Game/Steam/123/slot1.sav").as_deref(), Some("documents/My Games/Game/slot1.sav"));
        assert_eq!(to_local(&binds, "appdata/Game/Steam/123/cfg/a.ini").as_deref(), Some("localappdata/Game/a.ini"));
        assert_eq!(to_local(&binds, "appdata/Game/Steam/1234/slot1.sav"), None);
        assert_eq!(uncovered(&binds, ["appdata/Game/Steam/123/x", "appdata/Game/other"]), v(&["appdata/Game/other"]));
        // Without bindings nothing has a place.
        assert_eq!(to_canon(&[], "a/b"), None);
    }

    #[test]
    fn the_same_files_in_another_folder() {
        let line = v(&["appdata/Game/Steam/7656/saves/slot1.sav", "appdata/Game/Steam/7656/saves/slot2.sav", "appdata/Game/Steam/7656/saves/steam_autocloud.vdf"]);
        let here = v(&["documents/My Games/Game/saves/slot1.sav", "documents/My Games/Game/saves/slot3.sav", "documents/My Games/Game/settings.ini"]);
        let p = propose(&line, &here);
        // The deepest folder that holds what both have: the settings beside
        // it stay outside.
        assert_eq!(p.binds, vec![b("appdata/Game/Steam/7656/saves", "documents/My Games/Game/saves")]);
        assert_eq!((p.matched, p.only_in_line, p.only_here), (1, 2, 1));
        assert!(p.uncovered.is_empty());
        assert_eq!(p.outside, v(&["documents/My Games/Game/settings.ini"]));
        assert!(p.usable());
        // Every file of the line has a place, and comes back to its path.
        for f in &line {
            let l = to_local(&p.binds, f).unwrap();
            assert_eq!(to_canon(&p.binds, &l).as_deref(), Some(f.as_str()));
        }
    }

    #[test]
    fn files_in_folders_above_widen_the_folder() {
        let line = v(&["appdata/Game/saves/a.sav", "appdata/Game/profile.dat"]);
        let here = v(&["documents/Game/saves/a.sav", "documents/Game/profile.dat", "documents/Game/log.txt"]);
        let p = propose(&line, &here);
        assert_eq!(p.binds, vec![b("appdata/Game", "documents/Game")]);
        assert_eq!((p.matched, p.only_in_line, p.only_here), (2, 0, 1));
        assert!(p.usable() && p.outside.is_empty());
    }

    #[test]
    fn two_folders_are_two_bindings() {
        let line = v(&["appdata/Game/saves/a.sav", "localappdata/Game/keys.cfg"]);
        let here = v(&["documents/Game/saves/a.sav", "install/cfg/keys.cfg"]);
        let p = propose(&line, &here);
        let mut binds = p.binds.clone();
        binds.sort_by(|x, y| x.canon.cmp(&y.canon));
        assert_eq!(binds, vec![b("appdata/Game/saves", "documents/Game/saves"), b("localappdata/Game", "install/cfg")]);
        assert!(p.usable());
    }

    #[test]
    fn a_line_with_files_that_have_no_folder_here_is_not_joined() {
        let line = v(&["appdata/Game/saves/a.sav", "localappdata/Game/keys.cfg"]);
        let here = v(&["documents/Game/saves/a.sav"]);
        let p = propose(&line, &here);
        assert_eq!(p.binds, vec![b("appdata/Game/saves", "documents/Game/saves")]);
        assert_eq!(p.uncovered, v(&["localappdata/Game/keys.cfg"]));
        assert!(!p.usable());
        // Nothing in common, or nothing here: nothing to go by.
        assert!(!propose(&line, &v(&["documents/Other/b.sav"])).usable());
        let none = propose(&line, &[]);
        assert!(none.binds.is_empty() && none.uncovered.len() == 2 && !none.usable());
        // Files under different names in the root itself: the roots are the
        // folders.
        let p = propose(&v(&["install/save.dat"]), &v(&["documents/Game/save.dat"]));
        assert_eq!(p.binds, vec![b("install", "documents/Game")]);
    }

    #[test]
    fn a_folder_is_bound_once() {
        // The slots are in one folder there and in another here, and a file
        // above them is in a folder that would contain the first pair.
        let line = v(&["appdata/G/saves/a.sav", "appdata/G/saves/b.sav", "appdata/G/top.dat"]);
        let here = v(&["documents/Other/saves/a.sav", "documents/Other/saves/b.sav", "documents/G/top.dat"]);
        let p = propose(&line, &here);
        assert_eq!(p.binds, vec![b("appdata/G/saves", "documents/Other/saves")]);
        assert_eq!(p.uncovered, v(&["appdata/G/top.dat"]));
        assert!(!p.usable());
    }
}
