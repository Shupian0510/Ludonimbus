// Game backups and the cloud speed test (docs/plan-game-backup.md). Kept apart
// from api.ts so this feature stays in its own files. The types are generated
// from the Rust types (src/bindings, docs/development.md).
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { inTauri } from "./api";
import type { BackupInfo, BackupJob, BackupSettingsPatch, PreviewPart, SpeedReport, Targets } from "./bindings";

export type {
  BackupFinished,
  BackupInfo,
  BackupJob,
  BackupSettingsPatch as SettingsPatch,
  PartKind,
  PartSummary,
  PreviewPart,
  RestoreDefaults,
  SpeedPhase,
  SpeedProgress,
  SpeedReport,
  Targets,
  Verdict,
  VersionSummary,
} from "./bindings";

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) return tauriInvoke<T>(cmd, args);
  const { gameBackupMock } = await import("./gameBackupMock");
  return (await gameBackupMock(cmd, args ?? {})) as T;
}

export const gameBackupApi = {
  speedTest: (mb?: number, threads?: number) => invoke<SpeedReport>("speed_test", { mb, threads }),
  speedTestCancel: () => invoke<void>("speed_test_cancel"),
  info: (key: string) => invoke<BackupInfo>("game_backup_info", { key }),
  preview: (key: string) => invoke<PreviewPart[]>("game_backup_preview", { key }),
  set: (key: string, patch: BackupSettingsPatch) => invoke<void>("game_backup_set", { key, patch }),
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
