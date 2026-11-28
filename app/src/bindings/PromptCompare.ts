// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { PromptSide } from "./PromptSide";

/**
 * Two saves to choose between before playing: the one offered (another
 * version's newer progress, the cloud's) and the one here.
 */
export type PromptCompare = { there: PromptSide, 
/**
 * `None`: there are no saves here yet.
 */
here: PromptSide | null, 
/**
 * How the saves are converted to be used here ("xbox", "rule",
 * "files"); empty when they are taken as they are.
 */
convert: string, 
/**
 * What else to know before choosing, one line each.
 */
notes: Array<string>, };
