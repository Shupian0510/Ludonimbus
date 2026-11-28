// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { JoinSuggestion } from "./JoinSuggestion";

/**
 * Another version's line the game can share (「共用进度」, an advanced
 * choice in the version bar's menu), with how that version is called.
 */
export type ShareOffer = { suggestion: JoinSuggestion, 
/**
 * The version whose line it is ("Xbox", "Steam", "Ryujinx").
 */
label: string, };
