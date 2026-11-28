// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { CloudOnlyView } from "./CloudOnlyView";
import type { CloudView } from "./CloudView";
import type { GameView } from "./GameView";

export type Overview = { deviceId: string, deviceName: string, 
/**
 * What kind of device this is ([`ludonimbus_core::handheld::device_kind`]).
 */
deviceKind: string, cloud: CloudView, steamFound: boolean, steamRunning: boolean, games: Array<GameView>, 
/**
 * Games the cloud has but this PC doesn't, from the copy of the cloud's
 * list kept on this PC (refreshed by `refresh_cloud_library`).
 */
cloudOnly: Array<CloudOnlyView>, cloudFetchedAt: string | null, };
