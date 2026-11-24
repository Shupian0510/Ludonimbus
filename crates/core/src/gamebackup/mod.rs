//! Game backups: the game's own files (not its saves) in the cloud, so a game
//! that is gone from this PC can be installed again from there. Design and
//! cloud layout: docs/plan-game-backup.md.
//!
//! Everything here works on a path-based [`Store`](crate::cloud::store::Store); callers pass
//! `provider.files()` of the current cloud, tests a local folder or WebDAV.
//!
//! - [`chunker`]   content-defined chunking of the folder stream
//! - [`chunk`]     stored chunk objects (zstd or raw), named by BLAKE3
//! - [`speedtest`] upload / download speed of a cloud

pub mod chunk;
pub mod chunker;
pub mod speedtest;

use crate::cloud::{self, CloudProvider, NotConfigured};
use crate::config::AppConfig;
use anyhow::{anyhow, Result};

/// The current cloud; use `.files()` for the path-based store.
pub fn open(cfg: &AppConfig) -> Result<Box<dyn CloudProvider>> {
    cloud::open(cfg)?.ok_or_else(|| anyhow!(NotConfigured))
}
