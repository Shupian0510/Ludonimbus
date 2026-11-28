// Ludonimbus in Steam Deck Game Mode (Decky Loader, docs/plan-platforms.md
// §2.2, docs/steam-plugin.md). The quick access menu shows:
//   - the game that's running (or was last started), with its cover: its
//     cloud save status, sync now, the conflict choice, taking saves back
//     when they're missing here, keeping the saves as they are now
//     (「保留这一份」), newer saves from another version of the game, the
//     saves in the cloud with "make current", and handing a game Steam
//     installed (backup only) to Ludonimbus (hands.ts);
//   - every game of the library with its status: sync now or take back per
//     game, the conflict choice for any game, and sync everything;
//   - pausing syncing on this device, and going on;
//   - when Ludonimbus doesn't answer: starting its background service.
// What it shows is read again when the app says something changed (a
// long poll, `GET /v1/changes`, API 6); an older app is asked every few
// seconds instead. Everything works with the controller.
import { callable, definePlugin, toaster, useQuickAccessVisible as apiQuickAccessVisible } from "@decky/api";
import { ButtonItem, ConfirmModal, PanelSection, PanelSectionRow, Router, showModal, staticClasses, useQuickAccessVisible as uiQuickAccessVisible } from "@decky/ui";
import { useCallback, useEffect, useRef, useState } from "react";
import { FaCloud } from "react-icons/fa";
import { SteamHands } from "./hands";

type Reply<T> = { ok: boolean; status: number; body: T & { error?: string } };
const request = callable<[method: string, path: string], Reply<any>>("request");
type Started = { ok: boolean; already?: boolean; error?: "notInstalled" | "failed" | "noAnswer"; detail?: string };
const startService = callable<[], Started>("start_service");

/** Steam's Steam Cloud switches and launch options, for Ludonimbus (API 5). */
const hands = new SteamHands(request);

/** Whether the quick access menu is open (older loaders: @decky/ui's copy). */
const useQuickAccessVisible: () => boolean = apiQuickAccessVisible ?? uiQuickAccessVisible ?? (() => true);

/** An app older than API 6 has no change feed: it is asked this often. */
const POLL_MS = 5000;
/** Ludonimbus doesn't answer: looked at again this often. */
const RETRY_MS = 3000;

type Phase = "idle" | "syncing" | "preLaunch" | "playing" | "uploading";

interface Backlog {
  state: "uploading" | "waiting";
  versions: number;
  bytes: number;
}

interface Status {
  managed: boolean;
  key: string;
  name: string;
  phase: Phase;
  status: string | null;
  lastSyncAt: string | null;
  lastSyncedDevice: string | null;
  conflict: boolean;
  lastError: string | null;
  lang: string;
  /** API 4: a game Steam itself installed: "steam", "backup" or "managed". */
  store?: string | null;
  /** API 4: what is left to do in the background after playing. */
  backlog?: Backlog | null;
  /** API 6: syncing is paused on this device. */
  paused?: boolean;
  apiVersion?: number;
}

/** A game in `GET /v1/games` (API 6). */
interface Listed {
  key: string;
  name: string;
  appid: number | null;
  phase: Phase;
  status: string | null;
  conflict: boolean;
  lastError: string | null;
  lastSyncAt: string | null;
  store: string | null;
  backlog: Backlog | null;
  cover: number | null;
  wide: number | null;
}

interface Library {
  games: Listed[];
  paused: boolean;
  lang: string;
  apiVersion: number;
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
    notRunning: "Ludonimbus 没有运行。",
    start: "启动 Ludonimbus",
    starting: "正在启动…",
    startFailed: {
      notInstalled: "后台服务没有安装：在桌面模式里运行 Linux 下载包里的 install-linux.sh。",
      failed: (detail: string) => `启动失败：${detail}`,
      noAnswer: (detail: string) => `后台服务启动了，但没有回应（${detail}）。`,
    },
    noGame: "先启动一个游戏，这里会显示它的云存档。",
    notManaged: "这个游戏不在 Ludonimbus 里（在桌面模式里添加）。",
    steamKeeps: "这个游戏的存档由 Steam 云管。",
    backlogUploading: (size: string) => `正在后台上传${size ? `（${size}）` : ""}`,
    backlogWaiting: "没连上，稍后再传",
    backlogQueued: (size: string) => `等待上传${size ? `（${size}）` : ""}`,
    backedUp: "已备份",
    syncNow: "立即同步",
    syncAll: "全部同步",
    syncAllStarted: "已开始全部同步",
    takeBack: "取回",
    choose: "选一份…",
    syncing: "正在同步…",
    playing: "正在玩",
    started: "已开始同步",
    conflict: "本机和云端都有新的改动，选一份使用：",
    useCloud: "使用云端存档",
    useLocal: "使用本机存档",
    cancel: "取消",
    keep: "保留这一份",
    kept: "已开始保留",
    newer: (label: string, device: string, when: string) => `另一个版本（${label}）有更新的存档：${device} · ${when}`,
    apply: "发到这个版本",
    dismiss: "不用",
    versions: "存档",
    games: "全部游戏",
    restore: "设为当前",
    restored: "已开始切换",
    lastSync: (when: string, device: string) => `上次同步 ${when}${device ? ` · ${device}` : ""}`,
    never: "从未同步",
    status: {
      synced: "已同步",
      localChanged: "待上传",
      notUploaded: "待上传",
      cloudNewer: "云端更新",
      canRestore: "可以取回",
      savesMissing: "本机存档不见了",
      conflict: "存档冲突",
      offline: "离线",
      error: "出错",
      empty: "没有存档",
      noCloud: "没有连接云盘",
    } as Record<string, string>,
    notChecked: "还没检查",
    paused: "同步已暂停",
    pause: "暂停同步",
    resume: "继续同步",
    resumed: "已继续同步",
    steamCloud: "Steam 云",
    takeOver: "交给 Ludonimbus",
    takeOverTitle: "交给 Ludonimbus？",
    takeOverText: "会关掉这个游戏的 Steam 云存档。之后由 Ludonimbus 在启动前取回最新的存档、退出后上传，现在的存档会先在云端保留一份。在 Ludonimbus 里可以交还给 Steam。",
    takingOver: "正在交给 Ludonimbus…",
    takenOver: "已交给 Ludonimbus",
    cantTakeOver: (reason: string) => `没有交过去：${reason}`,
  },
  en: {
    notRunning: "Ludonimbus isn't running.",
    start: "Start Ludonimbus",
    starting: "Starting…",
    startFailed: {
      notInstalled: "The background service isn't installed: in desktop mode, run install-linux.sh from the Linux download.",
      failed: (detail: string) => `Couldn't start it: ${detail}`,
      noAnswer: (detail: string) => `The background service started but doesn't answer (${detail}).`,
    },
    noGame: "Start a game and its cloud saves show here.",
    notManaged: "This game isn't in Ludonimbus (add it in desktop mode).",
    steamKeeps: "Steam Cloud keeps this game's saves.",
    backlogUploading: (size: string) => `Uploading in the background${size ? ` (${size})` : ""}`,
    backlogWaiting: "Couldn't reach the cloud; trying again later",
    backlogQueued: (size: string) => `Waiting to upload${size ? ` (${size})` : ""}`,
    backedUp: "Backed up",
    syncNow: "Sync now",
    syncAll: "Sync all",
    syncAllStarted: "Syncing all games",
    takeBack: "Get back",
    choose: "Choose…",
    syncing: "Syncing…",
    playing: "Playing",
    started: "Sync started",
    conflict: "Both this device and the cloud have new changes. Choose one:",
    useCloud: "Use cloud saves",
    useLocal: "Use this device's saves",
    cancel: "Cancel",
    keep: "Keep this one",
    kept: "Keeping these saves",
    newer: (label: string, device: string, when: string) => `Another version (${label}) has newer saves: ${device} · ${when}`,
    apply: "Send to this version",
    dismiss: "No thanks",
    versions: "Saves",
    games: "All games",
    restore: "Make current",
    restored: "Switching started",
    lastSync: (when: string, device: string) => `Last sync ${when}${device ? ` · ${device}` : ""}`,
    never: "Never synced",
    status: {
      synced: "Synced",
      localChanged: "To upload",
      notUploaded: "To upload",
      cloudNewer: "Newer in cloud",
      canRestore: "Can get back",
      savesMissing: "Saves missing here",
      conflict: "Conflict",
      offline: "Offline",
      error: "Error",
      empty: "No saves",
      noCloud: "No cloud",
    } as Record<string, string>,
    notChecked: "Not checked yet",
    paused: "Syncing is paused",
    pause: "Pause syncing",
    resume: "Resume syncing",
    resumed: "Syncing goes on",
    steamCloud: "Steam Cloud",
    takeOver: "Hand to Ludonimbus",
    takeOverTitle: "Hand this game to Ludonimbus?",
    takeOverText:
      "This turns Steam Cloud off for this game. Ludonimbus then fetches the newest saves before it starts and uploads them after you quit; the saves as they are now are kept in the cloud first. You can give it back to Steam in Ludonimbus.",
    takingOver: "Handing it to Ludonimbus…",
    takenOver: "Handed to Ludonimbus",
    cantTakeOver: (reason: string) => `Not handed over: ${reason}`,
  },
};
type Text = (typeof TEXT)["en"];

/** Ludonimbus's language when it answered, else Steam's. */
let lang = navigator.language?.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
const text = (l?: string): Text => {
  if (l) lang = l;
  return TEXT[lang === "zh-CN" ? "zh-CN" : "en"];
};

const when = (iso: string) => new Date(iso).toLocaleString();

const mb = (n: number) => (n > 0 ? `${(n / 1024 / 1024).toFixed(1)} MB` : "");

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

const toast = (body: string) => toaster.toast({ title: "Ludonimbus", body });

/** The game running now, else the one Steam started last. */
function currentAppId(): number | null {
  const app = (Router as any).MainRunningApp;
  return app?.appid ? Number(app.appid) : null;
}

let lastAppId: number | null = null;

// ---------------------------------------------------------------------------
// Changes: one long poll for every part of the panel that is shown.

const listeners = new Set<() => void>();
let feedRunning = false;

/** Call `fn` whenever Ludonimbus says something changed, until unsubscribed. */
function subscribe(fn: () => void): () => void {
  listeners.add(fn);
  void runFeed();
  return () => void listeners.delete(fn);
}

async function runFeed() {
  if (feedRunning) return;
  feedRunning = true;
  let seq = 0;
  try {
    while (listeners.size > 0) {
      const r = await request("GET", `/v1/changes?after=${seq}&wait=25`).catch(() => null);
      if (r?.ok && typeof r.body?.seq === "number") {
        if (r.body.seq !== seq) {
          const first = seq === 0;
          seq = r.body.seq;
          if (!first) listeners.forEach((fn) => fn());
        }
        continue;
      }
      // An older app (no change feed), or Ludonimbus doesn't answer: ask again in a while.
      seq = 0;
      await sleep(r?.status === 404 ? POLL_MS : RETRY_MS);
      listeners.forEach((fn) => fn());
    }
  } finally {
    feedRunning = false;
  }
}

/** `load` now and on every change while `on`. */
function useLive(on: boolean, load: () => Promise<void>) {
  useEffect(() => {
    if (!on) return;
    let running = false;
    let again = false;
    // One at a time; a change during a load loads once more after it.
    const tick = () => {
      if (running) {
        again = true;
        return;
      }
      running = true;
      load()
        .catch(() => undefined)
        .finally(() => {
          running = false;
          if (again) {
            again = false;
            tick();
          }
        });
    };
    tick();
    return subscribe(tick);
  }, [on, load]);
}

// ---------------------------------------------------------------------------
// Artwork: data URLs from the local API, kept while the plugin is loaded.

const art = new Map<string, string | null>();
const artQueue: (() => Promise<void>)[] = [];
let artBusy = false;

async function nextArt() {
  if (artBusy) return;
  artBusy = true;
  while (artQueue.length > 0) await artQueue.shift()!().catch(() => undefined);
  artBusy = false;
}

/** The game's image of `kind` (cover, wide, icon), one request at a time. */
function useArt(key: string, kind: "cover" | "wide" | "icon", version: number | null | undefined, on = true): string | null {
  const id = `${key}|${kind}|${version ?? ""}`;
  const [url, setUrl] = useState<string | null>(art.get(id) ?? null);
  useEffect(() => {
    if (!on) return;
    if (art.has(id)) {
      setUrl(art.get(id) ?? null);
      return;
    }
    let alive = true;
    artQueue.push(async () => {
      if (art.has(id)) return;
      const path = kind === "icon" ? `/v1/games/${key}/icon` : `/v1/games/${key}/art/${kind}`;
      const r = await request("GET", path);
      art.set(id, r.ok ? (r.body?.dataUrl ?? null) : null);
      if (alive) setUrl(art.get(id) ?? null);
    });
    void nextArt();
    return () => void (alive = false);
  }, [id, on]);
  return url;
}

// ---------------------------------------------------------------------------
// What a game's status says

function stateText(t: Text, s: { status: string | null; store?: string | null; backlog?: Backlog | null }): string {
  const b = s.backlog;
  // Played, and the cloud doesn't have it yet: said first.
  if (b) return b.state === "uploading" ? t.backlogUploading(mb(b.bytes)) : s.status === "offline" ? t.backlogWaiting : t.backlogQueued(mb(b.bytes));
  if (s.store === "steam") return t.steamCloud;
  if (s.store === "backup" && s.status === "synced") return t.backedUp;
  if (!s.status) return t.notChecked;
  return t.status[s.status] ?? "";
}

/** The newer saves in the cloud are taken back (a sync the user starts takes them). */
const takesBack = (status: string | null) => status === "savesMissing" || status === "cloudNewer" || status === "canRestore";

const inConflict = (s: { conflict: boolean; status: string | null }) => s.conflict || s.status === "conflict";

/** Run an action of the API and say how it went. */
async function post(path: string, ok: string): Promise<boolean> {
  const r = await request("POST", path);
  toast(r.ok ? ok : (r.body?.error ?? String(r.status)));
  return r.ok;
}

/** The conflict choice in Steam's own dialog: the cloud's saves, or this device's. */
function askConflict(t: Text, key: string, name: string) {
  showModal(
    <ConfirmModal
      strTitle={name}
      strDescription={t.conflict}
      strOKButtonText={t.useCloud}
      strMiddleButtonText={t.useLocal}
      strCancelButtonText={t.cancel}
      onOK={() => void post(`/v1/games/${key}/sync?resolution=cloud`, t.started)}
      onMiddleButton={() => void post(`/v1/games/${key}/sync?resolution=local`, t.started)}
    />,
  );
}

// ---------------------------------------------------------------------------
// The game that's running, or was started last

function Current({ appid, listed, paused, full }: { appid: number; listed: Listed | null; paused: boolean; full: boolean }) {
  const [status, setStatus] = useState<Status | null>(null);
  const [newer, setNewer] = useState<Newer | null>(null);
  const [snapshots, setSnapshots] = useState<Snapshot[] | null>(null);
  const t = text(status?.lang);
  const cover = useArt(listed?.key ?? "", listed?.wide ? "wide" : "cover", listed?.wide ?? listed?.cover, !!listed && !!(listed.wide || listed.cover));

  // The game shown now: answers about the one before are dropped.
  const shown = useRef(appid);
  useEffect(() => {
    shown.current = appid;
  }, [appid]);
  // When the version list and newer saves are read again: they always ask the
  // cloud, so only when the panel opens, after an action, and when a sync
  // has changed something.
  const wantDetails = useRef(true);
  const detailsFor = useRef("");
  const refreshed = useRef(false);

  const load = useCallback(async () => {
    // The first look asks the cloud when the cached status is old.
    const r = await request("GET", `/v1/steam/apps/${appid}${refreshed.current ? "" : "?refresh=1"}`);
    refreshed.current = true;
    if (shown.current !== appid || !r.ok) return;
    const s = r.body as Status;
    setStatus(s);
    // Not while a sync runs (it is read when it has finished), nor while paused.
    if (!s.managed || s.phase !== "idle" || s.paused) return;
    const mark = `${s.key}|${s.lastSyncAt}|${s.status}`;
    if (!wantDetails.current && mark === detailsFor.current) return;
    wantDetails.current = false;
    detailsFor.current = mark;
    const [n, v] = await Promise.all([request("GET", `/v1/games/${s.key}/newer`), request("GET", `/v1/games/${s.key}/snapshots`)]);
    if (shown.current !== appid) return;
    setNewer(n.ok ? n.body : null);
    setSnapshots(v.ok ? (v.body.snapshots as Snapshot[]).filter((x) => x.trigger !== "pre-restore" && x.trigger !== "conflict-keep").slice(0, 8) : null);
  }, [appid]);
  useEffect(() => {
    refreshed.current = false;
    wantDetails.current = true;
    setStatus(null);
  }, [appid]);
  useLive(true, load);

  const act = async (path: string, ok: string) => {
    await post(path, ok);
    wantDetails.current = true;
    void load();
  };

  /** Hand a backed-up game to Ludonimbus: it runs in the app, and this follows it to the end. */
  const takeOver = async (key: string) => {
    const r = await request("POST", `/v1/games/${key}/steam-cloud/take-over`);
    if (!r.ok) {
      toast(t.cantTakeOver(r.body?.error ?? String(r.status)));
      return;
    }
    toast(t.takingOver);
    for (let i = 0; i < 80; i++) {
      await sleep(1500);
      const s = await request("GET", `/v1/steam/apps/${appid}`);
      if (!s.ok || s.body?.phase !== "idle") continue;
      toast(s.body.store === "managed" ? t.takenOver : t.cantTakeOver(s.body.lastError ?? ""));
      break;
    }
    wantDetails.current = true;
    void load();
  };

  /** Steam's own confirmation: this turns a protection off. */
  const askTakeOver = (key: string) =>
    showModal(
      <ConfirmModal
        strTitle={t.takeOverTitle}
        strDescription={t.takeOverText}
        strOKButtonText={t.takeOver}
        strCancelButtonText={t.cancel}
        onOK={() => void takeOver(key)}
      />,
    );

  if (!status) return <PanelSection><PanelSectionRow>…</PanelSectionRow></PanelSection>;
  if (!status.managed) return <PanelSection><PanelSectionRow>{t.notManaged}</PanelSectionRow></PanelSection>;
  if (status.store === "steam") return <PanelSection title={status.name}><PanelSectionRow>{t.steamKeeps}</PanelSectionRow></PanelSection>;

  const busy = status.phase !== "idle";
  const stopped = busy || paused;
  const key = status.key;
  const main = inConflict(status) ? (
    <>
      <PanelSectionRow>{t.conflict}</PanelSectionRow>
      <PanelSectionRow>
        <ButtonItem layout="below" disabled={stopped} onClick={() => act(`/v1/games/${key}/sync?resolution=cloud`, t.started)}>
          {t.useCloud}
        </ButtonItem>
      </PanelSectionRow>
      <PanelSectionRow>
        <ButtonItem layout="below" disabled={stopped} onClick={() => act(`/v1/games/${key}/sync?resolution=local`, t.started)}>
          {t.useLocal}
        </ButtonItem>
      </PanelSectionRow>
    </>
  ) : (
    <PanelSectionRow>
      <ButtonItem layout="below" disabled={stopped} onClick={() => act(`/v1/games/${key}/sync`, t.started)}>
        {busy ? (status.phase === "playing" ? t.playing : t.syncing) : takesBack(status.status) ? t.takeBack : t.syncNow}
      </ButtonItem>
    </PanelSectionRow>
  );
  return (
    <>
      <PanelSection title={status.name}>
        {cover && (
          <PanelSectionRow>
            <img src={cover} style={{ width: "100%", borderRadius: "4px", display: "block" }} />
          </PanelSectionRow>
        )}
        <PanelSectionRow>
          {stateText(t, status)} · {status.lastSyncAt ? t.lastSync(when(status.lastSyncAt), status.lastSyncedDevice ?? "") : t.never}
        </PanelSectionRow>
        {status.lastError && <PanelSectionRow>{status.lastError}</PanelSectionRow>}
        {main}
        {full && (
          <PanelSectionRow>
            <ButtonItem layout="below" disabled={busy || status.status === "savesMissing"} onClick={() => act(`/v1/games/${key}/keep`, t.kept)}>
              {t.keep}
            </ButtonItem>
          </PanelSectionRow>
        )}
        {status.store === "backup" && (status.apiVersion ?? 0) >= 5 && hands.connected && (
          <PanelSectionRow>
            <ButtonItem layout="below" disabled={stopped} onClick={() => askTakeOver(key)}>
              {t.takeOver}
            </ButtonItem>
          </PanelSectionRow>
        )}
      </PanelSection>
      {newer && !paused && (
        <PanelSection>
          <PanelSectionRow>{t.newer(newer.label, newer.snapshot.deviceName, when(newer.snapshot.createdAt))}</PanelSectionRow>
          <PanelSectionRow>
            <ButtonItem layout="below" disabled={busy} onClick={() => act(`/v1/games/${key}/newer/${newer.snapshot.id}/apply`, t.started)}>
              {t.apply}
            </ButtonItem>
          </PanelSectionRow>
          <PanelSectionRow>
            <ButtonItem layout="below" onClick={() => act(`/v1/games/${key}/newer/${newer.snapshot.id}/dismiss`, t.dismiss)}>
              {t.dismiss}
            </ButtonItem>
          </PanelSectionRow>
        </PanelSection>
      )}
      {snapshots && snapshots.length > 0 && !paused && (
        <PanelSection title={t.versions}>
          {snapshots.map((s) => (
            <PanelSectionRow key={s.id}>
              <ButtonItem layout="below" label={`${when(s.createdAt)} · ${s.deviceName}`} disabled={busy} onClick={() => act(`/v1/games/${key}/restore/${s.id}`, t.restored)}>
                {t.restore}
              </ButtonItem>
            </PanelSectionRow>
          ))}
        </PanelSection>
      )}
    </>
  );
}

// ---------------------------------------------------------------------------
// Every game of the library (API 6)

/** Attention first: conflicts, missing saves, errors; then by name. */
function rank(g: Listed): number {
  if (inConflict(g)) return 0;
  if (g.status === "savesMissing") return 1;
  if (g.lastError || g.status === "error") return 2;
  return 3;
}

function GameRow({ g, t, paused }: { g: Listed; t: Text; paused: boolean }) {
  const icon = useArt(g.key, "icon", null);
  const busy = g.phase !== "idle";
  const conflict = inConflict(g);
  const action = busy ? (g.phase === "playing" ? t.playing : t.syncing) : conflict ? t.choose : takesBack(g.status) ? t.takeBack : t.syncNow;
  const onClick = () => (conflict ? askConflict(t, g.key, g.name) : void post(`/v1/games/${g.key}/sync`, t.started));
  return (
    <PanelSectionRow>
      <ButtonItem
        layout="below"
        label={g.name}
        description={g.lastError && !conflict ? `${stateText(t, g)} · ${g.lastError}` : stateText(t, g)}
        icon={icon ? <img src={icon} style={{ width: "20px", height: "20px", objectFit: "contain" }} /> : undefined}
        disabled={busy || paused || g.store === "steam"}
        onClick={onClick}
      >
        {action}
      </ButtonItem>
    </PanelSectionRow>
  );
}

function Games({ lib, t }: { lib: Library; t: Text }) {
  const games = [...lib.games].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
  if (games.length === 0) return null;
  return (
    <PanelSection title={t.games}>
      <PanelSectionRow>
        <ButtonItem layout="below" disabled={lib.paused} onClick={() => void post("/v1/sync-all", t.syncAllStarted)}>
          {t.syncAll}
        </ButtonItem>
      </PanelSectionRow>
      {games.map((g) => (
        <GameRow key={g.key} g={g} t={t} paused={lib.paused} />
      ))}
    </PanelSection>
  );
}

// ---------------------------------------------------------------------------
// The panel

function Panel() {
  const visible = useQuickAccessVisible();
  // `undefined`: not asked yet; `null`: an app older than API 6 (no list).
  const [lib, setLib] = useState<Library | null | undefined>(undefined);
  const [down, setDown] = useState(false);
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const appid = currentAppId() ?? lastAppId;
  if (appid) lastAppId = appid;
  const t = text(lib?.lang);
  const refreshed = useRef(false);

  const load = useCallback(async () => {
    const r = await request("GET", `/v1/games${refreshed.current ? "" : "?refresh=1"}`);
    if (r.ok) {
      refreshed.current = true;
      setDown(false);
      setLib(r.body as Library);
      return;
    }
    if (r.status === 404) {
      setDown(false);
      setLib(null);
      return;
    }
    // Ping tells "not running" from an error of the list.
    const ping = await request("GET", "/v1/ping");
    setDown(!ping.ok);
  }, []);
  useEffect(() => {
    if (visible) refreshed.current = false;
  }, [visible]);
  useLive(visible, load);

  const start = async () => {
    setStarting(true);
    setStartError(null);
    const r = await startService().catch((e) => ({ ok: false, error: "failed" as const, detail: String(e) }));
    setStarting(false);
    if (!r.ok) {
      const f = t.startFailed;
      setStartError(r.error === "notInstalled" ? f.notInstalled : r.error === "noAnswer" ? f.noAnswer(r.detail ?? "") : f.failed(r.detail ?? ""));
      return;
    }
    refreshed.current = false;
    void load();
  };

  if (down) {
    return (
      <PanelSection>
        <PanelSectionRow>{t.notRunning}</PanelSectionRow>
        <PanelSectionRow>
          <ButtonItem layout="below" disabled={starting} onClick={start}>
            {starting ? t.starting : t.start}
          </ButtonItem>
        </PanelSectionRow>
        {startError && <PanelSectionRow>{startError}</PanelSectionRow>}
      </PanelSection>
    );
  }
  if (lib === undefined) return <PanelSection><PanelSectionRow>…</PanelSectionRow></PanelSection>;

  const paused = !!lib?.paused;
  const listed = (appid && lib?.games.find((g) => g.appid === appid)) || null;
  return (
    <>
      {paused && (
        <PanelSection title={t.paused}>
          <PanelSectionRow>
            <ButtonItem layout="below" onClick={() => void post("/v1/sync/resume", t.resumed)}>
              {t.resume}
            </ButtonItem>
          </PanelSectionRow>
        </PanelSection>
      )}
      {appid ? <Current appid={appid} listed={listed} paused={paused} full={!!lib} /> : <PanelSection><PanelSectionRow>{t.noGame}</PanelSectionRow></PanelSection>}
      {lib && <Games lib={lib} t={t} />}
      {lib && !paused && (
        <PanelSection>
          <PanelSectionRow>
            <ButtonItem layout="below" onClick={() => void post("/v1/sync/pause", t.paused)}>
              {t.pause}
            </ButtonItem>
          </PanelSectionRow>
        </PanelSection>
      )}
    </>
  );
}

export default definePlugin(() => {
  // Runs while the plugin is loaded, not only while the menu is open: the
  // app may ask for Steam's settings at any time.
  hands.start();
  return {
    name: "Ludonimbus",
    titleView: <div className={staticClasses.Title}>Ludonimbus</div>,
    content: <Panel />,
    icon: <FaCloud />,
    onDismount: () => hands.stop(),
  };
});
