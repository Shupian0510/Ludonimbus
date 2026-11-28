// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type PromptButton = { id: string, label: string, 
/**
 * The controller button: "a", "b", "x" or "y". "b" also answers when
 * the window is closed.
 */
key: "a" | "b" | "x" | "y", 
/**
 * A small line under the button: what choosing it means besides its
 * name. A system dialog has no place for it and says it in the text.
 */
detail?: string, };
