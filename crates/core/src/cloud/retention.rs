//! Which snapshots of a save line are kept (docs/plan-v2.md, section 5.4).
//!
//! [`plan`] is one pure function: from a [`LineView`], the snapshot each
//! device synced last, the policy and the current time, it says for every
//! snapshot of the line "delete" or "keep, and why". It reads no store and
//! no clock, so every device comes to the same plan for the same line.
//! Deleting is up to the caller, and so is removing the packs only the
//! deleted manifests used ([`super::packs`]).
//!
//! Kept whatever their age:
//!
//! | what | why |
//! |---|---|
//! | every head (one, or each side of a fork) | the current progress |
//! | snapshots with a pin | the user keeps them (section 3.7) |
//! | locked snapshots and manual backups | 0.6 keeps them forever, and they have no pin until the migration writes one |
//! | the newest snapshot of each platform | the saves tab shows it (section 10.2) |
//! | the snapshot a device synced last, until that device has not been seen for 90 days | the base of that device's next upload |
//!
//! Everything else is an automatic snapshot: the newest 20 stay, and the
//! newest of each day for 30 days (both numbers from the settings,
//! [`config::Retention`]). This is the rule of 0.6 (`sync::prune`) and it
//! counts the same way: a head, the newest of a platform and the base of a
//! device take one of the 20 places like any other automatic snapshot;
//! pinned, locked and manual ones take none.
//!
//! Snapshots on the side (made before a restore, the losing side of a
//! conflict) are automatic snapshots too, because 0.6 has no rule of its
//! own for them: `Trigger::is_pinned` is true for manual backups only. They
//! take a place among the 20 and a day like the others, and go the same
//! way.
//!
//! A manifest that can't be understood is not among a [`LineView`]'s
//! snapshots, so it is never named here: what this build can't read, it
//! doesn't delete.
//!
//! The plan names snapshots, not files. A line can live in several folders
//! ([`super::line::load_all`]), and every snapshot of the view is judged the
//! same way wherever its manifest is. Whether a folder that is only read
//! may be cleaned up at all is for the caller to decide
//! ([`LineView::place`]).

use super::line::LineView;
use super::v2::IndexEntry;
use crate::{config, vv};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};

/// A device that has not been seen for this many days no longer has its
/// base kept. A device that stayed away for 60 days uploads a complete pack
/// again instead of building on its base, so the 30 days between are slack.
pub const DEVICE_DAYS: i64 = 90;

/// The trigger of a backup the user made by hand.
const MANUAL: &str = "manual";
/// What 0.6 sets in a snapshot's own file when the user locks it.
const LOCKED: &str = "locked";

/// How much is kept. Policy, not format (section 3.9): it can change from
/// one version of the program to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// Always keep this many of the newest automatic snapshots.
    pub keep_recent: usize,
    /// Also keep the newest automatic snapshot of each day, for this many
    /// days.
    pub keep_daily_days: i64,
    /// Keep the snapshot a device synced last until the device has not been
    /// seen for this many days.
    pub device_days: i64,
}

impl Default for Policy {
    fn default() -> Self {
        Policy::from(&config::Retention::default())
    }
}

impl From<&config::Retention> for Policy {
    fn from(settings: &config::Retention) -> Self {
        Policy { keep_recent: settings.keep_recent, keep_daily_days: settings.keep_daily_days, device_days: DEVICE_DAYS }
    }
}

/// The snapshot one install synced last on this line, from the device's own
/// record (`devices/<device id>.json`, section 3.5). A device with two
/// installs on the line has two. The device that runs the plan lists itself
/// too, seen now: 0.6 never deletes what this device synced last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceBase {
    pub device_id: String,
    /// Id of the snapshot. One that is no longer on the line is ignored.
    pub snapshot: String,
    /// When the device was last seen.
    pub seen_at: DateTime<Utc>,
}

/// Why a snapshot stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reason {
    /// The newest version, or one side of a fork.
    Head,
    /// The user keeps it: it has a pin.
    Kept,
    /// Locked in 0.6.
    Locked,
    /// A backup the user made by hand.
    Manual,
    /// The newest snapshot of its platform.
    PlatformLatest,
    /// What a device synced last.
    DeviceBase,
    /// One of the newest automatic snapshots.
    Recent,
    /// The newest automatic snapshot of its day.
    Daily,
}

/// What [`plan`] decided. Every snapshot of the view is in exactly one of
/// the two.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Ids of the snapshots to delete, oldest first.
    pub delete: Vec<String>,
    /// Every other snapshot with all the reasons it stays, in the order of
    /// [`Reason`].
    pub keep: BTreeMap<String, Vec<Reason>>,
}

/// Why the user wants this snapshot kept; empty for an automatic snapshot.
/// A trigger this build doesn't know is not "manual", so such a snapshot is
/// an automatic one.
fn asked_for(s: &IndexEntry) -> Vec<Reason> {
    let mut why = Vec::new();
    if s.pin.is_some() {
        why.push(Reason::Kept);
    }
    // v2 has pins instead of these two, but a 0.6 device still writes them,
    // and until the migration has written a pin they are all that says
    // "keep this".
    if s.extra.get(LOCKED).and_then(|v| v.as_bool()) == Some(true) {
        why.push(Reason::Locked);
    }
    if s.trigger == MANUAL {
        why.push(Reason::Manual);
    }
    why
}

/// Decide what to delete from the line in `view`. `bases` are the snapshots
/// the devices synced last (see [`DeviceBase`]); `now` is the caller's clock.
pub fn plan(view: &LineView, bases: &[DeviceBase], policy: &Policy, now: DateTime<Utc>) -> Plan {
    // Oldest first by the snapshots' own times rather than by the order of
    // the view: what "newest" means below, and the order of `delete`, must
    // not depend on how the caller put the list together.
    let mut all: Vec<&IndexEntry> = view.snapshots.iter().collect();
    all.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));

    let mut why: BTreeMap<String, BTreeSet<Reason>> = BTreeMap::new();
    let mut mark = |s: &IndexEntry, reason: Reason| {
        why.entry(s.id.clone()).or_default().insert(reason);
    };

    for s in view.head_entries() {
        mark(s, Reason::Head);
    }

    // The newest of each platform is found like the head of the line, by
    // vectors: a device whose clock runs ahead can't make an old snapshot
    // the newest. That is one per platform normally, and each side when two
    // devices of one platform have forked. Snapshots on the side are never
    // the newest of anything. Snapshots without a platform (0.6 wrote none)
    // are one group.
    let mut platforms: BTreeMap<&str, Vec<&IndexEntry>> = BTreeMap::new();
    for s in all.iter().filter(|s| !s.is_side()) {
        platforms.entry(s.platform.as_str()).or_default().push(s);
    }
    for group in platforms.values() {
        for i in vv::heads(group, |s| &s.vv) {
            mark(group[i], Reason::PlatformLatest);
        }
    }

    let based: BTreeSet<&str> = bases.iter().filter(|b| (now - b.seen_at).num_days() < policy.device_days).map(|b| b.snapshot.as_str()).collect();
    for s in all.iter().filter(|s| based.contains(s.id.as_str())) {
        mark(s, Reason::DeviceBase);
    }

    // What the user asked to keep is never an automatic snapshot. All the
    // rest are, the ones kept above and the ones on the side included: this
    // is the list 0.6 counts its 20 and its days in.
    let mut autos: Vec<&IndexEntry> = Vec::new();
    for s in all.iter().rev() {
        let asked = asked_for(s);
        if asked.is_empty() {
            autos.push(s);
        }
        for reason in asked {
            mark(s, reason);
        }
    }
    for s in autos.iter().take(policy.keep_recent) {
        mark(s, Reason::Recent);
    }
    // Days are UTC days. 0.6 used the time zone of the machine that pruned;
    // two devices in different zones would then keep different snapshots of
    // the same line, and each would delete what the other one kept.
    let mut days = BTreeSet::new();
    for s in &autos {
        if (now - s.created_at).num_days() < policy.keep_daily_days && days.insert(s.created_at.date_naive()) {
            mark(s, Reason::Daily);
        }
    }

    let delete = all.iter().filter(|s| !why.contains_key(&s.id)).map(|s| s.id.clone()).collect();
    let keep = why.into_iter().map(|(id, reasons)| (id, reasons.into_iter().collect())).collect();
    Plan { delete, keep }
}
