// The commands of the desktop app (app/src-tauri). The types of what they
// take and return are generated from the Rust types (src/bindings,
// docs/development.md); the few declared here are made from those.
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type {
  AccountInfo,
  AdapterInfo,
  ArtView,
  CloudConfig,
  CloudGame,
  CloudGameInfo,
  CloudLibrary,
  CollectionStats,
  DeviceInfo,
  Found,
  Gallery,
  GameFolder,
  GamePatch,
  HandheldInfo,
  HandheldMode,
  JoinSuggestion,
  LaunchInfo,
  LineList,
  LocalFiles,
  MigrateReport,
  NewerVersion,
  NxStatus,
  Onboarding,
  OpenListInfo,
  OtherInstall,
  OtherSaves,
  Outcome,
  Overview,
  PlaytimeSummary,
  SaveCandidate,
  SaveLine,
  SaveSources,
  SaveTitle,
  ScreenshotReport,
  Settings,
  SettingsPatch,
  SgdbGame,
  SgdbImage,
  SgdbStatus,
  ShortcutView,
  SnapshotList,
  SnapshotMeta,
  StatusReport,
  SteamAccounts,
  SteamAddResult,
  SteamCloudApp,
  SteamCloudDone,
  SteamCloudStatus,
  SteamPluginInstall,
  SteamPluginStatus,
  SyncFolder,
  TransferPlan,
  TrashedSnapshot,
  UpdateInfo,
} from "./bindings";

export type {
  AccountInfo,
  AdapterInfo,
  BaiduDeviceCode,
  CloudConfig,
  CloudGame,
  CloudGameInfo,
  CloudKind,
  CloudLibrary,
  CloudOnlyView,
  CollectionStats,
  ConflictInfo,
  DeviceInfo,
  FileEntry,
  Found,
  Gallery,
  GameFolder,
  GameSource,
  GameView,
  HandheldInfo,
  HandheldMode,
  Health,
  JoinSuggestion,
  LaunchInfo,
  LineMember,
  LocalFile,
  MigrateProgress,
  MigrateReport,
  MillenniumInstall,
  NewerElsewhere,
  NewerVersion,
  NxStatus,
  OpenListInfo,
  OtherInstall,
  OtherSave,
  Outcome,
  Overview,
  PlatformMark,
  PlaytimeDevice,
  PlaytimeSummary,
  PromptCompare,
  PromptRequest,
  PromptSide,
  SaveBind,
  SaveCandidate,
  SaveLine,
  SaveSource,
  SaveSources,
  SaveTake,
  SaveTitle,
  ScreenshotMode,
  ScreenshotReport,
  Settings,
  SgdbGame,
  SgdbImage,
  ShortcutView,
  Shot,
  SnapshotList,
  SnapshotMeta,
  SnapshotSummary,
  SourceKind,
  Status,
  StatusReport,
  SteamAccount,
  SteamCloudApp,
  SteamCloudDone,
  SteamCloudState,
  SteamCloudStatus,
  SteamPluginProgress,
  SteamPluginStatus,
  SteamWrapperHealth,
  SyncEvent,
  SyncFolder,
  TransferPlan,
  TrashedSnapshot,
  Trigger,
  UnsentSave,
  UpdateInfo,
  UpdateProgress,
} from "./bindings";

/** True inside the Tauri app; false when the UI runs in a plain browser (dev preview). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) return tauriInvoke<T>(cmd, args);
  const { mockInvoke } = await import("./mock");
  return (await mockInvoke(cmd, args)) as T;
}

/** How each platform the versions name is shown. */
export type PlatformMarks = SnapshotList["platforms"];

/** The artwork kinds a game has. */
export type ArtKind = keyof ArtView;

/** The clouds connected with an address and a secret (`cloud_test`, `cloud_connect`). */
export type WebDavConfig = Extract<CloudConfig, { kind: "webDav" }>;
export type S3Config = Extract<CloudConfig, { kind: "s3" }>;

/** A game's playtime per platform, when it was played on more than one: `[name, seconds]`. */
export function playtimePlatforms(p: PlaytimeSummary): Array<[string, number]> {
  const list = (p.platforms ?? []).filter((x) => x.secs > 0);
  return list.length > 1 ? list.map((x) => [p.platformNames?.[x.platform] ?? x.platform, x.secs]) : [];
}

export const api = {
  overview: () => invoke<Overview>("get_overview"),
  check: (key: string) => invoke<StatusReport>("check_game", { key }),
  sync: (key: string, resolution: "auto" | "local" | "cloud" = "auto") =>
    invoke<Outcome>("sync_game", { key, resolution }),
  backup: (key: string, comment: string) => invoke<Outcome>("backup_game", { key, comment }),
  lines: (key: string) => invoke<SaveLine[]>("game_lines", { key }),
  lineMembers: (key: string) => invoke<LineList>("game_line_members", { key }),
  otherSaves: (key: string) => invoke<OtherSaves>("other_saves", { key }),
  /** Take back the last time saves of the cloud replaced the ones here. */
  undoReplace: (key: string) => invoke<Outcome>("undo_replace", { key }),
  /** Make a version of another save line the progress here. */
  takeSave: (key: string, line: string, id: string) => invoke<Outcome>("take_save", { key, line, id }),
  /** The line this version could share with one that keeps the same saves elsewhere; null when there is none. */
  joinSuggestion: (key: string) => invoke<JoinSuggestion | null>("join_suggestion", { key }),
  /** Share the suggested line (then sync with it), never suggest it again, or stop sharing. */
  joinAnswer: (key: string, answer: "accept" | "decline" | "leave", suggestion?: JoinSuggestion) =>
    invoke<Outcome | null>("join_answer", { key, answer, suggestion: suggestion ?? null }),
  /** Follow another save line: the shared one, one of this device's own, or the one another device keeps. */
  useLine: (key: string, choice: { kind: "shared" } | { kind: "independent"; name?: string } | { kind: "follow"; line: string }) =>
    invoke<Outcome>("use_game_line", {
      key,
      kind: choice.kind,
      line: choice.kind === "follow" ? choice.line : null,
      name: choice.kind === "independent" ? (choice.name ?? null) : null,
    }),
  snapshots: (key: string) => invoke<SnapshotList>("list_snapshots", { key }),
  snapshot: (key: string, id: string) => invoke<SnapshotMeta>("get_snapshot", { key, id }),
  restore: (key: string, id: string) => invoke<Outcome>("restore_snapshot", { key, id }),
  setSnapshot: (key: string, id: string, patch: { locked?: boolean; comment?: string }) =>
    invoke<void>("set_snapshot", { key, id, locked: patch.locked ?? null, comment: patch.comment ?? null }),
  /** Moves the version to 「最近删除」. */
  deleteSnapshot: (key: string, id: string) => invoke<void>("delete_snapshot", { key, id }),
  deletedSnapshots: (key: string) => invoke<TrashedSnapshot[]>("deleted_snapshots", { key }),
  /** Resolves to whether the version was also locked (so retention keeps it). */
  restoreDeletedSnapshot: (key: string, id: string) => invoke<boolean>("restore_deleted_snapshot", { key, id }),
  purgeDeletedSnapshot: (key: string, id: string) => invoke<void>("purge_deleted_snapshot", { key, id }),
  /** Save one version as a .lnsave file (to keep, or to give to someone); the path, or null when cancelled. */
  exportSnapshot: (key: string, id: string) => invoke<string | null>("export_snapshot", { key, id }),
  /** Take in a .lnsave file the user picks, beside the game's history; its id, or null when cancelled. */
  importSharedSave: (key: string) => invoke<string | null>("import_shared_save", { key }),
  /** Save the files a version that couldn't be uploaded complete has on this PC as a zip; the path, or null when cancelled. */
  exportUnsent: (key: string, id: string) => invoke<string | null>("export_unsent", { key, id }),
  /** Stop listing a version that couldn't be uploaded complete (its local copy stays). */
  dismissUnsent: (key: string, id: string) => invoke<void>("dismiss_unsent", { key, id }),
  localFiles: (key: string) => invoke<LocalFiles>("local_files", { key }),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  openSaveFolder: (key: string) => invoke<void>("open_save_folder", { key }),
  steamAccounts: () => invoke<SteamAccounts>("steam_accounts"),
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
  refreshCloudLibrary: () => invoke<CloudLibrary>("refresh_cloud_library"),
  /** Put the versions of one game on the collection's shelf ("finished", "archived"), or take them off it. */
  setShelf: (keys: string[], tag: "finished" | "archived", on: boolean) => invoke<void>("set_shelf", { keys, tag, on }),
  collectionStats: () => invoke<CollectionStats>("collection_stats"),
  cloudGameInfo: (key: string) => invoke<CloudGameInfo>("cloud_game_info", { key }),
  otherInstalls: (key: string | null) => invoke<OtherInstall[]>("other_installs", { key }),
  addInstall: (id: string) => invoke<string>("add_install", { id }),
  linkGames: (keys: string[]) => invoke<void>("link_games", { keys }),
  unlinkGame: (key: string) => invoke<void>("unlink_game", { key }),
  transferPlan: (source: string, id: string, target: string, base: string | null) =>
    invoke<TransferPlan>("transfer_plan", { source, id, target, base }),
  transferApply: (source: string, id: string, target: string, base: string | null) =>
    invoke<Outcome>("transfer_apply", { source, id, target, base }),
  requestInstall: (key: string) => invoke<void>("game_backup_request_install", { key }),
  launchMode: () => invoke<LaunchInfo>("launch_mode"),
  promptAnswer: (choice: string | null) => invoke<void>("prompt_answer", { choice }),
  /** The question is on screen: the wrapper stops waiting to ask with a system dialog instead. */
  promptShown: () => invoke<void>("prompt_shown"),
  setFullscreen: (on: boolean) => invoke<void>("set_fullscreen", { on }),
  quitHandheld: () => invoke<void>("quit_handheld"),
  handheldZoom: (on: boolean) => invoke<void>("handheld_zoom", { on }),
  handheldInfo: () => invoke<HandheldInfo>("handheld_info"),
  setHandheldMode: (mode: HandheldMode) => invoke<void>("set_handheld_mode", { mode }),
  steamAddLudonimbus: (account: string, closeSteam: boolean) => invoke<number>("steam_add_ludonimbus", { account, closeSteam }),
  newerVersion: (key: string) => invoke<NewerVersion | null>("newer_version", { key }),
  dismissNewer: (key: string, id: string) => invoke<void>("dismiss_newer", { key, id }),
  // The other sources of a game's saves (the Saves tab's groups) and the newer progress among them (the hint row).
  saveSources: (key: string) => invoke<SaveSources>("save_sources", { key }),
  nxStatus: () => invoke<NxStatus>("nx_status"),
  nxSetEnabled: (enabled: boolean) => invoke<NxStatus>("nx_set_enabled", { enabled }),
  nxStartPairing: () => invoke<NxStatus>("nx_start_pairing"),
  nxCancelPairing: () => invoke<NxStatus>("nx_cancel_pairing"),
  nxRemoveDevice: (id: string) => invoke<NxStatus>("nx_remove_device", { id }),
  nxRenameDevice: (id: string, name: string) => invoke<NxStatus>("nx_rename_device", { id, name }),
  nxSetUser: (id: string, uid: string, sync: boolean, separate: boolean) => invoke<NxStatus>("nx_set_user", { id, uid, sync, separate }),
  addFromCloud: (key: string, account: string | null, appid: number | null, exe: string | null) =>
    invoke<string>("add_from_cloud", { key, account, appid, exe }),
  updateGame: (key: string, patch: GamePatch) => invoke<void>("update_game", { key, patch }),
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
  /** Games Steam installed and their state. */
  steamCloudApps: () => invoke<SteamCloudApp[]>("steam_cloud_apps"),
  /** Add games Steam installed, backup only. */
  steamCloudAdd: (appids: number[]) => invoke<SteamCloudDone[]>("steam_cloud_add", { appids }),
  steamCloudStatus: (key: string) => invoke<SteamCloudStatus>("steam_cloud_status", { key }),
  /**
   * Let Ludonimbus keep the game's saves. Without `viaPlugin` Steam Cloud must be off for it already;
   * with it (the user confirmed) a connected Steam plugin turns it off and sets the launch options in
   * Steam. Errors start with STEAM_RUNNING (launch options in Steam's file: close Steam, see
   * withSteamClosed), STEAM_NOT_SYNCED or STEAM_CLOUD_ON.
   */
  steamCloudTakeOver: (key: string, wrapper: boolean, closeSteam: boolean, viaPlugin = false) =>
    invoke<SteamCloudStatus>("steam_cloud_take_over", { key, wrapper, closeSteam, viaPlugin }),
  /** Back to Steam Cloud: launch options as before, backup only; with `viaPlugin` Steam turns Steam Cloud on again. */
  steamCloudHandBack: (key: string, closeSteam: boolean, viaPlugin = false) =>
    invoke<SteamCloudStatus>("steam_cloud_hand_back", { key, closeSteam, viaPlugin }),
  /** Open the game's properties in Steam (its Steam Cloud switch is there). */
  steamCloudOpenProperties: (key: string) => invoke<void>("steam_cloud_open_properties", { key }),
  /** Every game Steam installed whose save location is known, backup only. */
  steamCloudBackupAll: () => invoke<SteamCloudDone[]>("steam_cloud_backup_all"),
  /** Take over every backed-up game; refused (STEAM_CLOUD_ON) unless Steam Cloud is off for all games, or `viaPlugin` and a Steam plugin turns it off for each. */
  steamCloudTakeOverAll: (wrapper: boolean, closeSteam: boolean, viaPlugin = false) =>
    invoke<SteamCloudDone[]>("steam_cloud_take_over_all", { wrapper, closeSteam, viaPlugin }),
  settings: () => invoke<Settings>("get_settings"),
  saveSettings: (patch: SettingsPatch) => invoke<void>("save_settings", { patch }),
  screenshots: (key: string) => invoke<Gallery>("screenshot_gallery", { key }),
  screenshotFetch: (key: string, device: string, name: string, thumb: boolean) =>
    invoke<string>("screenshot_fetch", { key, device, name, thumb }),
  screenshotSync: (key: string) => invoke<ScreenshotReport>("screenshot_sync", { key }),
  screenshotReveal: (key: string, device: string, name: string) => invoke<void>("screenshot_reveal", { key, device, name }),
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
  saveTitles: (name: string, exe: string | null, install: string | null) =>
    invoke<SaveTitle[]>("savefind_titles", { name, exe, install }),
  exeInFolder: (path: string) => invoke<string | null>("exe_in_folder", { path }),
  sgdbStatus: () => invoke<SgdbStatus>("sgdb_status"),
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
    invoke<SteamAddResult[]>("steam_add_games", { keys, account, autoArt, closeSteam }),
  steamRemoveGame: (key: string, closeSteam: boolean) => invoke<void>("steam_remove_game", { key, closeSteam }),
  cloudAccount: () => invoke<AccountInfo>("cloud_account"),
  devices: () => invoke<DeviceInfo[]>("list_devices"),
  ludusaviSearch: (query: string) => invoke<string[]>("ludusavi_search", { query }),
  steamPluginStatus: () => invoke<SteamPluginStatus>("steam_plugin_status"),
  steamPluginInstall: (closeSteam: boolean) => invoke<SteamPluginInstall>("steam_plugin_install", { closeSteam }),
  steamPluginRemove: (closeSteam: boolean) => invoke<void>("steam_plugin_remove", { closeSteam }),
  language: () => invoke<string>("get_language"),
  onboarding: () => invoke<Onboarding>("get_onboarding"),
  finishOnboarding: (usage: string[]) => invoke<void>("finish_onboarding", { usage }),
  setLanguage: (language: string) => invoke<void>("set_language", { language }),
  checkUpdate: () => invoke<UpdateInfo>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
  /** 「全部同步」 for one game: upload what changed here, write nothing into its saves, and say how it stands. */
  uploadGame: (key: string) => invoke<UploadResult>("upload_game", { key }),
  /** Another version's line this one can share (「共用进度」), when it fits; null otherwise. */
  shareOffer: (key: string) => invoke<ShareOffer | null>("share_offer", { key }),
};

export type { ShareOffer, UploadResult } from "./bindings";
import type { ShareOffer, UploadResult } from "./bindings";

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
