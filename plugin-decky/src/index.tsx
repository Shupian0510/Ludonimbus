// Ludonimbus in Steam Deck Game Mode (Decky Loader, docs/plan-platforms.md
// §2.2): the quick access menu shows the cloud save status of the game
// that's running (or was last started), with sync now, the conflict choice
// and newer saves from another version of the game, all with the controller,
// and the version history with restore.
import { callable, definePlugin, toaster } from "@decky/api";
import { ButtonItem, PanelSection, PanelSectionRow, Router, staticClasses } from "@decky/ui";
import { useCallback, useEffect, useState } from "react";
import { FaCloud } from "react-icons/fa";

type Reply<T> = { ok: boolean; status: number; body: T & { error?: string } };
const request = callable<[method: string, path: string], Reply<any>>("request");

interface Status {
  managed: boolean;
  key: string;
  name: string;
  phase: string;
  status: string | null;
  lastSyncAt: string | null;
  lastSyncedDevice: string | null;
  conflict: boolean;
  lastError: string | null;
  lang: string;
}

interface Snapshot {
  id: string;
  createdAt: string;
  deviceName: string;
  trigger: string;
  fileCount: number;
  comment: string;
}

interface Newer {
  key: string;
  name: string;
  label: string;
  snapshot: Snapshot;
}

const TEXT = {
  "zh-CN": {
    notRunning: "Ludonimbus 没有运行。在桌面模式里打开它，或者按说明装好后台服务。",
    noGame: "先启动一个游戏，这里会显示它的云存档。",
    notManaged: "这个游戏不在 Ludonimbus 里（在桌面模式里添加）。",
    syncNow: "立即同步",
    syncing: "正在同步…",
    started: "已开始同步",
    conflict: "本机和云端都有新的改动，选一份使用：",
    useCloud: "使用云端存档",
    useLocal: "使用本机存档",
    newer: (label: string, device: string, when: string) => `另一个版本（${label}）有更新的存档：${device} · ${when}`,
    apply: "应用到这个版本",
    dismiss: "不用",
    versions: "存档版本",
    restore: "恢复",
    restored: "已开始恢复",
    lastSync: (when: string, device: string) => `上次同步 ${when}${device ? ` · ${device}` : ""}`,
    never: "从未同步",
    status: { synced: "已同步", localChanged: "待上传", cloudNewer: "云端更新", conflict: "存档冲突", offline: "离线", error: "出错", noSaves: "没有存档" } as Record<string, string>,
  },
  en: {
    notRunning: "Ludonimbus isn't running. Open it in desktop mode, or install its background service.",
    noGame: "Start a game and its cloud saves show here.",
    notManaged: "This game isn't in Ludonimbus (add it in desktop mode).",
    syncNow: "Sync now",
    syncing: "Syncing…",
    started: "Sync started",
    conflict: "Both this device and the cloud have new changes. Choose one:",
    useCloud: "Use cloud saves",
    useLocal: "Use this device's saves",
    newer: (label: string, device: string, when: string) => `Another version (${label}) has newer saves: ${device} · ${when}`,
    apply: "Apply to this version",
    dismiss: "No thanks",
    versions: "Save versions",
    restore: "Restore",
    restored: "Restore started",
    lastSync: (when: string, device: string) => `Last sync ${when}${device ? ` · ${device}` : ""}`,
    never: "Never synced",
    status: { synced: "Synced", localChanged: "To upload", cloudNewer: "Newer in cloud", conflict: "Conflict", offline: "Offline", error: "Error", noSaves: "No saves" } as Record<string, string>,
  },
};

const when = (iso: string) => new Date(iso).toLocaleString();

/** The game running now, else the one Steam started last. */
function currentAppId(): number | null {
  const app = (Router as any).MainRunningApp;
  return app?.appid ? Number(app.appid) : null;
}

let lastAppId: number | null = null;

function Panel() {
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [newer, setNewer] = useState<Newer | null>(null);
  const [snapshots, setSnapshots] = useState<Snapshot[] | null>(null);
  const appid = currentAppId() ?? lastAppId;
  if (appid) lastAppId = appid;
  const t = TEXT[status?.lang === "zh-CN" ? "zh-CN" : "en"];

  const load = useCallback(async () => {
    if (!appid) return;
    const r = await request("GET", `/v1/steam/apps/${appid}?refresh=1`);
    if (!r.ok) {
      setError(r.status === 0 ? TEXT.en.notRunning : r.body?.error ?? String(r.status));
      return;
    }
    setError(null);
    setStatus(r.body);
    if (r.body.managed) {
      const [n, s] = await Promise.all([request("GET", `/v1/games/${r.body.key}/newer`), request("GET", `/v1/games/${r.body.key}/snapshots`)]);
      setNewer(n.ok ? n.body : null);
      setSnapshots(s.ok ? (s.body.snapshots as Snapshot[]).filter((x) => x.trigger !== "pre-restore" && x.trigger !== "conflict-keep").slice(0, 8) : null);
    }
  }, [appid]);

  useEffect(() => {
    load();
    const id = setInterval(load, 4000);
    return () => clearInterval(id);
  }, [load]);

  const post = async (path: string, ok: string) => {
    const r = await request("POST", path);
    toaster.toast({ title: "Ludonimbus", body: r.ok ? ok : r.body?.error ?? String(r.status) });
    load();
  };

  if (error) return <PanelSection>{<PanelSectionRow>{error}</PanelSectionRow>}</PanelSection>;
  if (!appid) return <PanelSection><PanelSectionRow>{t.noGame}</PanelSectionRow></PanelSection>;
  if (!status) return <PanelSection><PanelSectionRow>…</PanelSectionRow></PanelSection>;
  if (!status.managed) return <PanelSection><PanelSectionRow>{t.notManaged}</PanelSectionRow></PanelSection>;

  const busy = status.phase !== "idle";
  return (
    <>
      <PanelSection title={status.name}>
        <PanelSectionRow>
          {(status.status && t.status[status.status]) ?? ""} · {status.lastSyncAt ? t.lastSync(when(status.lastSyncAt), status.lastSyncedDevice ?? "") : t.never}
        </PanelSectionRow>
        {status.lastError && <PanelSectionRow>{status.lastError}</PanelSectionRow>}
        {status.conflict || status.status === "conflict" ? (
          <>
            <PanelSectionRow>{t.conflict}</PanelSectionRow>
            <PanelSectionRow>
              <ButtonItem layout="below" disabled={busy} onClick={() => post(`/v1/games/${status.key}/sync?resolution=cloud`, t.started)}>
                {t.useCloud}
              </ButtonItem>
            </PanelSectionRow>
            <PanelSectionRow>
              <ButtonItem layout="below" disabled={busy} onClick={() => post(`/v1/games/${status.key}/sync?resolution=local`, t.started)}>
                {t.useLocal}
              </ButtonItem>
            </PanelSectionRow>
          </>
        ) : (
          <PanelSectionRow>
            <ButtonItem layout="below" disabled={busy} onClick={() => post(`/v1/games/${status.key}/sync`, t.started)}>
              {busy ? t.syncing : t.syncNow}
            </ButtonItem>
          </PanelSectionRow>
        )}
      </PanelSection>
      {newer && (
        <PanelSection>
          <PanelSectionRow>{t.newer(newer.label, newer.snapshot.deviceName, when(newer.snapshot.createdAt))}</PanelSectionRow>
          <PanelSectionRow>
            <ButtonItem layout="below" disabled={busy} onClick={() => post(`/v1/games/${status.key}/newer/${newer.snapshot.id}/apply`, t.started)}>
              {t.apply}
            </ButtonItem>
          </PanelSectionRow>
          <PanelSectionRow>
            <ButtonItem layout="below" onClick={() => post(`/v1/games/${status.key}/newer/${newer.snapshot.id}/dismiss`, t.dismiss)}>
              {t.dismiss}
            </ButtonItem>
          </PanelSectionRow>
        </PanelSection>
      )}
      {snapshots && snapshots.length > 0 && (
        <PanelSection title={t.versions}>
          {snapshots.map((s) => (
            <PanelSectionRow key={s.id}>
              <ButtonItem layout="below" label={`${when(s.createdAt)} · ${s.deviceName}`} disabled={busy} onClick={() => post(`/v1/games/${status.key}/restore/${s.id}`, t.restored)}>
                {t.restore}
              </ButtonItem>
            </PanelSectionRow>
          ))}
        </PanelSection>
      )}
    </>
  );
}

export default definePlugin(() => ({
  name: "Ludonimbus",
  titleView: <div className={staticClasses.Title}>Ludonimbus</div>,
  content: <Panel />,
  icon: <FaCloud />,
}));
