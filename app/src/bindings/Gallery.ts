// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { ScreenshotMode } from "./ScreenshotMode";
import type { Shot } from "./Shot";

export type Gallery = { shots: Array<Shot>, mode: ScreenshotMode, linked: boolean, folder: string | null, 
/**
 * Space the screenshots take in the cloud (images only).
 */
cloudBytes: number, 
/**
 * The cloud couldn't be listed; only this PC's screenshots are shown.
 */
cloudError: string | null, };
