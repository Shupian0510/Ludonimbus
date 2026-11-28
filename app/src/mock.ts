// Browser-only mock backend, used when the UI runs outside Tauri
// (`npm run dev` in a normal browser) so the interface can be developed and
// reviewed without the Rust side. Never loaded inside the app.
import type {
  CloudOnlyView,
  Found,
  GameView,
  JoinSuggestion,
  OtherInstall,
  OtherSave,
  Outcome,
  Overview,
  PlatformMarks,
  PlaytimeDevice,
  PlaytimeSummary,
  SaveLine,
  SnapshotSummary,
  StatusReport,
  SteamCloudApp,
  SteamCloudDone,
  SteamCloudStatus,
  TrashedSnapshot,
  UnsentSave,
  UploadResult,
} from "./api";
import type {
  AccountInfo,
  AdapterInfo,
  CloudGame,
  CloudGameInfo,
  CloudLibrary,
  CollectionStats,
  DeviceInfo,
  Gallery,
  GameFolder,
  GamePatch,
  HandheldInfo,
  HandheldMode,
  LaunchInfo,
  LineList,
  LocalFiles,
  MigrateReport,
  NewerVersion,
  NxStatus,
  Onboarding,
  OpenListInfo,
  OtherSaves,
  SaveCandidate,
  SaveTitle,
  ScreenshotReport,
  Settings,
  SgdbGame,
  SgdbImage,
  SgdbStatus,
  ShortcutView,
  SnapshotList,
  SnapshotMeta,
  SteamAccounts,
  SteamAddResult,
  SteamPluginInstall,
  SteamPluginStatus,
  SyncFolder,
  TransferPlan,
  UpdateInfo,
} from "./bindings";

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

// ?scene=… (docs screenshots) adjusts the sample data to the screen shown.
const mockScene = new URLSearchParams(location.search).get("scene") ?? "";
const noCloudScene = ["settings-cloud", "settings-cloud-quark"].includes(mockScene) || mockScene.startsWith("guide-");

// Steam plugin installed in this preview session (Settings and the first-run guide).
let steamPluginOn = false;

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
    snap("01OOT5", 35, "DESKTOP-MAIN", "auto-exit", 1258291, 14, { installedVersion: "1.2.2" }),
    snap("01OOT4", 60 * 26, "GPD-Win-Mini", "auto-exit", 1240000, 14),
    snap("01OOT3", 60 * 50, "DESKTOP-MAIN", "manual", 1100000, 12, { comment: L("进入水之神殿前", "Before the Water Temple"), locked: true }),
    snap("01OOT2", 60 * 75, "DESKTOP-MAIN", "pre-restore", 1090000, 12),
    snap("01OOT1", 60 * 24 * 4, "DESKTOP-MAIN", "auto-exit", 800000, 10),
  ],
  "fe-fortunes-weave": [snap("01FE2", 60 * 3, "DESKTOP-MAIN", "auto-exit", 56422, 4, { gameVersion: "0.2.7" }), snap("01FE1", 60 * 30, "DESKTOP-MAIN", "auto-exit", 40100, 3)],
  "bloodborne-lantern": [snap("01BB2", 60 * 5, "GPD-Win-Mini", "auto-exit", 15309618, 16), snap("01BB1", 60 * 48, "DESKTOP-MAIN", "auto-exit", 15309618, 16)],
  "slay-the-spire": [snap("01STS2", 60 * 2, "DESKTOP-MAIN", "auto-exit", 48200, 6), snap("01STS1", 60 * 28, "DESKTOP-MAIN", "auto-exit", 47100, 6)],
  hades: [snap("01HAD2", 60 * 4, "Steam Deck", "auto-exit", 2310000, 5), snap("01HAD1", 60 * 52, "DESKTOP-MAIN", "auto-exit", 2290000, 5)],
  // Two store versions on one save line: the newest of each comes first.
  celeste: [
    snap("01CE4", 50, "DESKTOP-MAIN", "auto-exit", 6210, 3),
    snap("01CE3", 60 * 5, "LAPTOP", "auto-exit", 6100, 3, { platform: "pc:xbox" }),
    snap("01CE2", 60 * 30, "DESKTOP-MAIN", "manual", 5900, 3, { comment: L("第七章之前", "Before chapter 7"), locked: true }),
    snap("01CE1", 60 * 72, "LAPTOP", "auto-exit", 5040, 3, { platform: "pc:xbox" }),
    snap("01CE0", 60 * 96, "DESKTOP-MAIN", "auto-exit", 4800, 3),
  ],
  // The PC saved the newest with a newer game than the emulator's (see STARDEW_NX).
  "ryujinx-0100e65002bb8000": [
    snap("01SDV2", 60 * 2, "DESKTOP-MAIN", "auto-exit", 412000, 4, { platform: "pc", gameVersion: "1.6.15" }),
    snap("01SDV1", 60 * 26, "Steam Deck", "auto-exit", 398000, 4, { platform: "emu:ryujinx", gameVersion: "1.6.8" }),
  ],
};

// Played on the Xbox app, to send to its Steam version (`mockInstalls`); not in the docs screenshots.
if (!mockScene) snapshots["doom-eternal"] = [snap("01DOOM1", 90, "DESKTOP-MAIN", "auto-exit", 2140000, 12, { platform: "pc:xbox" })];
// Versions missing a content pack (not in the docs screenshots): Slay the Spire's newest one, from the laptop,
// is marked incomplete; one of its older ones was let go for it. Hades has a version this PC couldn't upload complete.
const unsent: Record<string, UnsentSave[]> = {};
if (!mockScene) {
  snapshots["slay-the-spire"].unshift(snap("01STS3", 40, "LAPTOP", "auto-exit", 49100, 6, { incomplete: true }));
  unsent.hades = [{ id: "01HADX", createdAt: ago(60 * 30), totalSize: 2304000, fileCount: 5, platform: "pc:steam", at: ago(12) }];
}

// What each game's saves are made on, and how the app shows that (the logo
// is the program's own icon when this PC has it).
const mockPlatforms: Record<string, string> = { "zelda-oot-longshot": "recomp:longshot", "fe-fortunes-weave": "recomp:fefw", "bloodborne-lantern": "emu:shadps4", "doom-eternal": "pc:xbox", "slay-the-spire": "pc:steam", hades: "pc:steam" };
const platformMarks: PlatformMarks = {
  "recomp:longshot": { label: "Longshot", logo: art(4079546474, "_icon.png") },
  "recomp:fefw": { label: "fefw recomp", logo: art(2661209017, "_icon.png") },
  "emu:shadps4": { label: "Lantern Engine", logo: art(4000695018, "_icon.png") },
  pc: { label: "PC", logo: null },
  "pc:xbox": { label: "Xbox", logo: null },
  "pc:steam": { label: "Steam", logo: null },
};
for (const [k, list] of Object.entries(snapshots)) for (const s of list) s.platform ??= mockPlatforms[k] ?? "pc";

// The save this PC is at, where it isn't the newest one; the line each game follows here.
const syncedId: Record<string, string> = { "bloodborne-lantern": "01BB1", "slay-the-spire": "01STS2" };
const lineOf: Record<string, string> = {};
const joinAnswered = new Set<string>();
const undoTo: Record<string, string> = {};
let made = 0;
const headOf = (key: string) => (snapshots[key] ?? []).find((s) => s.trigger !== "pre-restore" && s.trigger !== "conflict-keep")?.id ?? null;
const kidLine = () => ({ id: "solo-9d2k", name: L("孩子的电脑", "Kid's PC") });
const kidSave = () => snap("01KID1", 60 * 20, "KID-PC", "auto-exit", 905000, 11, { platform: "recomp:longshot", head: true });

/** Who is on which save line of a game. */
function mockLines(key: string): SaveLine[] {
  const p = mockPlatforms[key] ?? "pc";
  const at = lineOf[key] ?? "main";
  const lines: SaveLine[] = [{ id: "main", name: "", shared: true, current: false, devices: [], members: [] }];
  if (key === "zelda-oot-longshot") {
    lines[0].members.push({ device: "Steam Deck", platform: p, here: false });
    lines.push({ ...kidLine(), shared: false, current: false, devices: [], members: [{ device: "KID-PC", platform: p, here: false }] });
  }
  if (key === "celeste") lines[0].members.push({ device: "LAPTOP", platform: "pc:xbox", here: false });
  if (key === "hades") lines[0].members.push({ device: "Steam Deck", platform: p, here: false });
  if (at === "own") lines.push({ id: "own", name: "DESKTOP-MAIN", shared: false, current: false, devices: [], members: [] });
  const mine = lines.find((l) => l.id === at) ?? lines[0];
  mine.current = true;
  mine.members.push({ device: "DESKTOP-MAIN", platform: p, here: true });
  for (const l of lines) l.devices = [...new Set(l.members.map((m) => m.device))];
  return lines;
}

const trash: Record<string, TrashedSnapshot[]> = {
  "zelda-oot-longshot": [{ snapshot: snap("01OOT0", 60 * 24 * 6, "DESKTOP-MAIN", "auto-exit", 790000, 10), deletedAt: ago(60 * 24 * 2), deletedBy: "DESKTOP-MAIN" }],
};
if (!mockScene) {
  trash["slay-the-spire"] = [
    { snapshot: snap("01STS0", 60 * 24 * 20, "LAPTOP", "auto-exit", 45000, 6), deletedAt: ago(60 * 24 * 3), deletedBy: "LAPTOP", reason: "incomplete" },
  ];
}

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
  "ryujinx-0100e65002bb8000": { ...report("ryujinx-0100e65002bb8000", "cloudNewer", 4, 398000), tooNew: "1.6.15" },
};
if (!mockScene) checks["slay-the-spire"] = { ...report("slay-the-spire", "synced", 6, 48200), incomplete: true };

function game(key: string, name: string, appid: number, platform: string, exe: string, saves: string[], extra: Partial<GameView> = {}): GameView {
  return {
    key,
    name,
    adapter: key,
    adapterName: name,
    platform,
    exe,
    exeExists: true,
    playable: true,
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
    playtime: playtime[key] ?? { totalSecs: 0, lastPlayed: null, devices: [] },
    // A recomp's logo is its program's icon, which is the game's icon here.
    source: { kind: "recomp", name: null, detail: platform || null, label: null, logo: appid ? art(appid, "_icon.png") : null },
    family: key,
    link: null,
    joined: false,
    xboxContainers: false,
    replaced: null,
    store: null,
    autoPull: !!appid,
    ...extra,
  };
}

/** A game only the cloud has (added on another device). */
function cloudGame(key: string, name: string, family: string, source: CloudOnlyView["source"], extra: Partial<CloudOnlyView> = {}): CloudOnlyView {
  return {
    key,
    name,
    adapter: null,
    adapterName: null,
    platform: null,
    ludusaviTitle: name,
    saves: [],
    source,
    art: { cover: null, wide: null, hero: null, logo: null, icon: null },
    playtime: devicePlaytime(0, 6, 0),
    family,
    link: null,
    ...extra,
  };
}

/** An emulator game (no artwork in the preview: shows the name instead). */
/** A Switch emulator's Stardew Valley: its PC version's line keeps the same saves by the game's rule. */
const STARDEW_NX = "ryujinx-0100e65002bb8000";

function emuGame(key: string, name: string, emulator: string, platform: string, exe: string, saves: string[], hours: number): GameView {
  return game(key, name, 0, platform, exe, saves, {
    adapter: emulator.toLowerCase(),
    adapterName: emulator,
    customSaves: true,
    steam: null,
    health: { state: "notLinked" },
    art: { cover: null, wide: null, hero: null, logo: null, icon: null },
    lastSyncAt: ago(60 * hours),
    lastSyncedDevice: "DESKTOP-MAIN",
    playtime: devicePlaytime(hours, 0, 0),
    // No emulator logos in the preview: they come from the emulators themselves.
    source: { kind: "emulator", name: emulator, detail: platform, label: null, logo: null },
  });
}

function devicePlaytime(hours: number, laptopHours: number, steamHours: number): PlaytimeSummary {
  const devices: PlaytimeDevice[] = [
    { device: "d-mock", deviceName: "DESKTOP-MAIN", secs: hours * 3600, lastPlayed: ago(26), thisDevice: true, steamMinutes: steamHours ? steamHours * 60 : null },
  ];
  if (laptopHours) devices.push({ device: "d-laptop", deviceName: "LAPTOP", secs: laptopHours * 3600, lastPlayed: ago(60 * 30), thisDevice: false, steamMinutes: null });
  return { totalSecs: (hours + laptopHours) * 3600, lastPlayed: ago(26), devices };
}

const playtime: Record<string, PlaytimeSummary> = {
  "zelda-oot-longshot": devicePlaytime(16.5, 4.2, 12),
  "fe-fortunes-weave": devicePlaytime(7.3, 0, 0),
  "bloodborne-lantern": devicePlaytime(31, 9.5, 20),
  // Played as two versions: the time per platform shows with the total.
  celeste: {
    ...devicePlaytime(9, 3.5, 0),
    platforms: [
      { platform: "pc:steam", secs: 9 * 3600 },
      { platform: "pc", secs: 3.5 * 3600 },
    ],
    platformNames: { "pc:steam": "Steam", pc: "PC" },
  },
};

const overview: Overview = {
  deviceId: "d-mock",
  deviceName: "DESKTOP-MAIN",
  cloud: { kind: "googleDrive", label: "player@example.com", error: null },
  steamFound: true,
  steamRunning: true,
  cloudFetchedAt: ago(2),
  // Hollow Knight was added on the laptop only; so was Celeste's GOG version
  // (one game with this PC's Celeste in the save library).
  cloudOnly: [
    // Played to the end on the laptop and put away: on the collection's shelves, not in the library.
    cloudGame("hollow-knight", "Hollow Knight", "hollow-knight", { kind: "pc", name: null, detail: null, label: null, logo: null }, { shelves: ["finished", "archived"] }),
    cloudGame("celeste-gog", "Celeste", "celeste", { kind: "pc", name: null, detail: null, label: "GOG", logo: null }),
  ],
  games: [
    game("zelda-oot-longshot", L("塞尔达传说：时之笛", "The Legend of Zelda: Ocarina of Time"), 4079546474, L("Longshot（原生重制）", "Longshot (native port)"), "C:\\Users\\me\\AppData\\Local\\Programs\\LongshotLauncher\\LongshotLauncher.exe", ["{data}/save/**"]),
    game("fe-fortunes-weave", L("火焰纹章：万缕千丝", "Fire Emblem: Fortune's Weave"), 2661209017, "fefw recomp", "C:\\Users\\me\\Desktop\\Switch\\fefw-recomp-0.2.7\\fefw.exe", ["{install}/save/**"], {
      art: { cover: art(2661209017, "p.jpg"), wide: null, hero: art(2661209017, "_hero.png"), logo: art(2661209017, "_logo.png"), icon: art(2661209017, "_icon.png") },
      health: { state: "changed", exe: "C:\\Users\\me\\Desktop\\Switch\\fefw-recomp-0.2.7\\fefw.exe", launchOptions: "" },
      shelves: ["finished"],
    }),
    game("bloodborne-lantern", "Bloodborne", 4000695018, L("Lantern Engine（shadPS4）", "Lantern Engine (shadPS4)"), "C:\\Users\\me\\Desktop\\Switch\\Bloodborne\\BloodbornePC\\Bloodborne.exe", ["{install}/user/savedata/1/CUSA00207/**"], {
      art: { cover: art(4000695018, "p.png"), wide: null, hero: art(4000695018, "_hero.jpg"), logo: art(4000695018, "_logo.png"), icon: art(4000695018, "_icon.png") },
      conflict: { cloudId: "01BB2", cloudDevice: "GPD-Win-Mini", cloudCreatedAt: ago(300), localHash: "y", detectedAt: ago(10), localIsCandidate: true },
    }),
    game("celeste", "Celeste", 0, "", "D:\\Games\\Celeste\\Celeste.exe", ["{data}/Saves/**"], {
      adapter: null,
      adapterName: null,
      platform: null,
      ludusaviTitle: "Celeste",
      steam: null,
      health: { state: "notLinked" },
      art: { cover: null, wide: null, hero: null, logo: null, icon: null },
      source: { kind: "pc", name: null, detail: null, label: null, logo: null },
    }),
    game("starfield", "Starfield", 0, "", "", ["{ludusavi}"], {
      adapter: null,
      adapterName: null,
      platform: null,
      exe: null,
      exeExists: false,
      playable: false,
      install: null,
      ludusaviTitle: "Starfield",
      steam: null,
      health: { state: "notLinked" },
      art: { cover: null, wide: null, hero: null, logo: null, icon: null },
      source: { kind: "pc", name: null, detail: null, label: null, logo: null },
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
      source: { kind: "xbox", name: null, detail: null, label: null, logo: null },
    }),
  ],
};

// Emulator games for the source groups; left out of the docs screenshots
// (?scene=…) except the one that shows the groups.
if (!mockScene || mockScene === "library-sources") {
  const gcWii = L("GameCube / Wii 模拟器", "GameCube / Wii emulator");
  const dolphin = "D:\\Emulators\\Dolphin-x64\\Dolphin.exe";
  const ryujinx = "C:\\Users\\me\\Desktop\\Switch\\ryujinx\\Ryujinx.exe";
  overview.games.push(
    emuGame("dolphin-rmge01", L("超级马力欧银河", "Super Mario Galaxy"), "Dolphin", gcWii, dolphin, ["{emu}/Wii/title/0001000?/524d4745/data/**"], 12.5),
    emuGame("dolphin-gale01", "Super Smash Bros. Melee", "Dolphin", gcWii, dolphin, ["{emu}/GC/*/Card ?/01-GALE-*.gci"], 40),
    emuGame("dolphin-gzle01", L("塞尔达传说：风之杖", "The Legend of Zelda: The Wind Waker"), "Dolphin", gcWii, dolphin, ["{emu}/GC/*/Card ?/01-GZLE-*.gci"], 22),
    emuGame("ryujinx-0100770008dd8000", "MONSTER HUNTER GENERATIONS ULTIMATE", "Ryujinx", L("Switch 模拟器", "Switch emulator"), ryujinx, ["{nxsave-0100770008dd8000}/**"], 64),
    emuGame("ryujinx-01006a800016e000", L("超级马力欧 奥德赛", "Super Mario Odyssey"), "Ryujinx", L("Switch 模拟器", "Switch emulator"), ryujinx, ["{nxsave-01006a800016e000}/**"], 18),
    // Its PC version's line keeps the same saves by the game's rule: asked once (join_suggestion).
    emuGame("ryujinx-0100e65002bb8000", L("星露谷物语", "Stardew Valley"), "Ryujinx", L("Switch 模拟器", "Switch emulator"), ryujinx, ["{nxsave-0100e65002bb8000}/**"], 9),
    // The disc image it was started with is gone: saves only.
    { ...emuGame("pcsx2-slus21115", L("大神", "Okami"), "PCSX2", L("PS2 模拟器", "PS2 emulator"), "D:\\Emulators\\PCSX2\\pcsx2-qt.exe", ["{memcards}/Mcd001.ps2"], 31), playable: false },
    emuGame("azahar-00040000001b5000", L("宝可梦 究极之日", "Pokémon Ultra Sun"), "Azahar", L("3DS 模拟器", "3DS emulator"), "D:\\Emulators\\Azahar\\azahar.exe", ["{user}/sdmc/Nintendo 3DS/*/*/title/00040000/001b5000/data/**"], 45),
    // Added by hand (no rule), put in a group named by the user.
    game("mgba-emerald", L("宝可梦 绿宝石", "Pokémon Emerald"), 0, "", "D:\\Emulators\\mGBA\\mGBA.exe", ["{install}/saves/emerald.sav"], {
      adapter: null,
      adapterName: null,
      platform: null,
      customSaves: true,
      steam: null,
      health: { state: "notLinked" },
      art: { cover: null, wide: null, hero: null, logo: null, icon: null },
      source: { kind: "custom", name: null, detail: null, label: "mGBA", logo: null },
    }),
  );
}

// Games Steam itself installed (see `steamCloudApps`): one backed up beside
// Steam Cloud, one Ludonimbus syncs. Not in the docs screenshots.
if (!mockScene) {
  const steamGame = (key: string, name: string, exe: string) =>
    game(key, name, 0, "", exe, ["{ludusavi}"], {
      adapter: null,
      adapterName: null,
      platform: null,
      ludusaviTitle: name,
      steam: null,
      health: { state: "notLinked" },
      art: { cover: null, wide: null, hero: null, logo: null, icon: null },
      source: { kind: "pc", name: null, detail: null, label: null, logo: null },
    });
  overview.games.push(
    steamGame("slay-the-spire", "Slay the Spire", "D:\\SteamLibrary\\steamapps\\common\\SlayTheSpire\\SlayTheSpire.exe"),
    steamGame("hades", "Hades", "D:\\SteamLibrary\\steamapps\\common\\Hades\\x64\\Hades.exe"),
  );
}

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
    storeAppid: null,
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

/** Games Steam installed: one backed up, one taken over, one left to Steam (Steam Cloud not done syncing), one without Steam Cloud. */
const steamCloudApps: SteamCloudApp[] = [
  { appid: 646570, name: "Slay the Spire", key: "slay-the-spire", state: "backup", syncState: "synchronized", cloudEnabled: true, cloudUsed: true },
  { appid: 1145360, name: "Hades", key: "hades", state: "managed", syncState: "synchronized", cloudEnabled: false, cloudUsed: true },
  { appid: 1716740, name: "Starfield", key: null, state: "steam", syncState: "changesincloud", cloudEnabled: true, cloudUsed: true },
  { appid: 2527500, name: "MiSide", key: null, state: "steam", syncState: null, cloudEnabled: true, cloudUsed: false },
];

function mockSteamStatus(key: string): SteamCloudStatus {
  const a = steamCloudApps.find((x) => x.key === key);
  return {
    mode: a?.state ?? "backup",
    syncState: a ? a.syncState : "synchronized",
    synchronized: (a ? a.syncState : "synchronized") === "synchronized",
    cloudEnabled: a?.cloudEnabled ?? true,
    cloudUsed: a?.cloudUsed ?? true,
    globalEnabled: true,
    steamRunning: true,
    wrapper: a?.state === "managed" && !mockNoWrapper.has(key) ? "ok" : null,
  };
}

/** Games taken over without the wrapper in their launch options. */
const mockNoWrapper = new Set<string>();

/** `GameView.store` of a game in the mock library. */
function mockStore(g: GameView): GameView["store"] {
  const a = steamCloudApps.find((x) => x.key === g.key);
  if (!a) return null;
  const managed = a.state === "managed";
  return { appid: a.appid, mode: managed ? "managed" : g.autoSync ? "backup" : "steam", wrapper: managed && !mockNoWrapper.has(g.key) };
}

/**
 * Versions installed here that aren't in the library (`other_installs`):
 * Steam's games not added yet, and the Steam version of the Xbox game DOOM
 * Eternal (not in the docs screenshots).
 */
function mockInstalls(key: string | null): OtherInstall[] {
  const norm = (n: string) => n.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");
  const all = [...overview.games, ...overview.cloudOnly];
  const of = all.find((g) => g.key === key);
  const names = new Set(all.filter((g) => of && g.family === of.family).map((g) => norm(g.name)));
  const extra = mockScene ? [] : [{ appid: 782330, name: "DOOM Eternal" }];
  return [...extra.filter((e) => !all.some((g) => g.key === `steam-${e.appid}`)), ...steamCloudApps.filter((a) => !a.key)].map((a) => ({
    id: `steam:${a.appid}`,
    name: a.name,
    store: "steam",
    logo: null,
    same: names.has(norm(a.name)),
  }));
}

/** Adds a version installed here as the backend would: a game Steam installed comes in backup only. */
function mockAddInstall(id: string): string {
  const appid = Number(id.replace("steam:", ""));
  const listed = steamCloudApps.find((a) => a.appid === appid);
  if (listed) return mockSteamAdd([appid])[0].key!;
  const key = `steam-${appid}`;
  const twin = overview.games.find((g) => g.key === "doom-eternal");
  overview.games.push({
    ...twin!,
    key,
    exe: "D:\\SteamLibrary\\steamapps\\common\\DOOMEternal\\DOOMEternalx64vk.exe",
    lastCheck: null,
    lastSyncAt: null,
    lastSyncedDevice: null,
    source: { kind: "pc", name: null, detail: null, label: null, logo: null },
  });
  steamCloudApps.push({ appid, name: "DOOM Eternal", key, state: "backup", syncState: "synchronized", cloudEnabled: true, cloudUsed: true });
  return key;
}

/** Adds games Steam installed to the mock library, backup only. */
function mockSteamAdd(appids: number[]): SteamCloudDone[] {
  return appids.map((appid) => {
    const a = steamCloudApps.find((x) => x.appid === appid);
    if (!a) return { appid, key: null, error: L("Steam 里没有装这个游戏", "This game isn't installed in Steam") };
    a.key ??= a.name.toLowerCase().replace(/[^a-z0-9]+/g, "-");
    if (a.state === "steam") a.state = a.cloudUsed ? "backup" : "managed";
    return { appid, key: a.key, error: null };
  });
}

/** Takes a game over as the backend would, or refuses with its error code. */
function mockSteamTakeOver(key: string, wrapper = true): SteamCloudStatus {
  const a = steamCloudApps.find((x) => x.key === key);
  if (a && a.cloudUsed && a.syncState !== "synchronized")
    throw L("STEAM_NOT_SYNCED: Steam 上次没有和云端同步完。请打开 Steam 等它同步完成，然后再接管", "STEAM_NOT_SYNCED: Steam didn't finish syncing with Steam Cloud last time. Start Steam and let it finish, then take it over");
  if (a?.cloudUsed && a.cloudEnabled)
    throw L("STEAM_CLOUD_ON: Steam 云存档还开着。请在 Steam 里右键这个游戏 → 属性 → 通用，关掉它，然后再接管", "STEAM_CLOUD_ON: Steam Cloud is still on. In Steam, right-click the game → Properties → General and turn it off, then take it over");
  if (a) a.state = "managed";
  if (wrapper) mockNoWrapper.delete(key);
  else mockNoWrapper.add(key);
  const g = overview.games.find((x) => x.key === key);
  if (g) g.autoSync = true;
  return mockSteamStatus(key);
}

let mockHandheldMode: HandheldMode = "auto";
const mockNx = {
  enabled: false,
  running: false,
  address: "192.168.1.20",
  port: 47716,
  fingerprint: "b4bdeea64b1574511583648318fb8953f8ca4905d04b4a460ede11ce11d6d454",
  devices: [{ id: "sw-1", name: "Switch", pairedAt: new Date(Date.now() - 86400000).toISOString(), lastSeen: new Date(Date.now() - 3600000).toISOString() }],
  pairingCode: null as string | null,
  error: null,
};

export async function mockInvoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
  const key = args.key as string;
  switch (cmd) {
    case "get_overview": {
      // A copy, so the interface sees the changes the mock commands make.
      const o: Overview = JSON.parse(JSON.stringify(overview));
      if (noCloudScene) o.cloud = { kind: "none", label: null, error: null };
      if (mockScene === "settings-cloud-folder") o.cloud = { kind: "localFolder", label: "D:\\Cloud\\Ludonimbus", error: null };
      if (mockScene === "game-detail-pending") {
        const g = o.games.find((x) => x.key === "zelda-oot-longshot")!;
        g.pending = 2;
      }
      for (const g of o.games) g.store = mockStore(g);
      return delay(o, 150);
    }
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
      return delay<Onboarding>({ doneAt: new URLSearchParams(location.search).has("guide") || mockScene.startsWith("guide-") ? null : ago(60), usage: [] });
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
      if (mockScene === "game-detail-pending" && key === "zelda-oot-longshot")
        return delay<StatusReport>({ ...checks[key], status: "offline", message: L("云端暂时无法访问：网络连接超时", "The cloud can't be reached right now: connection timed out") }, 400);
      return delay(checks[key], 400);
    case "sync_game":
    case "backup_game":
      return delay<Outcome>({ kind: "uploaded", id: "01NEW" }, 700);
    case "upload_game":
      return delay<UploadResult>({ outcome: { kind: "upToDate" }, check: checks[key] }, 300);
    case "share_offer": {
      const s = (await mockInvoke("join_suggestion", args)) as JoinSuggestion | null;
      return s?.fit === "ready" ? { suggestion: s, label: s.rule ? (s.from ?? "PC") : "Steam" } : null;
    }
    case "game_lines":
      return delay(mockLines(key), 200);
    case "game_line_members":
      return delay<LineList>({ lines: mockLines(key), platforms: platformMarks }, 200);
    case "use_game_line":
      lineOf[key] = args.kind === "shared" ? "main" : args.kind === "independent" ? "own" : String(args.line);
      return delay<Outcome>({ kind: "upToDate" }, 500);
    case "other_saves": {
      // The newest save of each line this PC doesn't follow.
      const saves: OtherSave[] = [];
      const at = lineOf[key] ?? "main";
      const head = (snapshots[key] ?? []).find((s) => s.id === headOf(key));
      if (at !== "main" && head) saves.push({ line: "main", name: "", shared: true, snapshot: head });
      if (key === "zelda-oot-longshot" && at !== kidLine().id) saves.push({ line: kidLine().id, name: kidLine().name, shared: false, snapshot: kidSave() });
      return delay<OtherSaves>({ saves, platforms: platformMarks }, 250);
    }
    case "restore_snapshot":
    case "take_save":
    case "undo_replace": {
      // The chosen save becomes a new, newest one made here; what was here can be taken back.
      const list = snapshots[key] ?? [];
      const g = overview.games.find((x) => x.key === key);
      const from = cmd === "undo_replace" ? list.find((x) => x.id === undoTo[key])! : (list.find((x) => x.id === args.id) ?? kidSave());
      undoTo[key] = syncedId[key] ?? headOf(key) ?? "";
      if (g) g.replaced = { device: from.deviceName, at: new Date().toISOString() };
      const id = `01NEW${++made}`;
      snapshots[key] = [
        { ...from, id, createdAt: new Date().toISOString(), deviceId: "d-mock", deviceName: "DESKTOP-MAIN", trigger: "restore", comment: "", locked: false, head: undefined, side: undefined },
        ...list,
      ];
      delete syncedId[key];
      return delay<Outcome>({ kind: "uploaded", id }, 600);
    }
    case "set_snapshot": {
      const s = (snapshots[key] ?? []).find((x) => x.id === args.id);
      if (s && args.locked != null) s.locked = Boolean(args.locked);
      if (s && args.comment != null) s.comment = String(args.comment);
      return delay(undefined);
    }
    case "export_snapshot":
      return delay(`C:\\Users\\me\\Downloads\\${overview.games.find((g) => g.key === key)?.name ?? key}.lnsave`, 300);
    case "import_shared_save": {
      // A friend's save: beside the history, kept, from no device of this account.
      const list = (snapshots[key] ??= []);
      const like = list[0];
      const id = `shared${list.length}`;
      list.push({
        ...(like ?? {}),
        id,
        deviceId: "shared",
        deviceName: L("分享", "Shared"),
        createdAt: new Date().toISOString(),
        trigger: "shared",
        comment: L("分享的存档", "Shared save"),
        locked: true,
        side: true,
        head: false,
        contentHash: id,
      } as SnapshotSummary);
      return delay(id, 600);
    }
    case "join_suggestion": {
      // The Xbox version here keeps the same saves as the laptop's version, in other folders.
      const s: JoinSuggestion = {
        record: key,
        def: "mock",
        device: "LAPTOP",
        platform: "pc",
        createdAt: ago(60 * 5),
        confirmed: false,
        fit: "ready",
        // Through its save containers: the files in them under the other version's names.
        binds: [{ canon: "home/Saved Games/id Software/DOOMEternal/base/savegame", local: "localappdata/Packages/BethesdaSoftworks.DOOMEternal-PC_3275kfvn8vcwc/SystemAppData/wgs/0009_00AB", codec: "wgs" }],
        matched: 6,
        onlyInLine: 0,
        onlyHere: 1,
        uncovered: [],
        outside: ["savegame/crash.dmp"],
        others: 0,
      };
      // A Switch emulator's version and the PC version's line: the same files by the game's rule.
      const byRule: JoinSuggestion = {
        record: key,
        def: "mock-pc",
        device: "DESKTOP-MAIN",
        platform: "pc",
        createdAt: ago(60 * 26),
        confirmed: false,
        fit: "ready",
        binds: [{ canon: "appdata/StardewValley/Saves", local: "nxsave-0100e65002bb8000" }],
        matched: 4,
        onlyInLine: 0,
        onlyHere: 0,
        uncovered: [],
        outside: [],
        rule: L("星露谷物语", "Stardew Valley"),
        from: "PC",
        others: 2,
      };
      if (joinAnswered.has(key)) return delay(null, 200);
      return delay(key === "doom-eternal" ? s : key === STARDEW_NX ? byRule : null, 200);
    }
    case "join_answer": {
      joinAnswered.add(key);
      const g = overview.games.find((x) => x.key === key);
      if (g) {
        g.joined = args.answer === "accept";
        g.xboxContainers = g.joined && !!(args.suggestion as JoinSuggestion | undefined)?.binds.some((b) => b.codec === "wgs");
      }
      return delay<Outcome | null>(args.answer === "decline" ? null : { kind: "upToDate" }, 500);
    }
    case "screenshot_gallery":
      return delay(mockGallery(key), 300);
    case "screenshot_fetch":
      return delay(shotImage(String(args.name), Boolean(args.thumb)), 200);
    case "screenshot_sync":
      return delay<ScreenshotReport>({ uploaded: 1, downloaded: 2, failed: 0 }, 900);
    case "screenshot_reveal":
      return delay(undefined);
    case "list_snapshots":
      return delay<SnapshotList>({
        snapshots: snapshots[key] ?? [],
        headId: headOf(key),
        // The PC's newest save is too new for the emulator's game: this device is a version behind.
        lastSyncedId: key === STARDEW_NX ? "01SDV1" : (syncedId[key] ?? headOf(key)),
        platforms: platformMarks,
        tooNew: key === STARDEW_NX ? ["01SDV2"] : undefined,
        unsent: unsent[key],
      });
    case "export_unsent":
      return delay(`C:\\Users\\me\\Desktop\\${args.id}.zip`);
    case "dismiss_unsent":
      unsent[key] = (unsent[key] ?? []).filter((u) => u.id !== args.id);
      return delay(undefined);
    case "delete_snapshot": {
      const list = snapshots[key] ?? [];
      const s = list.find((x) => x.id === args.id);
      if (s) {
        snapshots[key] = list.filter((x) => x !== s);
        trash[key] = [{ snapshot: s, deletedAt: new Date().toISOString(), deletedBy: "DESKTOP-MAIN" }, ...(trash[key] ?? [])];
      }
      return delay(undefined);
    }
    case "deleted_snapshots":
      return delay(trash[key] ?? []);
    case "restore_deleted_snapshot":
    case "purge_deleted_snapshot": {
      const t = (trash[key] ?? []).find((x) => x.snapshot.id === args.id);
      trash[key] = (trash[key] ?? []).filter((x) => x !== t);
      const restore = cmd === "restore_deleted_snapshot" && t;
      if (restore) snapshots[key] = [...(snapshots[key] ?? []), { ...t.snapshot, locked: true }].sort((a, b) => b.createdAt.localeCompare(a.createdAt));
      return delay(restore ? true : undefined);
    }
    case "get_snapshot": {
      const s = (snapshots[key] ?? []).find((x) => x.id === args.id)!;
      return delay<SnapshotMeta>({ ...s, game: key, roots: {}, appVersion: "0.1.0", files: [{ path: "install/save/slot_00/progress.sav", size: 162099, hash: "", mtime: now / 1000 }] });
    }
    case "local_files":
      return delay<LocalFiles>({
        files: [
          { logical: "install/save/account/0100000000000000706d635257464546/000/Auto", size: 30822, mtime: now / 1000 - 3600 },
          { logical: "install/save/account/0100000000000000706d635257464546/000/Global", size: 64, mtime: now / 1000 - 3600 },
        ],
        roots: { install: "C:\\Users\\me\\Desktop\\Switch\\fefw-recomp-0.2.7" },
      });
    case "steam_accounts":
      return delay<SteamAccounts>({
        accounts: [
          { id: "12345678", persona: "player", accountName: null, mostRecent: true, shortcutCount: 3 },
          { id: "87654321", persona: "player-2", accountName: null, mostRecent: false, shortcutCount: 1 },
        ],
        selected: "12345678",
        root: "C:\\Program Files (x86)\\Steam",
      });
    case "steam_shortcuts":
      return delay<ShortcutView[]>(
        overview.games.map((g) => ({
          account: "12345678",
          appid: g.steam!.appid,
          name: g.name,
          nameCorrupted: false,
          suggestedName: g.name,
          exe: g.exe ?? "",
          startDir: g.install ?? "",
          launchOptions: "",
          adapter: g.adapter,
          adapterName: g.name,
          saves: [],
          addedKey: g.key === "bloodborne-lantern" ? null : g.key,
          wrappedKey: null,
          cover: g.art.cover,
          icon: g.art.icon,
        })),
      );
    case "get_settings": {
      const s: Settings = {
        deviceId: "d-mock",
        deviceName: "DESKTOP-MAIN",
        retention: { keepRecent: 20, keepDailyDays: 30 },
        networkTimeoutSecs: 8,
        steamRoot: "C:\\Program Files (x86)\\Steam",
        steamRootCustom: false,
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
        version: "0.5.0",
        screenshots: "sync",
        independentSaves: false,
        launchWaitSecs: 10,
        exitWaitSecs: 10,
        waitForUpload: false,
        cloudNewerNotifications: true,
      };
      if (mockScene && mockScene !== "settings-cloud-migrate") s.previousCloud = null;
      if (noCloudScene) s.cloud = { kind: "none" };
      if (mockScene === "settings-cloud-folder") s.cloud = { kind: "localFolder", path: "D:\\Cloud\\Ludonimbus" };
      return delay(s);
    }
    case "cloud_test":
      return delay<AccountInfo>({ label: "https://dav.jianguoyun.com/dav/Ludonimbus", used: 1.2e9, total: 3e9 });
    case "cloud_connect":
      return delay("https://dav.jianguoyun.com/dav/Ludonimbus");
    case "cloud_migrate":
      return delay<MigrateReport>({ games: 3, copied: 42, skipped: 0, bytes: 6.4e8 }, 1500);
    case "cloud_connect_baidu":
      return delay(L("玩家", "player"), 4000);
    case "openlist_status":
      return delay<OpenListInfo>({
        installed: false,
        version: "v4.2.10",
        size: 59999218,
        url: "https://github.com/OpenListTeam/OpenList/releases/download/v4.2.10/openlist-windows-amd64.zip",
        asset: "openlist-windows-amd64.zip",
        project: "https://github.com/OpenListTeam/OpenList",
        license: "AGPL-3.0",
      });
    case "openlist_install":
      return delay(undefined, 1500);
    case "openlist_pick_zip":
      return delay(null);
    case "cloud_connect_quark":
      return delay(L("夸克网盘 · Ludonimbus", "Quark · Ludonimbus"), 3000);
    case "cloud_cancel_login":
      return delay(undefined);
    case "cloud_dismiss_previous":
      return delay(undefined);
    case "cloud_account":
      if (mockScene === "settings-cloud-folder") return delay<AccountInfo>({ label: "D:\\Cloud\\Ludonimbus", used: null, total: null });
      return delay<AccountInfo>({ label: "player@example.com", used: 3.2e9, total: 15e9 });
    case "list_devices":
      return delay<DeviceInfo[]>([
        { id: "d-mock", name: "DESKTOP-MAIN", os: "windows x86_64", appVersion: "1.0.0", lastSeen: ago(1), layout: 2 },
        { id: "d-laptop", name: "LAPTOP", os: "windows x86_64", appVersion: "1.0.0", lastSeen: ago(300), layout: 2 },
        // Still on 0.6: its record says no layout.
        { id: "d-handheld", name: "GPD-Win-Mini", os: "windows x86_64", appVersion: "0.6.2", lastSeen: ago(60 * 26) },
      ]);
    case "nx_status":
    case "nx_set_enabled":
    case "nx_start_pairing":
    case "nx_cancel_pairing":
    case "nx_remove_device": {
      if (cmd === "nx_set_enabled") mockNx.enabled = args.enabled as boolean;
      if (cmd === "nx_start_pairing") mockNx.pairingCode = "482915";
      if (cmd === "nx_cancel_pairing") mockNx.pairingCode = null;
      if (cmd === "nx_remove_device") mockNx.devices = [];
      return delay<NxStatus>({ ...mockNx, running: mockNx.enabled });
    }
    case "launch_mode": {
      // Browser preview: `?handheld` opens the handheld view, `?prompt` a launch question.
      const q = new URLSearchParams(location.search);
      if (q.has("prompt"))
        return delay<LaunchInfo>({
          mode: "prompt",
          prompt: {
            title: L("Ludonimbus · 存档冲突", "Ludonimbus · Save conflict"),
            text: L(
              "「Bloodborne」的存档在本机和云端都有新的改动。\n\n云端：GPD-Win-Mini 于 2026-11-24 21:30 保存（16 个文件，14.6 MB）\n本机：16 个文件，14.6 MB\n\n请选择这次要使用哪一份。没选的那份也会留在「存档」里，随时可以设回去。",
              "Both this PC and the cloud have new changes to the saves of \"Bloodborne\".\n\nCloud: saved by GPD-Win-Mini at 2026-11-24 21:30 (16 files, 14.6 MB)\nThis PC: 16 files, 14.6 MB\n\nChoose which one to use this time. The other one stays under Saves and can be made current again at any time.",
            ),
            level: "warn",
            buttons: [
              { id: "cloud", label: L("使用云端存档", "Use cloud saves"), key: "a" },
              { id: "local", label: L("使用本机存档", "Use this PC's saves"), key: "x" },
              { id: "cancel", label: L("暂不启动", "Don't start"), key: "b" },
            ],
          },
        });
      return delay<LaunchInfo>({ mode: q.has("handheld") ? "handheld" : "desktop", prompt: null });
    }
    case "prompt_answer":
      console.info("prompt answer:", args.choice);
      return delay(undefined);
    case "set_fullscreen":
    case "quit_handheld":
    case "handheld_zoom":
    case "dismiss_newer":
      return delay(undefined);
    case "handheld_info":
      return delay<HandheldInfo>({ device: "ROG Ally", steamGamepadUi: false, mode: mockHandheldMode, inSteam: null, xboxFse: false });
    case "set_handheld_mode":
      mockHandheldMode = args.mode as HandheldMode;
      return delay(undefined);
    case "steam_add_ludonimbus":
      return delay(3123456789, 600);
    case "newer_version":
      return delay<NewerVersion | null>(
        key === "celeste"
          ? { key: "celeste-gog", name: "Celeste", label: "GOG", snapshot: snap("01NEWER", 90, "LAPTOP", "auto-exit", 30_000, 3) }
          : null,
        300,
      );
    case "set_shelf": {
      // Every version of the game, as the card in the cloud.
      for (const v of [...overview.games, ...overview.cloudOnly] as Array<{ key: string; shelves?: string[] }>) {
        if (!(args.keys as string[]).includes(v.key)) continue;
        const rest = (v.shelves ?? []).filter((s) => s !== args.tag);
        v.shelves = args.on ? [...rest, String(args.tag)] : rest;
      }
      return delay(undefined, 300);
    }
    case "collection_stats":
      return delay<CollectionStats>({ achievementsUnlocked: 87, achievementsTotal: 142 }, 100);
    case "refresh_cloud_library": {
      const families: Record<string, string> = {};
      for (const g of overview.games) families[g.key] = g.family;
      for (const c of overview.cloudOnly) families[c.key] = c.family;
      return delay<CloudLibrary>({ cloudOnly: overview.cloudOnly, families, fetchedAt: ago(0), error: null }, 300);
    }
    case "cloud_game_info":
      return delay<CloudGameInfo>(
        {
          playtime: overview.cloudOnly.find((c) => c.key === key)?.playtime ?? devicePlaytime(0, 0, 0),
          gameBackup: key === "hollow-knight" ? { versions: 2, size: 9.1e9, createdAt: ago(60 * 26), deviceName: "GPD-Win-Mini" } : null,
        },
        300,
      );
    case "other_installs":
      return delay(mockInstalls((args.key as string | null) ?? null), 200);
    case "add_install":
      return delay(mockAddInstall(args.id as string), 400);
    case "remove_game": {
      const i = overview.games.findIndex((g) => g.key === key);
      if (i >= 0) overview.games.splice(i, 1);
      const a = steamCloudApps.findIndex((x) => x.key === key);
      // The extra Steam version (see `mockInstalls`) is listed from there again.
      if (a >= 0 && key.startsWith("steam-")) steamCloudApps.splice(a, 1);
      else if (a >= 0) Object.assign(steamCloudApps[a], { key: null, state: "steam" });
      return delay(undefined);
    }
    case "link_games": {
      const keys = args.keys as string[];
      const all = [...overview.games, ...overview.cloudOnly];
      const fam = all.find((g) => g.key === keys[1])?.family ?? keys[0];
      for (const g of all) if (keys.includes(g.key)) Object.assign(g, { family: fam, link: `link:${fam}` });
      return delay(undefined);
    }
    case "unlink_game": {
      const g = [...overview.games, ...overview.cloudOnly].find((x) => x.key === key);
      if (g) Object.assign(g, { family: key, link: `solo:${key}` });
      return delay(undefined);
    }
    case "transfer_plan": {
      const target = overview.games.find((g) => g.key === args.target);
      const xbox = target?.source.kind === "xbox";
      const store = target ? mockStore(target) : null;
      const base = xbox
        ? "C:\\Users\\me\\AppData\\Local\\Packages\\X_y\\SystemAppData\\wgs\\0009_00AB"
        : "C:\\Users\\me\\AppData\\Local\\Celeste\\Saves";
      const files = ["0.celeste", "1.celeste", "settings.celeste"];
      return delay<TransferPlan>(
        {
          same: false,
          fromXbox: false,
          toXbox: xbox,
          xboxRule: null,
          items: files.map((f, i) => ({ from: f, to: xbox ? `${f} / Data` : `${base}\\${f}`, size: 2048 * (i + 1), replaces: i !== 1 })),
          unmatched: [],
          base: (args.base as string | null) ?? base,
          matched: 2,
          needsFolder: false,
          crossPlatform: false,
          formatRule: null,
          skipped: [],
          sourceVersion: "1.6.15",
          targetVersion: "1.6.9",
          tooNew: true,
          cloudTarget: false,
          steamCloud: !!store && store.mode !== "managed",
        },
        500,
      );
    }
    case "transfer_apply":
      return delay<Outcome>({ kind: "uploaded", id: "01APPLIED" }, 600);
    case "game_backup_request_install":
      return delay(undefined);
    case "cloud_games":
      return delay<CloudGame[]>([{ key: "hollow-knight", name: "Hollow Knight", adapter: null, saves: [], exclude: [], waitFor: [], roots: {}, ludusaviTitle: "Hollow Knight" }]);
    case "ludusavi_search":
      return delay(["Hollow Knight", "Hollow Knight: Silksong"]);
    case "check_update":
      if (mockScene) return delay<UpdateInfo>({ current: "0.5.0", available: false, version: null, notes: null, date: null });
      return delay<UpdateInfo>(
        { current: "0.1.0", available: true, version: "0.1.1", notes: "### 中文\n\n- 修复：示例更新说明\n- 新增：示例功能\n\n### English\n\n- Fixed: sample release note\n- New: sample feature", date: new Date().toISOString() },
        600,
      );
    case "steam_plugin_status":
      return delay<SteamPluginStatus>({
        steamRoot: "C:\Program Files (x86)\Steam",
        steamRunning: true,
        bundled: true,
        millennium: steamPluginOn ? "installed" : "missing",
        pluginInstalled: steamPluginOn,
        pluginEnabled: steamPluginOn,
        pluginCurrent: steamPluginOn,
      });
    case "steam_plugin_install":
      steamPluginOn = true;
      return delay<SteamPluginInstall>({ millenniumInstalled: "v3.6.3", steamRestarted: true }, 1500);
    case "steam_plugin_remove":
      steamPluginOn = false;
      return delay(undefined, 800);
    case "scan_quick":
      return delay(quickScan, 500);
    case "scan_ludusavi":
      return delay(ludusaviScan, 2500);
    case "scan_folder":
      return delay(folderScan, 600);
    case "game_folders":
      return delay<GameFolder[]>([
        { label: "Steam C:", path: "C:\\Program Files (x86)\\Steam\\steamapps\\common" },
        { label: "Steam D:", path: "D:\\SteamLibrary\\steamapps\\common" },
        { label: "Xbox", path: "C:\\XboxGames" },
        { label: "Epic", path: "C:\\Program Files\\Epic Games" },
      ]);
    case "pick_exe":
      return delay(`${(args.start as string | null) ?? "C:\\Games"}\\Starfield\\Starfield.exe`);
    case "pick_folder":
      return delay("D:\\Games");
    case "steam_find_roots":
      return delay(["C:\\Program Files (x86)\\Steam", "D:\\Steam"], 400);
    case "steam_set_root":
      return delay((args.path as string | null) ?? "C:\\Program Files (x86)\\Steam");
    case "cloud_folder_suggestions":
      return delay<SyncFolder[]>(
        [
          { label: "OneDrive", path: "C:\\Users\\me\\OneDrive\\Ludonimbus", existing: false },
          { label: "Google Drive", path: "G:\\My Drive\\Ludonimbus", existing: true },
        ],
        400,
      );
    case "add_found":
      return delay((args.found as Found).addedKey ?? (args.name as string).toLowerCase().replace(/[^a-z0-9]+/g, "-"), 300);
    case "detect_adapter": {
      const exe = (args.exe as string).toLowerCase();
      if (exe.endsWith("dolphin.exe")) return delay<AdapterInfo>({ key: "dolphin", name: "Dolphin", label: L("Dolphin（整个模拟器的存档）", "Dolphin (whole emulator)"), emulator: true });
      if (exe.endsWith("soh.exe")) return delay<AdapterInfo>({ key: "soh", name: L("塞尔达传说：时之笛", "The Legend of Zelda: Ocarina of Time"), label: L("塞尔达传说：时之笛", "The Legend of Zelda: Ocarina of Time"), emulator: false });
      return delay(null);
    }
    case "savefind_suggest":
      return delay<SaveCandidate[]>([
        {
          path: "C:\\Users\\me\\AppData\\LocalLow\\Indie Studio\\Some Indie",
          pattern: "{home}/AppData/LocalLow/Indie Studio/Some Indie/**",
          files: 3,
          bytes: 48213,
          modified: ago(60 * 20),
          sample: ["save0.dat", "settings.json"],
          reason: L("Unity 游戏的存档位置（Indie Studio / Some Indie）", "where Unity games keep saves (Indie Studio / Some Indie)"),
          likely: true,
        },
        {
          path: "D:\\Games\\Some Indie\\saves",
          pattern: "{install}/saves/**",
          files: 2,
          bytes: 20480,
          modified: ago(60 * 24 * 3),
          sample: ["slot1.sav"],
          reason: L("游戏目录里的存档文件夹", "save folder in the game folder"),
          likely: false,
        },
        {
          path: "C:\\Users\\me\\Documents\\My Games\\Some Indie",
          pattern: "{documents}/My Games/Some Indie/**",
          files: 1,
          bytes: 2048,
          modified: ago(60 * 24 * 40),
          sample: ["settings.ini"],
          reason: L("名字和游戏相近（C:\\Users\\me\\Documents\\My Games）", "similar name (C:\\Users\\me\\Documents\\My Games)"),
          likely: false,
        },
      ]);
    case "savefind_titles":
      return delay<SaveTitle[]>(
        (args.name as string).trim()
          ? [
              { title: "Some Indie", likely: true, reason: L("名字相同", "same name"), files: 3 },
              { title: "Some Indie 2", likely: false, reason: L("名字相近", "similar name"), files: 0 },
            ]
          : [],
        700,
      );
    case "exe_in_folder":
      return delay(`${args.path as string}\\Some Indie.exe`);
    case "add_custom":
      return delay(((args.name as string) || (args.ludusaviTitle as string) || "game").toLowerCase().replace(/[^a-z0-9]+/g, "-"), 300);
    case "savefind_watch_start":
      return delay(undefined);
    case "savefind_watch_stop":
      return delay<SaveCandidate[]>(
        [{ path: "C:\\Users\\me\\Saved Games\\Some Indie", pattern: "{home}/Saved Games/Some Indie/**", files: 1, bytes: 9120, modified: ago(1), sample: ["slot1.sav"], reason: L("刚才被写入", "just written"), likely: false }],
        600,
      );
    case "folder_to_pattern":
      return delay(args.path as string);
    case "sgdb_status":
      return delay<SgdbStatus>({ hasKey: !["settings-sgdb", "guide-art"].includes(mockScene), keyPage: "https://www.steamgriddb.com/profile/preferences/api" });
    case "sgdb_set_key":
    case "sgdb_clear_key":
      return delay(undefined);
    case "sgdb_search":
      return delay<SgdbGame[]>([
        { id: 1, name: args.term as string, year: 2018, verified: true },
        { id: 2, name: `${args.term as string}: Farewell`, year: 2019, verified: false },
      ]);
    case "sgdb_images": {
      const pool = args.kind === "hero" || args.kind === "wide" ? sgdbHeroes : args.kind === "logo" ? sgdbLogos : args.kind === "icon" ? sgdbIcons : sgdbCovers;
      return delay<SgdbImage[]>(pool.map((u, i) => ({ id: i + 10, url: u, thumb: u, width: 600, height: 900, score: 3 - i, style: "alternate", author: "someone" })), 400);
    }
    case "art_set_url":
    case "art_set_file":
      return delay(undefined, 500);
    case "steam_add_games":
      return delay<SteamAddResult[]>((args.keys as string[]).map((k) => ({ key: k, error: null })), 1200);
    case "steam_cloud_apps":
      return delay(steamCloudApps.map((a) => ({ ...a })), 300);
    case "steam_cloud_add":
      return delay(mockSteamAdd(args.appids as number[]), 600);
    case "steam_cloud_backup_all":
      return delay(mockSteamAdd(steamCloudApps.filter((a) => !a.key).map((a) => a.appid)), 900);
    case "steam_cloud_status":
      return delay(mockSteamStatus(key), 200);
    case "steam_cloud_take_over":
      return delay(mockSteamTakeOver(key, args.wrapper !== false), 1200);
    case "steam_cloud_hand_back": {
      const a = steamCloudApps.find((x) => x.key === key);
      if (a) a.state = "backup";
      mockNoWrapper.delete(key);
      return delay(mockSteamStatus(key), 800);
    }
    case "steam_cloud_open_properties": {
      // As if the user then flipped the game's Steam Cloud switch in Steam.
      const a = steamCloudApps.find((x) => x.key === key);
      if (a) a.cloudEnabled = !a.cloudEnabled;
      return delay(undefined, 200);
    }
    case "steam_cloud_take_over_all":
      return delay<SteamCloudDone[]>(
        steamCloudApps
          .filter((a) => a.state === "backup")
          .map((a) => {
            try {
              mockSteamTakeOver(a.key!);
              return { appid: a.appid, key: a.key, error: null };
            } catch (e) {
              return { appid: a.appid, key: a.key, error: String(e) };
            }
          }),
        1500,
      );
    case "steam_add_game":
      return delay(3000000001, 800);
    case "steam_remove_game":
      return delay(undefined, 500);
    case "set_game_exe": {
      const g = overview.games.find((x) => x.key === key);
      if (g) Object.assign(g, { exe: args.path as string, exeExists: true, playable: true });
      return delay(undefined);
    }
    case "relocate_auto":
      return delay(key === "starfield" ? null : "C:\\Users\\me\\Desktop\\Switch\\fefw-recomp-0.2.8\\fefw.exe");
    case "update_game": {
      const g = overview.games.find((x) => x.key === key);
      const patch = args.patch as GamePatch;
      if (g && patch.name?.trim()) g.name = patch.name.trim();
      if (g && patch.autoSync !== undefined) g.autoSync = patch.autoSync;
      if (g && patch.source !== undefined) g.source = { ...g.source, label: patch.source.trim() || null };
      return delay(undefined);
    }
    default:
      return delay(null);
  }
}

// Screenshots: generated pictures (a gradient per screenshot).
const shotHues = [205, 28, 140, 265, 350, 90];
function shotImage(name: string, thumb: boolean): string {
  const n = [...name].reduce((a, c) => a + c.charCodeAt(0), 0);
  const h = shotHues[n % shotHues.length];
  const [w, hgt] = thumb ? [200, 112] : [1280, 720];
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${hgt}" viewBox="0 0 16 9"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl(${h},55%,42%)"/><stop offset="1" stop-color="hsl(${(h + 60) % 360},45%,18%)"/></linearGradient></defs><rect width="16" height="9" fill="url(#g)"/><circle cx="${4 + (n % 8)}" cy="3" r="1.4" fill="hsla(${h},80%,85%,.55)"/><path d="M0 9 L5 5 L8 7 L12 3 L16 6 L16 9Z" fill="hsla(${h},40%,10%,.6)"/></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

function mockGallery(key: string): Gallery {
  const shots = [
    ["20261122223831_1.jpg", "d-mock", "DESKTOP-MAIN", true, true],
    ["20261122153202_1.jpg", "d-mock", "DESKTOP-MAIN", true, true],
    ["20261121201544_1.jpg", "d-laptop", "LAPTOP", true, true],
    ["20261120175013_1.jpg", "d-mock", "DESKTOP-MAIN", true, false],
    ["20261120172408_1.jpg", "d-laptop", "LAPTOP", false, true],
    ["20261120164625_1.jpg", "d-mock", "DESKTOP-MAIN", true, true],
  ] as const;
  const parse = (n: string) => new Date(+n.slice(0, 4), +n.slice(4, 6) - 1, +n.slice(6, 8), +n.slice(8, 10), +n.slice(10, 12)).toISOString();
  return {
    shots: shots.map(([name, device, deviceName, onThisPc, inCloud]) => ({
      name,
      device,
      deviceName,
      size: 900_000 + name.length * 31_000,
      takenAt: parse(name),
      onThisPc,
      inCloud,
      hasThumb: true,
      thumbUrl: onThisPc ? shotImage(name, true) : null,
    })),
    mode: "sync",
    linked: key !== "custom",
    folder: "C:\\Program Files (x86)\\Steam\\userdata\\12345678\\760\\remote\\17521518688375668736\\screenshots",
    cloudBytes: 5_400_000,
    cloudError: null,
  };
}
