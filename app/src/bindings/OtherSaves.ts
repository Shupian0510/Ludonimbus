// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { OtherSave } from "./OtherSave";
import type { PlatformMark } from "./PlatformMark";

export type OtherSaves = { saves: Array<OtherSave>, platforms: { [key in string]: PlatformMark }, 
/**
 * As [`SnapshotList::too_new`](super::saves::SnapshotList::too_new).
 */
tooNew?: Array<string>, };
