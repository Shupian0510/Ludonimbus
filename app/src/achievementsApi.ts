// Achievements commands (app/src-tauri/src/achievements.rs). In the browser
// preview they are answered by achievementsMock.ts. The types are generated
// from the Rust types (src/bindings, docs/development.md), some under the
// names this feature has always used.
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { inTauri } from "./api";
import type { AchievementCandidate, AchievementSettings, AchievementsView, Binding, RaGame, RaProfile, ToastPayload } from "./bindings";

export type {
  AchievementCandidate as Candidate,
  AchievementItem,
  AchievementSettings,
  AchievementSource as SourceKind,
  AchievementsView,
  Binding,
  Grade,
  RaGame,
  RaProfile,
  ToastPayload,
} from "./bindings";

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) return tauriInvoke<T>(cmd, args);
  const { mockAchievements } = await import("./achievementsMock");
  return (await mockAchievements(cmd, args ?? {})) as T;
}

export const achievementsApi = {
  view: (key: string) => invoke<AchievementsView>("achievements_view", { key }),
  candidates: (key: string) => invoke<AchievementCandidate[]>("achievements_candidates", { key }),
  link: (key: string, binding: Binding) => invoke<AchievementsView>("achievements_link", { key, binding }),
  linkFile: (key: string, path: string) => invoke<AchievementsView>("achievements_link_file", { key, path }),
  pickFile: () => invoke<string | null>("achievements_pick_file"),
  unlink: (key: string) => invoke<void>("achievements_unlink", { key }),
  refresh: (key: string) => invoke<AchievementsView>("achievements_refresh", { key }),
  icon: (key: string, id: string) => invoke<string | null>("achievements_icon", { key, id }),
  gameIcon: (key: string) => invoke<string | null>("achievements_game_icon", { key }),
  setPopup: (key: string, popup: boolean | null) => invoke<void>("achievements_set_popup", { key, popup }),
  settings: () => invoke<AchievementSettings>("achievements_settings"),
  saveSettings: (steamPopup: boolean, fallbackPopup: boolean) =>
    invoke<void>("achievements_save_settings", { steamPopup, fallbackPopup }),
  setPs4Key: (key: string) => invoke<void>("achievements_set_ps4_key", { key }),
  testPopup: (delaySecs = 0, key?: string) => invoke<void>("achievements_test_popup", { key: key ?? null, delaySecs }),
  toastNext: () => invoke<ToastPayload | null>("achievement_toast_next"),
  raConnect: (user: string, key: string) => invoke<RaProfile>("ra_connect", { user, key }),
  raDisconnect: () => invoke<void>("ra_disconnect"),
  raRecentGames: () => invoke<RaGame[]>("ra_recent_games"),
};
