import { useMemo, useState } from "react";
import type { GameView, Overview, StatusReport } from "../api";
import { bytes, duration, relTime, statusInfo } from "../format";
import { currentLang, useT } from "../i18n";
import { Badge, Button, Empty, Icon, Spinner } from "./ui";

function loadView(): "grid" | "list" {
  try {
    return localStorage.getItem("ln.view") === "list" ? "list" : "grid";
  } catch {
    return "grid";
  }
}

export function StatusBadge({ game, check, checking }: { game: GameView; check?: StatusReport; checking?: boolean }) {
  const t = useT();
  if (game.running) return <Badge tone="info">{t.library.running}</Badge>;
  if (!check) return checking ? <Badge tone="muted">{t.library.checking}</Badge> : <Badge tone="muted">{t.library.notChecked}</Badge>;
  const s = statusInfo(check.status);
  return <Badge tone={s.tone}>{s.label}</Badge>;
}

function needsAttention(g: GameView): boolean {
  return ["changed", "runnerMoved", "shortcutMissing"].includes(g.health.state) || (!g.exeExists && !!g.exe);
}

function Cover({ game }: { game: GameView }) {
  const [failed, setFailed] = useState(false);
  if (game.art.cover && !failed) {
    return <img src={game.art.cover} alt="" loading="lazy" onError={() => setFailed(true)} />;
  }
  return (
    <div className="cover-fallback">
      <span>{game.name}</span>
    </div>
  );
}

export function Library({
  overview,
  checks,
  checking,
  onOpen,
  onAdd,
  onRefresh,
  onSettings,
}: {
  overview: Overview;
  checks: Record<string, StatusReport>;
  checking: Record<string, boolean>;
  onOpen: (key: string) => void;
  onAdd: () => void;
  onRefresh: () => Promise<void>;
  onSettings: () => void;
}) {
  const t = useT();
  const [view, setView] = useState<"grid" | "list">(loadView);
  const [query, setQuery] = useState("");

  const setViewSaved = (v: "grid" | "list") => {
    setView(v);
    try {
      localStorage.setItem("ln.view", v);
    } catch {
      /* ignore */
    }
  };

  const games = useMemo(
    () =>
      overview.games
        .filter((g) => g.name.toLowerCase().includes(query.trim().toLowerCase()))
        .sort((a, b) => a.name.localeCompare(b.name, currentLang())),
    [overview.games, query],
  );

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>{t.library.title}</h1>
          <p className="muted">{t.library.subtitle(overview.games.length)}</p>
        </div>
        <div className="head-actions">
          <div className="search">
            <Icon name="search" size={16} />
            <input placeholder={t.library.search} value={query} onChange={(e) => setQuery(e.target.value)} />
          </div>
          <div className="seg">
            <button className={view === "grid" ? "on" : ""} onClick={() => setViewSaved("grid")} title={t.library.grid}>
              <Icon name="grid" />
            </button>
            <button className={view === "list" ? "on" : ""} onClick={() => setViewSaved("list")} title={t.library.list}>
              <Icon name="list" />
            </button>
          </div>
          <Button icon="sync" onClick={onRefresh} title={t.library.refreshTitle}>
            {t.library.refresh}
          </Button>
          <Button kind="primary" icon="plus" onClick={onAdd}>
            {t.library.add}
          </Button>
        </div>
      </header>

      {overview.cloud.kind === "none" && (
        <div className="banner banner-info">
          <Icon name="cloud" />
          <span>{t.library.noCloud}</span>
          <Button small kind="primary" onClick={onSettings}>
            {t.library.connect}
          </Button>
        </div>
      )}
      {overview.cloud.error && overview.cloud.kind !== "none" && (
        <div className="banner banner-warn">
          <Icon name="warn" />
          <span>{overview.cloud.error}</span>
          <Button small onClick={onSettings}>
            {t.library.openSettings}
          </Button>
        </div>
      )}

      {overview.games.length === 0 ? (
        <Empty icon="library" title={t.library.emptyTitle}>
          <p className="muted">{t.library.emptyText}</p>
          <Button kind="primary" icon="plus" onClick={onAdd}>
            {t.library.add}
          </Button>
        </Empty>
      ) : view === "grid" ? (
        <div className="grid">
          {games.map((g) => (
            <button key={g.key} className="card" onClick={() => onOpen(g.key)}>
              <div className="cover">
                <Cover game={g} />
                {needsAttention(g) && (
                  <span className="attention" title={t.library.needsAttention}>
                    <Icon name="warn" size={15} />
                  </span>
                )}
                {checking[g.key] && (
                  <span className="card-spinner">
                    <Spinner />
                  </span>
                )}
              </div>
              <div className="card-meta">
                <div className="card-name">{g.name}</div>
                <div className="card-sub">
                  <StatusBadge game={g} check={checks[g.key]} checking={checking[g.key]} />
                  {g.playtime.totalSecs > 0 && <span className="card-time muted">{duration(g.playtime.totalSecs)}</span>}
                </div>
              </div>
            </button>
          ))}
        </div>
      ) : (
        <div className="table">
          <div className="tr th">
            <span>{t.library.col.game}</span>
            <span>{t.library.col.status}</span>
            <span>{t.library.col.local}</span>
            <span>{t.library.col.lastSync}</span>
            <span>{t.library.col.playtime}</span>
            <span>Steam</span>
          </div>
          {games.map((g) => {
            const c = checks[g.key];
            return (
              <button key={g.key} className="tr" onClick={() => onOpen(g.key)}>
                <span className="cell-game">
                  {g.art.icon ? <img src={g.art.icon} alt="" /> : <span className="icon-fallback">{g.name.slice(0, 1)}</span>}
                  <span>
                    <b>{g.name}</b>
                    <small className="muted">{g.platform ?? g.adapterName ?? (g.ludusaviTitle ? t.library.ludusavi : t.library.custom)}</small>
                  </span>
                </span>
                <span>
                  <StatusBadge game={g} check={c} checking={checking[g.key]} />
                </span>
                <span className="muted">{c ? `${t.library.files(c.localFiles)} · ${bytes(c.localSize)}` : "—"}</span>
                <span className="muted">
                  {relTime(g.lastSyncAt)}
                  {g.lastSyncedDevice ? ` · ${g.lastSyncedDevice}` : ""}
                </span>
                <span className="muted">{g.playtime.totalSecs > 0 ? duration(g.playtime.totalSecs) : "—"}</span>
                <span>
                  {needsAttention(g) ? (
                    <Badge tone="warn">{t.library.needsAttention}</Badge>
                  ) : g.steam?.enabled ? (
                    <Badge tone="ok">{t.library.steam.on}</Badge>
                  ) : g.steam ? (
                    <Badge tone="muted">{t.library.steam.off}</Badge>
                  ) : (
                    <Badge tone="muted">{t.library.steam.none}</Badge>
                  )}
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
