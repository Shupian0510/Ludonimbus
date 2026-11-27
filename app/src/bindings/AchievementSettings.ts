// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { Ps4KeySource } from "./Ps4KeySource";

export type AchievementSettings = { raUser: string | null, raConnected: boolean, steamPopup: boolean, fallbackPopup: boolean, 
/**
 * Where the PS4 trophy key comes from (shadPS4 or these settings), if anywhere.
 */
ps4KeySource: Ps4KeySource | null, };
