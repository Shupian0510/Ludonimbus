// Game backups and the cloud speed test (docs/plan-game-backup.md). Kept apart
// from api.ts so this feature stays in its own files.
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { inTauri } from "./api";

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) return tauriInvoke<T>(cmd, args);
  const { gameBackupMock } = await import("./gameBackupMock");
  return (await gameBackupMock(cmd, args ?? {})) as T;
}

export interface SpeedPhase {
  threads: number;
  bytes: number;
  objects: number;
  secs: number;
  bytesPerSec: number;
  capped: boolean;
  error: string | null;
}

export type Verdict = "good" | "slow" | "tooSlow" | "failed";

export interface SpeedReport {
  kind: string;
  label: string;
  objectBytes: number;
  totalBytes: number;
  smallWriteMs: number;
  smallReadMs: number;
  uploadSerial: SpeedPhase | null;
  uploadParallel: SpeedPhase | null;
  downloadSerial: SpeedPhase | null;
  downloadParallel: SpeedPhase | null;
  uploadBps: number;
  downloadBps: number;
  verdict: Verdict;
  cleanupError: string | null;
  notes: string[];
}

export interface SpeedProgress {
  phase: "latency" | "uploadSerial" | "uploadParallel" | "downloadSerial" | "downloadParallel" | "cleanup";
  doneBytes: number;
  totalBytes: number;
  elapsedSecs: number;
}

/** Parts of a game backup (docs/plan-game-backup.md, section 8). */
export type PartKind = "program" | "content" | "config" | "firmware" | "folder";

export interface PartSummary {
  kind: PartKind;
  name: string;
  version: string | null;
  totalSize: number;
  fileCount: number;
}

export interface VersionSummary {
  id: string;
  game: string;
  name: string;
  createdAt: string;
  deviceId: string;
  deviceName: string;
  appVersion: string;
  components: PartSummary[];
  totalSize: number;
  storedSize: number;
  newBytes: number;
  fileCount: number;
  comment: string;
}

export interface BackupJob {
  kind: "backup" | "restore" | "delete";
  /** starting, scanning, listing, uploading, verifying, committing, cleaning, preparing, checking, downloading, finishing, fixing, saves */
  phase: string;
  done: number;
  total: number;
  uploaded: number;
  elapsedSecs: number;
  target: string | null;
}

/** Where a restore puts things. */
export interface Targets {
  program: string;
  /** false keeps the program already in `program`. */
  restoreProgram: boolean;
  /** null = don't restore the game's own files. */
  content: string | null;
}

export interface RestoreDefaults {
  hasProgram: boolean;
  hasContent: boolean;
  programName: string | null;
  programVersion: string | null;
  installedProgram: string | null;
  installedSame: boolean;
  programDir: string | null;
  contentDir: string | null;
}

export interface BackupInfo {
  hasCloud: boolean;
  cloudKind: string;
  slow: boolean;
  root: string | null;
  rootExists: boolean;
  rootCustom: boolean;
  exeMissing: boolean;
  /** Newest first. */
  versions: VersionSummary[];
  storedBytes: number;
  /** Versions kept after a backup; 0 = all. */
  keep: number;
  exclude: string[];
  defaultExclude: string[];
  firmwareAvailable: boolean;
  firmware: boolean;
  skip: PartKind[];
  extraContent: string[];
  /** The game's rule describes its parts; otherwise the whole folder. */
  byParts: boolean;
  job: BackupJob | null;
  pendingRestore: { version: string; targets: Targets } | null;
  restore: RestoreDefaults | null;
  error: string | null;
}

export interface PreviewPart {
  kind: PartKind;
  name: string;
  version: string | null;
  files: number;
  bytes: number;
  excludedFiles: number;
  excludedBytes: number;
}

export interface BackupFinished {
  key: string;
  kind: BackupJob["kind"];
  ok: boolean;
  cancelled: boolean;
  message: string;
  outcome: unknown;
}

export interface SettingsPatch {
  root?: string;
  exclude?: string[];
  keep?: number;
  firmware?: boolean;
  skip?: PartKind[];
  extraContent?: string[];
}

export const gameBackupApi = {
  speedTest: (mb?: number, threads?: number) => invoke<SpeedReport>("speed_test", { mb, threads }),
  speedTestCancel: () => invoke<void>("speed_test_cancel"),
  info: (key: string) => invoke<BackupInfo>("game_backup_info", { key }),
  preview: (key: string) => invoke<PreviewPart[]>("game_backup_preview", { key }),
  set: (key: string, patch: SettingsPatch) => invoke<void>("game_backup_set", { key, patch }),
  start: (key: string) => invoke<void>("game_backup_start", { key }),
  resolveTarget: (chosen: string, original: string) => invoke<string>("game_backup_resolve_target", { chosen, original }),
  restore: (key: string, version: string, targets: Targets, fix: boolean) =>
    invoke<void>("game_backup_restore", { key, version, targets, fix }),
  remove: (key: string, version: string) => invoke<void>("game_backup_delete", { key, version }),
  cancel: (key: string) => invoke<void>("game_backup_cancel", { key }),
  job: (key: string) => invoke<BackupJob | null>("game_backup_job", { key }),
  takeInstall: (key: string) => invoke<boolean>("game_backup_take_install", { key }),
  pickFile: (start?: string | null) => invoke<string | null>("game_backup_pick_file", { start: start ?? null }),
};

/** Baidu Netdisk's download speed without membership (bytes/s; see the plan, 1.2). */
export const BAIDU_FREE_DOWN = 80 * 1024;

/** Seconds to move `bytes` at `bps`, or null when nothing was measured. */
export function etaSecs(bytes: number, bps: number): number | null {
  return bps > 0 ? Math.ceil(bytes / bps) : null;
}
