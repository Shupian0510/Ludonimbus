// Browser preview data for game backups and the speed test.
import type { BackupInfo, PartKind, PartSummary, SpeedReport, VersionSummary } from "./gameBackupApi";
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
const GB = 1024 ** 3;

function part(kind: PartKind, name: string, version: string | null, totalSize: number): PartSummary {
  return { kind, name, version, totalSize, fileCount: 100 };
}

function version(id: string, hoursAgo: number, device: string, components: PartSummary[]): VersionSummary {
  const total = components.reduce((n, c) => n + c.totalSize, 0);
  return {
    id,
    game: "",
    name: "",
    createdAt: ago(hoursAgo),
    deviceId: "d-1",
    deviceName: device,
    appVersion: "0.6.0",
    components,
    totalSize: total,
    storedSize: total * 0.8,
    newBytes: total * 0.05,
    fileCount: 1832,
    comment: "",
  };
}

const fefw = [part("program", "fefw recomp", "0.2.7", 0.18 * GB), part("config", L("设置和 mods", "Settings and mods"), null, 2_000_000)];
const lantern = [
  part("program", "Lantern Engine", "1.4", 0.3 * GB),
  part("content", "Bloodborne", "v1.09", 21.4 * GB),
  part("config", L("设置和 mods", "Settings and mods"), null, 40_000),
];

function info(key: string): BackupInfo {
  const roots: Record<string, string> = {
    "fe-fortunes-weave": "C:\\Users\\me\\Desktop\\Switch\\fefw-recomp-0.2.7",
    "bloodborne-lantern": "C:\\Users\\me\\Desktop\\Switch\\Bloodborne\\BloodbornePC",
  };
  const root = roots[key] ?? "D:\\Games\\Example";
  const versions =
    key === "fe-fortunes-weave"
      ? [version("01JF2", 20, "DESKTOP-MAIN", fefw), version("01JF1", 300, "GPD-Win-Mini", fefw)]
      : key === "bloodborne-lantern"
        ? [version("01JB1", 50, "DESKTOP-MAIN", lantern)]
        : [];
  const missing = key === "bloodborne-lantern";
  return {
    hasCloud: true,
    cloudKind: "googleDrive",
    slow: false,
    root,
    rootExists: !missing,
    rootCustom: false,
    exeMissing: missing,
    versions,
    storedBytes: versions.reduce((n, v) => n + v.storedSize, 0),
    keep: 2,
    exclude: [],
    defaultExclude: ["**/*.log", "**/*.dmp", "**/*.tmp", "**/crashdumps/**", "**/shadercache/**", "**/shader_cache/**"],
    firmwareAvailable: missing,
    firmware: false,
    skip: [],
    extraContent: [],
    byParts: key !== "celeste",
    job: null,
    pendingRestore: null,
    restore: versions.length
      ? {
          hasProgram: true,
          hasContent: missing,
          programName: versions[0].components[0].name,
          programVersion: versions[0].components[0].version,
          installedProgram: missing ? null : root,
          installedSame: true,
          programDir: missing ? root : root,
          contentDir: missing ? "D:\\PS4" : null,
        }
      : null,
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
      return delay(
        (key === "bloodborne-lantern" ? lantern : fefw).map((p) => ({
          kind: p.kind,
          name: p.name,
          version: p.version,
          files: 120,
          bytes: p.totalSize,
          excludedFiles: p.kind === "program" ? 14 : 0,
          excludedBytes: p.kind === "program" ? 2_400_000 : 0,
        })),
        600,
      );
    case "game_backup_resolve_target":
      return delay(args.chosen as string);
    case "game_backup_take_install":
      return delay(false);
    case "game_backup_job":
      return delay(null);
    case "game_backup_pick_file":
      return delay(null);
    default:
      return delay(undefined);
  }
}
