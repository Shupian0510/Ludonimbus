// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { DeviceInstall } from "./DeviceInstall";

export type DeviceInfo = { id: string, name: string, os: string, appVersion: string, lastSeen: string, 
/**
 * The newest cloud layout this device's Ludonimbus knows; 0 in a record
 * written by 0.6.
 */
layout?: number, 
/**
 * The save lines this device syncs, and where it is on each.
 */
installs?: Array<DeviceInstall>, 
/**
 * What kind of device it is, from its hardware ([`crate::handheld::device_kind`]):
 * "desktop", "laptop", "handheld", "deck" or "switch". Records of older
 * versions don't say.
 */
kind?: string, };
