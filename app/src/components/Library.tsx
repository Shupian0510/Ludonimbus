import { useEffect, useMemo, useRef, useState } from "react";
import { api, type GameView, type Overview, type StatusReport } from "../api";
import { bytes, duration, relTime, statusInfo } from "../format";
import { currentLang, msg, useT } from "../i18n";
import { sceneIs } from "../scene";
import { groupBySource, type Grouped, type SourceGroup, tagText } from "../source";
import { Badge, Button, Empty, Icon, Spinner, Toggle, useUi } from "./ui";

function stored(storage: () => Storage, key: string): string | null {
  try {
    return storage().getItem(key);
  } catch {
    return null;
  }
}

function store(storage: () => Storage, key: string, value: string | null) {
  try {
    if (value === null) storage().removeItem(key);
    else storage().setItem(key, value);
  } catch {
    /* ignore */
  }
}

const local = () => localStorage;
const session = () => sessionStorage;

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

const srcClass = (g: SourceGroup) => `src-${g.named ? "named" : g.kind}`;

/** A game's source badge: the emulator's name, "Recomp / port", the user's group… */
function SourceTag({ group, game, inline }: { group: SourceGroup; game: GameView; inline?: boolean }) {
  const text = tagText(group);
  if (!text) return null;
  const tip = game.source.detail ?? group.detail ?? undefined;
  return (
    <span className={`src-tag ${srcClass(group)}${inline ? " src-inline" : ""}`} title={tip}>
      {text}
    </span>
  );
}

/** Display options: group by source, show source badges. */
function ViewMenu({
  grouped,
  setGrouped,
  showTags,
  setShowTags,
}: {
  grouped: boolean;
  setGrouped: (v: boolean) => void;
  showTags: boolean;
  setShowTags: (v: boolean) => void;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);
  return (
    <div className="view-menu" ref={ref}>
      <div className="seg">
        <button className={open || grouped ? "on" : ""} onClick={() => setOpen(!open)} title={t.source.view} aria-expanded={open}>
          <Icon name="sliders" />
        </button>
      </div>
      {open && (
        <div className="popover" role="menu">
          <label className="popover-row">
            <span>{t.source.groupBy}</span>
            <Toggle checked={grouped} onChange={setGrouped} />
          </label>
          <label className="popover-row">
            <span>{t.source.showTags}</span>
            <Toggle checked={showTags} onChange={setShowTags} />
          </label>
        </div>
      )}
    </div>
  );
}

type GroupAction = { id: string; mode: "sync" | "backup"; progress: string };

/** A group's title, size and the actions that run on all of its games. */
function GroupHead({
  entry,
  busy,
  onRun,
}: {
  entry: Grouped;
  busy: GroupAction | null;
  onRun: (entry: Grouped, mode: "sync" | "backup") => void;
}) {
  const t = useT();
  const { group, games } = entry;
  const secs = games.reduce((n, g) => n + g.playtime.totalSecs, 0);
  const mine = busy?.id === group.id ? busy : null;
  const action = (mode: "sync" | "backup", icon: string, label: string, title: string) => (
    <button className="btn btn-secondary btn-sm" disabled={!!busy} onClick={() => onRun(entry, mode)} title={title}>
      {mine?.mode === mode ? <span className="spinner" /> : <Icon name={icon} size={15} />}
      {mine?.mode === mode ? `${label} ${mine.progress}` : label}
    </button>
  );
  return (
    <div className="lib-group-head">
      <span className={`src-dot ${srcClass(group)}`} />
      <h2>{group.label}</h2>
      <span className="muted lib-group-meta">
        {group.detail && <>{group.detail} · </>}
        {t.source.games(games.length)}
        {secs > 0 && <> · {duration(secs)}</>}
      </span>
      <div className="lib-group-actions">
        {action("sync", "sync", t.source.syncAll, t.source.syncAllTitle)}
        {action("backup", "upload", t.source.backupAll, t.source.backupAllTitle)}
      </div>
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
  onChanged,
  onSettings,
}: {
  overview: Overview;
  checks: Record<string, StatusReport>;
  checking: Record<string, boolean>;
  onOpen: (key: string) => void;
  onAdd: () => void;
  onRefresh: () => Promise<void>;
  /** After a group action: reload and check these games again. */
  onChanged: (keys: string[]) => Promise<void>;
  onSettings: () => void;
}) {
  const t = useT();
  const ui = useUi();
  const [view, setView] = useState<"grid" | "list">(() => (stored(local, "ln.view") === "list" ? "list" : "grid"));
  const [query, setQuery] = useState("");
  const [grouped, setGrouped] = useState(() => stored(local, "ln.group") === "1" || sceneIs("library-sources"));
  const [showTags, setShowTags] = useState(() => stored(local, "ln.sourceTags") !== "0");
  // Kept while moving between the library and a game, not across restarts.
  const [filter, setFilter] = useState<string | null>(() => stored(session, "ln.source"));
  const [busy, setBusy] = useState<GroupAction | null>(null);

  const setViewSaved = (v: "grid" | "list") => {
    setView(v);
    store(local, "ln.view", v);
  };
  const setGroupedSaved = (v: boolean) => {
    setGrouped(v);
    store(local, "ln.group", v ? "1" : "0");
  };
  const setTagsSaved = (v: boolean) => {
    setShowTags(v);
    store(local, "ln.sourceTags", v ? "1" : "0");
  };
  const setFilterSaved = (id: string | null) => {
    setFilter(id);
    store(session, "ln.source", id);
  };

  const sorted = useMemo(
    () => [...overview.games].sort((a, b) => a.name.localeCompare(b.name, currentLang())),
    [overview.games],
  );
  // Groups of the whole library (for the filter), and of the games shown.
  const all = useMemo(() => groupBySource(sorted), [sorted]);
  const active = filter && all.groups.some((g) => g.group.id === filter) ? filter : null;
  const games = useMemo(() => {
    const q = query.trim().toLowerCase();
    return sorted.filter((g) => g.name.toLowerCase().includes(q) && (!active || all.of.get(g.key)?.id === active));
  }, [sorted, query, active, all]);
  const sections = useMemo(() => {
    const shown = new Set(games.map((g) => g.key));
    return all.groups.map((e) => ({ group: e.group, games: e.games.filter((g) => shown.has(g.key)) })).filter((e) => e.games.length);
  }, [games, all]);

  const runGroup = async (entry: Grouped, mode: "sync" | "backup") => {
    const s = msg().source;
    const { group, games: list } = entry;
    if (overview.cloud.kind === "none") {
      ui.toast(msg().library.noCloud, "warn");
      return;
    }
    // Backups need saves: leave out games whose last check found none.
    const todo = list.filter((g) => !g.running && !(mode === "backup" && checks[g.key]?.localFiles === 0));
    if (!todo.length) {
      ui.toast(s.nothing, "info");
      return;
    }
    let comment = "";
    if (mode === "backup") {
      const c = await ui.prompt({
        title: s.backupPrompt(group.label, todo.length),
        text: s.backupText,
        placeholder: msg().game.backupPlaceholder,
        confirm: s.backupAll,
      });
      if (c === null) return;
      comment = c.trim();
    }
    const r = { done: 0, conflicts: 0, failed: [] as string[], skipped: list.length - todo.length };
    try {
      for (const [i, g] of todo.entries()) {
        setBusy({ id: group.id, mode, progress: s.progress(i + 1, todo.length) });
        try {
          const o = mode === "sync" ? await api.sync(g.key) : await api.backup(g.key, comment);
          if (o.kind === "conflict") r.conflicts++;
          else if (o.kind === "offline" || o.kind === "noCloud") r.failed.push(g.name);
          else r.done++;
        } catch {
          r.failed.push(g.name);
        }
      }
    } finally {
      setBusy(null);
    }
    ui.toast(s.summary(group.label, r), r.failed.length ? "bad" : r.conflicts ? "warn" : "ok");
    await onChanged(todo.map((g) => g.key));
  };

  const renderGrid = (list: GameView[]) => (
    <div className="grid">
      {list.map((g) => (
        <button key={g.key} className="card" onClick={() => onOpen(g.key)}>
          <div className="cover">
            <Cover game={g} />
            {showTags && <SourceTag group={all.of.get(g.key)!} game={g} />}
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
  );

  const renderTable = (list: GameView[]) => (
    <div className="table">
      <div className="tr th">
        <span>{t.library.col.game}</span>
        <span>{t.library.col.status}</span>
        <span>{t.library.col.local}</span>
        <span>{t.library.col.lastSync}</span>
        <span>{t.library.col.playtime}</span>
        <span>Steam</span>
      </div>
      {list.map((g) => {
        const c = checks[g.key];
        return (
          <button key={g.key} className="tr" onClick={() => onOpen(g.key)}>
            <span className="cell-game">
              {g.art.icon ? <img src={g.art.icon} alt="" /> : <span className="icon-fallback">{g.name.slice(0, 1)}</span>}
              <span>
                <b>{g.name}</b>
                <small className="muted">
                  {showTags && <SourceTag group={all.of.get(g.key)!} game={g} inline />}
                  {g.platform ?? g.adapterName ?? (g.ludusaviTitle ? t.library.ludusavi : t.library.custom)}
                </small>
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
  );

  const renderList = (list: GameView[]) => (view === "grid" ? renderGrid(list) : renderTable(list));

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
          <ViewMenu grouped={grouped} setGrouped={setGroupedSaved} showTags={showTags} setShowTags={setTagsSaved} />
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

      {all.groups.length > 1 && (
        <div className="chips source-chips">
          <button className={`chip${active ? "" : " on"}`} onClick={() => setFilterSaved(null)}>
            {t.source.all}
            <span className="chip-count">{overview.games.length}</span>
          </button>
          {all.groups.map(({ group, games: list }) => (
            <button
              key={group.id}
              className={`chip${active === group.id ? " on" : ""}`}
              onClick={() => setFilterSaved(active === group.id ? null : group.id)}
              title={group.detail ?? undefined}
            >
              <span className={`src-dot ${srcClass(group)}`} />
              {group.label}
              <span className="chip-count">{list.length}</span>
            </button>
          ))}
        </div>
      )}

      {overview.games.length === 0 ? (
        <Empty icon="library" title={t.library.emptyTitle}>
          <p className="muted">{t.library.emptyText}</p>
          <Button kind="primary" icon="plus" onClick={onAdd}>
            {t.library.add}
          </Button>
        </Empty>
      ) : grouped || active ? (
        sections.map((entry) => (
          <section key={entry.group.id} className="lib-group">
            <GroupHead entry={entry} busy={busy} onRun={runGroup} />
            {renderList(entry.games)}
          </section>
        ))
      ) : (
        renderList(games)
      )}
    </div>
  );
}
