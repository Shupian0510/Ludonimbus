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
  steam: { account: string; appid: number; enabled: boolean; created: boolean } | null;
  health: Health;
  art: { cover: string | null; wide: string | null; hero: string | null; logo: string | null; icon: string | null };
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
  cloud: { kind: CloudKind; label: string | null; error: string | null };
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

/** A game found on this PC by a scan. */
export interface Found {
  id: string;
  name: string;
  /** `emulator`: a game with saves in an emulator's data folder. */
  source: "steam" | "epic" | "gog" | "xbox" | "recomplauncher" | "emulator" | "ludusavi" | "folder";
  exe: string | null;
  install: string | null;
  args: string;
  steamAccount: string | null;
  steamAppid: number | null;
  adapter: string | null;
  adapterName: string | null;
  /** Save patterns the scan worked out (one emulator game, or a folder picked in RecompLauncher). */
  saves: string[];
  ludusaviTitle: string | null;
  saveFiles: number;
  saveBytes: number;
  saveSample: string[];
  steamCloud: boolean;
  storeGame: boolean;
  addedKey: string | null;
  registryOnly: boolean;
  /** Where the install of a game found by its saves was found. */
  installStore: string | null;
  matchKey: string;
}

/** The built-in rule an exe matches. */
export interface AdapterInfo {
  key: string;
  /** The game's name (the emulator's name for emulators). */
  name: string;
  /** What to show, e.g. "Dolphin（整个模拟器的存档）". */
  label: string;
  emulator: boolean;
}

/** A folder a game store installs games into. */
export interface GameFolder {
  label: string;
  path: string;
}

/** A folder a sync client (OneDrive, Dropbox…) keeps in the cloud; `path` is `Ludonimbus` inside it. */
export interface SyncFolder {
  label: string;
  path: string;
  existing: boolean;
}

/** A folder that may hold a game's saves. */
export interface SaveCandidate {
  path: string;
  files: number;
  bytes: number;
  modified: string | null;
  sample: string[];
  reason: string;
}

export type ArtKind = "cover" | "wide" | "hero" | "logo" | "icon";

export interface SgdbGame {
  id: number;
  name: string;
  year: number | null;
  verified: boolean;
}

export interface SgdbImage {
  id: number;
  url: string;
  thumb: string;
  width: number;
  height: number;
  score: number;
  style: string;
  author: string;
}

export type CloudKind = "none" | "localFolder" | "googleDrive" | "webDav" | "s3" | "baiduPan" | "quark";

export type WebDavConfig = { kind: "webDav"; url: string; username: string; folder: string };
export type S3Config = {
  kind: "s3";
  endpoint: string;
  region: string;
  bucket: string;
  prefix: string;
  accessKeyId: string;
  pathStyle: boolean;
};

export type CloudConfig =
  | { kind: "none" }
  | { kind: "localFolder"; path: string }
  | { kind: "googleDrive"; clientId: string; clientSecret: string; folder: string; account: string | null }
  | WebDavConfig
  | S3Config
  | { kind: "baiduPan"; appKey: string; secretKey: string; appName: string; account: string | null }
  | { kind: "quark"; folder: string };

export interface OpenListInfo {
  installed: boolean;
  version: string;
  size: number;
  url: string;
  asset: string;
  project: string;
  license: string;
}

export interface BaiduDeviceCode {
  userCode: string;
  verificationUrl: string;
  qrcodeUrl: string;
  expiresIn: number;
  interval: number;
}

export interface MigrateProgress {
  game: string;
  done: number;
  total: number;
}

export interface MigrateReport {
  games: number;
  copied: number;
  skipped: number;
  bytes: number;
}

export interface Settings {
  deviceId: string;
  deviceName: string;
  retention: { keepRecent: number; keepDailyDays: number };
  networkTimeoutSecs: number;
  steamRoot: string | null;
  /** The Steam folder was picked by hand (not auto-detected). */
  steamRootCustom: boolean;
  steamAccount: string | null;
  cloud: CloudConfig;
  /** Cloud used before the last switch whose saves can still be copied. */
  previousCloud: { kind: CloudKind; name: string } | null;
  googleBuiltin: boolean;
  baiduBuiltin: boolean;
  baiduAppName: string;
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

export type MillenniumInstall = "missing" | "legacy" | "installed";

export interface SteamPluginStatus {
  steamRoot: string | null;
  steamRunning: boolean;
  bundled: boolean;
  millennium: MillenniumInstall | null;
  pluginInstalled: boolean;
  pluginEnabled: boolean;
  pluginCurrent: boolean;
}

export interface SteamPluginProgress {
  stage: "download" | "install";
  done: number;
  total: number;
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
  cloudFolderSuggestions: () => invoke<SyncFolder[]>("cloud_folder_suggestions"),
  steamFindRoots: () => invoke<string[]>("steam_find_roots"),
  /** `null` goes back to auto-detecting; returns the Steam folder now in use. */
  steamSetRoot: (path: string | null) => invoke<string | null>("steam_set_root", { path }),
  cloudConnectGoogle: (clientId: string, clientSecret: string) =>
    invoke<string>("cloud_connect_google", { clientId, clientSecret }),
  cloudDisconnect: () => invoke<void>("cloud_disconnect"),
  cloudTest: (cloud: WebDavConfig | S3Config, secret: string) => invoke<AccountInfo>("cloud_test", { cloud, secret }),
  cloudConnect: (cloud: WebDavConfig | S3Config, secret: string) => invoke<string>("cloud_connect", { cloud, secret }),
  cloudMigrate: () => invoke<MigrateReport>("cloud_migrate"),
  cloudDismissPrevious: () => invoke<void>("cloud_dismiss_previous"),
  cloudConnectBaidu: (appKey: string, secretKey: string, appName: string) =>
    invoke<string>("cloud_connect_baidu", { appKey, secretKey, appName }),
  cloudCancelLogin: () => invoke<void>("cloud_cancel_login"),
  openlistStatus: () => invoke<OpenListInfo>("openlist_status"),
  openlistInstall: (zip: string | null) => invoke<void>("openlist_install", { zip }),
  openlistPickZip: () => invoke<string | null>("openlist_pick_zip"),
  cloudConnectQuark: () => invoke<string>("cloud_connect_quark"),
  scanQuick: () => invoke<Found[]>("scan_quick"),
  scanLudusavi: () => invoke<Found[]>("scan_ludusavi"),
  scanFolder: (path: string) => invoke<Found[]>("scan_folder", { path }),
  gameFolders: () => invoke<GameFolder[]>("game_folders"),
  addFound: (found: Found, choice: { name?: string; saves?: string[]; ludusaviTitle?: string | null; exe?: string | null } = {}) =>
    invoke<string>("add_found", {
      found,
      name: choice.name ?? found.name,
      saves: choice.saves ?? [],
      ludusaviTitle: choice.ludusaviTitle ?? null,
      exe: choice.exe ?? null,
    }),
  detectAdapter: (exe: string) => invoke<AdapterInfo | null>("detect_adapter", { exe }),
  saveSuggest: (name: string, exe: string | null, install: string | null) =>
    invoke<SaveCandidate[]>("savefind_suggest", { name, exe, install }),
  saveWatchStart: (exe: string | null, args: string | null, install: string | null) =>
    invoke<void>("savefind_watch_start", { exe, args, install }),
  saveWatchStop: () => invoke<SaveCandidate[]>("savefind_watch_stop"),
  sgdbStatus: () => invoke<{ hasKey: boolean; keyPage: string }>("sgdb_status"),
  sgdbSetKey: (key: string) => invoke<void>("sgdb_set_key", { key }),
  sgdbClearKey: () => invoke<void>("sgdb_clear_key"),
  sgdbSearch: (term: string) => invoke<SgdbGame[]>("sgdb_search", { term }),
  sgdbImages: (gameId: number, kind: ArtKind) => invoke<SgdbImage[]>("sgdb_images", { gameId, kind }),
  artSetUrl: (key: string, kind: ArtKind, url: string) => invoke<void>("art_set_url", { key, kind, url }),
  artSetFile: (key: string, kind: ArtKind, path: string) => invoke<void>("art_set_file", { key, kind, path }),
  pickImage: () => invoke<string | null>("pick_image"),
  steamAddGame: (key: string, account: string, closeSteam: boolean) =>
    invoke<number>("steam_add_game", { key, account, closeSteam }),
  steamAddGames: (keys: string[], account: string, autoArt: boolean, closeSteam: boolean) =>
    invoke<{ key: string; error: string | null }[]>("steam_add_games", { keys, account, autoArt, closeSteam }),
  steamRemoveGame: (key: string, closeSteam: boolean) => invoke<void>("steam_remove_game", { key, closeSteam }),
  cloudAccount: () => invoke<AccountInfo>("cloud_account"),
  devices: () => invoke<DeviceInfo[]>("list_devices"),
  ludusaviSearch: (query: string) => invoke<string[]>("ludusavi_search", { query }),
  ludusaviPreview: (title: string) => invoke<string[]>("ludusavi_preview", { title }),
  steamPluginStatus: () => invoke<SteamPluginStatus>("steam_plugin_status"),
  steamPluginInstall: (closeSteam: boolean) =>
    invoke<{ millenniumInstalled: string | null; steamRestarted: boolean }>("steam_plugin_install", { closeSteam }),
  steamPluginRemove: (closeSteam: boolean) => invoke<void>("steam_plugin_remove", { closeSteam }),
  language: () => invoke<string>("get_language"),
  onboarding: () => invoke<{ doneAt: string | null; usage: string[] }>("get_onboarding"),
  finishOnboarding: (usage: string[]) => invoke<void>("finish_onboarding", { usage }),
  setLanguage: (language: string) => invoke<void>("set_language", { language }),
  checkUpdate: () => invoke<UpdateInfo>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
};

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
