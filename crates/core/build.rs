//! Builds the game rules (`rules/games/*.yaml`) into the program: their file
//! names and text, for `crosssaves::rules`. A new file is picked up at the
//! next build. Without the folder the build fails: the rules are part of the
//! source, so a checkout without them is incomplete.

use std::path::PathBuf;
use std::{env, fs};

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets it")).join("../../rules/games");
    println!("cargo:rerun-if-changed={}", dir.display());
    let unreadable = |e: std::io::Error| format!("the game rules in {} can't be read ({e}); is the checkout complete?", dir.display());
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}", unreadable(e)))
        .map(|e| e.unwrap_or_else(|e| panic!("{}", unreadable(e))).path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "yaml"))
        .collect();
    files.sort();
    let mut out = String::from("/// The game rules in `rules/games/`: file name and text.\npub const GAME_RULES: &[(&str, &str)] = &[\n");
    for f in &files {
        let name = f.file_name().expect("a file").to_string_lossy();
        out.push_str(&format!("    ({name:?}, include_str!({:?})),\n", f.display().to_string()));
    }
    out.push_str("];\n");
    fs::write(PathBuf::from(env::var("OUT_DIR").expect("cargo sets it")).join("game_rules.rs"), out).expect("write game_rules.rs");
}
