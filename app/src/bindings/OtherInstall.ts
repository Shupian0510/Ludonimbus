// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * A version of a game installed on this PC that isn't in the library.
 */
export type OtherInstall = { 
/**
 * `steam:<app id>` or `xbox:<folder>`.
 */
id: string, name: string, 
/**
 * `steam` or `xbox`.
 */
store: string, 
/**
 * Its store's logo.
 */
logo: string | null, 
/**
 * A version of the game asked about.
 */
same: boolean, };
