// Browser preview data for game backups and the speed test.
import type { BackupInfo, SpeedReport, VersionSummary } from "./gameBackupApi";
import { currentLang } from "./i18n";

const MB = 1024 * 1024;
const L = (zh: string, en: string) => (currentLang() === "en" ? en : zh);
const delay = <T>(v: T, ms = 300) => new Promise<T>((r) => setTimeout(() => r(v), ms));

function phase(threads: number, mbps: number, objects: number, capped = false) {
  const bytes = objects * 8 * MB;
  return { threads, bytes, objects, secs: bytes / (mbps * MB), bytesPerSec: mbps * MB, capped, error: null };
}

const speed: SpeedReport = {
  kind: "baiduPan",
  label: "Baidu Netdisk",
  objectBytes: 8 * MB,
  totalBytes: 200 * MB,
  smallWriteMs: 1240,
  smallReadMs: 3900,
  uploadSerial: phase(1, 2.1, 11, true),
  uploadParallel: phase(4, 6.8, 25),
  downloadSerial: phase(1, 0.08, 1, true),
  downloadParallel: phase(4, 0.31, 3, true),
  uploadBps: 6.8 * MB,
  downloadBps: 0.31 * MB,
  verdict: "tooSlow",
  cleanupError: null,
  notes: [
    L(
      "百度网盘非会员下载限速：实测 9 MB 用了约 116 秒（约 80 KB/s），10 GB 的游戏要下载一天半以上。",
      "Baidu Netdisk throttles downloads without membership: 9 MB took about 116 s in a test (about 80 KB/s), so a 10 GB game takes over a day and a half to download.",
    ),
  ],
};

const ago = (h: number) => new Date(Date.now() - h * 3600_000).toISOString();

function version(id: string, hoursAgo: number, device: string, total: number, root: string): VersionSummary {
  return {
    id,
    game: "",
    name: "",
    createdAt: ago(hoursAgo),
    deviceId: "d-1",
    deviceName: device,
    appVersion: "0.6.0",
    root,
    totalSize: total,
    storedSize: total * 0.8,
    newBytes: total * 0.05,
    fileCount: 1832,
    chunkCount: 412,
    comment: "",
  };
}

function info(key: string): BackupInfo {
  const roots: Record<string, string> = {
    "fe-fortunes-weave": "C:\\Users\\me\\Desktop\\Switch\\fefw-recomp-0.2.7",
    "bloodborne-lantern": "C:\\Users\\me\\Desktop\\Switch\\Bloodborne\\BloodbornePC",
  };
  const root = roots[key] ?? "D:\\Games\\Example";
  const versions =
    key === "fe-fortunes-weave"
      ? [version("01JF2", 20, "DESKTOP-MAIN", 3.4 * 1024 ** 3, root), version("01JF1", 300, "GPD-Win-Mini", 3.3 * 1024 ** 3, root)]
      : key === "bloodborne-lantern"
        ? [version("01JB1", 50, "DESKTOP-MAIN", 21.7 * 1024 ** 3, root)]
        : [];
  return {
    hasCloud: true,
    cloudKind: "googleDrive",
    slow: false,
    root,
    rootExists: key !== "bloodborne-lantern",
    rootCustom: false,
    exeMissing: key === "bloodborne-lantern",
    versions,
    storedBytes: versions.reduce((n, v) => n + v.storedSize, 0),
    keep: 2,
    exclude: [],
    defaultExclude: ["**/*.log", "**/*.dmp", "**/*.tmp", "**/crashdumps/**", "**/shadercache/**", "**/shader_cache/**"],
    fileOutside: null,
    job: null,
    pendingRestore: null,
    defaultTarget: key === "bloodborne-lantern" ? root : null,
    error: null,
  };
}

export async function gameBackupMock(cmd: string, args: Record<string, unknown>): Promise<unknown> {
  const key = args.key as string;
  switch (cmd) {
    case "speed_test":
      return delay(speed, 2500);
    case "game_backup_info":
      return delay(info(key));
    case "game_backup_preview":
      return delay({ root: info(key).root, files: 1832, bytes: 3.4 * 1024 ** 3, excludedFiles: 14, excludedBytes: 2_400_000, skippedLinks: 0 }, 600);
    case "game_backup_resolve_target":
      return delay(args.chosen as string);
    case "game_backup_take_install":
      return delay(false);
    case "game_backup_job":
      return delay(null);
    default:
      return delay(undefined);
  }
}
