// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { PlatformMark } from "./PlatformMark";
import type { SaveLine } from "./SaveLine";

export type LineList = { lines: Array<SaveLine>, platforms: { [key in string]: PlatformMark }, };
