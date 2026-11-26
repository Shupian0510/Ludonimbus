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
  | "conflict-keep"
  /** A save someone exported, taken in beside the history. */
  | "shared";

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
  /** The game version that wrote these saves (read from them, where a rule says how). */
  gameVersion?: string | null;
  /** The game version installed on the device that saved. */
  installedVersion?: string | null;
  /** What it was made on (`pc`, `pc:xbox`, `emu:<rule>`…); absent when not known. */
  platform?: string;
  /** The newest version of its save line. */
  head?: boolean;
  /** Kept beside the history (the saves a restore replaced). */
  side?: boolean;
}

/** How a platform is shown: its name, and the program's own logo when it is on this PC. */
export interface PlatformMark {
  label: string;
  logo: string | null;
}

export type PlatformMarks = Record<string, PlatformMark>;

export interface SnapshotList {
  snapshots: SnapshotSummary[];
  headId: string | null;
  lastSyncedId: string | null;
  platforms: PlatformMarks;
}

/** One version of a game on one device, on a save line. */
export interface LineMember {
  device: string;
  platform: string;
  here: boolean;
}

/** The newest version of a save line this device doesn't follow. */
export interface OtherSave {
  line: string;
  name: string;
  shared: boolean;
  snapshot: SnapshotSummary;
}

export interface FileEntry {
  path: string;
  size: number;
  hash: string;
  mtime: number;
}

/** A version in 「最近删除」, restorable until 30 days after `deletedAt`. */
export interface TrashedSnapshot {
  snapshot: SnapshotSummary;
  deletedAt: string;
  /** Device it was deleted on. */
  deletedBy: string;
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
  /**
   * The game itself is on this PC (its program, and the game file its
   * arguments name). False: only its saves are here, shown grey at the end.
   */
  playable: boolean;
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
  playtime: PlaytimeSummary;
  source: GameSource;
  /**
   * Which game this is a version of (the key of its first version): versions
   * of one game share it and show as one card (docs/plan-phase5.md §5).
   */
  family: string;
  /** The user's "same game" link (`solo:…` = kept apart); null = detected. */
  link: string | null;
  /** It shares the save line of another version through matched folders. */
  joined: boolean;
  /** It reads and writes that line through the Xbox app's save containers: writing into them is experimental. */
  xboxContainers: boolean;
  /** The last time saves of the cloud replaced the ones here, while that can still be undone. */
  replaced: { device: string; at: string } | null;
  /** For a game Steam installed: who keeps its saves, and whether its launch options go through the wrapper. */
  store: { appid: number; mode: SteamCloudState; wrapper: boolean } | null;
}

/** A game the cloud has but this PC doesn't: grey in the library. */
export interface CloudOnlyView {
  key: string;
  name: string;
  adapter: string | null;
  adapterName: string | null;
  platform: string | null;
  ludusaviTitle: string | null;
  saves: string[];
  source: GameSource;
  art: GameView["art"];
  playtime: PlaytimeSummary;
  family: string;
  link: string | null;
}

export interface CloudLibrary {
  cloudOnly: CloudOnlyView[];
  families: Record<string, string>;
  fetchedAt: string | null;
  error: string | null;
}

export interface CloudGameInfo {
  playtime: PlaytimeSummary;
  gameBackup: { versions: number; size: number; createdAt: string; deviceName: string } | null;
}

/** What applying a save version to another version of the game would do. */
export interface TransferPlan {
  same: boolean;
  fromXbox: boolean;
  toXbox: boolean;
  xboxRule: string | null;
  items: { from: string; to: string; size: number; replaces: boolean }[];
  unmatched: string[];
  base: string | null;
  matched: number;
  needsFolder: boolean;
  /** Different save formats: only versions the user linked by hand. */
  crossPlatform: boolean;
  /** Both versions follow this cross-platform rule (`adapters/cross-saves.yaml`). */
  formatRule: string | null;
  /** Source files that aren't saves under that rule (settings…): not written. */
  skipped: string[];
  sourceVersion: string | null;
  targetVersion: string | null;
  /** The source's saves are from a newer game version than the target's. */
  tooNew: boolean;
  /** The target is a Switch version only in the cloud: uploaded as its newest version. */
  cloudTarget: boolean;
}

/** A question of the launch wrapper, shown full screen (`ludonimbus-app --prompt`). */
export interface PromptRequest {
  title: string;
  text: string;
  level: "info" | "warn" | "error";
  buttons: { id: string; label: string; key: "a" | "b" | "x" | "y" }[];
}

export interface LaunchInfo {
  mode: "desktop" | "handheld" | "prompt";
  prompt: PromptRequest | null;
}

export type HandheldMode = "auto" | "on" | "off";

export interface HandheldInfo {
  device: string | null;
  steamGamepadUi: boolean;
  mode: HandheldMode;
  inSteam: { account: string; appid: number } | null;
}

/** Settings → Switch: the Switch client's LAN connection. */
export interface NxStatus {
  enabled: boolean;
  running: boolean;
  address: string | null;
  port: number;
  fingerprint: string | null;
  devices: { id: string; name: string; pairedAt: string; lastSeen: string | null }[];
  pairingCode: string | null;
  error: string | null;
}

/** Another version of a game with newer saves than this one. */
export interface NewerVersion {
  key: string;
  name: string;
  label: string;
  snapshot: SnapshotSummary;
}

/** What a game runs on: detected from its save rule and program. */
export type SourceKind = "emulator" | "console" | "recomp" | "pc" | "xbox" | "custom";

export interface GameSource {
  kind: SourceKind;
  /** The emulator (`Dolphin`); null for the other kinds. */
  name: string | null;
  /** The rule's platform (`GameCube / Wii emulator`, `Longshot (native remake)`). */
  detail: string | null;
  /** The group the user chose instead; null = detected. */
  label: string | null;
  /**
   * The icon of the program the game runs on (its emulator or recomp, or the
   * Xbox app), read from the program on this PC; null when there is none.
   */
  logo: string | null;
}

export interface PlaytimeDevice {
  device: string;
  deviceName: string;
  secs: number;
  lastPlayed: string | null;
  thisDevice: boolean;
  /** Minutes imported from Steam (played before Ludonimbus kept time). */
  steamMinutes: number | null;
}

export interface PlaytimeSummary {
  totalSecs: number;
  lastPlayed: string | null;
  devices: PlaytimeDevice[];
  /** Time per platform, most first. Sessions from before they said where they were played are only in the total. */
  platforms?: { platform: string; secs: number }[];
  /** What each of those platforms is called. */
  platformNames?: Record<string, string>;
}

/** A game's playtime per platform, when it was played on more than one: `[name, seconds]`. */
export function playtimePlatforms(p: PlaytimeSummary): Array<[string, number]> {
  const list = (p.platforms ?? []).filter((x) => x.secs > 0);
  return list.length > 1 ? list.map((x) => [p.platformNames?.[x.platform] ?? x.platform, x.secs]) : [];
}

export interface Overview {
  deviceId: string;
  deviceName: string;
  cloud: { kind: CloudKind; label: string | null; error: string | null };
  steamFound: boolean;
  steamRunning: boolean;
  games: GameView[];
  /** Games the cloud has but this PC doesn't (from the copy kept on this PC). */
  cloudOnly: CloudOnlyView[];
  cloudFetchedAt: string | null;
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
  /** Its app id in Steam when it is known which installed game it is: added as a game Steam installed (backup only). */
  storeAppid?: number | null;
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

export type ScreenshotMode = "off" | "upload" | "sync";

export interface Shot {
  name: string;
  /** Device that took it. */
  device: string;
  deviceName: string | null;
  size: number;
  takenAt: string | null;
  onThisPc: boolean;
  inCloud: boolean;
  hasThumb: boolean;
  thumbUrl: string | null;
}

export interface Gallery {
  shots: Shot[];
  mode: ScreenshotMode;
  linked: boolean;
  folder: string | null;
  cloudBytes: number;
  cloudError: string | null;
}

export interface ScreenshotReport {
  uploaded: number;
  downloaded: number;
  failed: number;
}

export interface Settings {
  screenshots: ScreenshotMode;
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
  /** Games new to this device keep their saves to themselves. */
  independentSaves: boolean;
  /** Before playing, how long the launch wrapper waits for the cloud (seconds). */
  launchWaitSecs: number;
  /** After playing, how long it waits for the upload before handing it over (seconds). */
  exitWaitSecs: number;
  /** After playing, always wait until the upload is done. */
  waitForUpload: boolean;
}

/** A save line a game can follow on this device. */
export interface SaveLine {
  id: string;
  /** What a device called the line it keeps to itself; "" for the shared line. */
  name: string;
  shared: boolean;
  /** The line the game follows on this device. */
  current: boolean;
  /** Names of the devices on it. */
  devices: string[];
  /** The same, one for each version of the game a device has on it. */
  members: LineMember[];
}

/** One folder of a save line and the folder of this version that holds the same files. */
export interface SaveBind {
  canon: string;
  local: string;
  /** How this version keeps them: plain files when absent, "wgs" in the Xbox app's save containers. */
  codec?: string;
}

/** A save line this version could share although it keeps its saves in other folders. */
export interface JoinSuggestion {
  /** The game record to belong to. */
  record: string;
  /** Which files the line holds (what a decline remembers). */
  def: string;
  /** The line's newest version: who made it, on what, when. */
  device: string;
  platform: string;
  createdAt: string;
  /** Confirmed on another device already. */
  confirmed: boolean;
  /** ready: can be shared. needsSaves: play once on this device first. noMatch: the files don't correspond. */
  fit: "ready" | "needsSaves" | "noMatch";
  binds: SaveBind[];
  matched: number;
  onlyInLine: number;
  onlyHere: number;
  /** Files of the line with no folder here. */
  uncovered: string[];
  /** Files here that stay outside and are no longer uploaded once shared. */
  outside: string[];
}

/**
 * A game Steam installed, in one of three states: Steam Cloud keeps it and Ludonimbus does nothing,
 * Ludonimbus stores versions of what Steam Cloud syncs (backup), or Steam Cloud is off for it and
 * Ludonimbus syncs it (managed).
 */
export type SteamCloudState = "steam" | "backup" | "managed";

/** A game Steam installed on this PC. */
export interface SteamCloudApp {
  appid: number;
  name: string;
  /** Its library entry, once added. */
  key: string | null;
  state: SteamCloudState;
  /** How Steam's last cloud sync of it went ("synchronized", "changesincloud", …). */
  syncState: string | null;
  /** Its own Steam Cloud switch. */
  cloudEnabled: boolean;
  /** Steam Cloud syncs it on this device (false: the game has none). */
  cloudUsed: boolean;
}

/** Whether a managed game's launch options still go through the wrapper. */
export type SteamWrapperHealth = "ok" | "missing" | "changed" | "runnerMoved";

export interface SteamCloudStatus {
  mode: SteamCloudState;
  syncState: string | null;
  /** Steam's last cloud sync of the game left nothing behind (taking over needs it). */
  synchronized: boolean;
  cloudEnabled: boolean;
  /** Steam Cloud syncs it on this device (false: the game has none). */
  cloudUsed: boolean;
  /** The account's Steam Cloud switch for every game. */
  globalEnabled: boolean;
  steamRunning: boolean;
  /** null when the launch options aren't meant to go through the wrapper. */
  wrapper: SteamWrapperHealth | null;
}

/** What a batch did for one game Steam installed. */
export interface SteamCloudDone {
  appid: number;
  key: string | null;
  error: string | null;
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
  lines: (key: string) => invoke<SaveLine[]>("game_lines", { key }),
  lineMembers: (key: string) => invoke<{ lines: SaveLine[]; platforms: PlatformMarks }>("game_line_members", { key }),
  otherSaves: (key: string) => invoke<{ saves: OtherSave[]; platforms: PlatformMarks }>("other_saves", { key }),
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
  refreshCloudLibrary: () => invoke<CloudLibrary>("refresh_cloud_library"),
  cloudGameInfo: (key: string) => invoke<CloudGameInfo>("cloud_game_info", { key }),
  linkGames: (keys: string[]) => invoke<void>("link_games", { keys }),
  unlinkGame: (key: string) => invoke<void>("unlink_game", { key }),
  transferPlan: (source: string, id: string, target: string, base: string | null) =>
    invoke<TransferPlan>("transfer_plan", { source, id, target, base }),
  transferApply: (source: string, id: string, target: string, base: string | null) =>
    invoke<Outcome>("transfer_apply", { source, id, target, base }),
  requestInstall: (key: string) => invoke<void>("game_backup_request_install", { key }),
  launchMode: () => invoke<LaunchInfo>("launch_mode"),
  promptAnswer: (choice: string | null) => invoke<void>("prompt_answer", { choice }),
  setFullscreen: (on: boolean) => invoke<void>("set_fullscreen", { on }),
  quitHandheld: () => invoke<void>("quit_handheld"),
  handheldInfo: () => invoke<HandheldInfo>("handheld_info"),
  setHandheldMode: (mode: HandheldMode) => invoke<void>("set_handheld_mode", { mode }),
  steamAddLudonimbus: (account: string, closeSteam: boolean) => invoke<number>("steam_add_ludonimbus", { account, closeSteam }),
  newerVersion: (key: string) => invoke<NewerVersion | null>("newer_version", { key }),
  dismissNewer: (key: string, id: string) => invoke<void>("dismiss_newer", { key, id }),
  nxStatus: () => invoke<NxStatus>("nx_status"),
  nxSetEnabled: (enabled: boolean) => invoke<NxStatus>("nx_set_enabled", { enabled }),
  nxStartPairing: () => invoke<NxStatus>("nx_start_pairing"),
  nxCancelPairing: () => invoke<NxStatus>("nx_cancel_pairing"),
  nxRemoveDevice: (id: string) => invoke<NxStatus>("nx_remove_device", { id }),
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
      /** Library group; "" = detected. */
      source?: string;
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
  /** Games Steam installed and their state. */
  steamCloudApps: () => invoke<SteamCloudApp[]>("steam_cloud_apps"),
  /** Add games Steam installed, backup only. */
  steamCloudAdd: (appids: number[]) => invoke<SteamCloudDone[]>("steam_cloud_add", { appids }),
  steamCloudStatus: (key: string) => invoke<SteamCloudStatus>("steam_cloud_status", { key }),
  /**
   * Let Ludonimbus keep the game's saves (Steam Cloud must be off for it). Errors start with
   * STEAM_RUNNING (with the wrapper: close Steam, see withSteamClosed), STEAM_NOT_SYNCED or STEAM_CLOUD_ON.
   */
  steamCloudTakeOver: (key: string, wrapper: boolean, closeSteam: boolean) =>
    invoke<SteamCloudStatus>("steam_cloud_take_over", { key, wrapper, closeSteam }),
  /** Back to Steam Cloud: launch options as before, backup only. */
  steamCloudHandBack: (key: string, closeSteam: boolean) => invoke<SteamCloudStatus>("steam_cloud_hand_back", { key, closeSteam }),
  /** Open the game's properties in Steam (its Steam Cloud switch is there). */
  steamCloudOpenProperties: (key: string) => invoke<void>("steam_cloud_open_properties", { key }),
  /** Every game Steam installed whose save location is known, backup only. */
  steamCloudBackupAll: () => invoke<SteamCloudDone[]>("steam_cloud_backup_all"),
  /** Take over every backed-up game; refused (STEAM_CLOUD_ON) unless Steam Cloud is off for all games. */
  steamCloudTakeOverAll: (wrapper: boolean, closeSteam: boolean) =>
    invoke<SteamCloudDone[]>("steam_cloud_take_over_all", { wrapper, closeSteam }),
  settings: () => invoke<Settings>("get_settings"),
  saveSettings: (patch: {
    deviceName?: string;
    keepRecent?: number;
    keepDailyDays?: number;
    networkTimeoutSecs?: number;
    steamAccount?: string;
    screenshots?: ScreenshotMode;
    independentSaves?: boolean;
    launchWaitSecs?: number;
    exitWaitSecs?: number;
    waitForUpload?: boolean;
  }) => invoke<void>("save_settings", { patch }),
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
