// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { PlatformMark } from "./PlatformMark";
import type { SnapshotSummary } from "./SnapshotSummary";
import type { UnsentSave } from "./UnsentSave";

export type SnapshotList = { snapshots: Array<SnapshotSummary>, headId: string | null, lastSyncedId: string | null, 
/**
 * How to show each platform the versions name.
 */
platforms: { [key in string]: PlatformMark }, 
/**
 * The versions saved on another platform by a newer game than the one
 * that wrote the saves here, which the game's rule says an older game
 * can't load ([`sync::too_new`]).
 */
tooNew?: Array<string>, 
/**
 * Versions made on this PC that couldn't be uploaded complete
 * ([`sync::unsent`]): listed once, as this PC's, until dismissed.
 */
unsent?: Array<UnsentSave>, };
