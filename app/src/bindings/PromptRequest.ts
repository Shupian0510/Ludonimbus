// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { PromptButton } from "./PromptButton";
import type { PromptCompare } from "./PromptCompare";

export type PromptRequest = { title: string, text: string, 
/**
 * "info", "warn" or "error".
 */
level: string, buttons: Array<PromptButton>, 
/**
 * The two saves the question is about, for the window to show side by
 * side. A system dialog has only [`Request::text`], which says the same.
 */
compare?: PromptCompare, 
/**
 * A window of its own on the desktop, centred, in front, not full
 * screen ([`ask_dialog`]).
 */
dialog: boolean, };
