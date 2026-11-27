//! New achievement unlocks waiting for the Steam plugin, which shows them
//! with Steam's own popup (`plugin-millennium/frontend/toast.ts`) and says
//! whether it could. The desktop app's achievement watcher adds them
//! ([`push`]) and shows its own popup when the plugin didn't
//! ([`plugin_alive`], [`shown`]); the headless agent watches no achievements,
//! so its queue stays empty. See docs/plan-achievements.md.
//!
//! ```text
//! GET  /v1/achievements/events?after={seq}              new unlocks to show
//! POST /v1/achievements/events/{seq}/ack?shown=steam|failed&reason=
//! ```

use crate::achievements::{self as ach, Grade, NewUnlock};
use crate::config::GameEntry;
use crate::logger;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Unlocks kept for a plugin that polls late.
const KEEP_EVENTS: usize = 50;
/// A plugin that asked for events this recently is there to show the next one.
const PLUGIN_ALIVE: Duration = Duration::from_secs(10);
/// Longest reason a plugin gives for a popup it couldn't show (it is logged).
const MAX_REASON: usize = 300;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub seq: u64,
    pub key: String,
    pub game_name: String,
    /// The Steam shortcut's app id on this PC (Steam needs it for its popup).
    pub appid: Option<u32>,
    pub id: String,
    pub name: String,
    pub description: String,
    /// `https://…` or a `data:` URL.
    pub icon: Option<String>,
    pub unlocked_at: DateTime<Utc>,
    pub hardcore: bool,
    pub points: Option<u32>,
    pub grade: Option<Grade>,
    pub test: bool,
    /// What the plugin answered: "steam" (shown in Steam) or "failed".
    #[serde(skip)]
    pub shown: Option<String>,
}

struct Queue {
    seq: u64,
    events: VecDeque<Event>,
    plugin_seen: Option<Instant>,
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue { seq: 0, events: VecDeque::new(), plugin_seen: None });

/// The queue stays whole even when a thread panicked while holding it:
/// every change to it is a single push, pop or assignment.
fn queue() -> MutexGuard<'static, Queue> {
    let mut q = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    seed(&mut q);
    q
}

/// Numbers start from the launch time, so they keep growing across restarts
/// of the app: a plugin still holding the last run's number gets this run's
/// unlocks, and a plugin that synced at 0 isn't stuck on `after=0`, which
/// [`list`] reads as "just (re)loaded, start from now".
fn seed(q: &mut Queue) {
    if q.seq == 0 {
        q.seq = u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0).max(1);
    }
}

/// The achievement's icon, else the game's.
fn icon_for(key: &str, a: &ach::Achievement) -> Option<String> {
    let own = match a.icon.as_deref() {
        Some(i) if ach::is_web(i) => return Some(i.to_string()),
        Some(p) => ach::icon_data_url(std::path::Path::new(p)).ok(),
        None => None,
    };
    own.or_else(|| ach::game_icon_path(key).and_then(|p| ach::icon_data_url(&p).ok()))
}

/// Queue an unlock for the plugin and return it with its number.
pub fn push(g: &GameEntry, u: &NewUnlock, test: bool) -> Event {
    // Files are read before the queue is held.
    let icon = icon_for(&g.key, &u.achievement);
    let mut q = queue();
    q.seq += 1;
    let ev = Event {
        seq: q.seq,
        key: g.key.clone(),
        game_name: g.name.clone(),
        appid: g.steam.as_ref().map(|l| l.appid).filter(|a| *a != 0),
        id: u.achievement.id.clone(),
        name: u.achievement.name.clone(),
        description: u.achievement.description.clone(),
        icon,
        unlocked_at: u.unlocked_at,
        hardcore: u.hardcore,
        points: u.achievement.points,
        grade: u.achievement.grade,
        test,
        shown: None,
    };
    q.events.push_back(ev.clone());
    while q.events.len() > KEEP_EVENTS {
        q.events.pop_front();
    }
    ev
}

/// Whether a plugin asked for events within the last few seconds.
pub fn plugin_alive() -> bool {
    queue().plugin_seen.is_some_and(|t| t.elapsed() < PLUGIN_ALIVE)
}

/// What the plugin answered for an unlock: "steam", "failed", or nothing yet.
pub fn shown(seq: u64) -> Option<String> {
    queue().events.iter().find(|e| e.seq == seq).and_then(|e| e.shown.clone())
}

/// `GET /v1/achievements/events?after={seq}`: the unlocks after `seq`
/// (none for 0) and the newest number. Marks the plugin as alive.
pub fn list(after: u64) -> Value {
    let lang = crate::i18n::current().code();
    let mut q = queue();
    q.plugin_seen = Some(Instant::now());
    // A plugin that just (re)loaded starts from now: old unlocks aren't replayed.
    let events: Vec<&Event> = if after == 0 { Vec::new() } else { q.events.iter().filter(|e| e.seq > after).collect() };
    json!({ "seq": q.seq, "events": events, "lang": lang })
}

/// `POST /v1/achievements/events/{seq}/ack`: `shown` is "steam" or "failed",
/// `reason` (already decoded) why.
pub fn ack(seq: u64, shown: &str, reason: Option<&str>) {
    let found = queue().events.iter_mut().find(|e| e.seq == seq).map(|e| {
        e.shown = Some(shown.to_string());
        (e.key.clone(), e.id.clone())
    });
    if let Some((key, id)) = found {
        let how = if shown == "steam" { "shown in Steam" } else { "not shown in Steam" };
        let reason: Option<String> = reason.filter(|r| !r.is_empty()).map(|r| r.chars().take(MAX_REASON).collect());
        logger::log("achievements", format!("{key}: popup for {id} {how}{}", reason.map(|r| format!(" ({r})")).unwrap_or_default()));
    }
}
