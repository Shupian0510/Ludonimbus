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

/// Every game in the real Ludonimbus config (read-only) that has save rules:
/// downloads the TrophyNX list and evaluates it against the real saves.
/// `cargo test -p ludonimbus-core --test achievements_live real_save_rules -- --ignored --nocapture`
#[test]
#[ignore]
fn real_save_rules() {
    let real = std::env::var("APPDATA").map(|a| PathBuf::from(a).join("Ludonimbus").join("config.json")).unwrap();
    let cfg_real: AppConfig = serde_json::from_slice(&std::fs::read(&real).unwrap()).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path());
    for g in &cfg_real.games {
        let Some(tid) = achievements::rules::title_id(g) else { continue };
        let list = match achievements::rules::load_list(&tid, true) {
            Ok(Some(l)) => l,
            other => {
                eprintln!("{}: {tid}: no list ({:?})", g.key, other.err());
                continue;
            }
        };
        let files = achievements::rules::find_save_files(g, &list.save_files);
        let unlocks = achievements::rules::evaluate(&list, &files, achievements::rules::Stats::default()).unwrap();
        eprintln!("{}: {} ({tid}), reader {}, {} achievements, {} save files, {} unlocked", g.key, list.name, list.reader, list.achievements.len(), files.len(), unlocks.len());
        for u in unlocks.iter().take(6) {
            let name = list.achievements.iter().find(|a| a.id == u.id).map(|a| a.name.as_str()).unwrap_or("?");
            eprintln!("  {} ({})", name, u.id);
        }
    }
}

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
    // The repository has no trophy key: use the one this PC already has
    // (shadPS4's TrophyKey or the Ludonimbus setting), or check numbers only.
    let key = achievements::trp::trophy_key();
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path());
    if let Some((k, _)) = key {
        let hex: String = k.iter().map(|b| format!("{b:02X}")).collect();
        achievements::update_settings(|s| s.ps4_trophy_key = Some(hex)).unwrap();
    }
    eprintln!("trophy key: {:?}", key.map(|(_, from)| from));

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
    let named = v.items.iter().filter(|i| i.name != achievements::placeholder(&i.id).name).count();
    if key.is_some() {
        assert!(v.total == 0 || named > 0, "a trophy key is set but no names came out");
    } else {
        eprintln!("no trophy key: names skipped ({named} named)");
    }
    eprintln!("watch files: {:?}", achievements::trophy::watch_files(&achievements::settings().games["live"]));
}
