//! Game records of cloud layout 2: refreshing the library, changing a record
//! from two devices, records that turn out to be the same game, and how many
//! requests each costs (docs/plan-v2.md, sections 3.3 and 5.6).

use chrono::{DateTime, TimeZone, Utc};
use ludonimbus_core::cloud::library::{self, Library, LibraryCache, Match};
use ludonimbus_core::cloud::memory::{Counts, Memory};
use ludonimbus_core::cloud::store::Store;
use ludonimbus_core::cloud::v2::{self, GameRecord, Line};
use serde_json::{json, Value};

fn at(minutes: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_780_000_000 + minutes * 60, 0).unwrap()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// A game added on `device`: a new record, saved.
fn create(store: &Memory, name: &str, ids: &[&str], format: &str, device: &str, minute: i64) -> GameRecord {
    let record = library::new_record(name, &strings(ids), format, device, at(minute));
    library::save(store, &record, device, at(minute)).unwrap()
}

/// The same with the id chosen, for the tests where two devices add a game
/// of the same name: that both draw the same four characters, once in a
/// million times, is not what they are about.
fn create_as(store: &Memory, id: &str, name: &str, ids: &[&str], format: &str, device: &str, minute: i64) -> GameRecord {
    let mut record = library::new_record(name, &strings(ids), format, device, at(minute));
    record.id = id.into();
    record.folders = vec![id.into()];
    record.lines[0].path = v2::line_dir(id, library::MAIN_LINE);
    library::save(store, &record, device, at(minute)).unwrap()
}

fn refresh(store: &Memory, cache: &mut LibraryCache) -> (Library, Counts) {
    store.take();
    let library = library::load(store, cache).unwrap();
    (library, store.take())
}

/// (lists, reads) of a refresh, which does nothing else.
fn requests(c: Counts) -> (u32, u32) {
    assert_eq!(c.total(), c.list + c.read, "refreshing only lists and reads");
    (c.list, c.read)
}

/// (reads, writes) of a save, which does nothing else.
fn save(store: &Memory, record: &GameRecord, device: &str, minute: i64) -> (GameRecord, (u32, u32)) {
    store.take();
    let saved = library::save(store, record, device, at(minute)).unwrap();
    let c = store.take();
    assert_eq!(c.total(), c.read + c.write, "saving only reads and writes");
    (saved, (c.read, c.write))
}

fn in_cloud(store: &Memory, id: &str) -> Value {
    serde_json::from_slice(&store.peek(&v2::game_record_path(id)).unwrap()).unwrap()
}

fn ids_of(library: &Library) -> Vec<&str> {
    library.records.iter().map(|r| r.id.as_str()).collect()
}

#[test]
fn refreshing_costs_one_listing_and_one_read_per_changed_record() {
    let store = Memory::new();
    let mut made = Vec::new();
    for n in 0..30 {
        made.push(create(&store, &format!("Game {n}"), &[&format!("steam:{n}")], "pc", "d-a", n));
    }

    // A device that has never seen the library.
    let mut cache = LibraryCache::default();
    let (library, c) = refresh(&store, &mut cache);
    assert_eq!(requests(c), (1, 30));
    assert_eq!(library.records.len(), 30);
    assert!(library.skipped.is_empty());
    assert!(library.records.windows(2).all(|w| w[0].id < w[1].id), "sorted by id");
    assert_eq!(library::get(&library.records, &made[7].id), Some(&made[7]));

    // Nothing changed: the listing is enough.
    let (again, c) = refresh(&store, &mut cache);
    assert_eq!(requests(c), (1, 0));
    assert_eq!(again, library);

    // Two records changed and one is new: three reads, not thirty-one.
    for n in [3, 11] {
        let mut r = made[n].clone();
        r.tags.push("finished".into());
        library::save(&store, &r, "d-b", at(100)).unwrap();
    }
    let new = create(&store, "Game 30", &["steam:30"], "pc", "d-b", 101);
    let (library, c) = refresh(&store, &mut cache);
    assert_eq!(requests(c), (1, 3));
    assert_eq!(library.records.len(), 31);
    assert_eq!(library::get(&library.records, &made[3].id).unwrap().tags, ["finished"]);
    assert_eq!(library::get(&library.records, &new.id), Some(&new));

    // The cache is what a device keeps on disk between runs.
    let mut restored: LibraryCache = serde_json::from_slice(&serde_json::to_vec(&cache).unwrap()).unwrap();
    assert_eq!(restored, cache);
    let (same, c) = refresh(&store, &mut restored);
    assert_eq!(requests(c), (1, 0));
    assert_eq!(same, library);

    // An empty library is one request too.
    let (empty, c) = refresh(&Memory::new(), &mut LibraryCache::default());
    assert_eq!(requests(c), (1, 0));
    assert!(empty.records.is_empty() && empty.skipped.is_empty());
}

#[test]
fn a_deleted_record_leaves_the_cache() {
    let store = Memory::new();
    let hades = create(&store, "Hades", &["steam:1145360"], "pc", "d-a", 0);
    let celeste = create(&store, "Celeste", &["steam:504230"], "pc", "d-a", 1);
    let mut cache = LibraryCache::default();
    let (library, _) = refresh(&store, &mut cache);
    assert_eq!(library.records.len(), 2);

    // Removed on another device.
    store.remove(&v2::game_record_path(&hades.id)).unwrap();
    let (library, c) = refresh(&store, &mut cache);
    assert_eq!(requests(c), (1, 0));
    assert_eq!(ids_of(&library), [celeste.id.as_str()]);
    assert_eq!(cache.records.keys().collect::<Vec<_>>(), [&format!("{}.json", celeste.id)]);

    // Put back with other content: read again, not taken from memory.
    let mut back = hades.clone();
    back.name = "Hades II".into();
    store.put(&v2::game_record_path(&hades.id), &serde_json::to_vec(&back).unwrap());
    let (library, c) = refresh(&store, &mut cache);
    assert_eq!(requests(c), (1, 1));
    assert_eq!(library::get(&library.records, &hades.id).unwrap().name, "Hades II");
}

#[test]
fn files_that_are_not_records_are_skipped_and_reported() {
    let store = Memory::new();
    let hades = create(&store, "Hades", &["steam:1145360"], "pc", "d-a", 0);
    let good = serde_json::to_vec(&hades).unwrap();

    // Not named like a record: never read.
    store.put("library/notes.txt", b"hello");
    store.put("library/hades.json.lntmp", &good);
    store.put("library/.json", &good);
    store.put("library/trash/old.json", &good);
    // Named like one, but damaged, not a record, or about another game.
    store.put("library/broken.json", b"{ \"id\": \"broken\", \"na");
    store.put("library/index.json", b"{ \"v\": 1, \"games\": [] }");
    store.put("library/copy-of-hades.json", &good);

    let mut cache = LibraryCache::default();
    let (library, c) = refresh(&store, &mut cache);
    assert_eq!(requests(c), (1, 4), "the record and the three files named like one");
    assert_eq!(ids_of(&library), [hades.id.as_str()]);
    assert_eq!(library.skipped, [".json", "broken.json", "copy-of-hades.json", "hades.json.lntmp", "index.json", "notes.txt"]);

    // They are reported every time and fetched once.
    let (again, c) = refresh(&store, &mut cache);
    assert_eq!(requests(c), (1, 0));
    assert_eq!(again, library);
    let mut restored: LibraryCache = serde_json::from_slice(&serde_json::to_vec(&cache).unwrap()).unwrap();
    assert_eq!(requests(refresh(&store, &mut restored).1), (1, 0));

    // A newer build may understand what this one couldn't: it looks again.
    restored.app_version = "0.0.1".into();
    let (again, c) = refresh(&store, &mut restored);
    assert_eq!(requests(c), (1, 3));
    assert_eq!(again, library);

    // Repaired: one read, and it is a card.
    let fixed = GameRecord { id: "broken".into(), name: "Fixed".into(), ..Default::default() };
    store.put("library/broken.json", &serde_json::to_vec(&fixed).unwrap());
    let (library, c) = refresh(&store, &mut cache);
    assert_eq!(requests(c), (1, 1));
    assert_eq!(ids_of(&library), ["broken", hades.id.as_str()]);
    assert!(!library.skipped.contains(&"broken.json".to_string()));

    // Saving never replaces a record this build can't read.
    let before = store.peek("library/index.json");
    let record = GameRecord { id: "index".into(), name: "Index".into(), ..Default::default() };
    assert!(library::save(&store, &record, "d-a", at(9)).is_err());
    assert_eq!(store.peek("library/index.json"), before);
    assert!(library::save(&store, &GameRecord::default(), "d-a", at(9)).is_err());
    assert!(library::save(&store, &GameRecord { id: "a/b".into(), ..Default::default() }, "d-a", at(9)).is_err());
}

#[test]
fn a_record_whose_places_lead_elsewhere_is_left_out() {
    let store = Memory::new();
    let hades = create(&store, "Hades", &["steam:1145360"], "pc", "d-a", 0);
    let mut cache = LibraryCache::default();
    // A record as `create_as` makes it, changed, put in the cloud as it is.
    let changed = |id: &str, change: &dyn Fn(&mut GameRecord)| {
        let mut r = library::new_record(id, &[], "pc", "d-a", at(1));
        r.id = id.into();
        r.folders = vec![id.into()];
        r.lines[0].path = v2::line_dir(id, library::MAIN_LINE);
        change(&mut r);
        store.put(&v2::game_record_path(id), &serde_json::to_vec(&r).unwrap());
    };
    // Each place a record names, pointing out of the game's own folders,
    // out of the cloud, or at a drive.
    changed("up", &|r| r.folders = vec!["..".into()]);
    changed("two", &|r| r.folders = vec!["a/b".into()]);
    changed("drive", &|r| r.lines[0].path = "C:/Users/me/AppData".into());
    changed("back", &|r| r.lines[0].path = r"games\..\..\x".into());
    changed("out", &|r| r.lines[0].path = "games/out/../../../x".into());
    changed("abs", &|r| r.lines[0].path = "/etc/x".into());
    changed("also", &|r| r.lines[0].also = vec!["games/also/saves/main/".into()]);

    let (library, _) = refresh(&store, &mut cache);
    assert_eq!(ids_of(&library), [hades.id.as_str()]);
    assert_eq!(library.skipped, ["abs.json", "also.json", "back.json", "drive.json", "out.json", "two.json", "up.json"]);
    // Read on its own, it is an error for that game.
    assert!(library::read(&store, "out").is_err());
    assert!(library::read(&store, &hades.id).unwrap().is_some());

    // Put right, it is a card again.
    changed("out", &|_| {});
    let (library, _) = refresh(&store, &mut cache);
    assert!(ids_of(&library).contains(&"out"));
}

#[test]
fn saving_is_one_read_and_one_write() {
    let store = Memory::new();
    let record = library::new_record("Stardew Valley", &strings(&["steam:413150"]), "pc", "d-a", at(0));
    assert_eq!(record.rev, 0);

    // Creating is the same call: the read finds nothing.
    let (saved, cost) = save(&store, &record, "d-a", 1);
    assert_eq!(cost, (1, 1));
    assert_eq!((saved.rev, saved.updated_at, saved.updated_by.as_str()), (1, Some(at(1)), "d-a"));
    assert_eq!(saved.created_at, Some(at(0)));
    assert_eq!(store.paths(), [format!("library/{}.json", record.id)]);
    assert_eq!(in_cloud(&store, &record.id), serde_json::to_value(&saved).unwrap());
    assert_eq!(library::read(&store, &record.id).unwrap(), Some(saved.clone()));
    assert_eq!(library::read(&store, "no-such-game").unwrap(), None);

    // A change.
    let mut edit = saved.clone();
    edit.name = "Stardew".into();
    edit.tags.push("finished".into());
    let (second, cost) = save(&store, &edit, "d-b", 2);
    assert_eq!(cost, (1, 1));
    assert_eq!((second.rev, second.updated_at, second.updated_by.as_str()), (2, Some(at(2)), "d-b"));
    assert_eq!((second.name.as_str(), second.tags.clone()), ("Stardew", strings(&["finished"])));

    // Nothing to change: nothing written, so no other device reads the
    // record again for it.
    let (same, cost) = save(&store, &second, "d-a", 3);
    assert_eq!(cost, (1, 0));
    assert_eq!(same, second);
    // Also from a copy that is out of date and has nothing new.
    let (same, cost) = save(&store, &saved, "d-a", 3);
    assert_eq!(cost, (1, 0));
    assert_eq!(same, second);
}

#[test]
fn two_devices_changing_one_record_lose_nothing_from_its_lists() {
    let store = Memory::new();
    let made = create(&store, "Stardew Valley", &["steam:413150"], "pc", "d-a", 0);

    // Both read the same version, then each changes its own copy.
    let mut a = made.clone();
    a.name = "Stardew Valley (co-op)".into();
    a.ids.push("xbox:ConcernedApe.StardewValleyPC".into());
    a.tags.push("finished".into());
    a.extra.insert("collection".into(), json!({ "shelf": 2 }));

    let mut b = made.clone();
    b.ids.push("nx:0100E65002BB8000".into());
    b.folders.push("stardew".into());
    b.tags.push("co-op".into());
    b.lines[0].also.push("games/stardew/snapshots".into());
    b.lines.push(Line {
        id: "solo-9d2k".into(),
        format: "pc".into(),
        scope: v2::SCOPE_INDEPENDENT.into(),
        name: Some("Kid's PC".into()),
        path: v2::line_dir(&made.id, "solo-9d2k"),
        ..Default::default()
    });
    b.lines[0].extra.insert("owner".into(), json!("family"));

    let (first, cost) = save(&store, &a, "d-a", 10);
    assert_eq!(cost, (1, 1));
    assert_eq!(first.rev, 2);
    // The second device's copy is out of date now. Still one read and one
    // write, and both devices' changes are there.
    let (both, cost) = save(&store, &b, "d-b", 11);
    assert_eq!(cost, (1, 1));
    assert_eq!((both.rev, both.updated_at, both.updated_by.as_str()), (3, Some(at(11)), "d-b"));
    assert_eq!(both.ids, strings(&["steam:413150", "xbox:ConcernedApe.StardewValleyPC", "nx:0100E65002BB8000"]));
    assert_eq!(both.folders, strings(&[made.id.as_str(), "stardew"]));
    assert_eq!(both.tags, strings(&["finished", "co-op"]));
    assert_eq!(both.lines.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["main", "solo-9d2k"]);
    assert_eq!(both.lines[0].also, strings(&["games/stardew/snapshots"]));
    assert_eq!(both.lines[0].extra["owner"], "family");
    assert_eq!(both.lines[1], b.lines[1]);
    assert_eq!(both.extra["collection"]["shelf"], 2);
    // The first device's rename is not undone by a copy that never saw it.
    assert_eq!(both.name, "Stardew Valley (co-op)");
    assert_eq!(in_cloud(&store, &made.id), serde_json::to_value(&both).unwrap());

    // In the other order the lists come out the same.
    let other = Memory::new();
    other.put(&v2::game_record_path(&made.id), &serde_json::to_vec(&made).unwrap());
    library::save(&other, &b, "d-b", at(10)).unwrap();
    let flipped = library::save(&other, &a, "d-a", at(11)).unwrap();
    let sorted = |list: &[String]| {
        let mut list = list.to_vec();
        list.sort();
        list
    };
    assert_eq!(sorted(&flipped.ids), sorted(&both.ids));
    assert_eq!(sorted(&flipped.folders), sorted(&both.folders));
    assert_eq!(sorted(&flipped.tags), sorted(&both.tags));
    assert_eq!(flipped.lines.len(), 2);
    assert_eq!(flipped.lines[0].also, both.lines[0].also);
    assert_eq!(flipped.extra, both.extra);
}

#[test]
fn something_is_taken_out_of_a_list_when_nobody_wrote_in_between() {
    let store = Memory::new();
    let mut made = create(&store, "Hades", &["steam:1145360", "gog:1"], "pc", "d-a", 0);
    made.tags = strings(&["backlog", "roguelike"]);
    let (made, _) = save(&store, &made, "d-a", 1);

    // The copy is the version in the cloud: what it lacks is gone.
    let mut edit = made.clone();
    edit.tags.retain(|t| t != "backlog");
    edit.ids.retain(|id| id != "gog:1");
    let (saved, cost) = save(&store, &edit, "d-a", 2);
    assert_eq!(cost, (1, 1));
    assert_eq!((saved.tags.clone(), saved.ids.clone()), (strings(&["roguelike"]), strings(&["steam:1145360"])));
    assert_eq!(saved.rev, 3);

    // Another device wrote meanwhile: its lists and this copy's are joined,
    // so the tag this copy took out is back, and the returned record says so.
    let mut theirs = saved.clone();
    theirs.tags.push("finished".into());
    let (theirs, _) = save(&store, &theirs, "d-b", 3);
    let mut edit = saved.clone();
    edit.tags.clear();
    let (merged, _) = save(&store, &edit, "d-a", 4);
    assert_eq!(merged.tags, strings(&["roguelike", "finished"]));
    assert_eq!(merged, theirs, "nothing new to write");

    // From the record as it is now, it works.
    let mut edit = merged.clone();
    edit.tags.clear();
    let (saved, _) = save(&store, &edit, "d-a", 5);
    assert!(saved.tags.is_empty());
    assert!(in_cloud(&store, &made.id).get("tags").is_none());
}

/// The example of section 3.3, with its dates filled in.
fn example() -> Value {
    json!({
        "id": "stardew-valley-7k3f",
        "name": "Stardew Valley",
        "ids": ["steam:413150", "nx:0100E65002BB8000", "ludusavi:stardew valley",
                "xbox:ConcernedApe.StardewValleyPC", "rule:stardew-valley"],
        "rule": "stardew-valley",
        "folders": ["stardew-valley-7k3f"],
        "lines": [
            { "id": "main", "format": "rule:stardew-valley", "scope": "shared",
              "path": "games/stardew-valley-7k3f/saves/main" },
            { "id": "solo-9d2k", "format": "rule:stardew-valley", "scope": "independent",
              "name": "孩子的电脑", "path": "games/stardew-valley-7k3f/saves/solo-9d2k" }
        ],
        "tags": ["finished"], "notes": "",
        "createdAt": "2026-11-20T08:00:00Z", "rev": 12,
        "updatedAt": "2026-11-26T10:00:00Z", "updatedBy": "d-01j"
    })
}

#[test]
fn the_example_record_of_the_design_comes_back_as_it_was() {
    // An empty note is not written; it means the same as none.
    let mut written = example();
    written.as_object_mut().unwrap().remove("notes");

    let store = Memory::new();
    store.put("library/stardew-valley-7k3f.json", &serde_json::to_vec(&example()).unwrap());
    let (library, _) = refresh(&store, &mut LibraryCache::default());
    let record = library.records[0].clone();
    assert!(record.extra.is_empty() && record.lines.iter().all(|l| l.extra.is_empty()), "every field is known");
    assert_eq!(serde_json::to_value(&record).unwrap(), written);

    // Every id but none is a name, and each one finds the game.
    for id in &record.ids {
        assert!(library::is_strong(id), "{id}");
        assert_eq!(library::find(&library.records, std::slice::from_ref(id)), Match::Record(&record));
    }
    assert_eq!(library::find(&library.records, &[library::name_id("STARDEW VALLEY!").unwrap()]), Match::Suggestions(vec![&record]));

    // Through a save: only what the save is for changes.
    let mut edit = record.clone();
    edit.notes = "co-op farm".into();
    library::save(&store, &edit, "d-02k", at(0)).unwrap();
    let mut expected = example();
    expected["notes"] = json!("co-op farm");
    expected["rev"] = json!(13);
    expected["updatedAt"] = json!(at(0));
    expected["updatedBy"] = json!("d-02k");
    assert_eq!(in_cloud(&store, "stardew-valley-7k3f"), expected);
}

#[test]
fn a_record_from_a_later_version_keeps_its_fields_through_a_save() {
    let mut later = example();
    later["collection"] = json!({ "shelf": 2, "order": [3, 1] });
    later["lines"][0]["owner"] = json!("family");
    later["lines"][1]["scope"] = json!("group:kids");
    later["lines"].as_array_mut().unwrap().push(json!({
        "id": "vr", "format": "holo:next", "scope": "shared",
        "path": "games/stardew-valley-7k3f/saves/vr", "codec": { "v": 3 }
    }));
    later["ids"].as_array_mut().unwrap().push(json!("holo:9000"));
    later.as_object_mut().unwrap().remove("notes");

    let store = Memory::new();
    store.put("library/stardew-valley-7k3f.json", &serde_json::to_vec(&later).unwrap());
    let (library, _) = refresh(&store, &mut LibraryCache::default());
    assert!(library.skipped.is_empty());
    let record = library.records[0].clone();
    assert!(library::is_strong("holo:9000"));

    // This build adds a tag; a device with the later version changes its
    // own field at the same time and saves first.
    let mut theirs = later.clone();
    theirs["collection"]["shelf"] = json!(5);
    theirs["rev"] = json!(13);
    theirs["updatedAt"] = json!(at(1));
    store.put("library/stardew-valley-7k3f.json", &serde_json::to_vec(&theirs).unwrap());
    let mut edit = record.clone();
    edit.tags.push("co-op".into());
    library::save(&store, &edit, "d-02k", at(2)).unwrap();

    let mut expected = theirs.clone();
    expected["tags"] = json!(["finished", "co-op"]);
    expected["rev"] = json!(14);
    expected["updatedAt"] = json!(at(2));
    expected["updatedBy"] = json!("d-02k");
    assert_eq!(in_cloud(&store, "stardew-valley-7k3f"), expected);
}

#[test]
fn a_game_made_on_two_devices_becomes_one_card() {
    let store = Memory::new();
    // Both offline: the Steam version on the desktop, the Switch version
    // through the other PC. The rule both recognised is what they share.
    let format = "rule:stardew-valley";
    let desktop = create_as(&store, "stardew-valley-7k3f", "Stardew Valley", &["steam:413150", "rule:stardew-valley"], format, "d-a", 0);
    let laptop = create_as(&store, "stardew-valley-x2m9", "Stardew Valley", &["nx:0100E65002BB8000", "rule:stardew-valley"], format, "d-b", 5);
    let hades = create(&store, "Hades", &["steam:1145360"], "pc", "d-a", 6);
    // Each has played: a snapshot in its own folder.
    store.put(&v2::manifest_path(&desktop.lines[0].path, "01JTEST0000000000000000001"), b"{}");
    store.put(&v2::manifest_path(&laptop.lines[0].path, "01JTEST0000000000000000002"), b"{}");
    let files = store.paths();

    let mut cache = LibraryCache::default();
    let (library, _) = refresh(&store, &mut cache);
    assert_eq!(library::live(&library.records).count(), 3);
    let mut found = library::dedupe(&library.records);
    assert_eq!(found.len(), 1);
    let duplicate = found.remove(0);
    assert_eq!(duplicate.survivor.id, desktop.id, "the record made first stays");
    assert_eq!(duplicate.losers.len(), 1);

    // Two records to write, the survivor first.
    let (survivor, cost) = save(&store, &duplicate.survivor, "d-b", 10);
    assert_eq!(cost, (1, 1));
    let (_, cost) = save(&store, &duplicate.losers[0], "d-b", 11);
    assert_eq!(cost, (1, 1));
    // Only records changed: no file was moved, added or deleted.
    assert_eq!(store.paths(), files);

    // What every device sees from now on.
    let (library, c) = refresh(&store, &mut cache);
    assert_eq!(requests(c), (1, 2));
    let cards: Vec<&GameRecord> = library::live(&library.records).collect();
    assert_eq!(cards.len(), 2);
    assert_eq!(library.records.len(), 3);
    let card = library::resolve(&library.records, &laptop.id).unwrap();
    assert_eq!(card, &survivor);
    assert_eq!(library::resolve(&library.records, &hades.id), Some(&hades));
    assert_eq!(card.ids, strings(&["steam:413150", "rule:stardew-valley", "nx:0100E65002BB8000"]));
    assert_eq!(card.folders, [desktop.id.clone(), laptop.id.clone()]);

    // One line, in the survivor's folder, that also reads the other's:
    // where both have progress, reading it shows a fork like any other.
    assert_eq!(card.lines.len(), 1);
    assert_eq!(card.lines[0].path, desktop.lines[0].path);
    assert_eq!(card.lines[0].also, [laptop.lines[0].path.clone()]);
    // The laptop still knows its line by the old record.
    let (game, line) = library::resolve_line(&library.records, &laptop.id, "main").unwrap();
    assert_eq!((game.id.as_str(), line), (desktop.id.as_str(), &card.lines[0]));

    // The ids of either version find the one card.
    assert_eq!(library::find(&library.records, &strings(&["nx:0100e65002bb8000"])), Match::Record(card));
    assert_eq!(library::find(&library.records, &strings(&["steam:413150"])), Match::Record(card));
    assert!(library::dedupe(&library.records).is_empty());

    // A device that found the same duplicate at the same time writes
    // nothing more.
    let (_, cost) = save(&store, &duplicate.survivor, "d-a", 12);
    assert_eq!(cost, (1, 0));
    let (_, cost) = save(&store, &duplicate.losers[0], "d-a", 12);
    assert_eq!(cost, (1, 0));
}

#[test]
fn a_duplicate_found_while_the_other_record_changes_is_finished_later() {
    let store = Memory::new();
    let old = create_as(&store, "hades-2b9d", "Hades", &["steam:1145360"], "pc", "d-a", 0);
    let new = create_as(&store, "hades-8m1x", "Hades", &["steam:1145360"], "pc", "d-b", 5);
    let (library, _) = refresh(&store, &mut LibraryCache::default());
    let duplicate = library::dedupe(&library.records).remove(0);

    // Before the result is written, the newer record's device adds a tag.
    let mut edit = new.clone();
    edit.tags.push("finished".into());
    library::save(&store, &edit, "d-b", at(6)).unwrap();

    library::save(&store, &duplicate.survivor, "d-a", at(7)).unwrap();
    let late = library::save(&store, &duplicate.losers[0], "d-a", at(8)).unwrap();
    // The cloud's version is the later one, so its (missing) `mergedInto`
    // stayed: still two cards, both reading the same folders.
    assert_eq!(late.merged_into, None);
    let (library, _) = refresh(&store, &mut LibraryCache::default());
    assert_eq!(library::live(&library.records).count(), 2);

    // The next refresh finds it again and finishes, taking the tag along.
    let duplicate = library::dedupe(&library.records).remove(0);
    library::save(&store, &duplicate.survivor, "d-a", at(9)).unwrap();
    library::save(&store, &duplicate.losers[0], "d-a", at(10)).unwrap();
    let (library, _) = refresh(&store, &mut LibraryCache::default());
    let cards: Vec<&GameRecord> = library::live(&library.records).collect();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].id, old.id);
    assert_eq!(cards[0].tags, ["finished"]);
    assert_eq!(cards[0].lines.len(), 1);
    assert_eq!(cards[0].lines[0].also, [new.lines[0].path.clone()]);
    assert_eq!(library::resolve(&library.records, &new.id).unwrap().id, old.id);
}

/// One card, but the caller isn't sure the two versions save the same files:
/// their histories stay apart.
#[test]
fn two_records_of_one_game_can_keep_their_lines_apart() {
    let store = Memory::new();
    let steam = create_as(&store, "hades", "Hades", &["ludusavi:hades"], "pc", "d-a", 0);
    let xbox = create_as(&store, "hades-2", "Hades", &["ludusavi:hades", "xbox:supergiant.hades"], "pc", "d-b", 5);
    let mut cache = LibraryCache::default();
    let (library, _) = refresh(&store, &mut cache);

    let never = |_: &Line, _: &Line| false;
    let duplicate = library::dedupe_with(&library.records, &never).remove(0);
    assert_eq!(duplicate.survivor.id, steam.id);
    assert_eq!(duplicate.losers[0].merged_into.as_deref(), Some("hades"));
    let lines = &duplicate.survivor.lines;
    assert_eq!(lines.len(), 2);
    assert_eq!((lines[0].id.as_str(), lines[0].path.as_str()), ("main", steam.lines[0].path.as_str()));
    assert!(lines[0].also.is_empty());
    // The other line keeps its folder and gets an id that says where it is
    // from.
    assert_eq!((lines[1].id.as_str(), lines[1].path.as_str()), ("main-hades-2", xbox.lines[0].path.as_str()));
    assert_eq!(duplicate.survivor.folders, strings(&["hades", "hades-2"]));

    // An install that was on the other record finds its line again.
    save(&store, &duplicate.survivor, "d-a", 10);
    save(&store, &duplicate.losers[0], "d-a", 11);
    let (library, _) = refresh(&store, &mut cache);
    let (record, line) = library::resolve_line(&library.records, "hades-2", "main").unwrap();
    assert_eq!((record.id.as_str(), line.id.as_str()), ("hades", "main-hades-2"));
    // Nothing left to do, whoever looks and whatever they would answer.
    assert!(library::dedupe(&library.records).is_empty());

    // Asked the other way, it is one history that reads both folders.
    let store = Memory::new();
    let steam = create_as(&store, "hades", "Hades", &["ludusavi:hades"], "pc", "d-a", 0);
    let xbox = create_as(&store, "hades-2", "Hades", &["ludusavi:hades"], "pc", "d-b", 5);
    let (library, _) = refresh(&store, &mut LibraryCache::default());
    let joined = library::dedupe(&library.records).remove(0).survivor;
    assert_eq!(joined.lines.len(), 1);
    assert_eq!(joined.lines[0].path, steam.lines[0].path);
    assert_eq!(joined.lines[0].also, [xbox.lines[0].path.clone()]);
}
