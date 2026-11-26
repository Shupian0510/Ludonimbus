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

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// One folder of a save line, and the folder that holds the same files in
/// a version of the game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bind {
    /// The folder in the line's files.
    pub canon: String,
    /// The folder in the version's own files.
    pub local: String,
}

/// `path` below the folder `dir` (or `dir` itself): what follows `dir`,
/// with its leading `/`.
fn below<'a>(dir: &str, path: &'a str) -> Option<&'a str> {
    let head = path.get(..dir.len())?;
    let rest = &path[dir.len()..];
    (same(head, dir) && (rest.is_empty() || rest.starts_with('/'))).then_some(rest)
}

fn mapped(binds: &[Bind], path: &str, from: impl Fn(&Bind) -> &str, to: impl Fn(&Bind) -> &str) -> Option<String> {
    binds
        .iter()
        .filter_map(|b| below(from(b), path).map(|rest| (b, rest)))
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
        binds.push(Bind { canon: cd, local: ld });
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
        Bind { canon: canon.into(), local: local.into() }
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
