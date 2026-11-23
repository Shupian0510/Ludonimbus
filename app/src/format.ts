import type { Health, Outcome, Status, Trigger } from "./api";

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
  if (!iso) return "从未";
  const t = new Date(iso).getTime();
  const diff = (Date.now() - t) / 1000;
  if (diff < 45) return "刚刚";
  if (diff < 3600) return `${Math.round(diff / 60)} 分钟前`;
  if (diff < 86400) return `${Math.round(diff / 3600)} 小时前`;
  if (diff < 86400 * 30) return `${Math.round(diff / 86400)} 天前`;
  return dateTime(iso);
}

export function dateTime(iso: string | number): string {
  const d = typeof iso === "number" ? new Date(iso * 1000) : new Date(iso);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export const triggerLabel: Record<Trigger, string> = {
  "auto-exit": "退出游戏后自动",
  "pre-launch": "启动前补传",
  passive: "后台检测到退出",
  "sync-now": "手动同步",
  manual: "手动备份",
  restore: "恢复旧版本",
  "pre-restore": "恢复前保护",
  "conflict-keep": "冲突时保留",
};

export const statusInfo: Record<Status, { label: string; tone: "ok" | "info" | "warn" | "bad" | "muted" }> = {
  synced: { label: "已同步", tone: "ok" },
  localChanged: { label: "待上传", tone: "info" },
  cloudNewer: { label: "云端较新", tone: "info" },
  canRestore: { label: "可从云端恢复", tone: "info" },
  notUploaded: { label: "尚未上传", tone: "info" },
  conflict: { label: "存档冲突", tone: "warn" },
  offline: { label: "云端离线", tone: "muted" },
  noCloud: { label: "未连接云盘", tone: "muted" },
  empty: { label: "暂无存档", tone: "muted" },
  error: { label: "出错", tone: "bad" },
};

export function outcomeText(o: Outcome, name?: string): string {
  const n = name ? `「${name}」` : "";
  switch (o.kind) {
    case "upToDate":
      return `${n}已是最新`;
    case "uploaded":
      return `${n}已上传到云端`;
    case "queued":
      return `${n}云端暂时无法访问，存档已保存在本机，稍后自动上传`;
    case "pulled":
      return `${n}已从云端恢复（来自 ${o.device}）`;
    case "conflict":
      return `${n}本机和云端都有新改动，请选择要保留哪一份`;
    case "nothingToSave":
      return `${n}没有找到存档文件`;
    case "noCloud":
      return "还没有连接云盘";
    case "offline":
      return `云端暂时无法访问：${o.message}`;
  }
}

export function healthText(h: Health): { text: string; tone: "ok" | "warn" | "muted" | "bad"; fixable: boolean } {
  switch (h.state) {
    case "ok":
      return { text: "从 Steam 启动时会自动同步", tone: "ok", fixable: false };
    case "disabled":
      return { text: "未启用：从 Steam 启动时不会自动同步", tone: "muted", fixable: false };
    case "notLinked":
      return { text: "没有关联 Steam 快捷方式", tone: "muted", fixable: false };
    case "noSteam":
      return { text: "没有找到 Steam", tone: "muted", fixable: false };
    case "changed":
      return {
        text: `Steam 快捷方式被修改了（现在指向 ${h.exe}），自动同步已失效`,
        tone: "warn",
        fixable: true,
      };
    case "runnerMoved":
      return { text: "Ludonimbus 的位置变了，快捷方式需要更新", tone: "warn", fixable: true };
    case "shortcutMissing":
      return h.candidateAppid
        ? { text: "快捷方式被重新创建过，可以一键重新关联", tone: "warn", fixable: true }
        : { text: "Steam 里找不到这个游戏的快捷方式了", tone: "bad", fixable: false };
  }
}
