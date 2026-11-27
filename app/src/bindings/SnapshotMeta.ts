// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { FileEntry } from "./FileEntry";
import type { Trigger } from "./Trigger";

export type SnapshotMeta = { id: string, game: string, parent: string | null, deviceId: string, deviceName: string, createdAt: string, trigger: Trigger, 
/**
 * Absolute root folders on the device that made the snapshot.
 */
roots: { [key in string]: string }, contentHash: string, files: Array<FileEntry>, totalSize: number, appVersion: string, comment: string, locked: boolean, 
/**
 * The game version that wrote these saves, read from the saves where a
 * rule says how (`rules/games/`), e.g. `1.6.15`.
 */
gameVersion?: string | null, 
/**
 * The version of the game installed on the device that made the
 * snapshot: an emulated game's update, a recompilation's release, an
 * Xbox app package's version, a program's own version.
 */
installedVersion?: string | null, };
