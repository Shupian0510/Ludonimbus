// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { SnapshotSummary } from "./SnapshotSummary";

/**
 * Another version of a game whose newest saves are newer than anything
 * this version has.
 */
export type NewerVersion = { key: string, name: string, 
/**
 * What it runs on ([`version_label`]).
 */
label: string, snapshot: SnapshotSummary, };
