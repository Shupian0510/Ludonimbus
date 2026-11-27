// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type MigrateReport = { 
/**
 * Games with at least one version in the old cloud.
 */
games: number, 
/**
 * Versions copied, and versions the new cloud had already.
 */
copied: number, skipped: number, bytes: number, };
