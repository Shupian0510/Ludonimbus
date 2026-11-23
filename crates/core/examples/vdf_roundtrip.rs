//! Diagnostic: check that a binary VDF file (e.g. shortcuts.vdf) round-trips
//! byte-for-byte through Ludonimbus' parser. Read-only.
//!
//! cargo run -p ludonimbus-core --example vdf_roundtrip -- <path>
fn main() {
    let path = std::env::args().nth(1).expect("usage: vdf_roundtrip <file>");
    let bytes = std::fs::read(&path).expect("read");
    let doc = ludonimbus_core::vdf::parse(&bytes).expect("parse");
    let out = ludonimbus_core::vdf::write(&doc);
    println!("{} bytes, round-trip identical: {}", bytes.len(), out == bytes);
    std::process::exit(if out == bytes { 0 } else { 1 });
}
