import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorText, GameView, inTauri, Outcome, Overview, StatusReport, UpdateInfo } from "./api";
import { AddGame } from "./components/AddGame";
import { Onboarding } from "./components/Onboarding";
import { Devices } from "./components/Devices";
import { GameDetail } from "./components/GameDetail";
import { Library } from "./components/Library";
import { SettingsPage } from "./components/Settings";
import { Icon, Spinner, useUi } from "./components/ui";
import { UpdateDialog } from "./components/Update";
import { outcomeText } from "./format";
import { msg, useT } from "./i18n";

export type Page = { name: "library" } | { name: "game"; key: string } | { name: "devices" } | { name: "settings" };

export default function App() {
  const ui = useUi();
  const t = useT();
  const [overview, setOverview] = useState<Overview | null>(null);
  const [checks, setChecks] = useState<Record<string, StatusReport>>({});
  const [checking, setChecking] = useState<Record<string, boolean>>({});
  const [page, setPage] = useState<Page>({ name: "library" });
  const [adding, setAdding] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [showUpdate, setShowUpdate] = useState(false);
  const [guide, setGuide] = useState(false);
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
        if (error) ui.toast(msg().app.backgroundSyncFailed(name, error), "bad");
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

  // First-run guide: only for someone who has not used Ludonimbus yet
  // (no cloud, no games). The browser preview shows it with ?guide.
  const guideChecked = useRef(false);
  useEffect(() => {
    if (!overview || guideChecked.current) return;
    guideChecked.current = true;
    api.onboarding().then(
      (o) => {
        const preview = !inTauri && new URLSearchParams(location.search).has("guide");
        if (!o.doneAt && (preview || (overview.cloud.kind === "none" && overview.games.length === 0))) setGuide(true);
      },
      () => undefined,
    );
  }, [overview]);

  const finishGuide = async (usage: string[], next: "library" | "scan" | "settings") => {
    setGuide(false);
    await api.finishOnboarding(usage).catch(() => undefined);
    if (next === "scan") {
      setPage({ name: "library" });
      setAdding(true);
    } else if (next === "settings") {
      setPage({ name: "settings" });
    }
  };

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
          <NavItem icon="library" label={t.app.nav.library} active={page.name === "library" || page.name === "game"} onClick={() => setPage({ name: "library" })} />
          <NavItem icon="devices" label={t.app.nav.devices} active={page.name === "devices"} onClick={() => setPage({ name: "devices" })} />
          <NavItem icon="settings" label={t.app.nav.settings} active={page.name === "settings"} onClick={() => setPage({ name: "settings" })} />
        </nav>
        <div className="sidebar-foot">
          {update && (
            <button className="update-chip" onClick={() => setShowUpdate(true)}>
              <Icon name="download" size={16} />
              <span>{t.app.updateAvailable(update.version)}</span>
            </button>
          )}
          {overview && (
            <button className={`cloud-chip ${overview.cloud.kind === "none" || overview.cloud.error ? "off" : "on"}`} onClick={() => setPage({ name: "settings" })}>
              <Icon name="cloud" size={16} />
              <span>
                {overview.cloud.kind === "none"
                  ? t.app.cloud.none
                  : overview.cloud.error
                    ? t.app.cloud.reconnect
                    : overview.cloud.kind === "googleDrive"
                      ? overview.cloud.label ?? "Google Drive"
                      : overview.cloud.kind === "baiduPan"
                        ? overview.cloud.label ?? t.cloudNames.baiduPan
                        : overview.cloud.kind === "quark"
                          ? t.cloudNames.quark
                          : overview.cloud.kind === "webDav"
                          ? "WebDAV"
                          : overview.cloud.kind === "s3"
                            ? t.app.cloud.s3
                            : t.app.cloud.folder}
              </span>
            </button>
          )}
          {overview && <div className="device-name">{t.app.thisDevice(overview.deviceName)}</div>}
        </div>
      </aside>

      <main className="main">
        {loadError && (
          <div className="banner banner-bad">
            <Icon name="warn" /> {t.app.loadFailed(loadError)}
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
          <div className="center muted">{t.app.gameGone}</div>
        )}
        {page.name === "devices" && <Devices overview={overview} />}
        {page.name === "settings" && <SettingsPage onChanged={refresh} onGuide={() => setGuide(true)} />}
      </main>

      {showUpdate && update && <UpdateDialog info={update} onClose={() => setShowUpdate(false)} />}
      {guide && <Onboarding onFinish={finishGuide} onChanged={refresh} />}

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
          onChanged={async (keys) => {
            await reload();
            for (const k of keys) check(k);
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
