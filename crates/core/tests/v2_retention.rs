//! What retention keeps of a save line (docs/plan-v2.md, section 5.4): each
//! rule on its own, all of them together, and that nothing 0.6 keeps is
//! deleted. The lines are written into a store and read back with
//! `line::load` (`line::load_all` for the one in two folders), so the heads
//! are the ones a real sync would see.

use chrono::{DateTime, Duration, TimeZone, Utc};
use ludonimbus_core::cloud::line::{self, LineCache, LineView};
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::retention::{plan, DeviceBase, Plan, Policy, Reason};
use ludonimbus_core::cloud::v2::{self, FileRef, IndexEntry, Manifest, Pin};
use ludonimbus_core::config::Retention;
use ludonimbus_core::vv::Vv;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const DIR: &str = "games/stardew-7k3f/saves/main";
/// Where 0.6 wrote the snapshots of the game.
const OLD: &str = "games/stardew/snapshots";

/// Only the rules that keep a snapshot whatever its age.
const NO_AUTOMATIC: Policy = Policy { keep_recent: 0, keep_daily_days: 0, device_days: 90 };

fn id(n: u64) -> String {
    format!("01JTEST{n:019}")
}

/// The moment every plan here is made: 2026-11-26 22:00 UTC.
fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 26, 22, 0, 0).unwrap()
}

/// `days` days and `hours` hours before [`now`].
fn ago(days: i64, hours: i64) -> DateTime<Utc> {
    now() - Duration::days(days) - Duration::hours(hours)
}

fn on_the_side(trigger: &str) -> bool {
    matches!(trigger, "pre-restore" | "conflict-keep")
}

/// A save line in a store, written one snapshot after the other.
struct Line {
    store: Memory,
    /// Number of the next snapshot.
    next: u64,
    /// The vector of the newest snapshot of the history.
    last: Vv,
    /// The folder the 0.6 snapshots are in.
    legacy_dir: &'static str,
}

impl Line {
    fn new() -> Self {
        Line { store: Memory::new(), next: 1, last: Vv::new(), legacy_dir: DIR }
    }

    /// A line whose 0.6 snapshots are still where 0.6 wrote them: the
    /// migration moves nothing.
    fn migrated() -> Self {
        Line { legacy_dir: OLD, ..Line::new() }
    }

    /// A snapshot with the vector `vv`, wherever that puts it in the
    /// history. Returns its id.
    fn put(&mut self, device: &str, platform: &str, at: DateTime<Utc>, trigger: &str, vv: Vv) -> String {
        let n = self.next;
        self.next += 1;
        let m = Manifest {
            v: 2,
            id: id(n),
            game: "stardew-7k3f".into(),
            line: "main".into(),
            vv,
            device_id: device.into(),
            device_name: device.to_uppercase(),
            platform: platform.into(),
            created_at: at,
            trigger: trigger.into(),
            side: on_the_side(trigger),
            content_hash: format!("c{n}"),
            total_size: 10,
            files: vec![FileRef { path: "saves/a".into(), size: 10, hash: format!("h{n}"), ..Default::default() }],
            app_version: "1.0.0".into(),
            ..Default::default()
        };
        line::write_manifest(&self.store, DIR, &m).unwrap();
        m.id
    }

    /// A snapshot made on `device` from the newest one.
    fn add(&mut self, device: &str, platform: &str, at: DateTime<Utc>, trigger: &str) -> String {
        self.last = Vv::next([&self.last], device, 0);
        self.put(device, platform, at, trigger, self.last.clone())
    }

    /// A snapshot kept beside the history: the saves from before a restore,
    /// the side that lost a conflict.
    fn side(&mut self, device: &str, platform: &str, at: DateTime<Utc>, trigger: &str) -> String {
        assert!(on_the_side(trigger));
        self.put(device, platform, at, trigger, self.last.clone())
    }

    /// A snapshot as 0.6 writes it: no `v`, no vector, no platform, and
    /// `comment` and `locked` in the snapshot's own file. `last` is only
    /// right while no snapshot of the new kind has been written.
    fn legacy(&mut self, at: DateTime<Utc>, trigger: &str, locked: bool) -> String {
        let n = self.next;
        self.next += 1;
        let meta = json!({
            "id": id(n),
            "game": "stardew",
            "parent": null,
            "deviceId": "d-a",
            "deviceName": "Desktop",
            "createdAt": at,
            "trigger": trigger,
            "roots": { "appdata": "C:\\Users\\me\\AppData\\Roaming" },
            "contentHash": format!("c{n}"),
            "files": [
                { "path": "appdata/StardewValley/Saves/Farm_1/Farm_1", "size": 5, "hash": format!("h{n}"), "mtime": 1732100000 }
            ],
            "totalSize": 5,
            "appVersion": "0.6.2",
            "comment": "",
            "locked": locked
        });
        self.store.put(&v2::manifest_path(self.legacy_dir, &id(n)), &serde_json::to_vec(&meta).unwrap());
        if !on_the_side(trigger) {
            // The vector `line::load` gives the newest of them.
            self.last.join(&line::legacy_vv(self.legacy_dir, at));
        }
        id(n)
    }

    fn pin(&self, id: &str, name: &str) {
        line::write_pin(&self.store, DIR, id, &Pin { name: name.into(), ..Default::default() }).unwrap();
    }

    fn view(&self) -> LineView {
        let mut cache = LineCache::default();
        if self.legacy_dir == DIR {
            line::load(&self.store, DIR, &mut cache).unwrap()
        } else {
            line::load_all(&self.store, &[DIR, self.legacy_dir], &mut cache).unwrap()
        }
    }
}

fn base(device: &str, snapshot: &str, seen_at: DateTime<Utc>) -> DeviceBase {
    DeviceBase { device_id: device.into(), snapshot: snapshot.into(), seen_at }
}

/// The plan for `view`, after checking what holds for every plan: each
/// snapshot is in exactly one of the two lists, and `delete` is oldest
/// first.
fn decide(view: &LineView, bases: &[DeviceBase], policy: &Policy) -> Plan {
    let p = plan(view, bases, policy, now());

    let all: BTreeSet<&str> = view.snapshots.iter().map(|s| s.id.as_str()).collect();
    let delete: BTreeSet<&str> = p.delete.iter().map(String::as_str).collect();
    let keep: BTreeSet<&str> = p.keep.keys().map(String::as_str).collect();
    assert_eq!(delete.len(), p.delete.len(), "a snapshot is listed twice");
    assert!(delete.is_disjoint(&keep), "a snapshot is both deleted and kept");
    assert_eq!(delete.union(&keep).copied().collect::<BTreeSet<_>>(), all, "the plan covers the line");

    for (id, reasons) in &p.keep {
        assert!(!reasons.is_empty(), "{id} is kept for no reason");
        assert!(reasons.windows(2).all(|w| w[0] < w[1]), "{id}: {reasons:?}");
    }
    let times: Vec<DateTime<Utc>> = p.delete.iter().map(|id| view.get(id).unwrap().created_at).collect();
    assert!(times.windows(2).all(|w| w[0] <= w[1]), "delete is oldest first");
    p
}

/// Why `id` stays; empty when it is deleted.
fn why<'a>(plan: &'a Plan, id: &str) -> &'a [Reason] {
    plan.keep.get(id).map(Vec::as_slice).unwrap_or(&[])
}

fn kept(plan: &Plan) -> BTreeSet<String> {
    plan.keep.keys().cloned().collect()
}

fn is_locked(s: &IndexEntry) -> bool {
    s.extra.get("locked").and_then(|v| v.as_bool()).unwrap_or(false)
}

/// `sync::prune` of 0.6 on the snapshots of a view: the ids it keeps. One
/// thing differs on purpose: a day is a UTC day, not a day of this
/// machine's time zone.
fn kept_by_0_6(view: &LineView, last_synced: Option<&str>, settings: &Retention) -> BTreeSet<String> {
    let exempt = |s: &IndexEntry| is_locked(s) || s.trigger == "manual";
    let mut keep = BTreeSet::new();
    let mainline_head = view.snapshots.iter().filter(|s| !on_the_side(&s.trigger)).max_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    keep.extend(mainline_head.map(|s| s.id.clone()));
    keep.extend(last_synced.map(str::to_string));
    let mut sorted: Vec<&IndexEntry> = view.snapshots.iter().collect();
    sorted.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    keep.extend(sorted.iter().filter(|s| exempt(s)).map(|s| s.id.clone()));
    let autos: Vec<&IndexEntry> = sorted.into_iter().filter(|s| !exempt(s)).collect();
    keep.extend(autos.iter().take(settings.keep_recent).map(|s| s.id.clone()));
    let mut days = BTreeSet::new();
    for s in &autos {
        if (now() - s.created_at).num_days() < settings.keep_daily_days && days.insert(s.created_at.date_naive()) {
            keep.insert(s.id.clone());
        }
    }
    keep
}

#[test]
fn the_policy_comes_from_the_settings() {
    assert_eq!(Policy::default(), Policy { keep_recent: 20, keep_daily_days: 30, device_days: 90 });
    let settings = Retention { keep_recent: 5, keep_daily_days: 7 };
    assert_eq!(Policy::from(&settings), Policy { keep_recent: 5, keep_daily_days: 7, device_days: 90 });
}

#[test]
fn an_empty_line_has_nothing_to_delete() {
    let view = Line::new().view();
    assert!(view.snapshots.is_empty());
    assert_eq!(decide(&view, &[], &Policy::default()), Plan::default());
    // A base that names a snapshot which isn't there changes nothing.
    assert_eq!(decide(&view, &[base("d-a", &id(1), now())], &Policy::default()), Plan::default());
}

#[test]
fn a_line_shorter_than_the_limits_loses_nothing() {
    let mut l = Line::new();
    // Fifteen snapshots over ten months, some of them on the side.
    for n in (0..15).rev() {
        if n % 5 == 2 {
            l.side("d-a", "steam", ago(n * 20, 0), "pre-restore");
        } else {
            l.add("d-a", "steam", ago(n * 20, 0), "auto-exit");
        }
    }
    let view = l.view();
    let p = decide(&view, &[], &Policy::default());
    assert!(p.delete.is_empty());
    assert_eq!(p.keep.len(), 15);
    // The oldest is 280 days old: only being one of the newest 20 keeps it.
    assert_eq!(why(&p, &view.snapshots[0].id), [Reason::Recent]);
}

#[test]
fn a_long_history_keeps_the_newest_20_and_one_per_day() {
    let mut l = Line::new();
    // One device, three sessions a day (08:00, 14:00 and 20:00 UTC) for 60 days.
    let mut sessions: Vec<[String; 3]> = Vec::new();
    for day in (0..60).rev() {
        sessions.push([14, 8, 2].map(|hours| l.add("d-a", "steam", ago(day, hours), "auto-exit")));
    }
    // sessions[n]: the day `n` days ago.
    sessions.reverse();
    let view = l.view();
    assert_eq!(view.snapshots.len(), 180);
    let head = view.head().unwrap().id.clone();
    assert_eq!(head, sessions[0][2]);

    let p = decide(&view, &[base("d-a", &head, now())], &Policy::default());
    // The newest 20 are six whole days and two sessions of the seventh.
    // Days 7 to 29 keep their last session.
    assert_eq!(p.keep.len(), 20 + 23);
    assert_eq!(p.delete.len(), 180 - 43);
    for (day, [morning, noon, evening]) in sessions.iter().enumerate() {
        match day {
            0 => {
                assert_eq!(why(&p, morning), [Reason::Recent]);
                assert_eq!(why(&p, noon), [Reason::Recent]);
                assert_eq!(why(&p, evening), [Reason::Head, Reason::PlatformLatest, Reason::DeviceBase, Reason::Recent, Reason::Daily]);
            }
            1..=5 => {
                assert_eq!(why(&p, morning), [Reason::Recent]);
                assert_eq!(why(&p, noon), [Reason::Recent]);
                assert_eq!(why(&p, evening), [Reason::Recent, Reason::Daily]);
            }
            6 => {
                assert!(why(&p, morning).is_empty());
                assert_eq!(why(&p, noon), [Reason::Recent]);
                assert_eq!(why(&p, evening), [Reason::Recent, Reason::Daily]);
            }
            // The last session of day 29 is 29 days and 2 hours old; the one
            // of day 30 is over the 30 days.
            7..=29 => {
                assert!(why(&p, morning).is_empty() && why(&p, noon).is_empty());
                assert_eq!(why(&p, evening), [Reason::Daily], "day {day}");
            }
            _ => assert!([morning, noon, evening].iter().all(|id| why(&p, id).is_empty()), "day {day}"),
        }
    }
    // Exactly what 0.6 keeps of such a line.
    assert_eq!(kept(&p), kept_by_0_6(&view, Some(&head), &Retention::default()));

    // The numbers are the policy's.
    let p = decide(&view, &[], &Policy { keep_recent: 3, keep_daily_days: 5, device_days: 90 });
    assert_eq!(p.keep.len(), 3 + 4);
    let p = decide(&view, &[], &NO_AUTOMATIC);
    assert_eq!(kept(&p), BTreeSet::from([head]));
}

#[test]
fn days_are_utc_days() {
    let at = |day, hour, minute| Utc.with_ymd_and_hms(2026, 11, day, hour, minute, 0).unwrap();
    let mut l = Line::new();
    // An hour apart, either side of midnight UTC. On a machine in UTC+8 both
    // are the morning of the 21st.
    let before_midnight = l.add("d-a", "steam", at(20, 23, 30), "auto-exit");
    let after_midnight = l.add("d-a", "steam", at(21, 0, 30), "auto-exit");
    // One UTC day, though in UTC+8 the second is already on the 23rd.
    let morning = l.add("d-a", "steam", at(22, 1, 0), "auto-exit");
    let night = l.add("d-a", "steam", at(22, 23, 0), "auto-exit");
    let head = l.add("d-a", "steam", at(25, 12, 0), "auto-exit");

    let p = decide(&l.view(), &[], &Policy { keep_recent: 0, keep_daily_days: 30, device_days: 90 });
    assert_eq!(why(&p, &before_midnight), [Reason::Daily]);
    assert_eq!(why(&p, &after_midnight), [Reason::Daily]);
    assert_eq!(why(&p, &night), [Reason::Daily]);
    assert_eq!(why(&p, &head), [Reason::Head, Reason::PlatformLatest, Reason::Daily]);
    assert_eq!(p.delete, [morning]);
}

#[test]
fn both_sides_of_a_fork_stay_until_it_is_settled() {
    let mut l = Line::new();
    // Both devices synced the same version two months ago. B played once
    // offline and has only just come back; A played on every day.
    let common = l.add("d-a", "steam", ago(61, 0), "auto-exit");
    let b_base = l.last.clone();
    let b_vv = Vv::next([&b_base], "d-b", 0);
    let b_side = l.put("d-b", "steam", ago(60, 0), "auto-exit", b_vv.clone());
    let a_days: Vec<String> = (0..40).rev().map(|day| l.add("d-a", "steam", ago(day, 0), "auto-exit")).collect();
    let a_head = a_days.last().unwrap().clone();

    let view = l.view();
    assert!(view.is_forked());
    let p = decide(&view, &[], &Policy::default());
    // B's side is two months old and 40 snapshots behind: only being a head
    // keeps it.
    assert_eq!(why(&p, &b_side), [Reason::Head, Reason::PlatformLatest]);
    assert_eq!(why(&p, &a_head), [Reason::Head, Reason::PlatformLatest, Reason::Recent, Reason::Daily]);
    // A's last 30 days and B's side stay.
    assert_eq!(p.keep.len(), 31);
    let mut gone = vec![common.clone()];
    gone.extend(a_days[..10].iter().cloned());
    assert_eq!(p.delete, gone);
    // With no automatic snapshots kept at all, the two heads are what is left.
    assert_eq!(kept(&decide(&view, &[], &NO_AUTOMATIC)), BTreeSet::from([a_head.clone(), b_side.clone()]));

    // The user picks A's side. The new version contains both, and B's side
    // is an automatic snapshot like any other again.
    let settled = l.put("d-a", "steam", now(), "sync-now", Vv::next([&l.last, &b_vv], "d-a", 0));
    let view = l.view();
    assert_eq!(view.head().unwrap().id, settled);
    let p = decide(&view, &[], &Policy::default());
    assert!(p.delete.contains(&b_side));
    assert_eq!(kept(&decide(&view, &[], &NO_AUTOMATIC)), BTreeSet::from([settled]));
}

#[test]
fn kept_saves_stay_and_take_no_place_among_the_newest() {
    let mut l = Line::new();
    // ids[0] is the oldest (45 days), ids[5] the newest (40 days).
    let ids: Vec<String> = (40..46).rev().map(|day| l.add("d-a", "steam", ago(day, 0), "auto-exit")).collect();
    l.pin(&ids[1], "Before the boss");
    l.pin(&ids[4], "");
    let policy = Policy { keep_recent: 2, keep_daily_days: 0, device_days: 90 };

    let p = decide(&l.view(), &[], &policy);
    assert_eq!(why(&p, &ids[1]), [Reason::Kept]);
    assert_eq!(why(&p, &ids[4]), [Reason::Kept]);
    // The two newest automatic ones are found without counting the kept one
    // between them.
    assert_eq!(why(&p, &ids[5]), [Reason::Head, Reason::PlatformLatest, Reason::Recent]);
    assert_eq!(why(&p, &ids[3]), [Reason::Recent]);
    assert_eq!(p.delete, [ids[0].clone(), ids[2].clone()]);

    // A pin that can't be read still keeps its snapshot.
    l.store.put(&v2::pin_path(DIR, &ids[0]), b"???");
    let p = decide(&l.view(), &[], &policy);
    assert_eq!(why(&p, &ids[0]), [Reason::Kept]);

    // No longer kept: an automatic snapshot again.
    line::remove_pin(&l.store, DIR, &ids[1]).unwrap();
    let p = decide(&l.view(), &[], &policy);
    assert_eq!(p.delete, [ids[1].clone(), ids[2].clone()]);
}

#[test]
fn the_newest_of_each_platform_stays() {
    let mut l = Line::new();
    // Three snapshots from 0.6, which wrote no platform.
    let old: Vec<String> = (88..91).rev().map(|day| l.legacy(ago(day, 0), "auto-exit", false)).collect();
    // Then three platforms take turns, three rounds.
    let turns = [("d-a", "steam"), ("d-b", "xbox"), ("d-c", "console:switch")];
    let mut newest: BTreeMap<&str, String> = BTreeMap::new();
    let mut day = 80;
    for _ in 0..3 {
        for (device, platform) in turns {
            newest.insert(platform, l.add(device, platform, ago(day, 0), "auto-exit"));
            day -= 1;
        }
    }
    // Saves from before a restore on the Xbox version, later than anything:
    // beside the history, so not the newest of the platform.
    let before_restore = l.side("d-b", "xbox", ago(60, 0), "pre-restore");

    let view = l.view();
    assert_eq!(view.snapshots.len(), 13);
    let p = decide(&view, &[], &NO_AUTOMATIC);
    assert_eq!(why(&p, &old[2]), [Reason::PlatformLatest]);
    assert_eq!(why(&p, &newest["steam"]), [Reason::PlatformLatest]);
    assert_eq!(why(&p, &newest["xbox"]), [Reason::PlatformLatest]);
    assert_eq!(why(&p, &newest["console:switch"]), [Reason::Head, Reason::PlatformLatest]);
    assert_eq!(p.keep.len(), 4);
    assert!(p.delete.contains(&before_restore));

    // With the two newest automatic snapshots kept as well: the saves from
    // before the restore are one of them, like any automatic snapshot, and
    // the newest of Steam still stays for the platform's sake alone.
    let p = decide(&view, &[], &Policy { keep_recent: 2, keep_daily_days: 30, device_days: 90 });
    assert_eq!(why(&p, &before_restore), [Reason::Recent]);
    assert_eq!(why(&p, &newest["console:switch"]), [Reason::Head, Reason::PlatformLatest, Reason::Recent]);
    assert_eq!(why(&p, &newest["steam"]), [Reason::PlatformLatest]);
    assert_eq!(p.keep.len(), 5);
}

#[test]
fn the_newest_of_a_platform_is_found_by_vectors_not_by_clocks() {
    let mut l = Line::new();
    // The clock of A runs a week ahead. B, on the same platform, plays on
    // from A's version; then the Xbox version takes over.
    let a = l.add("d-a", "steam", ago(33, 0), "auto-exit");
    let b = l.add("d-b", "steam", ago(40, 0), "auto-exit");
    let xbox = l.add("d-c", "xbox", ago(39, 0), "auto-exit");
    let p = decide(&l.view(), &[], &NO_AUTOMATIC);
    assert_eq!(why(&p, &b), [Reason::PlatformLatest]);
    assert_eq!(why(&p, &xbox), [Reason::Head, Reason::PlatformLatest]);
    assert_eq!(p.delete, [a]);

    // Two Steam devices fork and the Xbox version settles it: neither Steam
    // side contains the other, so both are still the newest of Steam.
    let mut l = Line::new();
    l.add("d-a", "steam", ago(50, 0), "auto-exit");
    let common = l.last.clone();
    let (a_vv, b_vv) = (Vv::next([&common], "d-a", 0), Vv::next([&common], "d-b", 0));
    let a = l.put("d-a", "steam", ago(49, 0), "auto-exit", a_vv.clone());
    let b = l.put("d-b", "steam", ago(48, 0), "auto-exit", b_vv.clone());
    let xbox = l.put("d-c", "xbox", ago(47, 0), "sync-now", Vv::next([&a_vv, &b_vv], "d-c", 0));
    let view = l.view();
    assert_eq!(view.head().unwrap().id, xbox);
    let p = decide(&view, &[], &NO_AUTOMATIC);
    assert_eq!(why(&p, &a), [Reason::PlatformLatest]);
    assert_eq!(why(&p, &b), [Reason::PlatformLatest]);
    assert_eq!(p.delete.len(), 1);
}

#[test]
fn a_device_keeps_its_base_for_90_days() {
    let mut l = Line::new();
    // ids[0] is the oldest.
    let ids: Vec<String> = (100..105).rev().map(|day| l.add("d-a", "steam", ago(day, 0), "auto-exit")).collect();
    let view = l.view();
    let seen = |at: DateTime<Utc>| [base("d-b", &ids[1], at)];

    let p = decide(&view, &seen(ago(89, 0)), &NO_AUTOMATIC);
    assert_eq!(why(&p, &ids[1]), [Reason::DeviceBase]);
    assert_eq!(p.delete, [ids[0].clone(), ids[2].clone(), ids[3].clone()]);
    assert_eq!(why(&decide(&view, &seen(ago(89, 23)), &NO_AUTOMATIC), &ids[1]), [Reason::DeviceBase]);
    // Not seen for 90 days: no longer kept for it.
    assert!(why(&decide(&view, &seen(ago(90, 0)), &NO_AUTOMATIC), &ids[1]).is_empty());
    let p = decide(&view, &seen(ago(91, 0)), &NO_AUTOMATIC);
    assert_eq!(p.delete, [ids[0].clone(), ids[1].clone(), ids[2].clone(), ids[3].clone()]);
    // A device whose clock is ahead was "seen" in the future.
    assert_eq!(why(&decide(&view, &seen(ago(-2, 0)), &NO_AUTOMATIC), &ids[1]), [Reason::DeviceBase]);

    // The number of days is the policy's.
    let week = Policy { device_days: 7, ..NO_AUTOMATIC };
    assert_eq!(why(&decide(&view, &seen(ago(6, 0)), &week), &ids[1]), [Reason::DeviceBase]);
    assert!(why(&decide(&view, &seen(ago(7, 0)), &week), &ids[1]).is_empty());

    // Two installs of one device, two devices on one snapshot, a device that
    // is gone, and a base that is no longer on the line.
    let bases = [
        base("d-b", &ids[0], ago(1, 0)),
        base("d-b", &ids[2], ago(1, 0)),
        base("d-c", &ids[2], ago(200, 0)),
        base("d-d", &ids[3], ago(200, 0)),
        base("d-e", &id(999), ago(1, 0)),
    ];
    let p = decide(&view, &bases, &NO_AUTOMATIC);
    assert_eq!(why(&p, &ids[0]), [Reason::DeviceBase]);
    assert_eq!(why(&p, &ids[2]), [Reason::DeviceBase]);
    assert_eq!(why(&p, &ids[4]), [Reason::Head, Reason::PlatformLatest]);
    assert_eq!(p.delete, [ids[1].clone(), ids[3].clone()]);
    assert!(!p.keep.contains_key(&id(999)));
}

#[test]
fn snapshots_on_the_side_are_automatic_ones_and_manual_backups_stay() {
    let mut l = Line::new();
    let manual = l.add("d-a", "steam", ago(300, 0), "manual");
    let played = l.add("d-a", "steam", ago(50, 0), "auto-exit");
    let before_restore = l.side("d-a", "steam", ago(49, 0), "pre-restore");
    let restored = l.add("d-a", "steam", ago(48, 0), "restore");
    let lost_conflict = l.side("d-a", "steam", ago(47, 0), "conflict-keep");
    let head = l.add("d-a", "steam", ago(46, 0), "auto-exit");
    let view = l.view();
    assert_eq!(view.head().unwrap().id, head);

    // As in 0.6: the manual backup stays and takes none of the two places,
    // the newer snapshot on the side takes one, the older one goes.
    let two = Policy { keep_recent: 2, keep_daily_days: 0, device_days: 90 };
    let p = decide(&view, &[], &two);
    assert_eq!(why(&p, &manual), [Reason::Manual]);
    assert_eq!(why(&p, &head), [Reason::Head, Reason::PlatformLatest, Reason::Recent]);
    assert_eq!(why(&p, &lost_conflict), [Reason::Recent]);
    assert_eq!(p.delete, [played, before_restore, restored]);
    assert_eq!(kept(&p), kept_by_0_6(&view, None, &Retention { keep_recent: 2, keep_daily_days: 0 }));
    // Nothing keeps a snapshot just for being on the side.
    let p = decide(&view, &[], &NO_AUTOMATIC);
    assert_eq!(kept(&p), BTreeSet::from([manual.clone(), head]));

    // A manual backup the user also keeps says both.
    l.pin(&manual, "Year one");
    assert_eq!(why(&decide(&l.view(), &[], &two), &manual), [Reason::Kept, Reason::Manual]);

    // The one per day counts them too: here the saves from before a restore
    // are the newest of their day, and the session before them goes.
    let mut l = Line::new();
    let played = l.add("d-a", "steam", ago(10, 5), "auto-exit");
    let before_restore = l.side("d-a", "steam", ago(10, 3), "pre-restore");
    let restored = l.add("d-a", "steam", ago(9, 0), "restore");
    let p = decide(&l.view(), &[], &Policy { keep_recent: 0, keep_daily_days: 30, device_days: 90 });
    assert_eq!(why(&p, &before_restore), [Reason::Daily]);
    assert_eq!(why(&p, &restored), [Reason::Head, Reason::PlatformLatest, Reason::Daily]);
    assert_eq!(p.delete, [played]);
}

#[test]
fn a_trigger_this_build_does_not_know_is_an_automatic_snapshot() {
    let mut l = Line::new();
    let scheduled = l.add("d-a", "steam", ago(50, 0), "scheduled");
    let not_quite = l.add("d-a", "steam", ago(49, 0), "Manual ");
    // A later version's kind of snapshot beside the history: `side` says so,
    // whatever the trigger is.
    let n = l.next;
    l.next += 1;
    let mut beside = Manifest {
        v: 2,
        id: id(n),
        game: "stardew-7k3f".into(),
        line: "main".into(),
        vv: l.last.clone(),
        device_id: "d-a".into(),
        device_name: "D-A".into(),
        platform: "steam".into(),
        created_at: ago(40, 0),
        trigger: "before-convert".into(),
        side: true,
        content_hash: "c".into(),
        total_size: 0,
        ..Default::default()
    };
    beside.extra.insert("reason".into(), json!("a field of a later version"));
    line::write_manifest(&l.store, DIR, &beside).unwrap();
    let head = l.add("d-a", "steam", ago(45, 0), "auto-exit");

    let view = l.view();
    assert_eq!(view.head().unwrap().id, head);
    let p = decide(&view, &[], &NO_AUTOMATIC);
    assert_eq!(p.delete, [scheduled.clone(), not_quite, beside.id.clone()]);
    let p = decide(&view, &[], &Policy { keep_recent: 1, keep_daily_days: 0, device_days: 90 });
    assert_eq!(why(&p, &beside.id), [Reason::Recent]);
    assert!(why(&p, &scheduled).is_empty());
}

#[test]
fn a_line_from_0_6_keeps_what_0_6_keeps() {
    let mut l = Line::new();
    // One snapshot a day for 90 days, as 0.6 wrote them; ids[n] is the one
    // from `n` days ago.
    let mut ids: Vec<String> = Vec::new();
    for day in (0..90).rev() {
        let (trigger, locked) = match day {
            85 => ("manual", false),
            80 => ("auto-exit", true),
            70 => ("pre-restore", true),
            60 => ("conflict-keep", false),
            3 => ("pre-restore", false),
            _ => ("auto-exit", false),
        };
        ids.push(l.legacy(ago(day, 0), trigger, locked));
    }
    ids.reverse();

    let view = l.view();
    assert_eq!(view.snapshots.len(), 90);
    assert!(view.snapshots.iter().all(|s| s.platform.is_empty()));
    // No vector in the cloud: the order they were made in stands for it.
    let head = view.head().unwrap().clone();
    assert_eq!(head.id, ids[0]);
    assert_eq!(head.vv, line::legacy_vv(DIR, head.created_at));
    // The lock is in the snapshot's own file and has to reach the plan.
    assert!(is_locked(view.get(&ids[80]).unwrap()));
    assert!(!is_locked(view.get(&ids[79]).unwrap()));

    let p = decide(&view, &[base("d-a", &head.id, now())], &Policy::default());
    assert_eq!(why(&p, &ids[85]), [Reason::Manual]);
    assert_eq!(why(&p, &ids[80]), [Reason::Locked]);
    assert_eq!(why(&p, &ids[70]), [Reason::Locked]);
    assert!(why(&p, &ids[60]).is_empty());
    assert_eq!(why(&p, &ids[3]), [Reason::Recent, Reason::Daily]);
    assert_eq!(why(&p, &ids[0]), [Reason::Head, Reason::PlatformLatest, Reason::DeviceBase, Reason::Recent, Reason::Daily]);
    // The last 30 days and the three the user asked for.
    assert_eq!(p.keep.len(), 33);
    assert_eq!(kept(&p), kept_by_0_6(&view, Some(&head.id), &Retention::default()));

    // The migration writes pins for the locked and the manual ones; they
    // then say both.
    l.pin(&ids[80], "2026-09-07");
    let view = l.view();
    assert_eq!(why(&decide(&view, &[], &Policy::default()), &ids[80]), [Reason::Kept, Reason::Locked]);

    // The index carries the lock, so a device that reads the line from it
    // comes to the same plan.
    let mut cache = LineCache::default();
    line::load(&l.store, DIR, &mut cache).unwrap();
    line::write_index(&l.store, DIR, &cache, "d-a").unwrap();
    l.store.take();
    let from_index = l.view();
    assert_eq!(l.store.take().read, 1, "the index and nothing else");
    assert_eq!(decide(&from_index, &[], &Policy::default()), decide(&view, &[], &Policy::default()));
}

#[test]
fn a_migrated_line_in_two_folders_is_one_history() {
    let mut l = Line::migrated();
    // What 0.6 left in its folder.
    let old_auto = l.legacy(ago(200, 0), "auto-exit", false);
    let old_locked = l.legacy(ago(199, 0), "auto-exit", true);
    let old_manual = l.legacy(ago(198, 0), "manual", false);
    let old_newest = l.legacy(ago(197, 0), "auto-exit", false);
    // The migration writes the pin beside the new manifests.
    l.pin(&old_locked, "2026-05-11");
    // Three versions since, in the new folder. Then a device still on 0.6
    // plays once more: a fork.
    let new: Vec<String> = (100..103).rev().map(|day| l.add("d-b", "steam", ago(day, 0), "auto-exit")).collect();
    let late = l.legacy(ago(90, 0), "auto-exit", false);

    let view = l.view();
    assert_eq!(view.snapshots.len(), 8);
    assert!(view.is_forked());
    let p = decide(&view, &[], &NO_AUTOMATIC);
    assert_eq!(why(&p, &old_locked), [Reason::Kept, Reason::Locked]);
    assert_eq!(why(&p, &old_manual), [Reason::Manual]);
    assert_eq!(why(&p, &new[2]), [Reason::Head, Reason::PlatformLatest]);
    assert_eq!(why(&p, &late), [Reason::Head, Reason::PlatformLatest]);
    assert_eq!(p.delete, [old_auto, old_newest, new[0].clone(), new[1].clone()]);
    // The plan names snapshots; the view says which folder each one is in.
    let places: Vec<usize> = p.delete.iter().map(|id| view.place(id).unwrap()).collect();
    assert_eq!(places, [1, 1, 0, 0]);
}

#[test]
fn all_the_rules_together() {
    let mut l = Line::new();
    // Before the migration.
    let old_auto = l.legacy(ago(200, 0), "auto-exit", false);
    let old_locked = l.legacy(ago(199, 0), "auto-exit", true);
    let old_manual = l.legacy(ago(198, 0), "manual", false);
    let old_newest = l.legacy(ago(197, 0), "auto-exit", false);
    // The Steam and the Xbox version on two PCs.
    let steam_1 = l.add("d-a", "steam", ago(100, 0), "auto-exit");
    let steam_kept = l.add("d-a", "steam", ago(99, 0), "auto-exit");
    l.pin(&steam_kept, "Chapter 3");
    let xbox = l.add("d-b", "xbox", ago(98, 0), "auto-exit");
    let steam_3 = l.add("d-a", "steam", ago(97, 0), "auto-exit");
    let steam_3_vv = l.last.clone();
    let before_restore = l.side("d-a", "steam", ago(96, 0), "pre-restore");
    // The Switch takes over: two sessions a day for 25 days. A played once
    // more on Steam without syncing first.
    let offline = l.put("d-a", "steam", ago(50, 0), "auto-exit", Vv::next([&steam_3_vv], "d-a", 0));
    let mut switch: Vec<[String; 2]> = Vec::new();
    for day in (0..25).rev() {
        switch.push([10, 2].map(|hours| l.add("d-c", "console:switch", ago(day, hours), "auto-exit")));
    }
    switch.reverse();
    let head = switch[0][1].clone();

    let view = l.view();
    assert_eq!(view.snapshots.len(), 60);
    assert!(view.is_forked());
    let bases = [
        base("d-b", &xbox, ago(10, 0)),
        // Sold a long time ago.
        base("d-old", &steam_3, ago(120, 0)),
        base("d-c", &head, now()),
    ];
    let p = decide(&view, &bases, &Policy::default());

    let mut expected: BTreeMap<String, Vec<Reason>> = BTreeMap::from([
        (old_locked, vec![Reason::Locked]),
        (old_manual, vec![Reason::Manual]),
        (old_newest, vec![Reason::PlatformLatest]),
        (steam_kept, vec![Reason::Kept]),
        (xbox, vec![Reason::PlatformLatest, Reason::DeviceBase]),
        (offline, vec![Reason::Head, Reason::PlatformLatest]),
    ]);
    let mut gone = vec![old_auto, steam_1, steam_3, before_restore];
    for (day, [first, second]) in switch.iter().enumerate() {
        match day {
            0 => {
                expected.insert(first.clone(), vec![Reason::Recent]);
                expected.insert(second.clone(), vec![Reason::Head, Reason::PlatformLatest, Reason::DeviceBase, Reason::Recent, Reason::Daily]);
            }
            1..=9 => {
                expected.insert(first.clone(), vec![Reason::Recent]);
                expected.insert(second.clone(), vec![Reason::Recent, Reason::Daily]);
            }
            _ => {
                expected.insert(second.clone(), vec![Reason::Daily]);
            }
        }
    }
    // Oldest first: the first sessions of days 24 to 10.
    gone.extend(switch[10..].iter().rev().map(|[first, _]| first.clone()));
    assert_eq!(p.keep, expected);
    assert_eq!(p.delete, gone);
}

#[test]
fn the_same_line_gives_the_same_plan() {
    let mut rng = Rng::new(7);
    let (l, bases, policy) = random_line(&mut rng);
    let view = l.view();
    let first = decide(&view, &bases, &policy);
    assert!(!first.delete.is_empty() && !first.keep.is_empty());
    assert_eq!(decide(&view, &bases, &policy), first);
    // Read again by another device.
    assert_eq!(decide(&l.view(), &bases, &policy), first);
    // The order of the devices doesn't matter, nor the order of the list.
    let backwards: Vec<DeviceBase> = bases.iter().rev().cloned().collect();
    assert_eq!(decide(&view, &backwards, &policy), first);
    let mut reversed = view.clone();
    reversed.snapshots.reverse();
    reversed.places.reverse();
    reversed.heads = view.heads.iter().map(|&i| view.snapshots.len() - 1 - i).collect();
    assert_eq!(plan(&reversed, &bases, &policy, now()), first);
}

/// A small generator, so that the lines below are the same on every run.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

/// A line as a few months of play could leave it: maybe a start in 0.6,
/// three devices on three platforms that mostly sync before they play and
/// sometimes don't, restores, conflicts, manual backups, kept saves.
fn random_line(rng: &mut Rng) -> (Line, Vec<DeviceBase>, Policy) {
    const TRIGGERS: [&str; 10] =
        ["auto-exit", "auto-exit", "auto-exit", "pre-launch", "passive", "sync-now", "manual", "restore", "pre-restore", "conflict-keep"];
    let mut l = Line::new();
    let count = rng.below(90);
    // Minutes before now, the oldest first.
    let mut minutes: Vec<i64> = (0..count).map(|_| rng.below(150 * 24 * 60) as i64).collect();
    minutes.sort_unstable_by(|a, b| b.cmp(a));
    let legacy = if rng.chance(40) { rng.below(20) as usize } else { 0 };

    let devices = [("d-a", "steam"), ("d-b", "xbox"), ("d-c", "console:switch")];
    // What each device has synced last, and everything the cloud has.
    let mut synced: [Vv; 3] = Default::default();
    let mut synced_id: [Option<String>; 3] = Default::default();
    let mut cloud = Vv::new();
    for (n, minutes) in minutes.into_iter().enumerate() {
        let at = now() - Duration::minutes(minutes);
        let trigger = TRIGGERS[rng.below(TRIGGERS.len() as u64) as usize];
        if n < legacy {
            l.legacy(at, trigger, rng.chance(15));
            cloud = l.last.clone();
            synced = [cloud.clone(), cloud.clone(), cloud.clone()];
            continue;
        }
        let d = rng.below(3) as usize;
        let (device, platform) = devices[d];
        if rng.chance(75) {
            synced[d] = cloud.clone();
        }
        if on_the_side(trigger) {
            l.put(device, platform, at, trigger, synced[d].clone());
            continue;
        }
        synced[d] = Vv::next([&synced[d]], device, 0);
        cloud.join(&synced[d]);
        let id = l.put(device, platform, at, trigger, synced[d].clone());
        if rng.chance(8) {
            l.pin(&id, "kept");
        }
        synced_id[d] = Some(id);
    }

    let mut bases = Vec::new();
    for (d, id) in synced_id.iter().enumerate() {
        if let Some(id) = id {
            bases.push(base(devices[d].0, id, now() - Duration::days(rng.below(140) as i64)));
        }
    }
    let policy = Policy { keep_recent: rng.below(25) as usize, keep_daily_days: rng.below(45) as i64, device_days: 90 };
    (l, bases, policy)
}

#[test]
fn every_plan_covers_its_line_and_never_keeps_less_than_0_6() {
    let (mut forked, mut deleting) = (0, 0);
    for seed in 0..300 {
        let mut rng = Rng::new(seed);
        let (l, bases, policy) = random_line(&mut rng);
        let view = l.view();
        // `decide` checks that the two lists are disjoint and cover the line.
        let p = decide(&view, &bases, &policy);
        forked += view.is_forked() as u32;
        deleting += !p.delete.is_empty() as u32;

        for s in view.head_entries() {
            assert!(why(&p, &s.id).contains(&Reason::Head), "seed {seed}: head {}", s.id);
        }
        for s in &view.snapshots {
            let reasons = why(&p, &s.id);
            assert_eq!(s.pin.is_some(), reasons.contains(&Reason::Kept), "seed {seed}: {}", s.id);
            assert_eq!(is_locked(s), reasons.contains(&Reason::Locked), "seed {seed}: {}", s.id);
            assert_eq!(s.trigger == "manual", reasons.contains(&Reason::Manual), "seed {seed}: {}", s.id);
            if s.is_side() {
                assert!(!reasons.contains(&Reason::Head) && !reasons.contains(&Reason::PlatformLatest));
            }
        }
        // The snapshot each platform got last.
        let mut newest: BTreeMap<&str, &IndexEntry> = BTreeMap::new();
        for s in view.snapshots.iter().filter(|s| !s.is_side()) {
            newest.insert(s.platform.as_str(), s);
        }
        for (platform, s) in newest {
            assert!(why(&p, &s.id).contains(&Reason::PlatformLatest), "seed {seed}: newest of {platform:?}");
        }
        for b in &bases {
            let recent = (now() - b.seen_at).num_days() < 90;
            assert_eq!(recent, why(&p, &b.snapshot).contains(&Reason::DeviceBase), "seed {seed}: {}", b.device_id);
        }
        // Everything 0.6 would keep with the same numbers is still kept.
        let settings = Retention { keep_recent: policy.keep_recent, keep_daily_days: policy.keep_daily_days };
        let before = kept_by_0_6(&view, None, &settings);
        let lost: Vec<&String> = before.iter().filter(|id| !p.keep.contains_key(*id)).collect();
        assert!(lost.is_empty(), "seed {seed}: 0.6 keeps {lost:?}");

        // Reading the line from its index gives the same plan.
        let mut cache = LineCache::default();
        line::load(&l.store, DIR, &mut cache).unwrap();
        line::write_index(&l.store, DIR, &cache, "d-a").unwrap();
        assert_eq!(decide(&l.view(), &bases, &policy), p, "seed {seed}");
    }
    // The lines are worth the name: many fork, most have something to delete.
    assert!(forked > 30 && deleting > 150, "{forked} forked, {deleting} with something to delete");
}
