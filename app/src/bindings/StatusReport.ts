// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { SnapshotSummary } from "./SnapshotSummary";
import type { Status } from "./Status";

export type StatusReport = { status: Status, message: string | null, localFiles: number, localSize: number, localHash: string | null, head: SnapshotSummary | null, snapshotCount: number, lastSyncedId: string | null, lastSyncAt: string | null, pending: number, checkedAt: string, 
/**
 * The game version that saved `head` on another platform, when that is
 * newer than the one that wrote the saves here and the game's rule says
 * an older game can't load it ([`too_new`]): a sync nobody chose
 * doesn't take it, and the app says so before the user does.
 */
tooNew?: string | null, 
/**
 * The line's newest version is marked as missing a content pack: no
 * device takes it until the pack is back ([`cloud::saves::mark`]), and
 * `status` is worked out without it.
 */
incomplete?: boolean, };
