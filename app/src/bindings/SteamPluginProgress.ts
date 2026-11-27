// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type SteamPluginProgress = { 
/**
 * "download" | "install"
 */
stage: "download" | "install", done: number, total: number, };
