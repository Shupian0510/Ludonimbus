// Texts for game backups and the cloud speed test (exported as gameBackup from en.ts).
import type * as Zh from "./gameBackup.zh-CN";
import { plural } from "./plural";

export const gameBackup: typeof Zh.gameBackup = {
  speed: {
    title: "Cloud speed test",
    intro: "Uploads random test files to the current cloud and downloads them again (one at a time, then in parallel), then deletes them. Shows whether this cloud is suited for backing up games.",
    amount: "Test data",
    threads: "Parallel",
    start: "Run speed test",
    cancel: "Stop",
    noCloud: "Connect a cloud first.",
    phase: {
      latency: "Measuring request time…",
      uploadSerial: "Upload (one at a time)",
      uploadParallel: "Upload (parallel)",
      downloadSerial: "Download (one at a time)",
      downloadParallel: "Download (parallel)",
      cleanup: "Deleting the test files…",
    },
    upload: "Upload",
    download: "Download",
    serial: "One at a time",
    parallel: (n) => `${n} parallel`,
    capped: "stopped at the time limit",
    request: (w, r) => `Fixed cost of one request: about ${w} ms to write, ${r} ms to read`,
    estimate: (size, up, down) => `A ${size} game: about ${up} to upload, ${down} to download`,
    verdict: {
      good: "Suited for game backups",
      slow: "Works, but large games take hours",
      tooSlow: "Too slow for games over a few GB",
      failed: "Speed test failed",
    },
    cleanupFailed: (e) => `Some test files couldn't be deleted: ${e}`,
    failed: (e) => `Speed test failed: ${e}`,
  },
  duration: {
    minutes: (n) => `${n} ${plural(n, "minute", "minutes")}`,
    hours: (n) => `${n} ${plural(n, "hour", "hours")}`,
    days: (n) => `${n} ${plural(n, "day", "days")}`,
    unknown: "can't estimate",
  },
};
