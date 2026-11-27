// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { JoinFit } from "./JoinFit";
import type { SaveBind } from "./SaveBind";

/**
 * A save line a game on this device could share ([`suggestion`]).
 */
export type JoinSuggestion = { 
/**
 * The game record to belong to.
 */
record: string, 
/**
 * Which files the line holds ([`crate::cloud::v2::Line::def`]).
 */
def: string, 
/**
 * The line's newest version: who made it, on what, and when.
 */
device: string, platform: string, createdAt: string, 
/**
 * A user confirmed these bindings on another device.
 */
confirmed: boolean, fit: JoinFit, binds: Array<SaveBind>, 
/**
 * Files both sides have.
 */
matched: number, 
/**
 * Files only the line has, and only this device has, in the bound
 * folders.
 */
onlyInLine: number, onlyHere: number, 
/**
 * Files of the line with no folder here (none when `fit` is ready).
 */
uncovered: Array<string>, 
/**
 * Files here outside the bound folders: they stay, and are no longer
 * uploaded once the line is shared.
 */
outside: Array<string>, 
/**
 * The cross-platform rule, by name, that says the line's versions keep
 * the same files as this one: the line is another platform's and the
 * bindings are the rule's. None for bindings worked out from the files.
 */
rule?: string | null, 
/**
 * That other platform, as it is called ("Switch", "PC"). Empty when
 * the line is of this version's own platform.
 */
from?: string, 
/**
 * Files of the line that are the other platform's, not this version's
 * (its settings, a device save): they stay on the line as they are.
 */
others: number, };
