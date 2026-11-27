// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { SteamCloudState } from "./SteamCloudState";

/**
 * A game Steam installed on this device.
 */
export type SteamCloudApp = { appid: number, name: string, 
/**
 * Its library entry, once it was added.
 */
key: string | null, state: SteamCloudState, 
/**
 * How Steam's last cloud sync of it went (`synchronized`, …).
 */
syncState: string | null, 
/**
 * Its own Steam Cloud switch.
 */
cloudEnabled: boolean, 
/**
 * Steam Cloud syncs it on this device (not: the game has none).
 */
cloudUsed: boolean, };
