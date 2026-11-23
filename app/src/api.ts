import { invoke as tauriInvoke } from "@tauri-apps/api/core";

/** True inside the Tauri app; false when the UI runs in a plain browser (dev preview). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) return tauriInvoke<T>(cmd, args);
  const { mockInvoke } = await import("./mock");
  return (await mockInvoke(cmd, args)) as T;
}

export type Trigger =
  | "auto-exit"
  | "pre-launch"
  | "passive"
  | "sync-now"
  | "manual"
  | "restore"
  | "pre-restore"
  | "conflict-keep";

export type Status =
  | "noCloud"
  | "empty"
  | "notUploaded"
  | "synced"
  | "localChanged"
  | "cloudNewer"
  | "canRestore"
  | "conflict"
  | "offline"
  | "error";

export type Health =
  | { state: "notLinked" }
  | { state: "disabled" }
  | { state: "ok" }
  | { state: "shortcutMissing"; candidateAppid: number | null }
  | { state: "changed"; exe: string; launchOptions: string }
  | { state: "runnerMoved"; exe: string }
  | { state: "noSteam" };

export interface SnapshotSummary {
  id: string;
  parent: string | null;
  deviceId: string;
  deviceName: string;
  createdAt: string;
  trigger: Trigger;
  contentHash: string;
  totalSize: number;
  fileCount: number;
  comment: string;
  locked: boolean;
}

export interface FileEntry {
  path: string;
  size: number;
  hash: string;
  mtime: number;
}

export interface SnapshotMeta extends Omit<SnapshotSummary, "fileCount"> {
  game: string;
  roots: Record<string, string>;
  files: FileEntry[];
  appVersion: string;
}

export interface StatusReport {
  status: Status;
  message: string | null;
  localFiles: number;
  localSize: number;
  localHash: string | null;
  head: SnapshotSummary | null;
  snapshotCount: number;
  lastSyncedId: string | null;
  lastSyncAt: string | null;
  pending: number;
  checkedAt: string;
}

export interface ConflictInfo {
  cloudId: string;
  cloudDevice: string;
  cloudCreatedAt: string;
  localHash: string;
  detectedAt: string;
}

export interface GameView {
  key: string;
  name: string;
  adapter: string | null;
  adapterName: string | null;
  platform: string | null;
  exe: string | null;
  exeExists: boolean;
  install: string | null;
  args: string;
  saves: string[];
  exclude: string[];
  waitFor: string[];
  customSaves: boolean;
  defaultSaves: string[];
  defaultWaitFor: string[];
  uploadOnGameExit: boolean;
  hasGameProcess: boolean;
  ludusaviTitle: string | null;
  autoSync: boolean;
  steam: { account: string; appid: number; enabled: boolean } | null;
  health: Health;
  art: { cover: string | null; hero: string | null; logo: string | null; icon: string | null };
  running: boolean;
  lastSyncAt: string | null;
  lastSyncedDevice: string | null;
  conflict: ConflictInfo | null;
  lastError: string | null;
  pending: number;
  lastCheck: StatusReport | null;
}

export interface Overview {
  deviceId: string;
  deviceName: string;
  cloud: { kind: "none" | "localFolder" | "googleDrive"; label: string | null; error: string | null };
  steamFound: boolean;
  steamRunning: boolean;
  games: GameView[];
}

export type Outcome =
  | { kind: "upToDate" }
  | { kind: "uploaded"; id: string }
  | { kind: "queued"; id: string; reason: string }
  | { kind: "pulled"; id: string; device: string; createdAt: string }
  | { kind: "conflict"; localFiles: number; localSize: number; cloud: SnapshotSummary }
  | { kind: "nothingToSave" }
  | { kind: "noCloud" }
  | { kind: "offline"; message: string };

export interface SteamAccount {
  id: string;
  persona: string | null;
  accountName: string | null;
  mostRecent: boolean;
  shortcutCount: number;
}

export interface ShortcutView {
  account: string;
  appid: number;
  name: string;
  nameCorrupted: boolean;
  suggestedName: string;
  exe: string;
  startDir: string;
  launchOptions: string;
  adapter: string | null;
  adapterName: string | null;
  addedKey: string | null;
  wrappedKey: string | null;
  cover: string | null;
  icon: string | null;
}

export interface CloudGame {
  key: string;
  name: string;
  adapter: string | null;
  saves: string[];
  ludusaviTitle: string | null;
}

export interface LocalFile {
  logical: string;
  size: number;
  mtime: number;
}

export type CloudConfig =
  | { kind: "none" }
  | { kind: "localFolder"; path: string }
  | { kind: "googleDrive"; clientId: string; clientSecret: string; folder: string; account: string | null };

export interface Settings {
  deviceId: string;
  deviceName: string;
  retention: { keepRecent: number; keepDailyDays: number };
  networkTimeoutSecs: number;
  steamRoot: string | null;
  steamAccount: string | null;
  cloud: CloudConfig;
  googleBuiltin: boolean;
  dataDir: string;
  runnerPath: string;
  runnerExists: boolean;
  ludusavi: string | null;
  version: string;
}

export interface DeviceInfo {
  id: string;
  name: string;
  os: string;
  appVersion: string;
  lastSeen: string;
}

export interface UpdateInfo {
  current: string;
  available: boolean;
  version: string | null;
  notes: string | null;
  date: string | null;
}

export interface AccountInfo {
  label: string;
  used: number | null;
  total: number | null;
}

export const api = {
  overview: () => invoke<Overview>("get_overview"),
  check: (key: string) => invoke<StatusReport>("check_game", { key }),
  sync: (key: string, resolution: "auto" | "local" | "cloud" = "auto") =>
    invoke<Outcome>("sync_game", { key, resolution }),
  backup: (key: string, comment: string) => invoke<Outcome>("backup_game", { key, comment }),
  snapshots: (key: string) =>
    invoke<{ snapshots: SnapshotSummary[]; headId: string | null; lastSyncedId: string | null }>(
      "list_snapshots",
      { key },
    ),
  snapshot: (key: string, id: string) => invoke<SnapshotMeta>("get_snapshot", { key, id }),
  restore: (key: string, id: string) => invoke<Outcome>("restore_snapshot", { key, id }),
  setSnapshot: (key: string, id: string, patch: { locked?: boolean; comment?: string }) =>
    invoke<void>("set_snapshot", { key, id, locked: patch.locked ?? null, comment: patch.comment ?? null }),
  deleteSnapshot: (key: string, id: string) => invoke<void>("delete_snapshot", { key, id }),
  exportSnapshot: (key: string, id: string) => invoke<string | null>("export_snapshot", { key, id }),
  localFiles: (key: string) =>
    invoke<{ files: LocalFile[]; roots: Record<string, string> }>("local_files", { key }),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  openSaveFolder: (key: string) => invoke<void>("open_save_folder", { key }),
  steamAccounts: () =>
    invoke<{ accounts: SteamAccount[]; selected: string | null; root: string | null }>("steam_accounts"),
  steamShortcuts: (account: string) => invoke<ShortcutView[]>("steam_shortcuts", { account }),
  addFromShortcut: (args: {
    account: string;
    appid: number;
    name?: string | null;
    adapter?: string | null;
    saves?: string[];
    key?: string | null;
  }) =>
    invoke<string>("add_from_shortcut", {
      account: args.account,
      appid: args.appid,
      name: args.name ?? null,
      adapter: args.adapter ?? null,
      saves: args.saves ?? [],
      key: args.key ?? null,
    }),
  addCustom: (name: string, exe: string | null, saves: string[], ludusaviTitle: string | null) =>
    invoke<string>("add_custom", { name, exe, saves, ludusaviTitle }),
  cloudGames: () => invoke<CloudGame[]>("cloud_games"),
  addFromCloud: (key: string, account: string | null, appid: number | null, exe: string | null) =>
    invoke<string>("add_from_cloud", { key, account, appid, exe }),
  updateGame: (
    key: string,
    patch: {
      name?: string;
      saves?: string[];
      exclude?: string[];
      waitFor?: string[];
      args?: string;
      autoSync?: boolean;
      uploadOnGameExit?: boolean;
    },
  ) => invoke<void>("update_game", { key, patch }),
  removeGame: (key: string) => invoke<void>("remove_game", { key }),
  relocateAuto: (key: string) => invoke<string | null>("relocate_auto", { key }),
  setGameExe: (key: string, path: string) => invoke<void>("set_game_exe", { key, path }),
  pickExe: (start?: string | null) => invoke<string | null>("pick_exe", { start: start ?? null }),
  pickFolder: (title?: string, start?: string | null) =>
    invoke<string | null>("pick_folder", { title: title ?? null, start: start ?? null }),
  folderToPattern: (path: string, exe?: string | null) =>
    invoke<string>("folder_to_pattern", { path, exe: exe ?? null }),
  steamSetEnabled: (key: string, enabled: boolean, closeSteam: boolean) =>
    invoke<void>("steam_set_enabled", { key, enabled, closeSteam }),
  steamRepair: (key: string, closeSteam: boolean) => invoke<void>("steam_repair", { key, closeSteam }),
  settings: () => invoke<Settings>("get_settings"),
  saveSettings: (patch: {
    deviceName?: string;
    keepRecent?: number;
    keepDailyDays?: number;
    networkTimeoutSecs?: number;
    steamAccount?: string;
  }) => invoke<void>("save_settings", { patch }),
  cloudUseFolder: (path: string) => invoke<void>("cloud_use_folder", { path }),
  cloudConnectGoogle: (clientId: string, clientSecret: string) =>
    invoke<string>("cloud_connect_google", { clientId, clientSecret }),
  cloudDisconnect: () => invoke<void>("cloud_disconnect"),
  cloudAccount: () => invoke<AccountInfo>("cloud_account"),
  devices: () => invoke<DeviceInfo[]>("list_devices"),
  ludusaviSearch: (query: string) => invoke<string[]>("ludusavi_search", { query }),
  ludusaviPreview: (title: string) => invoke<string[]>("ludusavi_preview", { title }),
  checkUpdate: () => invoke<UpdateInfo>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
};

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
