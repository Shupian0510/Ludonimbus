import type { Health, Outcome, Status, Trigger } from "./api";
import { msg } from "./i18n";

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 ? 0 : 1)} ${units[i]}`;
}

export function relTime(iso: string | null | undefined): string {
  const f = msg().format;
  if (!iso) return f.never;
  const t = new Date(iso).getTime();
  const diff = (Date.now() - t) / 1000;
  if (diff < 45) return f.justNow;
  if (diff < 3600) return f.minutesAgo(Math.round(diff / 60));
  if (diff < 86400) return f.hoursAgo(Math.round(diff / 3600));
  if (diff < 86400 * 30) return f.daysAgo(Math.round(diff / 86400));
  return dateTime(iso);
}

/** Playtime: minutes below an hour, then hours with one decimal. */
export function duration(secs: number): string {
  const f = msg().format;
  if (secs < 3600) return f.minutes(Math.max(1, Math.round(secs / 60)));
  const h = secs / 3600;
  return f.hours(h >= 100 ? String(Math.round(h)) : h.toFixed(1));
}

export function dateTime(iso: string | number): string {
  const d = typeof iso === "number" ? new Date(iso * 1000) : new Date(iso);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function triggerLabel(t: Trigger): string {
  return msg().format.trigger[t] ?? t;
}

const statusTone: Record<Status, "ok" | "info" | "warn" | "bad" | "muted"> = {
  synced: "ok",
  localChanged: "info",
  cloudNewer: "info",
  canRestore: "info",
  notUploaded: "info",
  conflict: "warn",
  offline: "muted",
  noCloud: "muted",
  empty: "muted",
  error: "bad",
};

export function statusInfo(s: Status): { label: string; tone: "ok" | "info" | "warn" | "bad" | "muted" } {
  return { label: msg().format.status[s], tone: statusTone[s] };
}

export function outcomeText(o: Outcome, name?: string): string {
  const f = msg().format.outcome;
  switch (o.kind) {
    case "upToDate":
      return f.upToDate(name);
    case "uploaded":
      return f.uploaded(name);
    case "queued":
      return f.queued(name);
    case "pulled":
      return f.pulled(name, o.device);
    case "conflict":
      return f.conflict(name);
    case "nothingToSave":
      return f.nothingToSave(name);
    case "noCloud":
      return f.noCloud;
    case "offline":
      return f.offline(o.message);
  }
}

export function healthText(h: Health): { text: string; tone: "ok" | "warn" | "muted" | "bad"; fixable: boolean } {
  const f = msg().format.health;
  switch (h.state) {
    case "ok":
      return { text: f.ok, tone: "ok", fixable: false };
    case "disabled":
      return { text: f.disabled, tone: "muted", fixable: false };
    case "notLinked":
      return { text: f.notLinked, tone: "muted", fixable: false };
    case "noSteam":
      return { text: f.noSteam, tone: "muted", fixable: false };
    case "changed":
      return { text: f.changed(h.exe), tone: "warn", fixable: true };
    case "runnerMoved":
      return { text: f.runnerMoved, tone: "warn", fixable: true };
    case "shortcutMissing":
      return h.candidateAppid
        ? { text: f.shortcutRecreated, tone: "warn", fixable: true }
        : { text: f.shortcutMissing, tone: "bad", fixable: false };
  }
}
