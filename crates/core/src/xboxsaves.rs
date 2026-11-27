//! The files in an Xbox app game's save containers ([`crate::wgs`]) under
//! the names the game's other versions (Steam, Epic, GOG) use, and back.
//! Per-game rules are in `adapters/xbox-saves.yaml`, ported from
//! XGP-save-extractor (MIT, © 2021 Mark Mäkinen); the handlers below follow
//! its `main.py`.
//!
//! A file can be written back when it is exactly one container file
//! ([`Named::origin`]): the containers' file is replaced with it
//! ([`crate::wgs::begin`]). Files put together from several container files
//! or made up are read only.

use crate::wgs::{self, Container};
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// One game in `adapters/xbox-saves.yaml` (built into the program). Unknown
/// fields are refused, so a misspelt one fails the tests.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    pub package: String,
    pub handler: String,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub icon_format: Option<String>,
}

pub fn rules() -> &'static [Rule] {
    static R: OnceLock<Vec<Rule>> = OnceLock::new();
    R.get_or_init(|| serde_yaml::from_str(include_str!("../../../adapters/xbox-saves.yaml")).expect("adapters/xbox-saves.yaml must be valid"))
}

/// The rule for a package (`<family name>` or `<family name>!<app>`).
pub fn rule_for(package: &str) -> Option<&'static Rule> {
    let family = package.split('!').next().unwrap_or(package);
    rules().iter().find(|r| r.package.eq_ignore_ascii_case(family))
}

/// One file of the other versions, made from container files.
#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    /// Relative path, `/` separated.
    pub path: String,
    /// The container file it is, when it is exactly one (so it can be
    /// written back); `None` when it's put together from several or made up.
    pub origin: Option<(String, String)>,
    pub data: Vec<u8>,
}

fn one(path: String, container: &str, file: &str, data: &[u8]) -> Named {
    Named { path, origin: Some((container.to_string(), file.to_string())), data: data.to_vec() }
}

/// The files in the containers of the wgs user folder `dir` ([`named`]),
/// with the folder's index.
pub fn read_dir(dir: &Path) -> Result<(wgs::Index, Vec<Named>)> {
    let (idx, containers) = wgs::read_dir(dir)?;
    let files = named(&idx.package, &containers);
    Ok((idx, files))
}

static SPOIL_READ_BACK: AtomicBool = AtomicBool::new(false);

/// Tests: the next [`read_back`] finds its first file changed, as if
/// something else had written into the containers in between.
#[doc(hidden)]
pub fn spoil_next_read_back() {
    SPOIL_READ_BACK.store(true, Ordering::SeqCst);
}

/// [`read_dir`] right after writing into the containers: what the write is
/// checked against.
pub fn read_back(dir: &Path) -> Result<Vec<Named>> {
    let (_, mut files) = read_dir(dir)?;
    if SPOIL_READ_BACK.swap(false, Ordering::SeqCst) {
        if let Some(f) = files.first_mut() {
            f.data.push(0);
        }
    }
    Ok(files)
}

/// The handler a package's containers are read with.
pub fn handler_for(package: &str, containers: &[Container]) -> (&'static str, Option<&'static Rule>) {
    match rule_for(package) {
        Some(r) => (r.handler.as_str(), Some(r)),
        None if containers.iter().all(|c| c.files.len() == 1) => ("1c1f", None),
        None => ("1cnf-folder", None),
    }
}

/// The containers' files under the names the game's other versions use.
/// Unknown games get the generic rule (see [`handler_for`]).
pub fn named(package: &str, containers: &[Container]) -> Vec<Named> {
    let (handler, rule) = handler_for(package, containers);
    let suffix = rule.and_then(|r| r.suffix.clone()).unwrap_or_default();
    let mut out = Vec::new();
    let first = |c: &Container| c.files.first().map(|(n, d)| (n.clone(), d.clone()));
    match handler {
        "1c1f" => {
            for c in containers {
                if let Some((f, d)) = first(c) {
                    out.push(one(format!("{}{suffix}", c.name), &c.name, &f, &d));
                }
            }
        }
        "1cnf" => {
            if let Some(c) = containers.first() {
                for (f, d) in &c.files {
                    out.push(one(format!("{f}{suffix}"), &c.name, f, d));
                }
            }
        }
        "1cnf-folder" => {
            for c in containers {
                for (f, d) in &c.files {
                    out.push(one(format!("{}/{f}", c.name), &c.name, f, d));
                }
            }
        }
        "forza" => {
            for c in containers {
                for (f, d) in &c.files {
                    out.push(one(format!("{}.{f}", c.name), &c.name, f, d));
                }
            }
        }
        "palworld" => {
            for c in containers {
                if let Some((f, d)) = first(c) {
                    out.push(one(format!("{}.sav", c.name.replace('-', "/")), &c.name, &f, &d));
                }
            }
        }
        "lies-of-p" => {
            for c in containers {
                if let Some((f, d)) = first(c) {
                    let trimmed = c.name.trim_start_matches(|ch: char| ch.is_ascii_digit());
                    let name = if trimmed.is_empty() { c.name.as_str() } else { trimmed };
                    out.push(one(format!("{name}.sav"), &c.name, &f, &d));
                }
            }
        }
        "coral-island" => {
            for c in containers {
                if let Some((f, d)) = first(c) {
                    let mut name = format!("{}.sav", c.name);
                    if let Some(rest) = name.strip_prefix("Backup") {
                        name = format!("Backup/{rest}");
                    }
                    out.push(one(name, &c.name, &f, &d));
                }
            }
        }
        "state-of-decay-2" => {
            if let Some(c) = containers.first() {
                for (f, d) in &c.files {
                    let last = f.rsplit('/').next().unwrap_or(f);
                    out.push(one(format!("{last}.sav"), &c.name, f, d));
                }
            }
        }
        "railway-empire-2" => {
            for c in containers {
                for (f, d) in c.files.iter().filter(|(f, _)| f == "savegame") {
                    out.push(one(c.name.clone(), &c.name, f, d));
                }
            }
        }
        "arcade-paradise" => {
            if let Some(c) = containers.first() {
                if let Some((f, d)) = first(c) {
                    out.push(one("RATSaveData.dat".into(), &c.name, &f, &d));
                }
            }
        }
        "cricket-24" => {
            for c in containers {
                for (f, d) in &c.files {
                    let name = f.strip_suffix(".CHUNK0").unwrap_or(f);
                    if name.contains("CHUNK") {
                        continue;
                    }
                    out.push(one(format!("{}/{name}.SAV", c.name), &c.name, f, d));
                }
            }
        }
        "like-a-dragon" => {
            let icon = rule.and_then(|r| r.icon_format.clone());
            for c in containers {
                let (parent, leaf) = match c.name.rsplit_once('/') {
                    Some((p, l)) => (Some(p), l),
                    None => (None, c.name.as_str()),
                };
                let leaf = match leaf {
                    "datasav" => "data.sav",
                    "datasys" => "data.sys",
                    other => other,
                };
                let path = match parent {
                    Some(p) => format!("{p}/{leaf}"),
                    None => leaf.to_string(),
                };
                for (f, d) in &c.files {
                    if f.eq_ignore_ascii_case("data") {
                        out.push(one(path.clone(), &c.name, f, d));
                    } else if f.eq_ignore_ascii_case("icon") {
                        if let Some(fmt) = &icon {
                            let parent_name = parent.and_then(|p| p.rsplit('/').next()).unwrap_or("");
                            let file = format!("{parent_name}_icon.{fmt}");
                            out.push(one(
                                match parent {
                                    Some(p) => format!("{p}/{file}"),
                                    None => file,
                                },
                                &c.name,
                                f,
                                d,
                            ));
                        }
                    }
                }
            }
        }
        "control" => {
            for c in containers {
                out.push(Named { path: format!("{}/--containerDisplayName.chunk", c.name), origin: None, data: c.name.as_bytes().to_vec() });
                for (f, d) in &c.files {
                    out.push(one(format!("{}/{f}.chunk", c.name), &c.name, f, d));
                }
            }
        }
        "starfield" => {
            for c in containers {
                let (parent, leaf) = match c.name.rsplit_once('/') {
                    Some((p, l)) => (p.rsplit('/').next().unwrap_or(p), l),
                    None => continue,
                };
                if parent != "Saves" {
                    continue;
                }
                let new_format = c.files.iter().any(|(f, _)| f == "toc");
                let mut parts: Vec<(u64, &[u8])> = Vec::new();
                let mut ok = true;
                for (f, d) in &c.files {
                    let idx = if new_format {
                        if f == "toc" {
                            continue;
                        }
                        f.strip_prefix("BlobData").and_then(|n| n.parse().ok())
                    } else if f == "BETHESDAPFH" {
                        Some(0)
                    } else {
                        f.trim_matches('P').parse::<u64>().ok().map(|n| n + 1)
                    };
                    match idx {
                        Some(i) => parts.push((i, d)),
                        None => ok = false,
                    }
                }
                if !ok {
                    continue;
                }
                parts.sort_by_key(|(i, _)| *i);
                let pad = b"padding\0padding\0";
                let mut data = Vec::new();
                for (_, d) in parts {
                    data.extend_from_slice(d);
                    let n = 16 - (d.len() % 16);
                    if n != 16 {
                        data.extend_from_slice(&pad[..n]);
                    }
                }
                out.push(Named { path: leaf.to_string(), origin: None, data });
            }
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str, files: &[(&str, &[u8])]) -> Container {
        Container { name: name.into(), files: files.iter().map(|(n, d)| (n.to_string(), d.to_vec())).collect() }
    }

    #[test]
    fn rules_load() {
        assert!(rules().len() >= 45);
        assert_eq!(rule_for("PocketpairInc.Palworld_ad4psfrxyesvt!AppPalworldShipping").unwrap().handler, "palworld");
        assert_eq!(rule_for("SabotageStudio.SeaofStars_p3aneehax6csy").unwrap().suffix.as_deref(), Some(".sos"));
    }

    #[test]
    fn names_by_handler() {
        let pal = named("PocketpairInc.Palworld_ad4psfrxyesvt", &[c("0123ABCD-Level", &[("Data", b"x")])]);
        assert_eq!(pal[0].path, "0123ABCD/Level.sav");
        assert_eq!(pal[0].origin, Some(("0123ABCD-Level".into(), "Data".into())));
        let lop = named("Neowiz.3616725F496B_r4z3116tdh636", &[c("1234SaveData", &[("Data", b"x")])]);
        assert_eq!(lop[0].path, "SaveData.sav");
        let coral = named("HumbleBundle.CoralIsland_q2mcdwmzx4qja", &[c("BackupSlot1", &[("Data", b"x")])]);
        assert_eq!(coral[0].path, "Backup/Slot1.sav");
        let lad = named("SEGAofAmericaInc.ProjectMacan_s751p9cej88mt", &[c("save/slot0/datasav", &[("data", b"d"), ("icon", b"i")])]);
        assert_eq!(lad.iter().map(|n| n.path.as_str()).collect::<Vec<_>>(), vec!["save/slot0/data.sav", "save/slot0/slot0_icon.png"]);
        // Unknown games: one file per container → its name; else folders.
        assert_eq!(named("Some.Game_x", &[c("slot1", &[("Data", b"x")])])[0].path, "slot1");
        assert_eq!(named("Some.Game_x", &[c("slot1", &[("a", b"x"), ("b", b"y")])])[1].path, "slot1/b");
    }

    #[test]
    fn starfield_saves_are_put_together() {
        let sf = named(
            "BethesdaSoftworks.ProjectGold_3275kfvn8vcwc",
            &[c("Saves/Save1.sfs", &[("P0", b"12345"), ("BETHESDAPFH", b"0123456789abcdef")]), c("Settings/x", &[("Data", b"s")])],
        );
        assert_eq!(sf.len(), 1);
        assert_eq!(sf[0].path, "Save1.sfs");
        // 16 bytes need no padding; 5 bytes get the first 11 of "padding\0padding\0".
        assert_eq!(sf[0].data, b"0123456789abcdef12345padding\0pad".to_vec());
        assert_eq!(sf[0].origin, None);
    }
}
