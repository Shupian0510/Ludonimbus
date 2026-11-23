// Browser-only mock backend, used when the UI runs outside Tauri
// (`npm run dev` in a normal browser) so the interface can be developed and
// reviewed without the Rust side. Never loaded inside the app.
import type { Found, GameView, Outcome, Overview, SnapshotSummary, StatusReport } from "./api";

// Sample data in the preview's language (?lang=, the saved choice, or the browser's).
const mockLang = (() => {
  const fromUrl = new URLSearchParams(location.search).get("lang");
  let saved: string | null = null;
  try {
    saved = localStorage.getItem("ln.lang");
  } catch {
    /* ignore */
  }
  const pref = fromUrl ?? saved ?? "system";
  if (pref === "en" || pref === "zh-CN") return pref;
  return (navigator.language ?? "").toLowerCase().startsWith("zh") ? "zh-CN" : "en";
})();
const L = (zh: string, en: string) => (mockLang === "en" ? en : zh);

const now = Date.now();
const ago = (min: number) => new Date(now - min * 60000).toISOString();
const art = (appid: number, kind: string) => `/mock-art/${appid}${kind}`;

function snap(id: string, min: number, device: string, trigger: SnapshotSummary["trigger"], size: number, files: number, extra: Partial<SnapshotSummary> = {}): SnapshotSummary {
  return {
    id,
    parent: null,
    deviceId: device === "DESKTOP-MAIN" ? "d-mock" : "d-laptop",
    deviceName: device,
    createdAt: ago(min),
    trigger,
    contentHash: id,
    totalSize: size,
    fileCount: files,
    comment: "",
    locked: false,
    ...extra,
  };
}

const snapshots: Record<string, SnapshotSummary[]> = {
  "zelda-oot-longshot": [
    snap("01OOT5", 35, "DESKTOP-MAIN", "auto-exit", 1258291, 14),
    snap("01OOT4", 60 * 26, "GPD-Win-Mini", "auto-exit", 1240000, 14),
    snap("01OOT3", 60 * 50, "DESKTOP-MAIN", "manual", 1100000, 12, { comment: L("进入水之神殿前", "Before the Water Temple"), locked: true }),
    snap("01OOT2", 60 * 75, "DESKTOP-MAIN", "pre-restore", 1090000, 12),
    snap("01OOT1", 60 * 24 * 4, "DESKTOP-MAIN", "auto-exit", 800000, 10),
  ],
  "fe-fortunes-weave": [snap("01FE2", 60 * 3, "DESKTOP-MAIN", "auto-exit", 56422, 4), snap("01FE1", 60 * 30, "DESKTOP-MAIN", "auto-exit", 40100, 3)],
  "bloodborne-lantern": [snap("01BB2", 60 * 5, "GPD-Win-Mini", "auto-exit", 15309618, 16), snap("01BB1", 60 * 48, "DESKTOP-MAIN", "auto-exit", 15309618, 16)],
};

function report(key: string, status: StatusReport["status"], files: number, size: number): StatusReport {
  const list = snapshots[key] ?? [];
  return {
    status,
    message: null,
    localFiles: files,
    localSize: size,
    localHash: "x",
    head: list.find((s) => s.trigger !== "pre-restore" && s.trigger !== "conflict-keep") ?? null,
    snapshotCount: list.length,
    lastSyncedId: list[0]?.id ?? null,
    lastSyncAt: list[0]?.createdAt ?? null,
    pending: 0,
    checkedAt: new Date().toISOString(),
  };
}

const checks: Record<string, StatusReport> = {
  "zelda-oot-longshot": report("zelda-oot-longshot", "synced", 14, 1258291),
  "fe-fortunes-weave": report("fe-fortunes-weave", "localChanged", 4, 56422),
  "bloodborne-lantern": report("bloodborne-lantern", "conflict", 16, 15309618),
};

function game(key: string, name: string, appid: number, platform: string, exe: string, saves: string[], extra: Partial<GameView> = {}): GameView {
  return {
    key,
    name,
    adapter: key,
    adapterName: name,
    platform,
    exe,
    exeExists: true,
    install: exe.replace(/\\[^\\]+$/, ""),
    args: "",
    saves,
    exclude: [],
    waitFor: [],
    customSaves: false,
    defaultSaves: saves,
    defaultWaitFor: key === "zelda-oot-longshot" ? ["longshot.exe"] : [],
    uploadOnGameExit: key === "zelda-oot-longshot",
    hasGameProcess: key === "zelda-oot-longshot",
    ludusaviTitle: null,
    autoSync: true,
    steam: { account: "12345678", appid, enabled: true, created: false },
    health: { state: "ok" },
    art: { cover: art(appid, "p.png"), wide: null, hero: art(appid, "_hero.png"), logo: art(appid, "_logo.png"), icon: art(appid, "_icon.png") },
    running: false,
    lastSyncAt: snapshots[key]?.[0]?.createdAt ?? null,
    lastSyncedDevice: snapshots[key]?.[0]?.deviceName ?? null,
    conflict: null,
    lastError: null,
    pending: 0,
    lastCheck: checks[key] ?? null,
    ...extra,
  };
}

const overview: Overview = {
  deviceId: "d-mock",
  deviceName: "DESKTOP-MAIN",
  cloud: { kind: "googleDrive", label: "player@example.com", error: null },
  steamFound: true,
  steamRunning: true,
  games: [
    game("zelda-oot-longshot", L("塞尔达传说：时之笛", "The Legend of Zelda: Ocarina of Time"), 4079546474, L("Longshot（原生重制）", "Longshot (native port)"), "C:\\Users\\me\\AppData\\Local\\Programs\\LongshotLauncher\\LongshotLauncher.exe", ["{data}/save/**"]),
    game("fe-fortunes-weave", L("火焰纹章：万缕千丝", "Fire Emblem: Fortune's Weave"), 2661209017, "fefw recomp", "C:\\Users\\me\\Desktop\\Switch\\fefw-recomp-0.2.7\\fefw.exe", ["{install}/save/**"], {
      art: { cover: art(2661209017, "p.jpg"), wide: null, hero: art(2661209017, "_hero.png"), logo: art(2661209017, "_logo.png"), icon: art(2661209017, "_icon.png") },
      health: { state: "changed", exe: "C:\\Users\\me\\Desktop\\Switch\\fefw-recomp-0.2.7\\fefw.exe", launchOptions: "" },
    }),
    game("bloodborne-lantern", "Bloodborne", 4000695018, L("Lantern Engine（shadPS4）", "Lantern Engine (shadPS4)"), "C:\\Users\\me\\Desktop\\Switch\\Bloodborne\\BloodbornePC\\Bloodborne.exe", ["{install}/user/savedata/1/CUSA00207/**"], {
      art: { cover: art(4000695018, "p.png"), wide: null, hero: art(4000695018, "_hero.jpg"), logo: art(4000695018, "_logo.png"), icon: art(4000695018, "_icon.png") },
      conflict: { cloudId: "01BB2", cloudDevice: "GPD-Win-Mini", cloudCreatedAt: ago(300), localHash: "y", detectedAt: ago(10) },
    }),
    game("celeste", "Celeste", 0, "", "D:\\Games\\Celeste\\Celeste.exe", ["{data}/Saves/**"], {
      adapter: null,
      adapterName: null,
      platform: null,
      ludusaviTitle: "Celeste",
      steam: null,
      health: { state: "notLinked" },
      art: { cover: null, wide: null, hero: null, logo: null, icon: null },
    }),
    game("starfield", "Starfield", 0, "", "", ["{ludusavi}"], {
      adapter: null,
      adapterName: null,
      platform: null,
      exe: null,
      exeExists: false,
      install: null,
      ludusaviTitle: "Starfield",
      steam: null,
      health: { state: "notLinked" },
      art: { cover: null, wide: null, hero: null, logo: null, icon: null },
    }),
    game("doom-eternal", "DOOM Eternal", 0, "", "C:\\XboxGames\\Doom Eternal - PC\\Content\\launcher\\idTechLauncher.exe", ["{ludusavi}"], {
      adapter: null,
      adapterName: null,
      platform: null,
      install: null,
      ludusaviTitle: "DOOM Eternal",
      steam: null,
      health: { state: "notLinked" },
      art: { cover: null, wide: null, hero: null, logo: null, icon: null },
    }),
  ],
};

function found(id: string, name: string, source: Found["source"], extra: Partial<Found> = {}): Found {
  return {
    id,
    name,
    source,
    exe: null,
    install: null,
    args: "",
    steamAccount: null,
    steamAppid: null,
    adapter: null,
    adapterName: null,
    saves: [],
    ludusaviTitle: null,
    saveFiles: 0,
    saveBytes: 0,
    saveSample: [],
    steamCloud: false,
    storeGame: false,
    addedKey: null,
    registryOnly: false,
    installStore: null,
    matchKey: name.toLowerCase().replace(/[^\p{L}\p{N}]/gu, ""),
    ...extra,
  };
}

const quickScan: Found[] = [
  found("steam:12345678:4079546474", L("塞尔达传说：时之笛", "The Legend of Zelda: Ocarina of Time"), "steam", { exe: overview.games[0].exe, adapter: "zelda-oot-longshot", adapterName: "Longshot", addedKey: "zelda-oot-longshot", steamAccount: "12345678", steamAppid: 4079546474 }),
  found("steam:12345678:4000695018", "Bloodborne", "steam", { exe: overview.games[2].exe, adapter: "bloodborne-lantern", adapterName: "Lantern Engine", addedKey: "bloodborne-lantern", steamAccount: "12345678", steamAppid: 4000695018 }),
  found("steam:12345678:3112233445", "Ryujinx", "steam", { exe: "C:\\Users\\me\\Desktop\\Switch\\ryujinx\\Ryujinx.exe", adapter: "ryujinx", adapterName: L("Ryujinx（整个模拟器的存档）", "Ryujinx (whole emulator)"), steamAccount: "12345678", steamAppid: 3112233445 }),
  found("steam:12345678:3220011223", L("超级马力欧银河", "Super Mario Galaxy"), "steam", { exe: "D:\\Emulators\\Dolphin-x64\\Dolphin.exe", args: '-b -e "D:\\ROMs\\Wii\\Super Mario Galaxy.rvz"', adapter: "dolphin", adapterName: L("Dolphin（RMGE01）", "Dolphin (RMGE01)"), saves: ["{emu}/Wii/title/0001000?/524d4745/data/**"], steamAccount: "12345678", steamAppid: 3220011223 }),
  found("emulator:ryujinx:default:0100770008DD8000", "MONSTER HUNTER GENERATIONS ULTIMATE", "emulator", { exe: "C:\\Users\\me\\Desktop\\Switch\\ryujinx\\Ryujinx.exe", args: '"C:\\Users\\me\\Desktop\\Switch\\Games\\ROMS\\MONSTER HUNTER GENERATIONS ULTIMATE [0100770008dd8000][v0].nsp"', adapter: "ryujinx", adapterName: L("Ryujinx（0100770008DD8000）", "Ryujinx (0100770008DD8000)"), saves: ["{nxsave-0100770008dd8000}/**", "{nxdevice-0100770008dd8000}/**"], saveFiles: 4, saveBytes: 6.2e6, saveSample: ["C:\\Users\\me\\AppData\\Roaming\\Ryujinx\\bis\\user\\save\\0000000000000003\\0\\system"] }),
  found("emulator:dolphin:gc:GALE01", "Super Smash Bros. Melee", "emulator", { adapter: "dolphin", adapterName: L("Dolphin（GALE01）", "Dolphin (GALE01)"), saves: ["{emu}/GC/*/Card ?/01-GALE-*.gci"], saveFiles: 1, saveBytes: 4.0e4, saveSample: ["C:\\Users\\me\\Documents\\Dolphin Emulator\\GC\\USA\\Card A\\01-GALE-SuperSmashBros0110290334.gci"] }),
  found("recomplauncher:zelda64recomp", "The Legend of Zelda: Majora's Mask", "recomplauncher", { exe: "D:\\RecompLauncher\\Library\\zelda64recomp\\Zelda64Recompiled.exe", install: "D:\\RecompLauncher\\Library\\zelda64recomp", adapter: "zelda64recomp", adapterName: "The Legend of Zelda: Majora's Mask" }),
  found("steam:87654321:3998877665", "Star Rail Launcher", "steam", { exe: "D:\\Games\\StarRail\\launcher.exe", steamAccount: "87654321", steamAppid: 3998877665 }),
  found("epic:Salt", "Celeste", "epic", { exe: "D:\\Games\\Celeste\\Celeste.exe", install: "D:\\Games\\Celeste", ludusaviTitle: "Celeste", addedKey: "celeste" }),
  found("xbox:Doom Eternal - PC", "Doom Eternal", "xbox", { exe: "C:\\XboxGames\\Doom Eternal - PC\\Content\\gamelaunchhelper.exe", install: "C:\\XboxGames\\Doom Eternal - PC\\Content", ludusaviTitle: "DOOM Eternal", installStore: "xbox" }),
  found("gog:1207658930", "The Witcher 3: Wild Hunt", "gog", { exe: "D:\\GOG Games\\The Witcher 3\\bin\\x64\\witcher3.exe", install: "D:\\GOG Games\\The Witcher 3", ludusaviTitle: "The Witcher 3: Wild Hunt" }),
];

const ludusaviScan: Found[] = [
  found("ludusavi:The Witcher 3: Wild Hunt", "The Witcher 3: Wild Hunt", "ludusavi", { ludusaviTitle: "The Witcher 3: Wild Hunt", saveFiles: 38, saveBytes: 412e6, saveSample: ["C:\\Users\\me\\Documents\\The Witcher 3\\gamesaves\\AutoSave_0.sav"] }),
  found("ludusavi:Hollow Knight", "Hollow Knight", "ludusavi", { ludusaviTitle: "Hollow Knight", saveFiles: 6, saveBytes: 3.1e6, saveSample: ["C:\\Users\\me\\AppData\\LocalLow\\Team Cherry\\Hollow Knight\\user1.dat"] }),
  found("ludusavi:DOOM Eternal", "DOOM Eternal", "ludusavi", { ludusaviTitle: "DOOM Eternal", saveFiles: 3, saveBytes: 1.1e6, exe: "C:\\XboxGames\\Doom Eternal - PC\\Content\\gamelaunchhelper.exe", installStore: "xbox", saveSample: ["C:\\XboxGames\\GameSave\\wgs\\000901F...\\container.1"] }),
  found("ludusavi:Phantom Blade Zero", "Phantom Blade Zero", "ludusavi", { ludusaviTitle: "Phantom Blade Zero", saveFiles: 71, saveBytes: 2.5e9, saveSample: ["C:\\Program Files (x86)\\Steam\\steamapps\\common\\PBZ\\ProjectPBZ\\Saved\\Cache\\PBZInstallVerify.bin"] }),
  found("ludusavi:Titanfall 2", "Titanfall 2", "ludusavi", { ludusaviTitle: "Titanfall 2", exe: "C:\\Program Files\\EA Games\\Titanfall2\\Titanfall2.exe", install: "C:\\Program Files\\EA Games\\Titanfall2", installStore: "registry", saveFiles: 4, saveBytes: 2.2e5, saveSample: ["C:\\Users\\me\\Documents\\Respawn\\Titanfall2\\profile\\profile.cfg"] }),
  found("ludusavi:Starfield", "Starfield", "ludusavi", { ludusaviTitle: "Starfield", saveFiles: 120, saveBytes: 2.4e9, steamCloud: true, storeGame: true }),
  found("ludusavi:No Man's Sky", "No Man's Sky", "ludusavi", { ludusaviTitle: "No Man's Sky", saveFiles: 22, saveBytes: 1.6e7, steamCloud: true, storeGame: true }),
  found("ludusavi:Zenless Zone Zero", "Zenless Zone Zero", "ludusavi", { ludusaviTitle: "Zenless Zone Zero", registryOnly: true }),
];

const folderScan: Found[] = [
  found("folder:D:\\Games\\Hades\\x64\\Hades.exe", "Hades", "folder", { exe: "D:\\Games\\Hades\\x64\\Hades.exe", install: "D:\\Games\\Hades", ludusaviTitle: "Hades" }),
  found("folder:D:\\Games\\Some Indie\\SomeIndie.exe", "Some Indie", "folder", { exe: "D:\\Games\\Some Indie\\SomeIndie.exe", install: "D:\\Games\\Some Indie" }),
];

const sgdbCovers = [art(4079546474, "p.png"), art(2661209017, "p.jpg"), art(4000695018, "p.png")];
const sgdbHeroes = [art(4079546474, "_hero.png"), art(2661209017, "_hero.png"), art(4000695018, "_hero.jpg")];
const sgdbLogos = [art(4079546474, "_logo.png"), art(2661209017, "_logo.png"), art(4000695018, "_logo.png")];
const sgdbIcons = [art(4079546474, "_icon.png"), art(2661209017, "_icon.png"), art(4000695018, "_icon.png")];

const delay = <T,>(v: T, ms = 250) => new Promise<T>((r) => setTimeout(() => r(v), ms));

export async function mockInvoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
  const key = args.key as string;
  switch (cmd) {
    case "get_overview":
      return delay(overview, 150);
    case "get_language": {
      // ?lang=en / ?lang=zh-CN picks the language of the browser preview (docs screenshots).
      const fromUrl = new URLSearchParams(location.search).get("lang");
      if (fromUrl) return fromUrl;
      try {
        return localStorage.getItem("ln.lang") ?? "system";
      } catch {
        return "system";
      }
    }
    case "get_onboarding":
      // ?guide shows the first-run guide in the browser preview.
      return delay({ doneAt: new URLSearchParams(location.search).has("guide") ? null : ago(60), usage: [] });
    case "finish_onboarding":
      return delay(undefined, 50);
    case "set_language":
      try {
        localStorage.setItem("ln.lang", args.language as string);
      } catch {
        /* ignore */
      }
      return delay(undefined, 50);
    case "check_game":
      return delay(checks[key], 400);
    case "sync_game":
    case "backup_game":
      return delay<Outcome>({ kind: "uploaded", id: "01NEW" }, 700);
    case "list_snapshots":
      return delay({ snapshots: snapshots[key] ?? [], headId: snapshots[key]?.[0]?.id ?? null, lastSyncedId: snapshots[key]?.[0]?.id ?? null });
    case "get_snapshot": {
      const s = (snapshots[key] ?? []).find((x) => x.id === args.id)!;
      return delay({ ...s, game: key, roots: {}, appVersion: "0.1.0", files: [{ path: "install/save/slot_00/progress.sav", size: 162099, hash: "", mtime: now / 1000 }] });
    }
    case "local_files":
      return delay({
        files: [
          { logical: "install/save/account/0100000000000000706d635257464546/000/Auto", size: 30822, mtime: now / 1000 - 3600 },
          { logical: "install/save/account/0100000000000000706d635257464546/000/Global", size: 64, mtime: now / 1000 - 3600 },
        ],
        roots: { install: "C:\\Users\\me\\Desktop\\Switch\\fefw-recomp-0.2.7" },
      });
    case "steam_accounts":
      return delay({
        accounts: [
          { id: "12345678", persona: "player", accountName: null, mostRecent: true, shortcutCount: 3 },
          { id: "87654321", persona: "player-2", accountName: null, mostRecent: false, shortcutCount: 1 },
        ],
        selected: "12345678",
        root: "C:\\Program Files (x86)\\Steam",
      });
    case "steam_shortcuts":
      return delay(
        overview.games.map((g) => ({
          account: "12345678",
          appid: g.steam!.appid,
          name: g.name,
          nameCorrupted: false,
          suggestedName: g.name,
          exe: g.exe,
          startDir: g.install,
          launchOptions: "",
          adapter: g.adapter,
          adapterName: g.name,
          addedKey: g.key === "bloodborne-lantern" ? null : g.key,
          wrappedKey: null,
          cover: g.art.cover,
          icon: g.art.icon,
        })),
      );
    case "get_settings":
      return delay({
        deviceId: "d-mock",
        deviceName: "DESKTOP-MAIN",
        retention: { keepRecent: 20, keepDailyDays: 30 },
        networkTimeoutSecs: 8,
        steamRoot: "C:\\Program Files (x86)\\Steam",
        steamAccount: "12345678",
        cloud: { kind: "googleDrive", clientId: "", clientSecret: "", folder: "Ludonimbus", account: "player@example.com" },
        previousCloud: { kind: "localFolder", name: L("本地 / 同步文件夹（D:\\Cloud\\Ludonimbus）", "Local / synced folder (D:\\Cloud\\Ludonimbus)") },
        googleBuiltin: true,
        baiduBuiltin: false,
        baiduAppName: "Ludonimbus",
        dataDir: "C:\\Users\\me\\AppData\\Roaming\\Ludonimbus",
        runnerPath: "C:\\Users\\me\\AppData\\Local\\Ludonimbus\\ludonimbus-run.exe",
        runnerExists: true,
        ludusavi: "ludusavi 0.31.0",
        version: "0.1.0",
      });
    case "cloud_test":
      return delay({ label: "https://dav.jianguoyun.com/dav/Ludonimbus", used: 1.2e9, total: 3e9 });
    case "cloud_connect":
      return delay("https://dav.jianguoyun.com/dav/Ludonimbus");
    case "cloud_migrate":
      return delay({ games: 3, copied: 42, skipped: 0, bytes: 6.4e8 }, 1500);
    case "cloud_connect_baidu":
      return delay(L("玩家", "player"), 4000);
    case "cloud_cancel_login":
      return delay(undefined);
    case "cloud_dismiss_previous":
      return delay(undefined);
    case "cloud_account":
      return delay({ label: "player@example.com", used: 3.2e9, total: 15e9 });
    case "list_devices":
      return delay([
        { id: "d-mock", name: "DESKTOP-MAIN", os: "windows x86_64", appVersion: "0.1.0", lastSeen: ago(1) },
        { id: "d-laptop", name: "GPD-Win-Mini", os: "windows x86_64", appVersion: "0.1.0", lastSeen: ago(300) },
      ]);
    case "cloud_games":
      return delay([{ key: "hollow-knight", name: "Hollow Knight", adapter: null, saves: [], ludusaviTitle: "Hollow Knight" }]);
    case "ludusavi_search":
      return delay(["Hollow Knight", "Hollow Knight: Silksong"]);
    case "ludusavi_preview":
      return delay(["C:\\Users\\me\\AppData\\LocalLow\\Team Cherry\\Hollow Knight\\user1.dat"]);
    case "check_update":
      return delay(
        { current: "0.1.0", available: true, version: "0.1.1", notes: "### 中文\n\n- 修复：示例更新说明\n- 新增：示例功能\n\n### English\n\n- Fixed: sample release note\n- New: sample feature", date: new Date().toISOString() },
        600,
      );
    case "steam_plugin_status":
      return delay({
        steamRoot: "C:\Program Files (x86)\Steam",
        steamRunning: true,
        bundled: true,
        millennium: "missing",
        pluginInstalled: false,
        pluginEnabled: false,
        pluginCurrent: false,
      });
    case "steam_plugin_install":
      return delay({ millenniumInstalled: "v3.6.3", steamRestarted: true }, 1500);
    case "scan_quick":
      return delay(quickScan, 500);
    case "scan_ludusavi":
      return delay(ludusaviScan, 2500);
    case "scan_folder":
      return delay(folderScan, 600);
    case "game_folders":
      return delay([
        { label: "Steam C:", path: "C:\\Program Files (x86)\\Steam\\steamapps\\common" },
        { label: "Steam D:", path: "D:\\SteamLibrary\\steamapps\\common" },
        { label: "Xbox", path: "C:\\XboxGames" },
        { label: "Epic", path: "C:\\Program Files\\Epic Games" },
      ]);
    case "pick_exe":
      return delay(`${(args.start as string | null) ?? "C:\\Games"}\\Starfield\\Starfield.exe`);
    case "pick_folder":
      return delay("D:\\Games");
    case "add_found":
      return delay((args.found as Found).addedKey ?? (args.name as string).toLowerCase().replace(/[^a-z0-9]+/g, "-"), 300);
    case "detect_adapter": {
      const exe = (args.exe as string).toLowerCase();
      if (exe.endsWith("dolphin.exe")) return delay({ key: "dolphin", name: "Dolphin", label: L("Dolphin（整个模拟器的存档）", "Dolphin (whole emulator)"), emulator: true });
      if (exe.endsWith("soh.exe")) return delay({ key: "soh", name: L("塞尔达传说：时之笛", "The Legend of Zelda: Ocarina of Time"), label: L("塞尔达传说：时之笛", "The Legend of Zelda: Ocarina of Time"), emulator: false });
      return delay(null);
    }
    case "savefind_suggest":
      return delay([
        { path: "C:\\Users\\me\\AppData\\LocalLow\\Indie Studio\\Some Indie", files: 3, bytes: 48213, modified: ago(60 * 20), sample: ["save0.dat", "settings.json"], reason: L("名字和游戏相近", "similar name") },
        { path: "D:\\Games\\Some Indie\\saves", files: 2, bytes: 20480, modified: ago(60 * 24 * 3), sample: ["slot1.sav"], reason: L("在游戏目录里", "in the game folder") },
      ]);
    case "savefind_watch_start":
      return delay(undefined);
    case "savefind_watch_stop":
      return delay([{ path: "C:\\Users\\me\\Saved Games\\Some Indie", files: 1, bytes: 9120, modified: ago(1), sample: ["slot1.sav"], reason: L("刚才被写入", "just written") }], 600);
    case "folder_to_pattern":
      return delay(args.path as string);
    case "sgdb_status":
      return delay({ hasKey: true, keyPage: "https://www.steamgriddb.com/profile/preferences/api" });
    case "sgdb_set_key":
    case "sgdb_clear_key":
      return delay(undefined);
    case "sgdb_search":
      return delay([
        { id: 1, name: args.term as string, year: 2018, verified: true },
        { id: 2, name: `${args.term as string}: Farewell`, year: 2019, verified: false },
      ]);
    case "sgdb_images": {
      const pool = args.kind === "hero" || args.kind === "wide" ? sgdbHeroes : args.kind === "logo" ? sgdbLogos : args.kind === "icon" ? sgdbIcons : sgdbCovers;
      return delay(pool.map((u, i) => ({ id: i + 10, url: u, thumb: u, width: 600, height: 900, score: 3 - i, style: "alternate", author: "someone" })), 400);
    }
    case "art_set_url":
    case "art_set_file":
      return delay(undefined, 500);
    case "steam_add_games":
      return delay((args.keys as string[]).map((k) => ({ key: k, error: null })), 1200);
    case "steam_add_game":
      return delay(3000000001, 800);
    case "steam_remove_game":
      return delay(undefined, 500);
    case "set_game_exe": {
      const g = overview.games.find((x) => x.key === key);
      if (g) Object.assign(g, { exe: args.path as string, exeExists: true });
      return delay(undefined);
    }
    case "relocate_auto":
      return delay(key === "starfield" ? null : "C:\\Users\\me\\Desktop\\Switch\\fefw-recomp-0.2.8\\fefw.exe");
    default:
      return delay(null);
  }
}
