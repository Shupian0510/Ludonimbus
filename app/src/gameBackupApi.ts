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

export const gameBackupApi = {
  speedTest: (mb?: number, threads?: number) => invoke<SpeedReport>("speed_test", { mb, threads }),
  speedTestCancel: () => invoke<void>("speed_test_cancel"),
};

/** Seconds to move `bytes` at `bps`, or null when nothing was measured. */
export function etaSecs(bytes: number, bps: number): number | null {
  return bps > 0 ? Math.ceil(bytes / bps) : null;
}
