// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { SteamCloudState } from "./SteamCloudState";
import type { SteamWrapperHealth } from "./SteamWrapperHealth";

/**
 * How a game Steam installed stands with Steam and Ludonimbus.
 */
export type SteamCloudStatus = { mode: SteamCloudState, syncState: string | null, 
/**
 * Steam's last cloud sync of the game left nothing behind.
 */
synchronized: boolean, cloudEnabled: boolean, 
/**
 * Steam Cloud syncs it on this device (not: the game has none).
 */
cloudUsed: boolean, 
/**
 * The account's Steam Cloud switch.
 */
globalEnabled: boolean, steamRunning: boolean, 
/**
 * Whether its launch options still go through the wrapper; `None` when
 * they aren't meant to.
 */
wrapper: SteamWrapperHealth | null, };
