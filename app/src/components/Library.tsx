import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { api, type GameSource, type GameView, type Overview, type StatusReport } from "../api";
import { bytes, duration, statusInfo } from "../format";
import { currentLang, msg, useT } from "../i18n";
import { sceneIs } from "../scene";
import { groupBySource, type Grouped, type SourceGroup, tagText } from "../source";
import { type Member, membersOf, variantLabel } from "./SaveLibrary";
import { DifferBanner, DifferDialog, differenceKey, needsChoice, uploadAll } from "./SyncPolicy";
import { DeviceChip, Pill, Stat, When } from "./kit";
import { Badge, Button, Empty, Hint, Icon, Spinner, Toggle, useUi } from "./ui";

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

type Tone = "ok" | "info" | "warn" | "bad" | "muted";

/** How a game on this PC stands with the cloud, in a word and a colour. */
export function statusOf(game: GameView, check?: StatusReport, checking?: boolean): { label: string; tone: Tone } {
  const t = msg().library;
  if (game.running) return { label: t.running, tone: "info" };
  if (!check) return { label: checking ? t.checking : t.notChecked, tone: "muted" };
  return statusInfo(check.status);
}

export function StatusBadge({ game, check, checking }: { game: GameView; check?: StatusReport; checking?: boolean }) {
  useT();
  const s = statusOf(game, check, checking);
  return <Pill tone={s.tone}>{s.label}</Pill>;
}

/**
 * The status on a card: a dot in the status's colour, named on hover. In
 * words only when something waits for the user.
 */
function StatusDot({ label, tone }: { label: string; tone: Tone }) {
  return (
    <span className={`status-dot status-${tone}`} title={label}>
      <i />
      {(tone === "warn" || tone === "bad") && label}
    </span>
  );
}

/** One mark per version of the game: what it runs on, and whether that is on this PC. */
interface Mark {
  label: string;
  logo: string | null;
  here: boolean;
}

function marksOf(members: Member[]): Mark[] {
  const out: Mark[] = [];
  for (const m of members) {
    const label = variantLabel(m);
    const seen = out.find((x) => x.label === label);
    if (seen) {
      seen.here ||= m.playable;
      seen.logo ??= m.source.logo;
    } else {
      out.push({ label, logo: m.source.logo, here: m.playable });
    }
  }
  // A game with one version and no program logo of its own has nothing to tell apart.
  return out.length > 1 || out.some((m) => m.logo) ? out.sort((a, b) => Number(b.here) - Number(a.here)) : [];
}

function Marks({ members }: { members: Member[] }) {
  const t = useT();
  const marks = useMemo(() => marksOf(members), [members]);
  const [failed, setFailed] = useState<string[]>([]);
  if (marks.length === 0) return null;
  return (
    <span className="card-marks">
      {marks.map((m) => (
        <span key={m.label} className={m.here ? "mark" : "mark off"} title={`${m.label} · ${m.here ? t.savelib.here : t.savelib.notHere}`}>
          {m.logo && !failed.includes(m.label) ? (
            <img className="src-logo" src={m.logo} alt="" width={14} height={14} onError={() => setFailed((f) => [...f, m.label])} />
          ) : (
            m.label
          )}
        </span>
      ))}
    </span>
  );
}

function needsAttention(g: GameView): boolean {
  return ["changed", "runnerMoved", "shortcutMissing"].includes(g.health.state) || (!g.exeExists && !!g.exe);
}

/**
 * One card in the library: a game with all its versions, on this PC and
 * only in the cloud (docs/plan-phase5.md §5). It shows its main version:
 * one this PC can start, else one on this PC, else one in the cloud.
 */
export interface Card {
  key: string;
  name: string;
  source: GameSource;
  main: Member;
  members: Member[];
  /** Some version of the game is on this PC: shown in colour. */
  playable: boolean;
  secs: number;
  /** The collection's shelves the game is on (any of its versions' cards). */
  shelves: string[];
}

export function cardsOf(overview: Overview): Card[] {
  const byFamily = new Map<string, Member[]>();
  for (const m of membersOf(overview)) {
    const list = byFamily.get(m.family);
    if (list) list.push(m);
    else byFamily.set(m.family, [m]);
  }
  const rank = (m: Member) => (m.playable ? 0 : m.game ? 1 : 2);
  return [...byFamily.values()].map((members) => {
    const main = [...members].sort((a, b) => rank(a) - rank(b))[0];
    const secs = members.reduce((n, m) => n + (m.game?.playtime.totalSecs ?? m.cloud?.playtime.totalSecs ?? 0), 0);
    const shelves = [...new Set(members.flatMap((m) => (m.game ?? m.cloud)?.shelves ?? []))];
    return { key: main.key, name: main.name, source: main.source, main, members, playable: members.some((m) => m.playable), secs, shelves };
  });
}

const artOf = (m: Member) => (m.game ?? m.cloud)!.art;

export function Cover({ name, cover }: { name: string; cover: string | null }) {
  const [failed, setFailed] = useState(false);
  if (cover && !failed) {
    return <img src={cover} alt="" loading="lazy" onError={() => setFailed(true)} />;
  }
  return (
    <div className="cover-fallback">
      <span>{name}</span>
    </div>
  );
}

/**
 * The logo of an emulator, recomp or the Xbox app: the icon of the program
 * itself (see crates/core/src/logos.rs). Nothing when there is none.
 */
function Logo({ src, size }: { src: string | null; size: number }) {
  const [failed, setFailed] = useState(false);
  if (!src || failed) return null;
  return <img className="src-logo" src={src} alt="" width={size} height={size} onError={() => setFailed(true)} />;
}

/** A game's source in the list: its program's logo and the group's name. */
function SourceTag({ group, source }: { group: SourceGroup; source: GameSource }) {
  const text = tagText(group);
  if (!text) return null;
  const detail = source.detail ?? group.detail;
  return (
    <span className="src-tag" title={detail ?? undefined}>
      <Logo src={source.logo ?? group.logo} size={13} />
      {text}
    </span>
  );
}

/** Display options: group by source, show the source filter, show games that aren't installed. */
function ViewMenu({
  grouped,
  setGrouped,
  showChips,
  setShowChips,
  showMissing,
  setShowMissing,
}: {
  grouped: boolean;
  setGrouped: (v: boolean) => void;
  showChips: boolean;
  setShowChips: (v: boolean) => void;
  showMissing: boolean;
  setShowMissing: (v: boolean) => void;
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
        <button className={open || grouped || !showMissing ? "on" : ""} onClick={() => setOpen(!open)} title={t.source.view} aria-expanded={open}>
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
            <span>{t.source.showChips}</span>
            <Toggle checked={showChips} onChange={setShowChips} />
          </label>
          <label className="popover-row">
            <span>{t.source.showMissing}</span>
            <Toggle checked={showMissing} onChange={setShowMissing} />
          </label>
        </div>
      )}
    </div>
  );
}

type GroupAction = { id: string; mode: "sync" | "backup"; progress: string };

/** The versions on this PC of the games in a group (what group actions run on). */
const localGames = (cards: Card[]) => cards.flatMap((c) => c.members.flatMap((m) => (m.game ? [m.game] : [])));

/** A group's title, size and the actions that run on all of its games. */
function GroupHead({
  entry,
  busy,
  onRun,
}: {
  entry: Grouped<Card>;
  busy: GroupAction | null;
  onRun: (entry: Grouped<Card>, mode: "sync" | "backup") => void;
}) {
  const t = useT();
  const { group, games } = entry;
  const secs = games.reduce((n, g) => n + g.secs, 0);
  const mine = busy?.id === group.id ? busy : null;
  const action = (mode: "sync" | "backup", icon: string, label: string, title: string) => (
    <button className="btn btn-secondary btn-sm" disabled={!!busy || localGames(games).length === 0} onClick={() => onRun(entry, mode)} title={title}>
      {mine?.mode === mode ? <span className="spinner" /> : <Icon name={icon} size={15} />}
      {mine?.mode === mode ? `${label} ${mine.progress}` : label}
    </button>
  );
  return (
    <div className="lib-group-head">
      {group.logo && (
        <span className="src-tile">
          <Logo src={group.logo} size={22} />
        </span>
      )}
      <h2>{group.label}</h2>
      <span className="lib-group-meta">
        {group.detail && <span className="muted">{group.detail}</span>}
        <Stat icon="library">{t.source.games(games.length)}</Stat>
        {secs > 0 && <Stat icon="clock">{duration(secs)}</Stat>}
      </span>
      <div className="lib-group-actions">
        {action("sync", "sync", t.source.syncAll, t.source.syncAllTitle)}
        {action("backup", "upload", t.source.backupAll, t.source.backupAllTitle)}
      </div>
    </div>
  );
}

/** Check every game again: an icon button that turns into a spinner while it runs. */
function RefreshButton({ onRefresh, title }: { onRefresh: () => Promise<void>; title: string }) {
  const [busy, setBusy] = useState(false);
  return (
    <button
      className="icon-btn icon-btn-lg"
      title={title}
      aria-label={title}
      disabled={busy}
      onClick={async () => {
        setBusy(true);
        try {
          await onRefresh();
        } finally {
          setBusy(false);
        }
      }}
    >
      {busy ? <Spinner /> : <Icon name="sync" />}
    </button>
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
  onCollection,
  statusesReady,
  onChecks,
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
  /** Open the collection, where the archived games are. */
  onCollection: () => void;
  /** Every game was checked since the app started: the line about games that differ from the cloud can show. */
  statusesReady?: boolean;
  /** Statuses that came back from 「全部同步」: take them as they are. */
  onChecks?: (reports: Record<string, StatusReport>) => Promise<void>;
}) {
  const t = useT();
  const ui = useUi();
  const [view, setView] = useState<"grid" | "list">(() => (stored(local, "ln.view") === "list" ? "list" : "grid"));
  const [query, setQuery] = useState("");
  const [grouped, setGrouped] = useState(() => stored(local, "ln.group") === "1" || sceneIs("library-sources"));
  const [showChips, setShowChips] = useState(() => stored(local, "ln.sourceChips") !== "0");
  const [showMissing, setShowMissing] = useState(() => stored(local, "ln.showMissing") !== "0");
  // Kept while moving between the library and a game, not across restarts.
  const [filter, setFilter] = useState<string | null>(() => stored(session, "ln.source"));
  const [busy, setBusy] = useState<GroupAction | null>(null);
  // The games that differ from the cloud, listed for the user to settle.
  const [settling, setSettling] = useState<GameView[] | null>(null);
  // 「全部同步」 over the whole library: how far it got.
  const [syncingAll, setSyncingAll] = useState<string | null>(null);
  const differing = overview.games.filter((g) => needsChoice(g, checks[g.key]));
  const differKey = differenceKey(differing.map((g) => ({ game: g, check: checks[g.key] })));
  const [dismissed, setDismissed] = useState(() => stored(session, "ln.differ"));
  // Docs screenshots: the dialog that settles them, once every game was checked.
  const differScene = useRef(sceneIs("library-differ"));
  useEffect(() => {
    if (differScene.current && statusesReady && differing.length > 0) {
      differScene.current = false;
      setSettling(differing);
    }
  }, [statusesReady, differing]);

  const setViewSaved = (v: "grid" | "list") => {
    setView(v);
    store(local, "ln.view", v);
  };
  const setGroupedSaved = (v: boolean) => {
    setGrouped(v);
    store(local, "ln.group", v ? "1" : "0");
  };
  const setChipsSaved = (v: boolean) => {
    setShowChips(v);
    store(local, "ln.sourceChips", v ? "1" : "0");
  };
  const setFilterSaved = (id: string | null) => {
    setFilter(id);
    store(session, "ln.source", id);
  };
  const setMissingSaved = (v: boolean) => {
    setShowMissing(v);
    store(local, "ln.showMissing", v ? "1" : "0");
  };

  // Archived games are on the collection's shelf, not in the library.
  const everything = useMemo(() => cardsOf(overview), [overview]);
  const allCards = useMemo(() => everything.filter((c) => !c.shelves.includes("archived")), [everything]);
  const archivedCount = everything.length - allCards.length;
  // Games on this PC first, then the grey ones (only their saves here, or
  // only in the cloud); by name within each. Groups keep this order.
  const sorted = useMemo(() => {
    const lang = currentLang();
    return allCards
      .filter((c) => showMissing || c.playable)
      .sort((a, b) => Number(b.playable) - Number(a.playable) || a.name.localeCompare(b.name, lang));
  }, [allCards, showMissing]);
  // Groups of the whole library (for the filter), and of the games shown.
  const all = useMemo(() => groupBySource(sorted), [sorted]);
  // A hidden filter bar shows the whole library.
  const active = showChips && filter && all.groups.some((g) => g.group.id === filter) ? filter : null;
  const cards = useMemo(() => {
    const q = query.trim().toLowerCase();
    return sorted.filter((c) => c.members.some((m) => m.name.toLowerCase().includes(q)) && (!active || all.of.get(c.key)?.id === active));
  }, [sorted, query, active, all]);
  const sections = useMemo(() => {
    const shown = new Set(cards.map((c) => c.key));
    return all.groups.map((e) => ({ group: e.group, games: e.games.filter((c) => shown.has(c.key)) })).filter((e) => e.games.length);
  }, [cards, all]);

  const runGroup = async (entry: Grouped<Card>, mode: "sync" | "backup") => {
    const s = msg().source;
    const { group } = entry;
    const list = localGames(entry.games);
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
    if (mode === "sync") {
      setBusy({ id: group.id, mode, progress: "" });
      try {
        await syncThen(group.label, todo, (progress) => setBusy({ id: group.id, mode, progress }));
      } finally {
        setBusy(null);
      }
      return;
    }
    const r = { done: 0, conflicts: 0, failed: [] as string[], skipped: list.length - todo.length };
    try {
      for (const [i, g] of todo.entries()) {
        setBusy({ id: group.id, mode, progress: s.progress(i + 1, todo.length) });
        try {
          const o = await api.backup(g.key, comment);
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

  /**
   * 「全部同步」: upload what changed in `games`, writing nothing into their
   * saves, then list the ones that need a choice (newer saves in the cloud
   * that don't come by themselves, conflicts). Only those games' own lines:
   * never another platform's or version's.
   */
  const syncThen = async (label: string, games: GameView[], progress: (text: string) => void) => {
    const r = await uploadAll(games, progress);
    if (onChecks) await onChecks(r.reports);
    else await onChanged(games.map((g) => g.key));
    const choose = games.filter((g) => needsChoice(g, r.reports[g.key]));
    if (choose.length > 0) setSettling(choose);
    ui.toast(msg().source.summary(label, r), r.failed.length ? "bad" : choose.length ? "warn" : "ok");
  };

  const syncEverything = async () => {
    if (overview.cloud.kind === "none") {
      ui.toast(msg().library.noCloud, "warn");
      return;
    }
    setSyncingAll("");
    try {
      await syncThen(msg().policy.everything, overview.games, setSyncingAll);
    } finally {
      setSyncingAll(null);
    }
  };

  /** Where the grey games start, under a divider. */
  const divider = (n: number) => (
    <div className="lib-divider" title={t.library.missingHint}>
      {t.library.missingTitle}
      <span className="chip-count">{n}</span>
    </div>
  );

  const hint = (c: Card) => (c.playable ? undefined : c.main.cloud ? t.savelib.cloudOnlyHint : t.library.missingHint);

  /** The same on a card, as a dot. */
  const dot = (c: Card) =>
    c.main.game ? <StatusDot {...statusOf(c.main.game, checks[c.main.key], checking[c.main.key])} /> : <StatusDot label={t.savelib.cloudOnly} tone="muted" />;

  /** Status of the card's main version: its sync status, or "only in the cloud". */
  const status = (c: Card) =>
    c.main.game ? (
      <StatusBadge game={c.main.game} check={checks[c.main.key]} checking={checking[c.main.key]} />
    ) : (
      <Badge tone="muted">{t.savelib.cloudOnly}</Badge>
    );

  const renderGrid = (list: Card[], divide: boolean) => {
    const at = divide ? list.findIndex((c) => !c.playable) : -1;
    if (at < 0) return renderCards(list);
    return (
      <>
        {at > 0 && renderCards(list.slice(0, at))}
        {divider(list.length - at)}
        {renderCards(list.slice(at))}
      </>
    );
  };

  const renderCards = (list: Card[]) => (
    <div className="grid">
      {list.map((c) => {
        const g = c.main.game;
        return (
          <button key={c.key} className={c.playable ? "card" : "card missing"} onClick={() => onOpen(c.key)} title={hint(c)}>
            <div className="cover">
              <Cover name={c.name} cover={artOf(c.main).cover} />
            </div>
            <div className="card-meta">
              <div className="card-name">{c.name}</div>
              <div className="card-sub">
                {checking[c.key] ? <Spinner /> : dot(c)}
                {g && needsAttention(g) && (
                  <span className="card-attention" title={t.library.needsAttention}>
                    <Icon name="warn" size={14} />
                  </span>
                )}
                <Marks members={c.members} />
                <span className="card-time muted">{c.secs > 0 && duration(c.secs)}</span>
              </div>
            </div>
          </button>
        );
      })}
    </div>
  );

  const renderTable = (list: Card[], divide: boolean) => (
    <div className="table">
      <div className="tr th">
        <span>{t.library.col.game}</span>
        <span>{t.library.col.status}</span>
        <span>{t.library.col.local}</span>
        <span>{t.library.col.lastSync}</span>
        <span>{t.library.col.playtime}</span>
        <span>Steam</span>
      </div>
      {list.map((c, i) => {
        const g = c.main.game;
        const chk = checks[c.key];
        const first = divide && !c.playable && (i === 0 || list[i - 1].playable);
        const icon = artOf(c.main).icon;
        return (
          <Fragment key={c.key}>
            {first && (
              <div className="tr th tr-divider" title={t.library.missingHint}>
                {t.library.missingTitle}
                <span className="chip-count">{list.length - i}</span>
              </div>
            )}
            <button className={c.playable ? "tr" : "tr missing"} onClick={() => onOpen(c.key)} title={hint(c)}>
              <span className="cell-game">
                {icon ? <img src={icon} alt="" /> : <span className="icon-fallback">{c.name.slice(0, 1)}</span>}
                <span>
                  <b>{c.name}</b>
                  <small className="muted">
                    <SourceTag group={all.of.get(c.key)!} source={c.source} />
                    {g
                      ? (g.platform ?? g.adapterName ?? (g.ludusaviTitle ? t.library.ludusavi : t.library.custom))
                      : (c.main.cloud?.platform ?? c.main.cloud?.adapterName ?? t.savelib.cloudOnly)}
                    {c.members.length > 1 && <span className="chip-count">{t.savelib.versions(c.members.length)}</span>}
                  </small>
                </span>
              </span>
              <span>{status(c)}</span>
              <span className="muted">
                {chk ? (
                  <Stat icon="file" title={t.kit.localSaves(chk.localFiles, bytes(chk.localSize))}>
                    {bytes(chk.localSize)}
                  </Stat>
                ) : (
                  "—"
                )}
              </span>
              <span className="cell-facts">
                {g ? <When at={g.lastSyncAt} /> : "—"}
                {g?.lastSyncedDevice && <DeviceChip name={g.lastSyncedDevice} here={g.lastSyncedDevice === overview.deviceName} />}
              </span>
              <span className="muted">{c.secs > 0 ? duration(c.secs) : "—"}</span>
              <span>
                {!g ? (
                  <Badge tone="muted">{t.library.steam.none}</Badge>
                ) : needsAttention(g) ? (
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
          </Fragment>
        );
      })}
    </div>
  );

  /** `divide`: put a divider before the grey games. */
  const renderList = (list: Card[], divide = false) => (view === "grid" ? renderGrid(list, divide) : renderTable(list, divide));

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>{t.library.title}</h1>
          <p className="muted">{t.library.subtitle(allCards.length)}</p>
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
          <ViewMenu
            grouped={grouped}
            setGrouped={setGroupedSaved}
            showChips={showChips}
            setShowChips={setChipsSaved}
            showMissing={showMissing}
            setShowMissing={setMissingSaved}
          />
          <RefreshButton onRefresh={onRefresh} title={t.library.refreshTitle} />
          {overview.games.length > 0 && (
            <Button icon="upload" onClick={syncEverything} disabled={!!busy || syncingAll !== null} title={t.policy.syncAllTitle}>
              {syncingAll !== null ? `${t.policy.syncAll} ${syncingAll}` : t.policy.syncAll}
            </Button>
          )}
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
      {overview.paused && (
        <div className="banner banner-info">
          <Icon name="clock" />
          <span>{t.library.paused}</span>
          <Hint text={t.library.pausedHint} />
          <Button small kind="primary" onClick={() => api.setSyncPaused(false).then(() => onChanged([]))}>
            {t.library.resume}
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

      {statusesReady && differing.length > 0 && dismissed !== differKey && (
        <DifferBanner
          games={differing}
          onReview={() => setSettling(differing)}
          onDismiss={() => {
            setDismissed(differKey);
            store(session, "ln.differ", differKey);
          }}
        />
      )}
      {settling && <DifferDialog games={settling} checks={checks} onClose={() => setSettling(null)} onChanged={onChanged} />}

      {showChips && all.groups.length > 1 && (
        <div className="chips source-chips">
          <button className={`chip${active ? "" : " on"}`} onClick={() => setFilterSaved(null)}>
            {t.source.all}
            <span className="chip-count">{sorted.length}</span>
          </button>
          {all.groups.map(({ group, games: list }) => (
            <button
              key={group.id}
              className={`chip${active === group.id ? " on" : ""}`}
              onClick={() => setFilterSaved(active === group.id ? null : group.id)}
              title={group.detail ?? undefined}
            >
              <Logo src={group.logo} size={15} />
              {group.label}
              <span className="chip-count">{list.length}</span>
            </button>
          ))}
        </div>
      )}

      {allCards.length === 0 ? (
        <Empty icon="library" title={t.library.emptyTitle}>
          <p className="muted">{t.library.emptyText}</p>
          <Button kind="primary" icon="plus" onClick={onAdd}>
            {t.library.add}
          </Button>
        </Empty>
      ) : sorted.length === 0 ? (
        <Empty icon="library" title={t.library.allMissingTitle}>
          <p className="muted">{t.library.allMissingText}</p>
          <Button onClick={() => setMissingSaved(true)}>{t.source.showMissing}</Button>
        </Empty>
      ) : grouped || active ? (
        sections.map((entry) => (
          <section key={entry.group.id} className="lib-group">
            <GroupHead entry={entry} busy={busy} onRun={runGroup} />
            {renderList(entry.games)}
          </section>
        ))
      ) : (
        renderList(cards, true)
      )}
      {archivedCount > 0 && (
        <button className="link lib-archived" onClick={onCollection}>
          <Icon name="star" size={14} /> {t.collection.inLibrary(archivedCount)}
        </button>
      )}
    </div>
  );
}
