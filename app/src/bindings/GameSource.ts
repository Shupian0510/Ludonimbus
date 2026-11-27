// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { SourceKind } from "./SourceKind";

export type GameSource = { kind: SourceKind, 
/**
 * The emulator (`Dolphin`); `None` for the other kinds, whose group
 * name the interface supplies.
 */
name: string | null, 
/**
 * The rule's platform in the interface language
 * (`GameCube / Wii 模拟器`, `Longshot（原生重制）`).
 */
detail: string | null, 
/**
 * The group the user chose instead (see [`GameEntry::source`]).
 */
label: string | null, 
/**
 * The logo of the program it runs on, as an image URL: filled in by the
 * app from [`crate::logos::for_game`].
 */
logo: string | null, };
