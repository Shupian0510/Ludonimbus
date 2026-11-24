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

export interface VersionSummary {
  id: string;
  game: string;
  name: string;
  createdAt: string;
  deviceId: string;
  deviceName: string;
  appVersion: string;
  root: string;
  totalSize: number;
  storedSize: number;
  newBytes: number;
  fileCount: number;
  chunkCount: number;
  comment: string;
}

export interface BackupJob {
  kind: "backup" | "restore" | "delete";
  /** starting, scanning, listing, uploading, verifying, committing, cleaning, preparing, downloading, finishing, fixing, saves */
  phase: string;
  done: number;
  total: number;
  uploaded: number;
  elapsedSecs: number;
  target: string | null;
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
  fileOutside: string | null;
  job: BackupJob | null;
  pendingRestore: { version: string; target: string } | null;
  defaultTarget: string | null;
  error: string | null;
}

export interface ScanSummary {
  root: string;
  files: number;
  bytes: number;
  excludedFiles: number;
  excludedBytes: number;
  skippedLinks: number;
}

export interface BackupFinished {
  key: string;
  kind: BackupJob["kind"];
  ok: boolean;
  cancelled: boolean;
  message: string;
  outcome: unknown;
}

export const gameBackupApi = {
  speedTest: (mb?: number, threads?: number) => invoke<SpeedReport>("speed_test", { mb, threads }),
  speedTestCancel: () => invoke<void>("speed_test_cancel"),
  info: (key: string) => invoke<BackupInfo>("game_backup_info", { key }),
  preview: (key: string) => invoke<ScanSummary>("game_backup_preview", { key }),
  set: (key: string, patch: { root?: string; exclude?: string[]; keep?: number }) =>
    invoke<void>("game_backup_set", { key, root: patch.root ?? null, exclude: patch.exclude ?? null, keep: patch.keep ?? null }),
  start: (key: string) => invoke<void>("game_backup_start", { key }),
  resolveTarget: (chosen: string, root: string, version: string) =>
    invoke<string>("game_backup_resolve_target", { chosen, root, version }),
  restore: (key: string, version: string, target: string, fix: boolean) =>
    invoke<void>("game_backup_restore", { key, version, target, fix }),
  remove: (key: string, version: string) => invoke<void>("game_backup_delete", { key, version }),
  cancel: (key: string) => invoke<void>("game_backup_cancel", { key }),
  job: (key: string) => invoke<BackupJob | null>("game_backup_job", { key }),
  takeInstall: (key: string) => invoke<boolean>("game_backup_take_install", { key }),
};

/** Throughput assumed for slow clouds before a speed test (bytes/s; see the plan, 1.2). */
export const SLOW_RATES: Record<string, { up: number; down: number }> = {
  baiduPan: { up: 2 * 1024 * 1024, down: 80 * 1024 },
  quark: { up: 0.5 * 1024 * 1024, down: 2.5 * 1024 * 1024 },
};

/** Seconds to move `bytes` at `bps`, or null when nothing was measured. */
export function etaSecs(bytes: number, bps: number): number | null {
  return bps > 0 ? Math.ceil(bytes / bps) : null;
}
