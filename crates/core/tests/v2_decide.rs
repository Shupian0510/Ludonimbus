//! What a sync decides (docs/plan-v2.md, section 5.2), on save lines read
//! the real way: every row of the table in `cloud::decide`, the rules that
//! override it, and whole stories of two devices.

use chrono::{TimeZone, Utc};
use ludonimbus_core::cloud::decide::{decide, next_vv, settle, Action, Base, Local, Status};
use ludonimbus_core::cloud::line::{self, LineCache, LineView};
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::store::Store;
use ludonimbus_core::cloud::v2::{self, FileRef, IndexEntry, Manifest};
use ludonimbus_core::vv::Vv;
use std::cell::Cell;
use std::collections::BTreeMap;

const DIR: &str = "games/stardew-7k3f/saves/main";
const DAY: i64 = 86_400;

fn id(n: u64) -> String {
    format!("01JTEST{n:019}")
}

fn vv(pairs: &[(&str, u64)]) -> Vv {
    let map: BTreeMap<&str, u64> = pairs.iter().copied().collect();
    serde_json::from_value(serde_json::to_value(map).unwrap()).unwrap()
}

fn base_of(s: &IndexEntry) -> Base {
    Base { id: s.id.clone(), vv: s.vv.clone(), content_hash: s.content_hash.clone() }
}

fn conflict(candidates: &[&str], local_is_candidate: bool) -> Action {
    Action::Conflict { candidates: candidates.iter().map(|c| c.to_string()).collect(), local_is_candidate }
}

fn pull(id: &str) -> Action {
    Action::Pull { id: id.to_string() }
}

fn adopt(id: &str) -> Action {
    Action::Adopt { id: id.to_string() }
}

/// The folder of one save line in the cloud. Snapshots are numbered in the
/// order they are made; a save's content hash is simply its name.
struct Cloud {
    store: Memory,
    made: Cell<u64>,
}

impl Cloud {
    fn new() -> Self {
        Cloud { store: Memory::new(), made: Cell::new(0) }
    }

    fn view(&self) -> LineView {
        line::load(&self.store, DIR, &mut LineCache::default()).unwrap()
    }

    /// A snapshot that is not in the cloud yet. `clock` is how many seconds
    /// the clock of the device making it is off.
    fn snapshot(&self, device: &str, vv: Vv, content: &str, clock: i64) -> Manifest {
        let n = self.made.get() + 1;
        self.made.set(n);
        Manifest {
            v: 2,
            id: id(n),
            game: "stardew-7k3f".into(),
            line: "main".into(),
            vv,
            device_id: device.into(),
            device_name: device.to_uppercase(),
            platform: "steam".into(),
            created_at: Utc.timestamp_opt(1_780_000_000 + n as i64 * 60 + clock, 0).unwrap(),
            trigger: "auto-exit".into(),
            content_hash: content.into(),
            total_size: 10,
            files: vec![FileRef { path: "saves/a".into(), size: 10, hash: content.into(), ..Default::default() }],
            app_version: "1.0.0".into(),
            ..Default::default()
        }
    }

    fn upload(&self, m: &Manifest) {
        line::write_manifest(&self.store, DIR, m).unwrap();
    }

    /// Saves kept beside the history: they are in the cloud, never a head.
    fn keep_on_the_side(&self, device: &str, vv: Vv, content: &str) -> Manifest {
        let mut m = self.snapshot(device, vv, content, 0);
        m.trigger = "conflict-keep".into();
        m.side = true;
        self.upload(&m);
        m
    }

    /// A snapshot as 0.6 writes it: no vector, no line.
    fn upload_legacy(&self, device: &str, content: &str, trigger: &str) -> Manifest {
        let mut m = self.snapshot(device, Vv::new(), content, 0);
        m.v = 1;
        m.line = String::new();
        m.trigger = trigger.into();
        self.upload(&m);
        m
    }

    fn remove(&self, id: &str) {
        self.store.remove(&v2::manifest_path(DIR, id)).unwrap();
    }
}

/// A device as the caller of `decide` keeps it: its saves, what it synced
/// last, its counter, and the uploads still waiting for the cloud.
struct Device<'a> {
    cloud: &'a Cloud,
    id: &'static str,
    saves: Option<String>,
    base: Option<Base>,
    counter: u64,
    clock: i64,
    waiting: Vec<Manifest>,
}

impl<'a> Device<'a> {
    fn new(cloud: &'a Cloud, id: &'static str) -> Self {
        Device { cloud, id, saves: None, base: None, counter: 0, clock: 0, waiting: Vec::new() }
    }

    /// A device that has these saves and remembers this base.
    fn with(cloud: &'a Cloud, id: &'static str, saves: &str, base: Option<Base>) -> Self {
        Device { saves: Some(saves.into()), base, ..Device::new(cloud, id) }
    }

    fn local(&self) -> Local {
        Local { content_hash: self.saves.clone(), base: self.base.clone() }
    }

    fn saves(&self) -> &str {
        self.saves.as_deref().unwrap_or("")
    }

    fn base_id(&self) -> String {
        self.base.as_ref().unwrap().id.clone()
    }

    fn play(&mut self, content: &str) {
        self.saves = Some(content.into());
    }

    /// Look, change nothing.
    fn decide(&self) -> Action {
        decide(&self.local(), &self.cloud.view())
    }

    fn status(&self) -> Status {
        let (local, view) = (self.local(), self.cloud.view());
        Status::of(&decide(&local, &view), &local, &view)
    }

    /// A sync with the cloud in reach: send what was waiting, decide, and do
    /// whatever needs no answer from the user.
    fn sync(&mut self) -> Action {
        self.connect();
        let view = self.cloud.view();
        let action = decide(&self.local(), &view);
        self.perform(&action, &view);
        action
    }

    fn perform(&mut self, action: &Action, view: &LineView) {
        match action {
            Action::Nothing | Action::Conflict { .. } => {}
            Action::Adopt { id } => self.base = Some(base_of(view.get(id).unwrap())),
            Action::Pull { id } => {
                let s = view.get(id).unwrap();
                self.saves = Some(s.content_hash.clone());
                self.base = Some(base_of(s));
            }
            Action::Push { bases, parent } => {
                let vv = next_vv(bases, view, self.id, self.counter);
                let m = self.snapshot(vv, parent.clone());
                self.cloud.upload(&m);
            }
        }
    }

    /// The saves here as a snapshot, which becomes the base.
    fn snapshot(&mut self, vv: Vv, parent: Option<String>) -> Manifest {
        let mut m = self.cloud.snapshot(self.id, vv, self.saves(), self.clock);
        m.parent = parent;
        self.counter = m.vv.get(self.id);
        self.base = Some(Base { id: m.id.clone(), vv: m.vv.clone(), content_hash: m.content_hash.clone() });
        m
    }

    /// After playing without the cloud: the snapshot is made from the base
    /// and waits on this device (section 5.5).
    fn save_offline(&mut self) -> Manifest {
        let vv = Vv::next(self.base.iter().map(|b| &b.vv), self.id, self.counter);
        let parent = self.base.as_ref().map(|b| b.id.clone());
        let m = self.snapshot(vv, parent);
        self.waiting.push(m.clone());
        m
    }

    fn connect(&mut self) {
        for m in self.waiting.drain(..) {
            self.cloud.upload(&m);
        }
    }

    /// The user's answer to a conflict: the saves on this device.
    fn keep_local(&mut self) {
        let view = self.cloud.view();
        let action = settle(&self.local(), &view);
        assert!(matches!(action, Action::Push { .. }));
        self.perform(&action, &view);
    }

    /// The user's answer to a conflict: this version from the cloud.
    fn keep_cloud(&mut self, id: &str) {
        let view = self.cloud.view();
        self.perform(&pull(id), &view);
        if view.is_forked() {
            let action = settle(&self.local(), &view);
            self.perform(&action, &view);
        }
    }
}

/// Two devices that both have the first version, "v1".
fn two(cloud: &Cloud) -> (Device<'_>, Device<'_>) {
    let mut a = Device::with(cloud, "d-a", "v1", None);
    assert!(matches!(a.sync(), Action::Push { .. }));
    let mut b = Device::new(cloud, "d-b");
    assert!(matches!(b.sync(), Action::Pull { .. }));
    assert_eq!(b.saves(), "v1");
    (a, b)
}

/// Both devices played from "v1" without seeing each other's upload: the
/// line has the two heads "a2" and "b2".
fn forked(cloud: &Cloud) -> (Device<'_>, Device<'_>, String, String) {
    let (mut a, mut b) = two(cloud);
    a.play("a2");
    let s_a = a.save_offline().id;
    b.play("b2");
    let s_b = b.save_offline().id;
    a.connect();
    b.connect();
    assert!(cloud.view().is_forked());
    (a, b, s_a, s_b)
}

// ------------------------------------------------------- the rows of the table

#[test]
fn the_head_is_what_was_synced_last() {
    let cloud = Cloud::new();
    let (mut a, b) = two(&cloud);

    // Unchanged: nothing.
    assert_eq!(a.decide(), Action::Nothing);
    assert_eq!(b.decide(), Action::Nothing);
    assert_eq!(a.status(), Status::Synced);

    // Changed: push, built on the base.
    a.play("v2");
    assert_eq!(a.decide(), Action::Push { bases: vec![vv(&[("d-a", 1)])], parent: Some(id(1)) });
    assert_eq!(a.status(), Status::LocalChanged);
    a.sync();
    let head = cloud.view().head().unwrap().clone();
    assert_eq!(head.vv, vv(&[("d-a", 2)]));
    assert_eq!(head.parent, Some(id(1)));
    assert_eq!(a.decide(), Action::Nothing);

    // The table goes by the base. A head whose hash is not the one this
    // device computes for the same saves causes no transfer.
    let odd = Device::with(&cloud, "d-c", "v2 hashed another way", None);
    let odd = Device { base: Some(Base { content_hash: odd.saves().into(), ..base_of(&head) }), ..odd };
    assert_eq!(odd.decide(), Action::Nothing);
}

#[test]
fn a_head_that_contains_what_was_synced_last_is_pulled() {
    let cloud = Cloud::new();
    let (mut a, mut b) = two(&cloud);
    b.play("v2");
    assert!(matches!(b.sync(), Action::Push { .. }));

    assert_eq!(a.decide(), pull(&b.base_id()));
    assert_eq!(a.status(), Status::CloudNewer);
    a.sync();
    assert_eq!(a.saves(), "v2");
    assert_eq!(a.decide(), Action::Nothing);
}

#[test]
fn changes_on_both_sides_are_a_conflict() {
    let cloud = Cloud::new();
    let (mut a, mut b) = two(&cloud);
    b.play("b2");
    b.sync();
    a.play("a2");

    let asked = conflict(&[&b.base_id()], true);
    assert_eq!(a.sync(), asked);
    assert_eq!(a.status(), Status::Conflict);
    // Asking again changes nothing, and nothing was touched.
    assert_eq!(a.sync(), asked);
    assert_eq!(a.saves(), "a2");
    assert_eq!(cloud.view().head().unwrap().content_hash, "b2");
}

/// The row the table of section 5.2 leaves out: nothing changed here, but
/// what this device synced last has changes the head lacks.
#[test]
fn a_head_that_lacks_what_was_synced_last_is_never_pulled_silently() {
    // The upload is still waiting on this device.
    let cloud = Cloud::new();
    let (mut a, mut b) = two(&cloud);
    a.play("a2");
    let waiting = a.save_offline();
    b.play("b2");
    b.sync();
    let asked = conflict(&[&b.base_id()], true);
    assert_eq!(a.decide(), asked);
    // Playing on is the same question.
    a.play("a3");
    assert_eq!(a.decide(), asked);
    a.play("a2");

    // It arrived, and was then deleted from the cloud on another device.
    a.connect();
    assert!(cloud.view().is_forked());
    cloud.remove(&waiting.id);
    assert_eq!(a.decide(), asked);
    assert_eq!(a.status(), Status::Conflict);

    // The user keeps the saves here: the new head contains both sides, the
    // deleted one included.
    let theirs = b.base.clone().unwrap().vv;
    assert_eq!(
        settle(&a.local(), &cloud.view()),
        Action::Push { bases: vec![theirs, waiting.vv.clone()], parent: Some(waiting.id.clone()) }
    );
    a.keep_local();
    let head = cloud.view().head().unwrap().clone();
    assert_eq!(head.content_hash, "a2");
    assert!(head.vv.contains(&waiting.vv) && head.vv.contains(&b.base.as_ref().unwrap().vv));
    assert_eq!(head.vv, vv(&[("d-a", 3), ("d-b", 1)]));
    assert_eq!(a.decide(), Action::Nothing);
    assert_eq!(b.sync(), pull(&head.id));

    // The saves here were put on the side when a conflict was found after
    // playing, and that copy is what the device remembers.
    let cloud = Cloud::new();
    let (a, mut b) = two(&cloud);
    b.play("b2");
    b.sync();
    let kept = cloud.keep_on_the_side("d-a", vv(&[("d-a", 2)]), "a2");
    let a = Device { saves: Some("a2".into()), base: Some(base_of(&kept.summary())), ..a };
    assert_eq!(cloud.view().head().unwrap().id, b.base_id());
    assert_eq!(a.decide(), conflict(&[&b.base_id()], true));
    // A device that remembers the id without a vector: the line still has
    // the snapshot, and its vector there is the one compared.
    let forgot = Device { base: Some(Base { vv: Vv::new(), ..a.base.clone().unwrap() }), ..a };
    assert_eq!(forgot.decide(), conflict(&[&b.base_id()], true));
}

#[test]
fn no_saves_here() {
    let cloud = Cloud::new();
    // Nothing anywhere.
    let mut c = Device::new(&cloud, "d-c");
    assert_eq!(c.decide(), Action::Nothing);
    assert_eq!(c.status(), Status::Empty);

    // A version in the cloud: pull it.
    let (mut a, _b) = two(&cloud);
    assert_eq!(c.decide(), pull(&id(1)));
    assert_eq!(c.status(), Status::CanRestore);
    c.sync();
    assert_eq!(c.saves(), "v1");

    // The game was reinstalled: what was synced last is the head, and it is
    // pulled all the same.
    a.saves = None;
    assert_eq!(a.decide(), pull(&id(1)));
    assert_eq!(a.status(), Status::CanRestore);

    // Two heads with different contents: nothing here to lose, but which one
    // to take is still a question.
    let cloud = Cloud::new();
    let (mut a, _b, s_a, s_b) = forked(&cloud);
    a.saves = None;
    assert_eq!(a.decide(), conflict(&[&s_a, &s_b], false));
    assert_eq!(Device::new(&cloud, "d-c").decide(), conflict(&[&s_a, &s_b], false));
    assert_eq!(settle(&a.local(), &cloud.view()), Action::Nothing);
}

#[test]
fn an_empty_line_takes_the_first_upload() {
    let cloud = Cloud::new();
    let mut a = Device::with(&cloud, "d-a", "v1", None);
    assert_eq!(a.decide(), Action::Push { bases: vec![], parent: None });
    assert_eq!(a.status(), Status::NotUploaded);
    a.sync();
    assert_eq!(cloud.view().head().unwrap().vv, vv(&[("d-a", 1)]));
    assert_eq!(a.status(), Status::Synced);

    // Only saves kept on the side: still a first upload, and the count this
    // device used there is not used again.
    let cloud = Cloud::new();
    cloud.keep_on_the_side("d-a", vv(&[("d-a", 3), ("d-b", 2)]), "old");
    let mut a = Device::with(&cloud, "d-a", "v1", None);
    assert_eq!(a.decide(), Action::Push { bases: vec![], parent: None });
    assert_eq!(a.status(), Status::NotUploaded);
    a.sync();
    assert_eq!(cloud.view().head().unwrap().vv, vv(&[("d-a", 4)]));

    // Every version was removed from the cloud: the saves here go up again,
    // built on what they were made from.
    let cloud = Cloud::new();
    let (mut a, mut b) = two(&cloud);
    b.play("v2");
    b.sync();
    a.sync();
    cloud.remove(&id(1));
    cloud.remove(&b.base_id());
    let was = a.base.clone().unwrap();
    assert_eq!(a.decide(), Action::Push { bases: vec![was.vv.clone()], parent: Some(was.id.clone()) });
    assert_eq!(a.status(), Status::NotUploaded);
    a.sync();
    assert_eq!(cloud.view().head().unwrap().vv, vv(&[("d-a", 2), ("d-b", 1)]));
}

#[test]
fn heads_with_different_contents_ask_which_one() {
    let cloud = Cloud::new();
    let (a, b, s_a, s_b) = forked(&cloud);
    let heads = [s_a.as_str(), s_b.as_str()];
    let first = base_of(cloud.view().get(&id(1)).unwrap());

    // Each side has one of the heads: its saves are not a third choice.
    assert_eq!(a.decide(), conflict(&heads, false));
    assert_eq!(b.decide(), conflict(&heads, false));
    assert_eq!(a.status(), Status::Conflict);

    // A third device still on "v1", which both heads contain.
    let mut c = Device::with(&cloud, "d-c", "v1", Some(first.clone()));
    assert_eq!(c.decide(), conflict(&heads, false));
    // It played as well: three candidates.
    c.play("c2");
    assert_eq!(c.decide(), conflict(&heads, true));
    // By chance it ended up with the same saves as one side.
    c.play("b2");
    assert_eq!(c.decide(), conflict(&heads, false));

    // Its own upload is still waiting: no head has that progress.
    let mut d = Device::with(&cloud, "d-d", "v1", Some(first));
    d.play("d2");
    d.save_offline();
    assert_eq!(d.decide(), conflict(&heads, true));

    // Devices that never synced: saves the line doesn't know are a choice,
    // an old version of the line is not.
    assert_eq!(Device::with(&cloud, "d-e", "e1", None).decide(), conflict(&heads, true));
    assert_eq!(Device::with(&cloud, "d-e", "v1", None).decide(), conflict(&heads, false));
}

// ------------------------------------------------ rules that override the table

#[test]
fn saves_identical_to_the_head_never_transfer_or_ask() {
    let cloud = Cloud::new();
    let (mut a, mut b) = two(&cloud);

    // A device that never synced, with the same saves.
    let mut c = Device::with(&cloud, "d-c", "v1", None);
    assert_eq!(c.decide(), adopt(&id(1)));
    assert_eq!(c.status(), Status::Synced);
    c.sync();
    assert_eq!(c.base_id(), id(1));
    assert_eq!(c.decide(), Action::Nothing);

    // Both devices made the same change; B uploaded it first.
    b.play("v2");
    b.sync();
    a.play("v2");
    assert_eq!(a.sync(), adopt(&b.base_id()));
    assert_eq!(a.decide(), Action::Nothing);

    // The same, with A's upload waiting on the device: its vector and the
    // head's are forked, the saves are the same.
    b.play("v3");
    b.sync();
    a.play("v3");
    let waiting = a.save_offline();
    let head = cloud.view().head().unwrap().clone();
    assert!(!head.vv.contains(&waiting.vv) && !waiting.vv.contains(&head.vv));
    assert_eq!(a.decide(), adopt(&head.id));
    assert_eq!(a.status(), Status::Synced);
    assert_eq!(cloud.view().snapshots.len(), 3, "nothing was uploaded");
}

#[test]
fn the_base_need_not_be_in_the_line() {
    let cloud = Cloud::new();
    let (mut a, mut b) = two(&cloud);
    for content in ["v2", "v3", "v4"] {
        b.play(content);
        b.sync();
    }
    // Retention removed everything but the newest version, and with it what
    // A synced last.
    for n in 1..=3 {
        cloud.remove(&id(n));
    }
    let view = cloud.view();
    assert_eq!(view.snapshots.len(), 1);
    assert!(view.get(&a.base_id()).is_none());

    // The head contains what A had: the cloud moved on.
    assert_eq!(a.decide(), pull(&id(4)));
    assert_eq!(a.status(), Status::CloudNewer);
    // Had A played in the meantime, that is a conflict like any other.
    let mut played = Device { saves: Some("a2".into()), base: a.base.clone(), ..Device::new(&cloud, "d-a") };
    assert_eq!(played.sync(), conflict(&[&id(4)], true));
    assert_eq!(played.saves(), "a2");

    a.sync();
    assert_eq!(a.saves(), "v4");
    assert_eq!(a.decide(), Action::Nothing);
}

#[test]
fn a_device_without_a_base_recognises_an_older_version() {
    let cloud = Cloud::new();
    let (mut a, _b) = two(&cloud);
    a.play("v2");
    a.sync();
    let head = a.base_id();

    // It lost its state, or the saves were copied over by hand: they are a
    // version the head contains.
    let c = Device::with(&cloud, "d-c", "v1", None);
    assert_eq!(c.decide(), pull(&head));
    assert_eq!(c.status(), Status::CloudNewer);

    // Saves the line has never seen can't be told from new progress.
    let c = Device::with(&cloud, "d-c", "mine", None);
    assert_eq!(c.decide(), conflict(&[&head], true));

    // A copy kept on the side is not history: it is what a conflict nobody
    // answered yet looks like.
    cloud.keep_on_the_side("d-c", vv(&[("d-a", 1), ("d-c", 1)]), "mine");
    assert_eq!(c.decide(), conflict(&[&head], true));
}

#[test]
fn saves_kept_on_the_side_are_never_a_head() {
    let cloud = Cloud::new();
    let (mut a, b) = two(&cloud);
    a.play("v2");
    a.sync();
    // Later than everything, with a vector nothing contains.
    cloud.keep_on_the_side("d-b", vv(&[("d-a", 2), ("d-b", 5)]), "before the restore");

    assert_eq!(a.decide(), Action::Nothing);
    assert_eq!(b.decide(), pull(&a.base_id()));
    assert_eq!(Device::new(&cloud, "d-c").decide(), pull(&a.base_id()));
    // B's next upload doesn't reuse the count the copy on the side has.
    let view = cloud.view();
    assert_eq!(next_vv(&[view.head().unwrap().vv.clone()], &view, "d-b", 0), vv(&[("d-a", 2), ("d-b", 6)]));
}

#[test]
fn a_count_is_never_used_twice() {
    let cloud = Cloud::new();
    let (mut a, mut b) = two(&cloud);
    for content in ["a2", "a3"] {
        a.play(content);
        a.sync();
    }
    b.sync();
    assert_eq!(a.counter, 3);

    // A lost its state and has an older version again: its next snapshot is
    // built on a base with a lower count than it already used.
    let view = cloud.view();
    let old = [view.get(&id(1)).unwrap().vv.clone()];
    assert_eq!(next_vv(&old, &view, "d-a", 0), vv(&[("d-a", 4)]));
    // What the device remembers counts as well: an upload that is still
    // waiting is in no view.
    assert_eq!(next_vv(&old, &view, "d-a", 7), vv(&[("d-a", 8)]));
    assert_eq!(next_vv(&[], &LineView::default(), "d-a", 0), vv(&[("d-a", 1)]));
}

#[test]
fn status_names_are_those_of_today() {
    let names: Vec<String> = [
        Status::Empty,
        Status::NotUploaded,
        Status::Synced,
        Status::LocalChanged,
        Status::CloudNewer,
        Status::CanRestore,
        Status::Conflict,
    ]
    .iter()
    .map(|s| serde_json::to_value(s).unwrap().as_str().unwrap().to_string())
    .collect();
    assert_eq!(names, ["empty", "notUploaded", "synced", "localChanged", "cloudNewer", "canRestore", "conflict"]);
}

// ------------------------------------------------------------------ whole stories

/// The example of sections 1 and 5.1: both devices have the same version, A
/// plays offline, B plays and uploads, then A comes back. With 0.6, A pulled
/// B's version without a word. A's clock being a day off changes nothing.
#[test]
fn offline_play_on_two_devices_asks_instead_of_overwriting() {
    for clock in [0, -DAY, DAY] {
        for keep_local in [true, false] {
            let cloud = Cloud::new();
            let (mut a, mut b) = two(&cloud);
            a.clock = clock;

            a.play("a2");
            let s_a = a.save_offline().id;
            b.play("b2");
            assert!(matches!(b.sync(), Action::Push { .. }));
            let s_b = b.base_id();

            // A is back: its upload goes through, and it is asked.
            let Action::Conflict { mut candidates, local_is_candidate } = a.sync() else {
                panic!("clock {clock}: A must be asked");
            };
            candidates.sort();
            assert_eq!(candidates, [s_a.clone(), s_b.clone()]);
            assert!(!local_is_candidate, "A's saves are one of the two heads");
            assert_eq!(a.saves(), "a2");
            assert_eq!(a.status(), Status::Conflict);
            // B is asked too if it gets there first.
            assert!(matches!(b.decide(), Action::Conflict { local_is_candidate: false, .. }));
            assert_eq!(b.saves(), "b2");

            let kept = if keep_local {
                a.keep_local();
                "a2"
            } else {
                a.keep_cloud(&s_b);
                "b2"
            };

            // One head that contains both sides; both sides are still there.
            let view = cloud.view();
            let head = view.head().expect("the fork is closed").clone();
            assert_eq!(head.content_hash, kept);
            assert_eq!(head.vv, vv(&[("d-a", 3), ("d-b", 1)]));
            assert!(view.get(&s_a).is_some() && view.get(&s_b).is_some());

            assert_eq!(a.saves(), kept);
            assert_eq!(a.decide(), Action::Nothing);
            // B takes the result: a download when A's saves were kept, only
            // a note of the new head when its own were.
            let expected = if keep_local { pull(&head.id) } else { adopt(&head.id) };
            assert_eq!(b.sync(), expected);
            assert_eq!(b.saves(), kept);
            assert_eq!(b.decide(), Action::Nothing);
            assert_eq!((a.status(), b.status()), (Status::Synced, Status::Synced));
        }
    }
}

#[test]
fn taking_turns_never_asks() {
    let cloud = Cloud::new();
    let (mut a, mut b) = two(&cloud);
    // One clock is a day behind: turns are told by vectors, not by time.
    b.clock = -DAY;

    for round in 0..6 {
        let (player, other) = if round % 2 == 0 { (&mut a, &mut b) } else { (&mut b, &mut a) };
        assert_eq!(player.sync(), Action::Nothing, "before playing");
        player.play(&format!("round {round}"));
        if round % 3 == 2 {
            // Played without the cloud, which nobody else touched meanwhile.
            player.save_offline();
            assert_eq!(player.sync(), Action::Nothing);
        } else {
            assert!(matches!(player.sync(), Action::Push { .. }));
        }
        assert_eq!(other.sync(), pull(&player.base_id()));
        assert_eq!(other.saves(), player.saves());
    }
    let view = cloud.view();
    assert_eq!(view.snapshots.len(), 7);
    assert_eq!(view.head().unwrap().vv, vv(&[("d-a", 4), ("d-b", 3)]));
    assert_eq!((a.decide(), b.decide()), (Action::Nothing, Action::Nothing));
}

#[test]
fn a_restored_older_version_reaches_the_other_device() {
    let cloud = Cloud::new();
    let (mut a, mut b) = two(&cloud);
    for content in ["v2", "v3"] {
        a.play(content);
        a.sync();
    }
    b.sync();
    assert_eq!(b.saves(), "v3");
    let newest = a.base_id();

    // A puts the first version back: a new head built on the current one.
    a.play("v1");
    assert_eq!(a.decide(), Action::Push { bases: vec![vv(&[("d-a", 3)])], parent: Some(newest) });
    a.sync();
    let restored = a.base_id();
    assert_eq!(cloud.view().head().unwrap().id, restored);

    assert_eq!(b.sync(), pull(&restored));
    assert_eq!(b.saves(), "v1");
    assert_eq!(b.decide(), Action::Nothing);
    // The versions in between are still in the line.
    assert_eq!(cloud.view().snapshots.len(), 4);
}

#[test]
fn a_line_from_0_6_carries_on() {
    let cloud = Cloud::new();
    let first = cloud.upload_legacy("d-a", "v1", "auto-exit");
    // The losing side of a conflict, as 0.6 marks it.
    cloud.upload_legacy("d-b", "lost", "conflict-keep");
    cloud.upload_legacy("d-a", "v2", "auto-exit");
    let newest = cloud.upload_legacy("d-a", "v3", "auto-exit").id;
    let view = cloud.view();
    // No vector in the cloud: the time it was made stands for it.
    let old = view.head().unwrap().vv.clone();
    assert_eq!(old, line::legacy_vv(view.head().unwrap().created_at));
    let old_count = old.get(ludonimbus_core::vv::LEGACY);

    // Upgraded devices know the id and the hash of what they synced last,
    // and no vector.
    let upgraded = |device, saves: &str, n: u64| {
        let base = Base { id: id(n), vv: Vv::new(), content_hash: saves.into() };
        Device::with(&cloud, device, saves, Some(base))
    };
    let mut a = upgraded("d-a", "v3", 4);
    let mut b = upgraded("d-b", "v1", 1);
    assert_eq!(a.decide(), Action::Nothing);
    assert_eq!(b.decide(), pull(&newest));
    // A device that had played before upgrading.
    let mut played = upgraded("d-c", "v1", 1);
    played.play("c2");
    assert_eq!(played.decide(), conflict(&[&newest], true));
    // A new device that happens to have the newest saves.
    assert_eq!(Device::with(&cloud, "d-d", "v3", None).decide(), adopt(&newest));

    // While the snapshot is still there, the view says where it stands,
    // whatever the device remembers.
    let stale = Base { id: id(1), vv: Vv::legacy(u64::MAX), content_hash: "v1".into() };
    assert_eq!(Device::with(&cloud, "d-g", "v1", Some(stale)).decide(), pull(&newest));

    // What a device synced last was cleaned up by 0.6: the cloud moved on,
    // as 0.6 would say. An upgraded device remembers no vector; one that
    // has read the line since remembers the snapshot's own.
    for remembered in [Vv::new(), line::legacy_vv(first.created_at)] {
        let base = Base { id: id(90), vv: remembered, content_hash: "v0".into() };
        let mut gone = Device::with(&cloud, "d-e", "v0", Some(base));
        assert_eq!(gone.decide(), pull(&newest));
        gone.play("e1");
        assert_eq!(gone.decide(), conflict(&[&newest], true));
    }

    // The first snapshot of the new kind is built on the newest old one.
    a.play("v4");
    assert_eq!(a.decide(), Action::Push { bases: vec![old.clone()], parent: Some(newest.clone()) });
    a.sync();
    let head = cloud.view().head().unwrap().clone();
    assert_eq!(head.vv, vv(&[("d-a", 1), ("legacy", old_count)]));
    assert_eq!(b.sync(), pull(&head.id));
    assert_eq!(b.saves(), "v4");
    assert_eq!((a.decide(), b.decide()), (Action::Nothing, Action::Nothing));

    // From here on it is a line like any other.
    b.play("v5");
    assert_eq!(b.decide(), Action::Push { bases: vec![head.vv.clone()], parent: Some(head.id.clone()) });
    b.sync();
    assert_eq!(cloud.view().head().unwrap().vv, vv(&[("d-a", 1), ("d-b", 1), ("legacy", old_count)]));
    assert_eq!(a.sync(), pull(&b.base_id()));

    // A device still on 0.6 uploads without seeing any of this: a fork, and
    // the user is asked (section 12.3). Also when it cleaned up an older
    // snapshot first: what stands for an old snapshot's vector doesn't
    // depend on how many others are left.
    cloud.remove(&first.id);
    let late = cloud.upload_legacy("d-f", "f1", "auto-exit").id;
    assert_eq!(a.decide(), conflict(&[&b.base_id(), &late], false));
}

/// Two devices end up with the same saves and both upload them offline: the
/// vectors say fork, the contents say there is nothing to choose.
#[test]
fn the_same_saves_uploaded_offline_by_two_devices_ask_nothing() {
    let cloud = Cloud::new();
    let (mut a, mut b) = two(&cloud);
    a.play("same");
    let s_a = a.save_offline();
    b.play("same");
    let s_b = b.save_offline();
    assert_eq!(a.sync(), Action::Nothing);
    assert_eq!(b.sync(), Action::Nothing);
    let view = cloud.view();
    assert!(view.is_forked());
    let first = base_of(view.get(&id(1)).unwrap());

    // Everybody else sees one version, under the name of the newer upload.
    assert_eq!(Device::with(&cloud, "d-c", "v1", Some(first.clone())).decide(), pull(&s_b.id));
    assert_eq!(Device::new(&cloud, "d-c").decide(), pull(&s_b.id));
    assert_eq!(Device::with(&cloud, "d-c", "same", None).decide(), adopt(&s_b.id));
    assert_eq!(Device::with(&cloud, "d-c", "same", Some(first.clone())).decide(), adopt(&s_b.id));
    // Only real changes against it are a conflict, with one candidate.
    assert_eq!(Device::with(&cloud, "d-c", "c2", Some(first)).decide(), conflict(&[&s_b.id], true));
    assert_eq!((a.status(), b.status()), (Status::Synced, Status::Synced));

    // The next upload is built on both and closes the fork.
    a.play("a3");
    assert_eq!(a.decide(), Action::Push { bases: vec![s_a.vv.clone(), s_b.vv.clone()], parent: Some(s_a.id.clone()) });
    a.sync();
    let view = cloud.view();
    assert!(!view.is_forked());
    assert_eq!(view.head().unwrap().vv, vv(&[("d-a", 3), ("d-b", 1)]));
    assert_eq!(b.sync(), pull(&a.base_id()));
    assert_eq!((a.decide(), b.decide()), (Action::Nothing, Action::Nothing));
}
