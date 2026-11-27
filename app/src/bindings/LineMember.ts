// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

/**
 * One version of a game on one device, on a save line.
 */
export type LineMember = { device: string, 
/**
 * [`platform`]; empty when the device hasn't said (a record written
 * before it did).
 */
platform: string, 
/**
 * It is this device.
 */
here: boolean, };
