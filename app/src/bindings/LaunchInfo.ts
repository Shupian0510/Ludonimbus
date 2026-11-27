// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { PromptRequest } from "./PromptRequest";

export type LaunchInfo = { 
/**
 * "desktop", "handheld" or "prompt".
 */
mode: "desktop" | "handheld" | "prompt", prompt: PromptRequest | null, };
