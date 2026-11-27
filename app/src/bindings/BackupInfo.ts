// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { BackupJob } from "./BackupJob";
import type { PartKind } from "./PartKind";
import type { PendingRestore } from "./PendingRestore";
import type { RestoreDefaults } from "./RestoreDefaults";
import type { VersionSummary } from "./VersionSummary";

export type BackupInfo = { hasCloud: boolean, cloudKind: string, 
/**
 * Baidu Netdisk / Quark: works but slow (show the warning).
 */
slow: boolean, 
/**
 * The program's folder (or the whole folder) on this PC.
 */
root: string | null, rootExists: boolean, rootCustom: boolean, exeMissing: boolean, 
/**
 * Newest first.
 */
versions: Array<VersionSummary>, 
/**
 * About how much this game's backups take in the cloud.
 */
storedBytes: number, 
/**
 * Versions kept after a backup; 0 = all.
 */
keep: number, 
/**
 * The user's own exclude patterns.
 */
exclude: Array<string>, defaultExclude: Array<string>, 
/**
 * The game's rule lists keys / firmware / BIOS files.
 */
firmwareAvailable: boolean, firmware: boolean, skip: Array<PartKind>, extraContent: Array<string>, 
/**
 * The game's rule describes its parts (emulators, recompilations);
 * otherwise the whole folder is backed up.
 */
byParts: boolean, job: BackupJob | null, pendingRestore: PendingRestore | null, restore: RestoreDefaults | null, 
/**
 * Problem reaching the cloud (the local part is still filled in).
 */
error: string | null, };
