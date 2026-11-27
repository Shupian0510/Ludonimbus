//! Ludonimbus core: everything except UI.
//!
//! - [`config`]   user configuration (device, cloud provider, games)
//! - [`adapters`] built-in save rules for emulators / recomps
//! - [`emulators`] which game an emulator shortcut starts, per-game saves
//! - [`emugames`] games with saves in emulators' own data folders
//! - [`switch`]   Switch title ids, Ryujinx's per-PC save numbers
//! - [`recomplauncher`] games installed with RecompLauncher
//! - [`spec`]     effective save spec + root path resolution per device
//! - [`snapshot`] scanning, hashing, zip snapshots, restoring
//! - [`cloud`]    cloud providers (local folder, Google Drive, WebDAV, S3)
//! - [`sync`]     Steam-Cloud-like sync decisions and conflict handling
//! - [`playtime`] play sessions per device, totals over all devices
//! - [`screenshots`] Steam screenshots of linked shortcuts in the cloud
//! - [`steam`]    Steam install, accounts, non-Steam shortcuts, artwork
//! - [`steamcloud`] games installed through Steam: backup only, taking over
//!   from Steam Cloud and handing back
//! - [`millennium`] Millennium install and the Ludonimbus Steam plugin
//! - [`openlist`] the OpenList server used to reach Quark
//! - [`games`]    high-level library operations shared by CLI / runner / app
//! - [`gamebackup`] game files in the cloud, speed test of the cloud
//! - [`i18n`]     interface language for texts made in Rust (`tr!`)
//! - [`xbox`]     Xbox app games: launch helper, real game processes

pub mod achievements;
pub mod adapters;
pub mod artsync;
pub mod bind;
pub mod catchup;
pub mod cloud;
pub mod config;
pub mod credstore;
pub mod crosssaves;
pub mod discover;
pub mod emugames;
pub mod emulators;
pub mod gamebackup;
pub mod games;
pub mod handheld;
pub mod i18n;
pub mod join;
pub mod localart;
pub mod logos;
pub mod lockfile;
pub mod logger;
pub mod ludusavi;
pub mod millennium;
pub mod nxlan;
pub mod openlist;
pub mod recomplauncher;
pub mod paths;
pub mod playtime;
pub mod process;
pub mod prompt;
pub mod relocate;
pub mod savelib;
pub mod savefind;
pub mod screenshots;
pub mod share;
pub mod shelf;
pub mod snapshot;
pub mod spec;
pub mod state;
pub mod steam;
pub mod steamcloud;
pub mod steamgriddb;
pub mod switch;
pub mod sync;
pub mod transfer;
pub mod util;
pub mod vdf;
pub mod vv;
pub mod wgs;
pub mod xbox;
pub mod xboxsaves;

pub use anyhow::{Error, Result};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
