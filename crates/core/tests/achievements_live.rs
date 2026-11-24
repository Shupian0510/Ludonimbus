//! Reads a real emulator's trophy data on this PC (read-only; Ludonimbus's
//! own files go to a temporary folder). Opt-in:
//!
//! ```powershell
//! $env:LUDONIMBUS_TROPHY_USER = "C:\…\BloodbornePC\user"   # Lantern / shadPS4 user folder
//! $env:LUDONIMBUS_TROPHY_SOURCE = "lantern"                # or shadps4
//! cargo test -p ludonimbus-core --test achievements_live -- --ignored --nocapture
//! ```

use ludonimbus_core::achievements::{self, detect, Binding, SourceKind};
use ludonimbus_core::config::{AppConfig, GameEntry};
use std::path::PathBuf;

#[test]
#[ignore]
fn real_trophy_folder() {
    let Ok(user) = std::env::var("LUDONIMBUS_TROPHY_USER") else {
        eprintln!("LUDONIMBUS_TROPHY_USER not set");
        return;
    };
    let user = PathBuf::from(user);
    let source = match std::env::var("LUDONIMBUS_TROPHY_SOURCE").as_deref() {
        Ok("shadps4") => SourceKind::Shadps4,
        _ => SourceKind::Lantern,
    };
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path());

    let exe = user.parent().unwrap().join("Bloodborne.exe");
    let game = GameEntry {
        key: "live".into(),
        name: "Live".into(),
        adapter: Some("bloodborne-lantern".into()),
        exe: exe.is_file().then_some(exe),
        ..Default::default()
    };
    for c in detect::candidates(&game) {
        eprintln!("candidate: {:?} {} {:?} sure={}", c.binding.source, c.binding.source_id, c.binding.path, c.sure);
    }
    let npwr = detect::candidates(&game).into_iter().next().map(|c| c.binding.source_id).unwrap_or_default();
    let binding = Binding { source, source_id: npwr, path: Some(user.clone()), popup: None };
    let mut cfg = AppConfig::default();
    cfg.device.name = "live".into();
    achievements::link(&cfg, &game, binding).unwrap();
    let v = achievements::view("live");
    eprintln!(
        "source={:?} id={:?} total={} unlocked={} icons={} error={:?}",
        v.source,
        v.source_id,
        v.total,
        v.unlocked,
        v.items.iter().filter(|i| i.has_icon).count(),
        v.last_error
    );
    for i in v.items.iter().take(5) {
        eprintln!("  {} {} unlocked={:?} icon={}", i.id, i.name, i.unlocked_at, i.has_icon);
    }
    eprintln!("watch files: {:?}", achievements::trophy::watch_files(&achievements::settings().games["live"]));
}
