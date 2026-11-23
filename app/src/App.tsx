import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorText, GameView, inTauri, Outcome, Overview, StatusReport, UpdateInfo } from "./api";
import { AddGame } from "./components/AddGame";
import { Devices } from "./components/Devices";
import { GameDetail } from "./components/GameDetail";
import { Library } from "./components/Library";
import { SettingsPage } from "./components/Settings";
import { Icon, Spinner, useUi } from "./components/ui";
import { UpdateDialog } from "./components/Update";
import { outcomeText } from "./format";

export type Page = { name: "library" } | { name: "game"; key: string } | { name: "devices" } | { name: "settings" };

export default function App() {
  const ui = useUi();
  const [overview, setOverview] = useState<Overview | null>(null);
  const [checks, setChecks] = useState<Record<string, StatusReport>>({});
  const [checking, setChecking] = useState<Record<string, boolean>>({});
  const [page, setPage] = useState<Page>({ name: "library" });
  const [adding, setAdding] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [showUpdate, setShowUpdate] = useState(false);
  const checkingRef = useRef<Record<string, boolean>>({});

  const reload = useCallback(async () => {
    try {
      const o = await api.overview();
      setOverview(o);
      setLoadError(null);
      // Seed statuses from the cached last check.
      setChecks((prev) => {
        const next = { ...prev };
        for (const g of o.games) if (!next[g.key] && g.lastCheck) next[g.key] = g.lastCheck;
        return next;
      });
      return o;
    } catch (e) {
      setLoadError(errorText(e));
      return null;
    }
  }, []);

  const check = useCallback(async (key: string) => {
    if (checkingRef.current[key]) return;
    checkingRef.current[key] = true;
    setChecking((c) => ({ ...c, [key]: true }));
    try {
      const r = await api.check(key);
      setChecks((c) => ({ ...c, [key]: r }));
    } catch {
      /* shown on the card via lastError */
    } finally {
      checkingRef.current[key] = false;
      setChecking((c) => ({ ...c, [key]: false }));
    }
  }, []);

  const checkAll = useCallback(
    async (games: GameView[]) => {
      // A few at a time to keep the cloud API happy.
      const queue = [...games];
      const workers = Array.from({ length: 3 }, async () => {
        while (queue.length) {
          const g = queue.shift()!;
          await check(g.key);
        }
      });
      await Promise.all(workers);
    },
    [check],
  );

  const refresh = useCallback(async () => {
    const o = await reload();
    if (o) await checkAll(o.games);
  }, [reload, checkAll]);

  useEffect(() => {
    refresh();
    const unlisten: Array<() => void> = [];
    if (inTauri) {
      listen("library-changed", () => reload()).then((u) => unlisten.push(u));
      // Clicked in Steam (Millennium plugin): show that game.
      listen<string>("open-game", (e) => {
        setAdding(false);
        setPage({ name: "game", key: e.payload });
        reload();
        check(e.payload);
      }).then((u) => unlisten.push(u));
      // A game (and the Steam wrapper's upload) finished: refresh its status.
      listen<string>("game-stopped", (e) => {
        reload();
        check(e.payload);
      }).then((u) => unlisten.push(u));
      listen<{ key: string; name: string; outcome: Outcome | null; error: string | null }>("sync-finished", (e) => {
        const { key, name, outcome, error } = e.payload;
        if (error) ui.toast(`「${name}」后台同步失败：${error}`, "bad");
        else if (outcome && outcome.kind !== "upToDate")
          ui.toast(outcomeText(outcome, name), outcome.kind === "conflict" ? "warn" : "ok");
        reload();
        check(key);
      }).then((u) => unlisten.push(u));
    }
    const timer = setInterval(() => reload(), 30000);
    // Re-check every game against the cloud now and then (other devices may have uploaded).
    const recheck = setInterval(() => {
      reload().then((o) => o && checkAll(o.games));
    }, 10 * 60 * 1000);
    return () => {
      unlisten.forEach((u) => u());
      clearInterval(timer);
      clearInterval(recheck);
    };
  }, [refresh, reload, check, checkAll, ui]);

  // Check for app updates shortly after start, then every 6 hours.
  useEffect(() => {
    const run = () =>
      api.checkUpdate().then(
        (u) => setUpdate(u.available ? u : null),
        () => undefined,
      );
    const first = setTimeout(run, 4000);
    const timer = setInterval(run, 6 * 3600 * 1000);
    return () => {
      clearTimeout(first);
      clearInterval(timer);
    };
  }, []);

  const game = page.name === "game" ? overview?.games.find((g) => g.key === page.key) : undefined;

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <img src="/app-icon.svg" alt="" />
          <span>Ludonimbus</span>
        </div>
        <nav>
          <NavItem icon="library" label="游戏库" active={page.name === "library" || page.name === "game"} onClick={() => setPage({ name: "library" })} />
          <NavItem icon="devices" label="设备" active={page.name === "devices"} onClick={() => setPage({ name: "devices" })} />
          <NavItem icon="settings" label="设置" active={page.name === "settings"} onClick={() => setPage({ name: "settings" })} />
        </nav>
        <div className="sidebar-foot">
          {update && (
            <button className="update-chip" onClick={() => setShowUpdate(true)}>
              <Icon name="download" size={16} />
              <span>新版本 {update.version} 可用</span>
            </button>
          )}
          {overview && (
            <button className={`cloud-chip ${overview.cloud.kind === "none" || overview.cloud.error ? "off" : "on"}`} onClick={() => setPage({ name: "settings" })}>
              <Icon name="cloud" size={16} />
              <span>
                {overview.cloud.kind === "none"
                  ? "未连接云盘"
                  : overview.cloud.error
                    ? "云盘需要重新连接"
                    : overview.cloud.kind === "googleDrive"
                      ? overview.cloud.label ?? "Google Drive"
                      : overview.cloud.kind === "baiduPan"
                        ? overview.cloud.label ?? "百度网盘"
                        : overview.cloud.kind === "webDav"
                        ? "WebDAV"
                        : overview.cloud.kind === "s3"
                          ? "S3 存储"
                          : "本地/同步文件夹"}
              </span>
            </button>
          )}
          {overview && <div className="device-name">本机：{overview.deviceName}</div>}
        </div>
      </aside>

      <main className="main">
        {loadError && (
          <div className="banner banner-bad">
            <Icon name="warn" /> 加载失败：{loadError}
          </div>
        )}
        {!overview && !loadError && (
          <div className="center">
            <Spinner />
          </div>
        )}
        {overview && page.name === "library" && (
          <Library
            overview={overview}
            checks={checks}
            checking={checking}
            onOpen={(key) => setPage({ name: "game", key })}
            onAdd={() => setAdding(true)}
            onRefresh={refresh}
            onSettings={() => setPage({ name: "settings" })}
          />
        )}
        {overview && page.name === "game" && game && (
          <GameDetail
            key={game.key}
            game={game}
            overview={overview}
            check={checks[game.key]}
            checking={!!checking[game.key]}
            onBack={() => setPage({ name: "library" })}
            onChanged={async () => {
              await reload();
              await check(game.key);
            }}
            onRemoved={async () => {
              setPage({ name: "library" });
              await reload();
            }}
          />
        )}
        {overview && page.name === "game" && !game && (
          <div className="center muted">这个游戏已经不在库里了。</div>
        )}
        {page.name === "devices" && <Devices overview={overview} />}
        {page.name === "settings" && <SettingsPage onChanged={refresh} />}
      </main>

      {showUpdate && update && <UpdateDialog info={update} onClose={() => setShowUpdate(false)} />}

      {adding && overview && (
        <AddGame
          overview={overview}
          onClose={() => setAdding(false)}
          onAdded={async (key) => {
            setAdding(false);
            const o = await reload();
            if (o) {
              await check(key);
              setPage({ name: "game", key });
            }
          }}
        />
      )}
    </div>
  );
}

function NavItem({ icon, label, active, onClick }: { icon: string; label: string; active: boolean; onClick: () => void }) {
  return (
    <button className={`nav-item${active ? " active" : ""}`} onClick={onClick}>
      <Icon name={icon} />
      <span>{label}</span>
    </button>
  );
}
