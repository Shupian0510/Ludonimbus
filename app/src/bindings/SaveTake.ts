// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * Whether, and how, a source's saves can be brought here.
 */
export type SaveTake = { "kind": "copy" } | { "kind": "convert", how: string, } | { "kind": "never", reason: string, };
