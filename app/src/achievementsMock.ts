// Browser-preview answers for the achievements commands (see mock.ts).
import type { AchievementItem, AchievementSettings, AchievementsView, Candidate, RaGame, ToastPayload } from "./achievementsApi";

const lang = (() => {
  const fromUrl = new URLSearchParams(location.search).get("lang");
  let saved: string | null = null;
  try {
    saved = localStorage.getItem("ln.lang");
  } catch {
    /* ignore */
  }
  const pref = fromUrl ?? saved ?? "";
  if (pref === "en" || pref === "zh-CN") return pref;
  return (navigator.language ?? "").toLowerCase().startsWith("zh") ? "zh-CN" : "en";
})();
const L = (zh: string, en: string) => (lang === "en" ? en : zh);
const ago = (h: number) => new Date(Date.now() - h * 3600_000).toISOString();
const delay = <T>(v: T, ms = 200) => new Promise<T>((r) => setTimeout(() => r(v), ms));

// Small coloured squares stand in for trophy icons.
function icon(hue: number): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl(${hue},55%,45%)"/><stop offset="1" stop-color="hsl(${hue + 40},60%,25%)"/></linearGradient></defs><rect width="64" height="64" rx="6" fill="url(#g)"/><path d="M22 18h20v6a10 10 0 0 1-20 0zM28 36h8v6h5v4H23v-4h5z" fill="rgba(255,255,255,0.85)"/></svg>`;
  return `data:image/svg+xml;base64,${btoa(svg)}`;
}

function item(id: string, name: string, desc: string, grade: AchievementItem["grade"], unlockedH: number | null, extra: Partial<AchievementItem> = {}): AchievementItem {
  return {
    id,
    name,
    description: desc,
    iconUrl: null,
    hasIcon: true,
    hidden: false,
    points: null,
    grade,
    unlockedAt: unlockedH === null ? null : ago(unlockedH),
    hardcore: false,
    deviceName: unlockedH === null ? null : unlockedH > 100 ? "LAPTOP" : "DESKTOP-MAIN",
    ...extra,
  };
}

const bloodborne: AchievementsView = {
  key: "bloodborne-lantern",
  source: "lantern",
  sourceLabel: "Lantern Engine",
  sourceId: "NPWR05818_00",
  title: null,
  total: 40,
  unlocked: 7,
  points: 0,
  pointsTotal: 0,
  popup: true,
  checkedAt: ago(0.2),
  lastError: null,
  items: [
    ...[12, 5, 3, 2, 1, 0, 9].map((n, i) => item(String(n), L(`奖杯 ${n}`, `Trophy ${n}`), "", null, [0.5, 30, 52, 120, 160, 200, 260][i])),
    ...Array.from({ length: 33 }, (_, i) => i + 13).map((n) => item(String(n), L(`奖杯 ${n}`, `Trophy ${n}`), "", null, null)),
  ],
};

const demoRa: AchievementsView = {
  key: "",
  source: "retroachievements",
  sourceLabel: "RetroAchievements",
  sourceId: "1",
  title: "Demo",
  total: 6,
  unlocked: 3,
  points: 18,
  pointsTotal: 60,
  popup: true,
  checkedAt: ago(1),
  lastError: null,
  items: [
    item("2", L("第一个迷宫", "First dungeon"), L("通关第一个迷宫", "Clear the first dungeon"), null, 2, { points: 5, hardcore: true }),
    item("1", L("出发", "Departure"), L("离开森林", "Leave the forest"), null, 26, { points: 3 }),
    item("4", L("收藏家", "Collector"), L("收集 10 个心之碎片", "Collect 10 pieces of heart"), null, 150, { points: 10 }),
    item("0", L("全部奖杯", "All trophies"), L("获得全部奖杯", "Earn every trophy"), null, null, { points: 25 }),
    item("3", L("时之勇者", "Hero of Time"), L("打倒最终首领", "Defeat the final boss"), null, null, { points: 10 }),
    item("5", "", "", null, null, { hidden: true, points: 7 }),
  ],
};

const settings: AchievementSettings = { raUser: null, raConnected: false, steamPopup: true, fallbackPopup: true };
const views: Record<string, AchievementsView> = { "bloodborne-lantern": bloodborne };

const none = (key: string): AchievementsView => ({
  key,
  source: null,
  sourceLabel: null,
  sourceId: null,
  title: null,
  total: 0,
  unlocked: 0,
  points: 0,
  pointsTotal: 0,
  items: [],
  popup: true,
  checkedAt: null,
  lastError: null,
});

const hues: Record<string, number> = {};

export async function mockAchievements(cmd: string, args: Record<string, unknown>): Promise<unknown> {
  const key = args.key as string;
  switch (cmd) {
    case "achievements_view":
    case "achievements_refresh":
      return delay(views[key] ?? none(key));
    case "achievements_icon": {
      const id = args.id as string;
      hues[id] ??= (Number(id) * 47) % 360;
      return delay(icon(hues[id]), 50);
    }
    case "achievements_candidates": {
      const c: Candidate[] = [
        {
          binding: { source: "shadps4", sourceId: "NPWR05818_00", path: "C:\\Users\\me\\AppData\\Roaming\\shadPS4", popup: null },
          title: "Bloodborne",
          location: "C:\\Users\\me\\AppData\\Roaming\\shadPS4",
          sure: false,
        },
      ];
      return delay(key === "bloodborne-lantern" ? c : []);
    }
    case "achievements_link":
      views[key] = { ...demoRa, key };
      return delay(views[key]);
    case "achievements_unlink":
      delete views[key];
      return delay(undefined);
    case "achievements_settings":
      return delay({ ...settings });
    case "achievements_save_settings":
      settings.steamPopup = args.steamPopup as boolean;
      settings.fallbackPopup = args.fallbackPopup as boolean;
      return delay(undefined);
    case "ra_connect":
      settings.raUser = args.user as string;
      settings.raConnected = true;
      return delay({ user: args.user, ulid: null, avatar: null, points: 1234 }, 600);
    case "ra_disconnect":
      settings.raUser = null;
      settings.raConnected = false;
      return delay(undefined);
    case "ra_recent_games": {
      const g: RaGame[] = [
        { id: "11332", title: "Final Fantasy Origins", console: "PlayStation", icon: null, lastPlayed: ago(1), achieved: 38, total: 119 },
        { id: "1", title: "Sonic the Hedgehog", console: "Mega Drive", icon: null, lastPlayed: ago(30), achieved: 23, total: 23 },
      ];
      return delay(g, 500);
    }
    case "achievement_toast_next": {
      // ?toast in the browser preview shows one sample popup.
      if ((window as any).__lnToastShown) return delay(null);
      (window as any).__lnToastShown = true;
      const t: ToastPayload = {
        name: L("第一个迷宫", "First dungeon"),
        description: L("通关第一个迷宫", "Clear the first dungeon"),
        icon: icon(200),
        game: L("塞尔达传说：时之笛", "The Legend of Zelda: Ocarina of Time"),
        hardcore: false,
        points: null,
        grade: "bronze",
        test: false,
        lang,
      };
      return delay(t);
    }
    default:
      return delay(null);
  }
}
