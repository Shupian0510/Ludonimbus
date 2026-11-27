// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * What to change in a game. A missing field stays as it is.
 */
export type GamePatch = { name?: string, saves?: Array<string>, exclude?: Array<string>, waitFor?: Array<string>, args?: string, autoSync?: boolean, uploadOnGameExit?: boolean, 
/**
 * Library group; empty = the detected source.
 */
source?: string, };
