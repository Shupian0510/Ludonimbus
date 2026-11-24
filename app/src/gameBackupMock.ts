// Browser preview data for game backups and the speed test.
import type { SpeedReport } from "./gameBackupApi";
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

export async function gameBackupMock(cmd: string, _args: Record<string, unknown>): Promise<unknown> {
  switch (cmd) {
    case "speed_test":
      return delay(speed, 2500);
    case "speed_test_cancel":
      return delay(undefined);
    default:
      return delay(null);
  }
}
