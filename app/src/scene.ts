// Browser preview only: `?scene=<name>` opens a page, dialog or state
// directly, so scripts/docs-screenshots.ps1 can capture every screen of the
// documentation site. Never active inside the app.
import { inTauri } from "./api";

export function scene(): string | null {
  if (inTauri) return null;
  return new URLSearchParams(location.search).get("scene");
}

export function sceneIs(...names: string[]): boolean {
  const s = scene();
  return !!s && names.includes(s);
}

/** Scenes that show a game page, and which sample game. */
export const SCENE_GAME: Record<string, string> = {
  "game-detail": "zelda-oot-longshot",
  "game-detail-steam": "zelda-oot-longshot",
  "game-detail-conflict": "bloodborne-lantern",
  "game-detail-pending": "zelda-oot-longshot",
  versions: "zelda-oot-longshot",
  "game-settings": "zelda-oot-longshot",
  "art-picker": "fe-fortunes-weave",
};

/** Settings scenes and the section they scroll to. */
export const SCENE_SETTINGS: Record<string, string | null> = {
  settings: null,
  "settings-cloud": "settings-cloud",
  "settings-cloud-google": "settings-cloud",
  "settings-cloud-folder": "settings-cloud",
  "settings-cloud-migrate": "settings-cloud",
  "settings-cloud-webdav": "cloud-webdav",
  "settings-cloud-s3": "cloud-s3",
  "settings-cloud-quark": "cloud-quark",
  "settings-language": "settings-language",
  "settings-sgdb": "settings-sgdb",
  "settings-steam-plugin": "settings-steam-plugin",
};

/** "Add game" scenes and their tab. */
export const SCENE_ADD: Record<string, "scan" | "cloud" | "ludusavi" | "manual"> = {
  "add-game-scan": "scan",
  "add-game-scan-folder": "scan",
  "add-game-cloud": "cloud",
  "add-game-manual": "manual",
  "save-finder": "scan",
  "add-to-steam": "scan",
};
