// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { ScreenshotMode } from "./ScreenshotMode";

/**
 * What to change in the settings. A missing field stays as it is.
 */
export type SettingsPatch = { deviceName?: string, keepRecent?: number, keepDailyDays?: number, networkTimeoutSecs?: number, steamAccount?: string, screenshots?: ScreenshotMode, independentSaves?: boolean, launchWaitSecs?: number, exitWaitSecs?: number, waitForUpload?: boolean, cloudNewerNotifications?: boolean, };
