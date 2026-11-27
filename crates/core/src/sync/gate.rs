//! Not waiting: what a sync is doing, for whoever waits for it ([`Phase`]),
//! and a way to stop waiting ([`Gate`]). A sync given up starts no new
//! request and never replaces the save files afterwards; the store a pull
//! downloads from goes by the gate too ([`Watched`]).

use crate::cloud::store::{Entry, Store};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// What a sync is doing, for whoever waits for it ([`Gate::phase`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Phase {
    /// Waiting for another sync of the game, reading its save line,
    /// deciding.
    #[default]
    Checking,
    /// Getting a newer version: `done` of `total` bytes of its files, as
    /// each content pack comes in.
    Downloading { done: u64, total: u64 },
    /// Replacing the save files with it, until the next step. Quick, on
    /// this device, and not to be given up.
    Applying,
    /// Uploading versions of this device, and what goes with them.
    Uploading,
    /// The sync has ended, or is about to.
    Done,
}

/// Inside a sync: whoever waited for it stopped. It ends as
/// [`Outcome::Offline`](super::Outcome::Offline).
#[derive(Debug)]
pub(super) struct GaveUp;

impl std::fmt::Display for GaveUp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", crate::tr!("没有等云盘，存档留到下次同步", "Didn't wait for the cloud; the saves are synced next time"))
    }
}

impl std::error::Error for GaveUp {}

pub(super) fn is_gave_up(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<GaveUp>().is_some())
}

#[derive(Default)]
struct GateState {
    phase: Phase,
    given_up: bool,
}

type Watcher = Arc<dyn Fn(Phase) + Send + Sync>;

#[derive(Default)]
struct GateInner {
    state: Mutex<GateState>,
    watcher: Mutex<Option<Watcher>>,
}

/// Shared by a sync and whoever waits for it: what the sync is doing, and
/// a way to stop waiting ([`Gate::give_up`]).
///
/// A sync that was given up doesn't replace the save files any more: the
/// check and the start of replacing them are one step
/// ([`Gate::start_applying`]). It also starts no further request and ends
/// as [`Outcome::Offline`](super::Outcome::Offline). A version it put in the queue stays there, and
/// an upload already under way may finish: packs go up first and the
/// manifest last, so an upload cut off is never half a version.
///
/// `Gate::default()` is one nobody gives up.
#[derive(Clone, Default)]
pub struct Gate(Arc<GateInner>);

impl Gate {
    pub fn new() -> Gate {
        Gate::default()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn tell(&self, phase: Phase) {
        let watcher = self.0.watcher.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(w) = watcher {
            w(phase);
        }
    }

    pub fn phase(&self) -> Phase {
        self.state().phase
    }

    pub fn given_up(&self) -> bool {
        self.state().given_up
    }

    /// Stop waiting. From here on the sync changes no save files. `false`
    /// when it is replacing them right now: then wait for it to end (that
    /// is quick, and on this device).
    pub fn give_up(&self) -> bool {
        let mut s = self.state();
        if s.phase == Phase::Applying {
            return false;
        }
        s.given_up = true;
        true
    }

    /// Be told of every change of phase, on the thread of the sync.
    pub fn watch(&self, f: impl Fn(Phase) + Send + Sync + 'static) {
        *self.0.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(f));
    }

    /// What the work the gate is about does now. The sync calls this; so may
    /// something that stands in for a sync.
    pub fn set_phase(&self, phase: Phase) {
        self.state().phase = phase;
        self.tell(phase);
    }

    /// Move on to replacing the save files, unless the waiting was given
    /// up: `false` then, and nothing may be replaced. One step with
    /// [`Gate::give_up`], so the two can't cross.
    pub fn start_applying(&self) -> bool {
        {
            let mut s = self.state();
            if s.given_up {
                return false;
            }
            s.phase = Phase::Applying;
        }
        self.tell(Phase::Applying);
        true
    }

    fn add_done(&self, bytes: u64) {
        let phase = {
            let mut s = self.state();
            match &mut s.phase {
                Phase::Downloading { done, total } => *done = (*done + bytes).min(*total),
                _ => return,
            }
            s.phase
        };
        self.tell(phase);
    }

    /// An error once the waiting was given up: the next step isn't taken.
    pub(super) fn check(&self) -> Result<()> {
        if self.given_up() {
            return Err(GaveUp.into());
        }
        Ok(())
    }
}

/// The store as [`Session::pull`](super::session::Session::pull) downloads from it: no new request once
/// the waiting was given up, and each content pack counted when it is in.
pub(super) struct Watched<'a> {
    pub(super) inner: &'a dyn Store,
    pub(super) gate: &'a Gate,
    /// Bytes of the wanted files in each pack, by pack id.
    pub(super) packs: BTreeMap<String, u64>,
}

impl Store for Watched<'_> {
    fn kind(&self) -> &'static str {
        self.inner.kind()
    }
    fn label(&self) -> String {
        self.inner.label()
    }
    fn quota(&self) -> Result<(Option<u64>, Option<u64>)> {
        self.inner.quota()
    }
    fn check(&self) -> Result<()> {
        self.gate.check()?;
        self.inner.check()
    }
    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        self.gate.check()?;
        self.inner.list(dir)
    }
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.gate.check()?;
        self.inner.read(path)
    }
    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.gate.check()?;
        self.inner.download(path, dest)?;
        let name = path.rsplit('/').next().unwrap_or(path);
        if let Some(bytes) = self.packs.get(name.strip_suffix(".zip").unwrap_or(name)) {
            self.gate.add_done(*bytes);
        }
        Ok(())
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.gate.check()?;
        self.inner.write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.gate.check()?;
        self.inner.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.gate.check()?;
        self.inner.remove(path)
    }
    fn cache_scope(&self) -> Option<String> {
        self.inner.cache_scope()
    }
}
