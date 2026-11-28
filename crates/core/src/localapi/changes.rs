//! What changed, for a plugin that waits for it instead of asking every few
//! seconds (API 6): `GET /v1/changes?after={seq}&wait={secs}` answers as
//! soon as something a panel shows may have changed, or after `wait`
//! seconds at most ([`MAX_WAIT`]), with the newest number. The plugin then
//! reads what it shows again and waits once more with that number.
//!
//! ```text
//! GET /v1/changes?after={seq}&wait={secs}   { seq, paused, apiVersion }
//! ```
//!
//! What counts as a change is looked at, not announced: the launch wrapper,
//! the app and the agent are separate programs that write the same files.
//! While somebody waits, every [`LOOK_EVERY`] the files are compared with
//! the last look: the configuration (the library), the state (statuses,
//! conflicts, errors), the pause, the launch wrappers' markers (before
//! launch, playing, uploading), the syncs owed and the upload queue; and
//! which games this program is syncing. Work this program starts for the
//! API is also announced ([`notify`]), so a wait ends at once.
//!
//! Bounded: at most [`MAX_WAITERS`] requests wait at once (more get the
//! current number at once), each at most [`MAX_WAIT`]; nothing is looked at
//! while nobody waits. Numbers start from the launch time, as in
//! [`super::events`], so a plugin holding a number of an earlier run gets an
//! answer at once; `after=0` too.

use super::{lock, Host, API_VERSION};
use crate::config::GameEntry;
use crate::{config, paths};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// The longest a request waits.
pub const MAX_WAIT: Duration = Duration::from_secs(25);
/// Requests that may wait at once.
pub const MAX_WAITERS: usize = 4;
/// How often the files are looked at while somebody waits.
pub const LOOK_EVERY: Duration = Duration::from_millis(700);

struct Feed {
    seq: u64,
    /// What the last look saw.
    stamp: Option<u64>,
    looked: Option<Instant>,
    waiting: usize,
    /// The games of the library, read again when the configuration changes.
    games: Vec<GameEntry>,
    config_stamp: Option<(SystemTime, u64)>,
}

static FEED: Mutex<Feed> = Mutex::new(Feed { seq: 0, stamp: None, looked: None, waiting: 0, games: Vec::new(), config_stamp: None });
static WOKE: Condvar = Condvar::new();

fn seed(f: &mut Feed) {
    if f.seq == 0 {
        f.seq = u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0).max(1);
    }
}

/// Something this program did changed what the panels show: the waits end.
pub fn notify() {
    let mut f = lock(&FEED);
    seed(&mut f);
    f.seq += 1;
    // The next look starts from what is there after this.
    f.stamp = None;
    drop(f);
    WOKE.notify_all();
}

fn file_stamp(h: &mut DefaultHasher, p: &Path) {
    match std::fs::metadata(p) {
        Ok(m) => (m.modified().ok(), m.len()).hash(h),
        Err(_) => 0u8.hash(h),
    }
}

/// The names, times and sizes of the entries of a folder (not below).
fn dir_stamp(h: &mut DefaultHasher, dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        0u8.hash(h);
        return;
    };
    let mut entries: Vec<(String, Option<SystemTime>, u64)> = rd
        .flatten()
        .take(2000)
        .map(|e| {
            let m = e.metadata().ok();
            (e.file_name().to_string_lossy().into_owned(), m.as_ref().and_then(|m| m.modified().ok()), m.map(|m| m.len()).unwrap_or(0))
        })
        .collect();
    entries.sort();
    entries.hash(h);
}

/// One look: what the panels show depends on.
fn look(f: &mut Feed, host: &dyn Host) -> u64 {
    let data = paths::data_dir();
    let cfg_file = paths::config_file();
    let cfg_now = std::fs::metadata(&cfg_file).ok().and_then(|m| Some((m.modified().ok()?, m.len())));
    if cfg_now != f.config_stamp {
        f.config_stamp = cfg_now;
        f.games = config::load().map(|c| c.games).unwrap_or_default();
    }
    let mut h = DefaultHasher::new();
    cfg_now.hash(&mut h);
    file_stamp(&mut h, &paths::state_file());
    file_stamp(&mut h, &crate::pause::file());
    for dir in ["running", "owed", "pending"] {
        dir_stamp(&mut h, &data.join(dir));
    }
    for g in &f.games {
        if host.is_busy(&g.key) {
            g.key.hash(&mut h);
        }
    }
    h.finish()
}

/// Look again when the last look is old enough; a new number when it saw
/// something else than the one before.
fn look_if_due(f: &mut Feed, host: &dyn Host) {
    if f.looked.is_some_and(|t| t.elapsed() < LOOK_EVERY) {
        return;
    }
    let now = look(f, host);
    f.looked = Some(Instant::now());
    if f.stamp.is_some_and(|s| s != now) {
        f.seq += 1;
    }
    f.stamp = Some(now);
}

/// `GET /v1/changes`: wait until the number isn't `after` any more, at most
/// `wait`.
pub(crate) fn wait(host: &dyn Host, after: u64, wait: Duration) -> Value {
    let deadline = Instant::now() + wait.min(MAX_WAIT);
    let mut f = lock(&FEED);
    seed(&mut f);
    if f.waiting < MAX_WAITERS {
        f.waiting += 1;
        loop {
            look_if_due(&mut f, host);
            let left = deadline.saturating_duration_since(Instant::now());
            if f.seq != after || left.is_zero() {
                break;
            }
            f = WOKE.wait_timeout(f, left.min(LOOK_EVERY)).unwrap_or_else(|e| e.into_inner()).0;
        }
        f.waiting -= 1;
    } else {
        // Enough are waiting: this one gets the number as it is.
        look_if_due(&mut f, host);
    }
    let seq = f.seq;
    drop(f);
    json!({ "seq": seq, "paused": crate::pause::is_paused(), "apiVersion": API_VERSION })
}
