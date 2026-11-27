// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { AchievementItem } from "./AchievementItem";
import type { AchievementSource } from "./AchievementSource";

export type AchievementsView = { key: string, source: AchievementSource | null, sourceLabel: string | null, sourceId: string | null, title: string | null, total: number, unlocked: number, points: number, pointsTotal: number, items: Array<AchievementItem>, popup: boolean, checkedAt: string | null, lastError: string | null, 
/**
 * Some achievements have no icon of their own and the game has one
 * ([`game_icon_path`]): show it for them, fetched once per game.
 */
gameIcon: boolean, };
