// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { SnapshotSummary } from "./SnapshotSummary";

/**
 * Another version of a game whose newest saves are newer than anything
 * this version has.
 */
export type NewerVersion = { 
/**
 * The version the saves are of; this game's own key when they are on
 * another line of its card (`line`).
 */
key: string, name: string, 
/**
 * What it is called ([`crate::elsewhere::Source::name`]).
 */
label: string, snapshot: SnapshotSummary, 
/**
 * The line of the card they are on, when they are taken from it as
 * they are ([`crate::sync::take_from`]).
 */
line?: string | null, };
