// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { Trigger } from "./Trigger";

export type SnapshotSummary = { id: string, parent: string | null, deviceId: string, deviceName: string, createdAt: string, trigger: Trigger, contentHash: string, totalSize: number, fileCount: number, comment: string, locked: boolean, gameVersion?: string | null, installedVersion?: string | null, 
/**
 * What the saves were made on, as the version says it (`pc`,
 * `pc:xbox`, `emu:<rule>`, `recomp:<rule>`, `console:switch`). Empty
 * where it isn't known (versions of 0.6).
 */
platform?: string, 
/**
 * A newest version of its save line (several when the line has forked).
 * Not set in lists read the way 0.6 reads them: there the newest
 * mainline snapshot is the head ([`crate::sync::mainline_head`]).
 */
head?: boolean, 
/**
 * Kept beside the history, never the newest version.
 */
side?: boolean, 
/**
 * Marked as missing a content pack in the cloud: never taken, and not
 * offered as the current one, until the pack is back.
 */
incomplete?: boolean, 
/**
 * How long the game had been played when it was made, in seconds
 * ([`crate::cloud::v2::PLAYTIME`]); where that is known.
 */
playtime?: number | null, 
/**
 * Where a copy came from ([`crate::cloud::v2::Origin::how`]: `restore`,
 * `copy`, `convert:…`, `share`); empty for saves made where they were
 * played.
 */
copied?: string, };
