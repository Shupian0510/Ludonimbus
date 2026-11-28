// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { CloudOnlyView } from "./CloudOnlyView";
import type { CloudView } from "./CloudView";
import type { GameView } from "./GameView";
import type { Pause } from "./Pause";

export type Overview = { deviceId: string, deviceName: string, cloud: CloudView, steamFound: boolean, steamRunning: boolean, games: Array<GameView>, 
/**
 * Games the cloud has but this PC doesn't, from the copy of the cloud's
 * list kept on this PC (refreshed by `refresh_cloud_library`).
 */
cloudOnly: Array<CloudOnlyView>, cloudFetchedAt: string | null, 
/**
 * Syncing is paused on this device (from the Steam Deck's panel or the
 * local API, see `ludonimbus_core::pause`).
 */
paused: Pause | null, };
