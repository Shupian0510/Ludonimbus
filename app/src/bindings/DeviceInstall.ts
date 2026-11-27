// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * One game on a device, as other devices need to know it: retention keeps
 * the snapshot each device synced last ([`retention::DeviceBase`]).
 */
export type DeviceInstall = { 
/**
 * Id of the game record.
 */
game: string, line: string, 
/**
 * The folder the line is written to: what names the line for good.
 */
dir: string, 
/**
 * The snapshot this device synced last, and when.
 */
synced?: string, syncedAt?: string, 
/**
 * What the game runs on there ([`crate::sync::platform`]), so other
 * devices can show who is on a line with what.
 */
platform?: string, 
/**
 * The snapshots versions waiting on that device take files from: kept
 * for it like `synced`, until those versions are uploaded.
 */
borrows?: Array<string>, };
