// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { SnapshotSummary } from "./SnapshotSummary";

/**
 * The newest version of a save line of the game that this device does
 * not follow.
 */
export type OtherSave = { 
/**
 * [`LineInfo::id`].
 */
line: string, 
/**
 * What a device called the line it keeps to itself; empty for the
 * shared line.
 */
name: string, shared: boolean, snapshot: SnapshotSummary, };
