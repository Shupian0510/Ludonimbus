//! What a sync should do (docs/plan-v2.md, section 5.2): one pure function
//! from "what this device has" and "what the save line looks like" to an
//! action. No clocks: only version vectors and content hashes.
//!
//! [`decide`] takes a [`Local`] (the content hash of the saves on this
//! device, and its [`Base`]: the snapshot it synced last) and a
//! [`super::line::LineView`]. It reads nothing else and does nothing:
//! transferring, asking the user and remembering the new base are up to the
//! caller.
//!
//! The words of the table:
//! - The saves here *changed* when their hash differs from the base's.
//! - *The head* is the one newest version of the line. Several heads with
//!   the same content hash are one version under several names and count as
//!   one head; actions name the last of them in the view (which one makes no
//!   difference).
//! - The head *is the base* when it has the base's id. It *contains* the
//!   base when its vector has every change the base's vector has.
//!
//! | saves here | the line | action |
//! |---|---|---|
//! | unchanged | the head is the base | nothing |
//! | unchanged | the head contains the base | pull it |
//! | unchanged | the head lacks changes the base has | conflict: the head, or the saves here |
//! | changed | the head is the base | push, built on the base |
//! | changed | a head that is not the base | conflict: the head, or the saves here |
//! | same as the head | whatever the vectors say | no transfer, no question: adopt the head as the base |
//! | any | several heads with different contents | conflict: the heads, and the saves here unless a candidate already stands for them |
//! | none | a head | pull it (heads with different contents: conflict among them) |
//! | any | empty | push (first upload); nothing without saves |
//!
//! **Same content, no transfer.** Saves here that are byte-identical to the
//! head never cause a transfer or a question, whatever the vectors say: two
//! devices that made the same change, or uploaded the same saves while
//! offline, have nothing to settle. [`Action::Adopt`] tells the caller to
//! remember that head as the base.
//!
//! **The head lacks changes the base has.** What this device synced last is
//! not part of the newest version: its upload is still waiting on this
//! device, or was removed from the cloud while it was one side of a fork, or
//! is kept on the side. The saves here then hold progress no head has.
//! Pulling would overwrite it, and pushing would put it in front of what the
//! other devices made without anyone choosing. So the user is asked, with
//! the saves here as a candidate, although they did not change.
//!
//! **Several heads with different contents.** Somebody has to choose, on
//! whichever device syncs next. The saves here are one more candidate unless
//! a candidate already stands for them: they equal a head, or they are
//! unchanged from a base some head contains.
//!
//! **The base's vector.** The base need not be in the line any more
//! (retention removed it, or this device's state is older than the cloud),
//! so it is never looked up to find out what to do: vectors are compared.
//! While it is still there, the vector the line gives it is used. That is
//! the same vector for a snapshot of the new layout, and the only one for a
//! snapshot of the 0.6 layout, which has none in the cloud: a device
//! upgraded from 0.6 passes [`Base::vv`] empty. Once such a base is gone
//! its vector is unknown and counts as contained in every head, "the cloud
//! moved on from what I had", which is how 0.6 decides.
//!
//! **No base.** A device that never synced this line, or lost its state.
//! Saves that equal a version in the line's history are taken to be that
//! version, unchanged: every such version is contained in a head, so the
//! device pulls instead of asking (0.6 does the same for a known older
//! version). Saves that equal nothing are changed. Snapshots kept on the
//! side do not count as history here: that is where the saves of a conflict
//! nobody answered yet are kept, and finding them there must not answer it.
//!
//! **After a conflict.** [`settle`] gives the push that ends it: built on
//! every head, so the new snapshot contains them all and is the only head
//! afterwards (section 5.2, last paragraph). [`next_vv`] turns the bases of
//! a push into the new snapshot's vector.
//!
//! Manifests the view could not read play no part here; reporting them is
//! the caller's business.

use super::line::LineView;
use super::v2::IndexEntry;
use crate::vv::{self, Vv};
use serde::{Deserialize, Serialize};

/// The snapshot this device synced last on a line: uploaded, pulled or
/// adopted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Base {
    pub id: String,
    /// Empty when the device doesn't know it (state written by 0.6): the
    /// line is then asked, by `id`.
    pub vv: Vv,
    /// The content hash of the saves here right after that sync.
    pub content_hash: String,
}

/// What this device has of a line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Local {
    /// The content hash of the saves here; `None` = there are none.
    pub content_hash: Option<String>,
    /// `None` = this device never synced the line, or forgot.
    pub base: Option<Base>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// The saves here already are this head: remember it as the base,
    /// transfer nothing.
    Adopt {
        id: String,
    },
    /// Replace the saves here with this head.
    Pull {
        id: String,
    },
    /// Upload the saves here as a new snapshot.
    Push {
        /// The vectors the new snapshot's vector is built from
        /// ([`next_vv`]): the base normally, every head when that closes a
        /// fork, none on a first upload.
        bases: Vec<Vv>,
        /// The snapshot the saves here were made from, for display. It may
        /// no longer be in the line.
        parent: Option<String>,
    },
    /// The user has to choose.
    Conflict {
        /// Heads of the line, oldest first.
        candidates: Vec<String>,
        /// The saves here are one more choice. They are not when there are
        /// none, or when a candidate already stands for them.
        local_is_candidate: bool,
    },
}

/// What to do about one save line on this device: the table at the top of
/// this file.
pub fn decide(local: &Local, line: &LineView) -> Action {
    let heads: Vec<&IndexEntry> = line.head_entries().collect();
    let Some(&head) = heads.last() else {
        return match local.content_hash {
            Some(_) => settle(local, line),
            None => Action::Nothing,
        };
    };
    let candidates = || heads.iter().map(|h| h.id.clone()).collect();
    let forked = heads.iter().any(|h| h.content_hash != head.content_hash);

    let Some(mine) = local.content_hash.as_deref() else {
        // Nothing here that a pull could overwrite.
        if forked {
            return Action::Conflict { candidates: candidates(), local_is_candidate: false };
        }
        return Action::Pull { id: head.id.clone() };
    };
    let base_is_head = local.base.as_ref().is_some_and(|b| heads.iter().any(|h| h.id == b.id));
    if !forked && head.content_hash == mine {
        if base_is_head {
            return Action::Nothing;
        }
        return Action::Adopt { id: head.id.clone() };
    }

    // `contained`: some head has every change the saves here were made from.
    let (changed, contained) = match &local.base {
        Some(base) => {
            let vv = base_vv(base, line);
            (base.content_hash != mine, heads.iter().any(|h| h.vv.contains(&vv)))
        }
        None => {
            // No memory of a sync: saves that equal a version of the line
            // are that version, and a head contains every version that is
            // not a head itself.
            let known = line.snapshots.iter().any(|s| !s.is_side() && s.content_hash == mine);
            (!known, known)
        }
    };

    if forked {
        let stands_for_local = heads.iter().any(|h| h.content_hash == mine) || (!changed && contained);
        return Action::Conflict { candidates: candidates(), local_is_candidate: !stands_for_local };
    }
    match (changed, base_is_head) {
        // Reached when the head's hash is not the one this device computes
        // for the same saves; the base says they are the same.
        (false, true) => Action::Nothing,
        (true, true) => settle(local, line),
        (false, false) if contained => Action::Pull { id: head.id.clone() },
        _ => Action::Conflict { candidates: vec![head.id.clone()], local_is_candidate: true },
    }
}

/// The push that makes the saves here the newest version of the line, built
/// on every head. [`decide`] returns it when nothing stands against it; the
/// caller asks for it to end a conflict, once the user has chosen:
///
/// - the saves here: `settle`;
/// - one of several heads: pull it, then `settle` with the saves as they are
///   then, so the fork closes;
/// - the single head: pull it, nothing more. An upload of the saves that
///   lost which is still waiting on this device has to go to the side, or
///   it arrives as a second head later and the question comes back.
///
/// The base is built on too when no head contains it, so that an upload of
/// this device that reaches the cloud later doesn't open the question again.
/// `Nothing` when there are no saves here.
pub fn settle(local: &Local, line: &LineView) -> Action {
    if local.content_hash.is_none() {
        return Action::Nothing;
    }
    let mut bases: Vec<Vv> = line.head_entries().map(|h| h.vv.clone()).collect();
    if let Some(base) = &local.base {
        let vv = base_vv(base, line);
        if !vv.is_empty() && !bases.iter().any(|b| b.contains(&vv)) {
            bases.push(vv);
        }
    }
    let parent = match &local.base {
        Some(base) => Some(base.id.clone()),
        None => line.head_entries().last().map(|h| h.id.clone()),
    };
    Action::Push { bases, parent }
}

/// The vector of the snapshot a push writes on `device`. `counter` is the
/// highest count the device remembers using on this line (0 when it
/// remembers none); the line is searched as well, so a count is not handed
/// out twice after the device lost its state.
pub fn next_vv(bases: &[Vv], line: &LineView, device: &str, counter: u64) -> Vv {
    let floor = vv::max_count(line.snapshots.iter().map(|s| &s.vv), device).max(counter);
    Vv::next(bases, device, floor)
}

/// The base's vector, as far as it can be known.
fn base_vv(base: &Base, line: &LineView) -> Vv {
    // Still in the line: what the line says. A snapshot of the 0.6 layout
    // has no vector in the cloud, only the one the view gives it, and a
    // device upgraded from 0.6 remembers none.
    if let Some(found) = line.get(&base.id).filter(|s| !s.vv.is_empty()) {
        return found.vv.clone();
    }
    base.vv.clone()
}

/// What to show for a game, for the cases that follow from [`decide`]. The
/// names are those of `sync::Status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// No saves here or in the cloud.
    Empty,
    /// Saves here, nothing in the cloud yet.
    NotUploaded,
    Synced,
    LocalChanged,
    CloudNewer,
    /// No saves here, the cloud has some (a reinstalled game).
    CanRestore,
    Conflict,
}

impl Status {
    /// `action` is what [`decide`] returned for `local` and `line`.
    pub fn of(action: &Action, local: &Local, line: &LineView) -> Status {
        let saves_here = local.content_hash.is_some();
        match action {
            Action::Nothing if saves_here => Status::Synced,
            Action::Nothing => Status::Empty,
            Action::Adopt { .. } => Status::Synced,
            Action::Pull { .. } if saves_here => Status::CloudNewer,
            Action::Pull { .. } => Status::CanRestore,
            Action::Push { .. } if line.heads.is_empty() => Status::NotUploaded,
            Action::Push { .. } => Status::LocalChanged,
            Action::Conflict { .. } => Status::Conflict,
        }
    }
}
