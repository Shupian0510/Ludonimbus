// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type Health = { "state": "notLinked" } | { "state": "disabled" } | { "state": "ok" } | { "state": "shortcutMissing", candidateAppid: number | null, } | { "state": "changed", exe: string, launchOptions: string, } | { "state": "runnerMoved", exe: string, } | { "state": "noSteam" };
