// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { AchievementSource } from "./AchievementSource";

/**
 * Where a game's achievements come from.
 */
export type Binding = { source: AchievementSource, 
/**
 * RetroAchievements game id, or the trophy set id (`NPWR12345_00`).
 */
sourceId: string, 
/**
 * Trophy sources: the emulator's `user` folder (shadPS4 / Lantern) or
 * the trophy set folder (RPCS3).
 */
path: string | null, 
/**
 * Show unlocks with Steam's achievement popup; `None` = the default.
 */
popup: boolean | null, };
