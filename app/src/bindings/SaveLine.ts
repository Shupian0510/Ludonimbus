// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { LineMember } from "./LineMember";

/**
 * A save line a game on this device can follow.
 */
export type SaveLine = { id: string, 
/**
 * What a device called the line it keeps to itself. Empty for the
 * shared line.
 */
name: string, 
/**
 * The line the game's devices share.
 */
shared: boolean, 
/**
 * The line the game follows on this device.
 */
current: boolean, 
/**
 * The devices on it, by name, as far as each has told the cloud.
 */
devices: Array<string>, 
/**
 * The same, one for each version of the game a device has on it.
 */
members: Array<LineMember>, };
