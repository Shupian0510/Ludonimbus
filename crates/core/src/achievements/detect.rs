//! Trophy sources a game probably uses, found from its adapter and folder.

use super::trophy;
use super::{Binding, SourceKind};
use crate::config::GameEntry;
use serde::Serialize;
use std::path::{Path, PathBuf};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "AchievementCandidate")]
pub struct Candidate {
    pub binding: Binding,
    /// Game title from the trophy files, when known.
    pub title: Option<String>,
    /// Where the files are.
    pub location: String,
    /// The only sensible choice (e.g. Bloodborne on Lantern): link it without asking.
    pub sure: bool,
}

fn install_dir(g: &GameEntry) -> Option<PathBuf> {
    g.install.clone().or_else(|| g.exe.as_ref().and_then(|e| e.parent().map(Path::to_path_buf)))
}

fn adapter_is(g: &GameEntry, prefix: &str) -> bool {
    g.adapter.as_deref().is_some_and(|a| a.starts_with(prefix))
}

fn shadps4_title(user: &Path, npwr: &str) -> Option<String> {
    let xml = user.join("trophy").join(npwr).join("Xml").join("TROP.XML");
    let text = std::fs::read_to_string(xml).ok()?;
    let start = text.find("<title-name>")? + "<title-name>".len();
    let end = text[start..].find("</title-name>")? + start;
    Some(text[start..end].trim().to_string()).filter(|t| !t.is_empty())
}

pub fn candidates(g: &GameEntry) -> Vec<Candidate> {
    let mut out = Vec::new();
    let install = install_dir(g);

    // Lantern Engine (Bloodborne): its own trophy log next to the saves.
    if let Some(dir) = install.as_ref().filter(|d| adapter_is(g, "bloodborne-lantern") || d.join("Lantern Engine.exe").is_file()) {
        let user = dir.join("user");
        let npwr =
            trophy::lantern_game_dir().and_then(|d| trophy::npbind_id(&d)).or_else(|| trophy::shadps4_sets(&user).into_iter().next()).unwrap_or_default();
        out.push(Candidate {
            binding: Binding { source: SourceKind::Lantern, source_id: npwr, path: Some(user.clone()), popup: None },
            title: Some(g.name.clone()),
            location: user.display().to_string(),
            sure: true,
        });
        return out;
    }

    // shadPS4: every trophy set in its user folder(s).
    let shad = adapter_is(g, "shadps4") || install.as_ref().is_some_and(|d| d.join("shadPS4.exe").is_file());
    if shad {
        let mut users: Vec<PathBuf> = Vec::new();
        if let Some(u) = g.roots.get("user") {
            users.push(PathBuf::from(u));
        }
        if let Some(d) = &install {
            users.push(d.join("user"));
        }
        if let Some(a) = dirs::config_dir() {
            users.push(a.join("shadPS4"));
        }
        let mut seen = Vec::new();
        for user in users.into_iter().filter(|u| u.is_dir()) {
            for npwr in trophy::shadps4_sets(&user) {
                if seen.contains(&npwr) {
                    continue;
                }
                seen.push(npwr.clone());
                out.push(Candidate {
                    title: shadps4_title(&user, &npwr),
                    location: user.display().to_string(),
                    binding: Binding { source: SourceKind::Shadps4, source_id: npwr, path: Some(user.clone()), popup: None },
                    sure: false,
                });
            }
        }
    }

    // RPCS3: every trophy set in dev_hdd0.
    let rpcs3 = install.as_ref().filter(|d| adapter_is(g, "rpcs3") || d.join("rpcs3.exe").is_file());
    if let Some(dir) = rpcs3 {
        for (set, title) in trophy::rpcs3_sets(dir) {
            let npwr = set.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            out.push(Candidate {
                title: Some(title),
                location: set.display().to_string(),
                binding: Binding { source: SourceKind::Rpcs3, source_id: npwr, path: Some(set), popup: None },
                sure: false,
            });
        }
    }

    // PS4 ports and recompilations: the original game's files in their folder.
    let platform = g.adapter.as_deref().and_then(crate::adapters::find).and_then(|a| a.platform.clone()).unwrap_or_default();
    if let Some(dir) = install.as_ref().filter(|_| platform.contains("PS4") && !shad) {
        if let Some((npwr, _)) = trophy::port_set(dir) {
            out.push(Candidate {
                binding: Binding { source: SourceKind::Ps4port, source_id: npwr, path: Some(dir.clone()), popup: None },
                title: Some(g.name.clone()),
                location: dir.display().to_string(),
                sure: true,
            });
        }
    }

    // Games Ludonimbus has its own save rules for (by adapter).
    if out.is_empty() {
        if let Some((key, list)) = super::rules::bundled_key(g).and_then(|k| super::rules::load(&k, false).ok().flatten().map(|l| (k, l))) {
            out.push(Candidate {
                binding: Binding { source: SourceKind::Rules, source_id: key, path: None, popup: None },
                title: Some(list.display_name()).filter(|n| !n.is_empty()),
                location: "Ludonimbus".to_string(),
                sure: true,
            });
        }
    }

    // Switch games (emulated or recompiled): save rules in TrophyNX's format.
    if out.is_empty() {
        if let Some(tid) = super::rules::title_id(g) {
            match super::rules::load_list(&tid, true) {
                Ok(Some(list)) => out.push(Candidate {
                    binding: Binding { source: SourceKind::Rules, source_id: tid, path: None, popup: None },
                    title: Some(list.name).filter(|n| !n.is_empty()),
                    location: "TrophyNX".to_string(),
                    sure: true,
                }),
                Ok(None) => {}
                Err(e) => super::log(format!("{}: achievement rules unavailable: {e:#}", g.key)),
            }
        }
    }

    // Prefer a set whose title matches the game's name.
    let name = g.name.to_lowercase();
    out.sort_by_key(|c| !c.title.as_ref().is_some_and(|t| !name.is_empty() && (t.to_lowercase().contains(&name) || name.contains(&t.to_lowercase()))));
    out
}

/// A source picked by hand: a trophy file or folder.
pub fn from_path(path: &Path) -> Option<Binding> {
    let name = path.file_name()?.to_string_lossy().to_ascii_uppercase();
    let dir = if path.is_dir() { path.to_path_buf() } else { path.parent()?.to_path_buf() };
    // RPCS3 set folder (TROPUSR.DAT / TROPCONF.SFM / the folder itself).
    if name == "TROPUSR.DAT" || name == "TROPCONF.SFM" || crate::util::find_file_ci(&dir, "TROPCONF.SFM").is_some() {
        let npwr = dir.file_name()?.to_string_lossy().to_string();
        return Some(Binding { source: SourceKind::Rpcs3, source_id: npwr, path: Some(dir), popup: None });
    }
    // Lantern: trophies.log in <user>/savedata/…
    if name == "TROPHIES.LOG" {
        let user = path.ancestors().find(|a| a.file_name().is_some_and(|n| n.eq_ignore_ascii_case("user")))?.to_path_buf();
        let npwr = trophy::shadps4_sets(&user).into_iter().next().unwrap_or_default();
        return Some(Binding { source: SourceKind::Lantern, source_id: npwr, path: Some(user), popup: None });
    }
    // shadPS4: <user>/home/<uid>/trophy/<NPWR>.xml or <user>/trophy/<NPWR>/Xml/TROP*.XML
    let stem = path.file_stem()?.to_string_lossy().to_string();
    if trophy::is_npwr(&stem) {
        let user = path.parent()?.parent()?.parent()?.parent()?.to_path_buf();
        return Some(Binding { source: SourceKind::Shadps4, source_id: stem, path: Some(user), popup: None });
    }
    if name.starts_with("TROP") && name.ends_with(".XML") {
        let set = path.parent()?.parent()?;
        let npwr = set.file_name()?.to_string_lossy().to_string();
        let user = set.parent()?.parent()?.to_path_buf();
        return Some(Binding { source: SourceKind::Shadps4, source_id: npwr, path: Some(user), popup: None });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picked_files() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user");
        let save = user.join("home/1000/trophy/NPWR05818_00.xml");
        std::fs::create_dir_all(save.parent().unwrap()).unwrap();
        std::fs::write(&save, "<trophyconf/>").unwrap();
        let b = from_path(&save).unwrap();
        assert_eq!(b.source, SourceKind::Shadps4);
        assert_eq!(b.source_id, "NPWR05818_00");
        assert_eq!(b.path.as_deref(), Some(user.as_path()));

        let log = user.join("savedata/1/trophies.log");
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        std::fs::write(&log, "").unwrap();
        let b = from_path(&log).unwrap();
        assert_eq!(b.source, SourceKind::Lantern);
        assert_eq!(b.path.as_deref(), Some(user.as_path()));

        let set = dir.path().join("dev_hdd0/home/00000001/trophy/NPWR00001_00");
        std::fs::create_dir_all(&set).unwrap();
        std::fs::write(set.join("TROPCONF.SFM"), "<trophyconf/>").unwrap();
        let b = from_path(&set.join("TROPUSR.DAT")).unwrap();
        assert_eq!(b.source, SourceKind::Rpcs3);
        assert_eq!(b.source_id, "NPWR00001_00");
    }
}
