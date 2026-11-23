// Browser-only mock backend, used when the UI runs outside Tauri
// (`npm run dev` in a normal browser) so the interface can be developed and
// reviewed without the Rust side. Never loaded inside the app.
import type { GameView, Outcome, Overview, SnapshotSummary, StatusReport } from "./api";

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
    snap("01OOT3", 60 * 50, "DESKTOP-MAIN", "manual", 1100000, 12, { comment: "进入水之神殿前", locked: true }),
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
    steam: { account: "12345678", appid, enabled: true },
    health: { state: "ok" },
    art: { cover: art(appid, "p.png"), hero: art(appid, "_hero.png"), logo: art(appid, "_logo.png"), icon: art(appid, "_icon.png") },
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
    game("zelda-oot-longshot", "塞尔达传说：时之笛", 4079546474, "Longshot（原生重制）", "C:\\Users\\me\\AppData\\Local\\Programs\\LongshotLauncher\\LongshotLauncher.exe", ["{data}/save/**"]),
    game("fe-fortunes-weave", "火焰纹章：万缕千丝", 2661209017, "fefw recomp", "C:\\Users\\me\\Desktop\\Switch\\fefw-recomp-0.2.7\\fefw.exe", ["{install}/save/**"], {
      art: { cover: art(2661209017, "p.jpg"), hero: art(2661209017, "_hero.png"), logo: art(2661209017, "_logo.png"), icon: art(2661209017, "_icon.png") },
      health: { state: "changed", exe: "C:\\Users\\me\\Desktop\\Switch\\fefw-recomp-0.2.7\\fefw.exe", launchOptions: "" },
    }),
    game("bloodborne-lantern", "Bloodborne", 4000695018, "Lantern Engine（shadPS4）", "C:\\Users\\me\\Desktop\\Switch\\Bloodborne\\BloodbornePC\\Bloodborne.exe", ["{install}/user/savedata/1/CUSA00207/**"], {
      art: { cover: art(4000695018, "p.png"), hero: art(4000695018, "_hero.jpg"), logo: art(4000695018, "_logo.png"), icon: art(4000695018, "_icon.png") },
      conflict: { cloudId: "01BB2", cloudDevice: "GPD-Win-Mini", cloudCreatedAt: ago(300), localHash: "y", detectedAt: ago(10) },
    }),
  ],
};

const delay = <T,>(v: T, ms = 250) => new Promise<T>((r) => setTimeout(() => r(v), ms));

export async function mockInvoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
  const key = args.key as string;
  switch (cmd) {
    case "get_overview":
      return delay(overview, 150);
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
        googleBuiltin: true,
        dataDir: "C:\\Users\\me\\AppData\\Roaming\\Ludonimbus",
        runnerPath: "C:\\Users\\me\\AppData\\Local\\Ludonimbus\\ludonimbus-run.exe",
        runnerExists: true,
        ludusavi: "ludusavi 0.31.0",
        version: "0.1.0",
      });
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
        { current: "0.1.0", available: true, version: "0.1.1", notes: "- 修复：示例更新说明\n- 新增：示例功能", date: new Date().toISOString() },
        600,
      );
    case "relocate_auto":
      return delay("C:\\Users\\me\\Desktop\\Switch\\fefw-recomp-0.2.8\\fefw.exe");
    default:
      return delay(null);
  }
}
