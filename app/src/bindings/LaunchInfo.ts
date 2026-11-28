// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { PlatformMark } from "./PlatformMark";
import type { PromptRequest } from "./PromptRequest";

export type LaunchInfo = { 
/**
 * "desktop", "handheld" or "prompt".
 */
mode: "desktop" | "handheld" | "prompt", prompt: PromptRequest | null, 
/**
 * How the platforms of the saves a question compares are shown (their
 * programs' icons, when this device has them).
 */
platforms: { [key in string]: PlatformMark }, };
