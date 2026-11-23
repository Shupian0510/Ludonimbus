//! Ludonimbus core: everything except UI.
//!
//! - [`config`]   user configuration (device, cloud provider, games)
//! - [`adapters`] built-in save rules for emulators / recomps
//! - [`spec`]     effective save spec + root path resolution per device
//! - [`snapshot`] scanning, hashing, zip snapshots, restoring
//! - [`cloud`]    cloud providers (local folder, Google Drive)
//! - [`sync`]     Steam-Cloud-like sync decisions and conflict handling
//! - [`steam`]    Steam install, accounts, non-Steam shortcuts, artwork
//! - [`games`]    high-level library operations shared by CLI / runner / app

pub mod adapters;
pub mod cloud;
pub mod config;
pub mod games;
pub mod lockfile;
pub mod logger;
pub mod ludusavi;
pub mod paths;
pub mod process;
pub mod relocate;
pub mod snapshot;
pub mod spec;
pub mod state;
pub mod steam;
pub mod sync;
pub mod util;
pub mod vdf;

pub use anyhow::{Error, Result};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
