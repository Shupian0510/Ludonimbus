//! Which snapshots of a save line are kept (docs/plan-v2.md, section 5.4).
//!
//! Contract of this module (nothing is here yet). One pure function: given a
//! [`super::line::LineView`], the snapshot each device synced last on this
//! line together with the time that device was last seen, the policy and
//! the current time, return what to delete and, for everything kept, why.
//!
//! Always kept:
//! - every head (one, or each side of a fork);
//! - snapshots the user keeps (they have a pin);
//! - the newest snapshot of each platform;
//! - the snapshot each device synced last, until that device has not been
//!   seen for 90 days;
//! - of the remaining automatic snapshots: the newest 20, and one per day
//!   for 30 days (both numbers from the settings, `config::Retention`).
//!
//! Snapshots on the side (made before a restore, the losing side of a
//! conflict) and manual backups follow the rules `sync::prune` applies to
//! them today.
//!
//! Deleting is up to the caller, and so is removing the packs only the
//! deleted manifests used ([`super::packs`]).
