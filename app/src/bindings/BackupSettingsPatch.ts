// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { PartKind } from "./PartKind";

export type BackupSettingsPatch = { 
/**
 * `""` = back to the game's folder.
 */
root?: string, exclude?: Array<string>, 
/**
 * 0 = keep all.
 */
keep?: number, firmware?: boolean, skip?: Array<PartKind>, extraContent?: Array<string>, };
