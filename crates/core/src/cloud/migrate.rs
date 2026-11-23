//! Copy everything from one cloud to another when the user switches clouds.
//!
//! Snapshots keep their ids, so each device's "last synced" state stays valid
//! after the switch. Snapshots already present in the target are skipped, so
//! an interrupted copy can simply be run again. The source is never changed.

use super::CloudProvider;
use crate::paths;
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{BTreeSet, HashSet};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub game: String,
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub games: usize,
    pub copied: usize,
    pub skipped: usize,
    pub bytes: u64,
}

/// Copy game definitions, snapshots and devices from `from` to `to`.
/// `extra_games` adds keys whose definition may be missing in `from`.
pub fn copy_all(
    from: &dyn CloudProvider,
    to: &dyn CloudProvider,
    extra_games: &[String],
    mut progress: impl FnMut(&Progress),
) -> Result<Report> {
    from.account().context(crate::tr!("无法访问原来的云盘", "Can't reach the old cloud"))?;
    to.account().context(crate::tr!("无法访问新的云盘", "Can't reach the new cloud"))?;
    let mut report = Report::default();

    let mut keys: BTreeSet<String> = extra_games.iter().cloned().collect();
    for g in from.list_games()? {
        to.put_game(&g)?;
        keys.insert(g.key);
    }

    let mut work = Vec::new();
    for key in &keys {
        let source = from.list_snapshots(key)?;
        if source.is_empty() {
            continue;
        }
        report.games += 1;
        let have: HashSet<String> = to.list_snapshots(key)?.into_iter().map(|s| s.id).collect();
        for s in source {
            if have.contains(&s.id) {
                report.skipped += 1;
            } else {
                work.push((key.clone(), s.id));
            }
        }
    }

    let tmp = paths::staging_dir().join(format!("migrate-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&tmp)?;
    let result = (|| -> Result<()> {
        for (i, (key, id)) in work.iter().enumerate() {
            progress(&Progress {
                game: key.clone(),
                done: i,
                total: work.len(),
            });
            let meta = from.get_meta(key, id)?;
            let zip = tmp.join(format!("{id}.zip"));
            from.download(key, id, &zip)?;
            report.bytes += std::fs::metadata(&zip).map(|m| m.len()).unwrap_or(0);
            to.upload(&meta, &zip)?;
            let _ = std::fs::remove_file(&zip);
            report.copied += 1;
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    progress(&Progress {
        game: String::new(),
        done: work.len(),
        total: work.len(),
    });

    for d in from.list_devices().unwrap_or_default() {
        let _ = to.put_device(&d);
    }
    Ok(report)
}
