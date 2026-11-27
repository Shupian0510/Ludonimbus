// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { SnapshotSummary } from "./SnapshotSummary";

/**
 * A version the user deleted: hidden from the history on every device and
 * restorable until it is purged [`TRASH_DAYS`] after `deleted_at`.
 */
export type TrashedSnapshot = { snapshot: SnapshotSummary, deletedAt: string, 
/**
 * Name of the device it was deleted on.
 */
deletedBy: string, 
/**
 * Why, when the user didn't delete it: `incomplete` for a version that
 * missed a content pack nobody could put back
 * ([`saves::retire`]).
 */
reason?: string | null, };
