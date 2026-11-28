// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * The game in Ludusavi's database, as the save finder offers it.
 */
export type SaveTitle = { title: string, 
/**
 * Found by the game's Steam app id or its exact name.
 */
likely: boolean, 
/**
 * Why it is offered (shown to the user).
 */
reason: string, 
/**
 * Save files Ludusavi finds for it on this device (counted for the
 * likely ones only).
 */
files: number, };
