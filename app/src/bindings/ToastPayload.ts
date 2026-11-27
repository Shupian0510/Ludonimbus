// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { Grade } from "./Grade";

/**
 * What the fallback popup window shows.
 */
export type ToastPayload = { name: string, description: string, 
/**
 * `https://…` or a `data:` URL.
 */
icon: string | null, 
/**
 * The game's name.
 */
game: string, hardcore: boolean, points: number | null, grade: Grade | null, 
/**
 * A made-up unlock ("Test popup").
 */
test: boolean, 
/**
 * The interface language ("zh-CN" or "en").
 */
lang: string, };
