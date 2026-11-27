// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type BackupJob = { 
/**
 * backup | restore | delete
 */
kind: "backup" | "restore" | "delete", phase: string, done: number, total: number, uploaded: number, elapsedSecs: number, 
/**
 * Where a restore goes.
 */
target: string | null, };
