// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { NewerElsewhere } from "./NewerElsewhere";
import type { PlatformMark } from "./PlatformMark";
import type { SaveSource } from "./SaveSource";

export type SaveSources = { sources: Array<SaveSource>, 
/**
 * How to show each platform the sources and their saves name, this
 * version's own too.
 */
platforms: { [key in string]: PlatformMark }, 
/**
 * As [`SnapshotList::too_new`](super::saves::SnapshotList::too_new).
 */
tooNew?: Array<string>, 
/**
 * The newest progress another version has, when it is newer than the
 * save here and the user hasn't waved that save off.
 */
newer?: NewerElsewhere, 
/**
 * What this version is called and runs on, for the group of the saves
 * here.
 */
hereName: string, herePlatform: string, };
