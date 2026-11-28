//! The TypeScript types of what the web UI and this app send each other:
//! the arguments and results of the commands, and the payloads of the
//! events. They are generated from the Rust types with ts-rs into
//! `app/src/bindings/`, one file per type, and committed; the web UI imports
//! them from there (`app/src/api.ts` and the other `*Api.ts`).
//!
//! `cargo test -p ludonimbus-app export_bindings` writes them again, and CI
//! fails when that changes them. A type a new command takes or returns gets
//! `#[derive(TS)]` next to its `Serialize` and goes into [`roots`]; the types
//! inside it come along by themselves, and need the derive too. Large
//! integers (`u64`, `i64`) are `number`: the JSON is a JavaScript number.
//! See docs/development.md.

use crate::{achievements, commands, gamebackup, handheld, library, nx, savelibrary, steam_plugin, updater, watcher};
use ludonimbus_core::achievements as ach;
use ludonimbus_core::gamebackup as gb;
use ludonimbus_core::{cloud, config, discover, join, savefind, savelib, screenshots, snapshot, steamcloud, steamgriddb, sync, transfer};
use std::any::{type_name, TypeId};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use ts_rs::{Config, TypeVisitor, TS};

/// The first line of every generated file.
const HEADER: &str =
    "// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).";

/// Every type a command takes or returns (by the area of main.rs's list),
/// and every event payload that is a Rust type.
fn roots(v: &mut impl TypeVisitor) {
    // The library page
    v.visit::<commands::overview::Overview>();
    v.visit::<commands::overview::CollectionStats>();
    // Saves and their versions
    v.visit::<sync::StatusReport>();
    v.visit::<sync::Outcome>();
    v.visit::<commands::saves::SnapshotList>();
    v.visit::<snapshot::SnapshotMeta>();
    v.visit::<cloud::TrashedSnapshot>();
    v.visit::<commands::saves::LocalFiles>();
    // Save lines
    v.visit::<sync::LineInfo>();
    v.visit::<join::Suggestion>();
    v.visit::<commands::lines::LineList>();
    v.visit::<commands::lines::OtherSaves>();
    v.visit::<commands::lines::ShareOffer>();
    v.visit::<commands::saves::UploadResult>();
    // Adding and changing games
    v.visit::<cloud::CloudGame>();
    v.visit::<commands::games::GamePatch>();
    // Steam
    v.visit::<commands::steam::AccountsView>();
    v.visit::<commands::steam::ShortcutView>();
    v.visit::<steamcloud::StoreApp>();
    v.visit::<steamcloud::Done>();
    v.visit::<steamcloud::StoreStatus>();
    // Settings
    v.visit::<commands::settings::SettingsView>();
    v.visit::<commands::settings::SettingsPatch>();
    v.visit::<config::Onboarding>();
    // The cloud
    v.visit::<cloud::local::SyncFolder>();
    v.visit::<config::CloudConfig>();
    v.visit::<cloud::AccountInfo>();
    v.visit::<cloud::migrate::Report>();
    v.visit::<commands::cloud::OpenListInfo>();
    v.visit::<cloud::DeviceInfo>();
    // Screenshots
    v.visit::<commands::screenshots::GalleryView>();
    v.visit::<screenshots::Report>();
    // Scanning, finding saves, artwork, adding games to Steam
    v.visit::<discover::Found>();
    v.visit::<discover::GameFolder>();
    v.visit::<library::AdapterInfo>();
    v.visit::<savefind::Candidate>();
    v.visit::<library::SgdbStatus>();
    v.visit::<steamgriddb::SgdbGame>();
    v.visit::<steamgriddb::SgdbImage>();
    v.visit::<library::BatchResult>();
    // The Steam plugin, updates
    v.visit::<steam_plugin::PluginView>();
    v.visit::<steam_plugin::InstallResult>();
    v.visit::<updater::UpdateInfo>();
    // Game backups and the speed test
    v.visit::<gb::speedtest::SpeedReport>();
    v.visit::<gamebackup::Info>();
    v.visit::<gb::backup::PreviewComponent>();
    v.visit::<gamebackup::SettingsPatch>();
    v.visit::<gb::restore::Targets>();
    v.visit::<gamebackup::JobView>();
    // The save library
    v.visit::<savelibrary::CloudLibrary>();
    v.visit::<savelibrary::CloudGameInfo>();
    v.visit::<savelibrary::InstallView>();
    v.visit::<transfer::Plan>();
    // Handheld mode, the Switch
    v.visit::<handheld::LaunchInfo>();
    v.visit::<handheld::HandheldInfo>();
    v.visit::<config::HandheldMode>();
    v.visit::<savelib::Newer>();
    v.visit::<nx::NxStatus>();
    // Achievements
    v.visit::<ach::GameView>();
    v.visit::<ach::detect::Candidate>();
    v.visit::<ach::Binding>();
    v.visit::<achievements::AchievementSettings>();
    v.visit::<ach::retro::Profile>();
    v.visit::<ach::retro::RaGame>();
    v.visit::<achievements::Toast>();
    // Event payloads. `openlist-progress` and `game-backup-progress` are
    // made with `json!`, and typed where the UI listens to them.
    v.visit::<watcher::SyncEvent>();
    v.visit::<cloud::migrate::Progress>();
    v.visit::<cloud::baidu::DeviceCode>();
    v.visit::<gb::speedtest::SpeedProgress>();
    v.visit::<gamebackup::Finished>();
    v.visit::<steam_plugin::Progress>();
    v.visit::<updater::Progress>();
}

/// Writes a type and the types inside it.
struct Export<'a>(&'a Config);

impl TypeVisitor for Export<'_> {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if let Err(e) = T::export_all(self.0) {
            panic!("{}: {e}", type_name::<T>());
        }
    }
}

/// The file each type goes to. ts-rs writes two types with the same
/// TypeScript name to one file and keeps only the first, so a second one is
/// an error here.
#[derive(Default)]
struct Files {
    seen: HashSet<TypeId>,
    by_file: BTreeMap<PathBuf, &'static str>,
}

impl TypeVisitor for Files {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        let Some(file) = T::output_path() else { return };
        if !self.seen.insert(TypeId::of::<T>()) {
            return;
        }
        if let Some(other) = self.by_file.insert(file.clone(), type_name::<T>()) {
            panic!("{other} and {} would both be {}: give one of them #[ts(rename = \"…\")]", type_name::<T>(), file.display());
        }
        T::visit_dependencies(self);
    }
}

fn ts_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> =
        std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "ts")).collect();
    out.sort();
    out
}

#[test]
fn export_bindings() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("src").join("bindings");
    let mut files = Files::default();
    roots(&mut files);

    // From an empty folder, so a type that is gone leaves no file behind.
    for f in ts_files(&dir) {
        std::fs::remove_file(&f).unwrap();
    }
    let cfg = Config::new().with_out_dir(&dir).with_large_int("number");
    roots(&mut Export(&cfg));

    let mut names = Vec::new();
    for f in ts_files(&dir) {
        let text = std::fs::read_to_string(&f).unwrap();
        let (first, rest) = text.split_once('\n').unwrap();
        assert!(first.contains("ts-rs"), "{}: {first}", f.display());
        std::fs::write(&f, format!("{HEADER}\n{rest}")).unwrap();
        names.push(f.file_stem().unwrap().to_string_lossy().into_owned());
    }
    assert_eq!(names.len(), files.by_file.len(), "files written: {names:?}");
    let index: String = names.iter().map(|n| format!("export type * from \"./{n}\";\n")).collect();
    std::fs::write(dir.join("index.ts"), format!("{HEADER}\n{index}")).unwrap();
}
