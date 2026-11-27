// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).

export type Shot = { 
/**
 * Thumbnail (or image) already on this PC, as a `ludoart://` URL.
 */
thumbUrl: string | null, name: string, 
/**
 * Device that took it: the name of its folder in the cloud, which is
 * `<device-id>~<key>` for some installs ([`gamedata::device_of`] gives
 * the device's id).
 */
device: string, deviceName: string | null, size: number, takenAt: string | null, 
/**
 * The image is in this PC's Steam folder.
 */
onThisPc: boolean, 
/**
 * Uploaded to the cloud.
 */
inCloud: boolean, hasThumb: boolean, };
