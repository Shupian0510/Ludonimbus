// Achievements commands (app/src-tauri/src/achievements.rs). In the browser
// preview they are answered by achievementsMock.ts.
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { inTauri } from "./api";

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) return tauriInvoke<T>(cmd, args);
  const { mockAchievements } = await import("./achievementsMock");
  return (await mockAchievements(cmd, args ?? {})) as T;
}

export type SourceKind = "retroachievements" | "shadps4" | "lantern" | "rpcs3";
export type Grade = "platinum" | "gold" | "silver" | "bronze";

export interface Binding {
  source: SourceKind;
  sourceId: string;
  path: string | null;
  popup: boolean | null;
}

export interface AchievementItem {
  id: string;
  name: string;
  description: string;
  iconUrl: string | null;
  hasIcon: boolean;
  hidden: boolean;
  points: number | null;
  grade: Grade | null;
  unlockedAt: string | null;
  hardcore: boolean;
  deviceName: string | null;
}

export interface AchievementsView {
  key: string;
  source: SourceKind | null;
  sourceLabel: string | null;
  sourceId: string | null;
  title: string | null;
  total: number;
  unlocked: number;
  points: number;
  pointsTotal: number;
  items: AchievementItem[];
  popup: boolean;
  checkedAt: string | null;
  lastError: string | null;
}

export interface Candidate {
  binding: Binding;
  title: string | null;
  location: string;
  sure: boolean;
}

export interface AchievementSettings {
  raUser: string | null;
  raConnected: boolean;
  steamPopup: boolean;
  fallbackPopup: boolean;
}

export interface RaGame {
  id: string;
  title: string;
  console: string;
  icon: string | null;
  lastPlayed: string | null;
  achieved: number;
  total: number;
}

export interface RaProfile {
  user: string;
  ulid: string | null;
  avatar: string | null;
  points: number;
}

/** What the fallback popup window shows. */
export interface ToastPayload {
  name: string;
  description: string;
  icon: string | null;
  game: string;
  hardcore: boolean;
  points: number | null;
  grade: Grade | null;
  test: boolean;
  lang: string;
}

export const achievementsApi = {
  view: (key: string) => invoke<AchievementsView>("achievements_view", { key }),
  candidates: (key: string) => invoke<Candidate[]>("achievements_candidates", { key }),
  link: (key: string, binding: Binding) => invoke<AchievementsView>("achievements_link", { key, binding }),
  linkFile: (key: string, path: string) => invoke<AchievementsView>("achievements_link_file", { key, path }),
  pickFile: () => invoke<string | null>("achievements_pick_file"),
  unlink: (key: string) => invoke<void>("achievements_unlink", { key }),
  refresh: (key: string) => invoke<AchievementsView>("achievements_refresh", { key }),
  icon: (key: string, id: string) => invoke<string | null>("achievements_icon", { key, id }),
  setPopup: (key: string, popup: boolean | null) => invoke<void>("achievements_set_popup", { key, popup }),
  settings: () => invoke<AchievementSettings>("achievements_settings"),
  saveSettings: (steamPopup: boolean, fallbackPopup: boolean) =>
    invoke<void>("achievements_save_settings", { steamPopup, fallbackPopup }),
  testPopup: (delaySecs = 0, key?: string) => invoke<void>("achievements_test_popup", { key: key ?? null, delaySecs }),
  toastNext: () => invoke<ToastPayload | null>("achievement_toast_next"),
  raConnect: (user: string, key: string) => invoke<RaProfile>("ra_connect", { user, key }),
  raDisconnect: () => invoke<void>("ra_disconnect"),
  raRecentGames: () => invoke<RaGame[]>("ra_recent_games"),
};
