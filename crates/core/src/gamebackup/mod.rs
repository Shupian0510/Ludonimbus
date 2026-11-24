//! Game backups: the game's own files (not its saves) in the cloud, so a game
//! that is gone from this PC can be installed again from there. Design and
//! cloud layout: docs/plan-game-backup.md.
//!
//! Everything here works on a path-based [`Store`]; callers pass
//! `provider.files()` of the current cloud, tests a local folder or WebDAV.
//!
//! - [`speedtest`] upload / download speed of a cloud

pub mod speedtest;

use crate::cloud::{self, CloudProvider, NotConfigured};
use crate::config::AppConfig;
use anyhow::{anyhow, Result};

/// The current cloud; use `.files()` for the path-based store.
pub fn open(cfg: &AppConfig) -> Result<Box<dyn CloudProvider>> {
    cloud::open(cfg)?.ok_or_else(|| anyhow!(NotConfigured))
}
