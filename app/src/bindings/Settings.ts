// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { CloudConfig } from "./CloudConfig";
import type { PreviousCloud } from "./PreviousCloud";
import type { Retention } from "./Retention";
import type { ScreenshotMode } from "./ScreenshotMode";

export type Settings = { deviceId: string, deviceName: string, retention: Retention, networkTimeoutSecs: number, steamRoot: string | null, 
/**
 * The Steam folder was picked in Settings (not auto-detected).
 */
steamRootCustom: boolean, steamAccount: string | null, cloud: CloudConfig, 
/**
 * Cloud used before the last switch whose data can still be copied.
 */
previousCloud: PreviousCloud | null, googleBuiltin: boolean, 
/**
 * A Baidu Netdisk app is built into this release.
 */
baiduBuiltin: boolean, 
/**
 * App name (= folder under 「我的应用数据」) of the built-in Baidu app.
 */
baiduAppName: string, dataDir: string, runnerPath: string, runnerExists: boolean, ludusavi: string | null, version: string, screenshots: ScreenshotMode, 
/**
 * Games new to this device keep their saves to themselves.
 */
independentSaves: boolean, 
/**
 * Before playing, how long the launch wrapper waits for the cloud.
 */
launchWaitSecs: number, 
/**
 * After playing, how long it waits for the upload before handing it over.
 */
exitWaitSecs: number, 
/**
 * After playing, always wait until the upload is done.
 */
waitForUpload: boolean, };
