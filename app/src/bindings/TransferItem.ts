// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type TransferItem = { 
/**
 * The source file (relative path).
 */
from: string, 
/**
 * Where it goes: a path on this PC, or `container / file` for Xbox.
 */
to: string, size: number, 
/**
 * Overwrites a file the target already has.
 */
replaces: boolean, };
