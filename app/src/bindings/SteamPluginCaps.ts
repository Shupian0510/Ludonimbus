// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * What a plugin can do in Steam.
 */
export type SteamPluginCaps = { 
/**
 * Read and turn a game's Steam Cloud switch.
 */
cloud: boolean, 
/**
 * Read and set a game's launch options while Steam runs.
 */
launchOptions: boolean, };
