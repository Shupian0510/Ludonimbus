// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { SteamCloudState } from "./SteamCloudState";

export type StoreView = { appid: number, mode: SteamCloudState, 
/**
 * Its launch options are meant to go through the wrapper.
 */
wrapper: boolean, };
