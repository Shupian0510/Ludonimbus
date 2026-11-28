// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { SaveSource } from "./SaveSource";
import type { SnapshotSummary } from "./SnapshotSummary";

/**
 * Newer progress in another version of the game than the save here.
 */
export type NewerElsewhere = { source: SaveSource, 
/**
 * Its newest save: the progress offered.
 */
snapshot: SnapshotSummary, 
/**
 * The save this device is at, as far as it knows; `None` without one.
 */
here: SnapshotSummary | null, 
/**
 * What this version is called ([`version_name`]).
 */
hereName: string, };
