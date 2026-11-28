// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { SaveSourceKind } from "./SaveSourceKind";
import type { SaveTake } from "./SaveTake";
import type { SnapshotSummary } from "./SnapshotSummary";

/**
 * One source of saves of a game besides the line this install follows.
 */
export type SaveSource = { 
/**
 * Unique among a game's sources: `line:<line id>` or `version:<key>`.
 */
id: string, kind: SaveSourceKind, 
/**
 * What it is called, in the interface language: "Xbox 版", "Ryujinx",
 * "孩子的电脑"…
 */
name: string, 
/**
 * What its newest save was made on ([`sync::platform`]): which
 * program's icon its group shows.
 */
platform: string, 
/**
 * The line to take from ([`sync::take_from`]), for a line of the card
 * that holds the same files.
 */
line?: string, 
/**
 * The version to bring saves from ([`transfer::apply`]), for another
 * version of the game.
 */
key?: string, 
/**
 * The devices its listed saves were made on, the newest first.
 */
devices: Array<string>, 
/**
 * Its newest save first (`head`, when it has one that isn't missing a
 * pack), then the ones kept there, newest first.
 */
snapshots: Array<SnapshotSummary>, take: SaveTake, };
