// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type BackupFinished = { key: string, 
/**
 * As [`JobView::kind`].
 */
kind: "backup" | "restore" | "delete", ok: boolean, cancelled: boolean, message: string, 
/**
 * What the job reported, a different shape for each kind: a backup's
 * `backup::Outcome`, a restore's report and path fixes (`json!`), a
 * deletion's clean-up.
 */
outcome: unknown, };
