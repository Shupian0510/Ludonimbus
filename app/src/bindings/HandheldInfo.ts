// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { HandheldMode } from "./HandheldMode";
import type { InSteam } from "./InSteam";

export type HandheldInfo = { 
/**
 * The handheld this is, by its firmware ("ROG Ally"…).
 */
device: string | null, 
/**
 * Started from Steam's controller interface right now.
 */
steamGamepadUi: boolean, mode: HandheldMode, 
/**
 * The Steam shortcut to the handheld screen.
 */
inSteam: InSteam | null, 
/**
 * The Xbox full screen experience is on right now.
 */
xboxFse: boolean, };
