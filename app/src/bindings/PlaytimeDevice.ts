// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type PlaytimeDevice = { device: string, deviceName: string, secs: number, lastPlayed: string | null, thisDevice: boolean, 
/**
 * Minutes imported from Steam (played before Ludonimbus kept time).
 */
steamMinutes: number | null, };
