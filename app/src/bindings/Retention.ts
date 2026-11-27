// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type Retention = { 
/**
 * Always keep this many newest automatic snapshots.
 */
keepRecent: number, 
/**
 * Additionally keep the newest snapshot of each day for this many days.
 */
keepDailyDays: number, };
