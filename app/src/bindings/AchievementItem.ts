// Generated from the Rust types by ts-rs. Don't edit: change the Rust type, then run `cargo test -p ludonimbus-app export_bindings` (docs/development.md).
import type { Grade } from "./Grade";

export type AchievementItem = { id: string, name: string, description: string, 
/**
 * Web address of the icon (RetroAchievements).
 */
iconUrl: string | null, 
/**
 * A local icon exists; fetch it through the app or the local API.
 */
hasIcon: boolean, hidden: boolean, points: number | null, grade: Grade | null, unlockedAt: string | null, hardcore: boolean, deviceName: string | null, };
