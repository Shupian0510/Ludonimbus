// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { PlaytimeDevice } from "./PlaytimeDevice";
import type { PlaytimePlatform } from "./PlaytimePlatform";

/**
 * A game's playtime as the app shows it: the summary, and what each
 * platform it was played on is called.
 */
export type PlaytimeSummary = { 
/**
 * Platform ([`playtime::PlatformTotal::platform`]) → its name.
 */
platformNames?: { [key in string]: string }, totalSecs: number, lastPlayed: string | null, devices: Array<PlaytimeDevice>, 
/**
 * Time per platform ([`Session::platform`]), most first. Sessions from
 * before sessions said where they were played, and minutes imported
 * from Steam, are only in the total.
 */
platforms?: Array<PlaytimePlatform>, };
