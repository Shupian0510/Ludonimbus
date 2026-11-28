// The handheld screen (docs/plan-phase5.md §1.1): full screen, large, run
// with a controller. Shows the save library (one card per game, grey when
// it isn't on this device), a game's sync status, its versions and its
// saves as the desktop's Saves tab arranges them (here first, then one group
// per other source; make one the current one, take one here, send it to
// another version), and the few settings that make sense without a
// keyboard. Adding games, signing in to a cloud and the save finder stay in
// the desktop view.
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, errorText, HandheldInfo, HandheldMode, Overview, PlatformMark as Mark, SaveSource, SaveSources, SnapshotList, SnapshotSummary, TransferPlan } from "../api";
import { cardsOf, type Card } from "../components/Library";
import { familyOf, variantLabel } from "../components/SaveLibrary";
import { arrange } from "../components/Saves";
import { versionOf } from "../components/SaveSources";
import { cmpOf, ConvertChip, Delta, DeviceChip, learnDeviceKinds, Pill, PlatformMark, PlayDelta, playedOf, SaveCard, SaveCompare, Side, Stat, When } from "../components/kit";
import { Badge, Icon, Spinner, useUi } from "../components/ui";
import { bytes, dateTime, duration, outcomeText, statusInfo } from "../format";
import { currentLang, msg, useT } from "../i18n";
import { sceneIs } from "../scene";
import { type Button, focusFirst, Glyph, Hints, NavRoot, useButtons } from "./nav";

type Screen = { name: "library" } | { name: "game"; key: string } | { name: "settings" };

export function Handheld({ onDesktop }: { onDesktop: () => void }) {
  return (
    <NavRoot>
      <Shell onDesktop={onDesktop} />
    </NavRoot>
  );
}

function Shell({ onDesktop }: { onDesktop: () => void }) {
  const t = useT();
  const h = t.handheld;
  const [overview, setOverview] = useState<Overview | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Browser preview: ?handheld&scene=handheld-game opens a game with saves from elsewhere (docs screenshots).
  const [screen, setScreen] = useState<Screen>(() => (sceneIs("handheld-game") ? { name: "game", key: "celeste" } : { name: "library" }));

  const reload = useCallback(async () => {
    try {
      const o = await api.overview();
      setOverview(o);
      learnDeviceKinds([{ name: o.deviceName, kind: o.deviceKind }]);
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  useEffect(() => {
    reload().then(() => api.refreshCloudLibrary().then(reload, () => undefined));
  }, [reload]);

  const tab = (name: "library" | "settings") => setScreen(name === "library" ? { name: "library" } : { name: "settings" });

  return (
    <div className="hh">
      <header className="hh-head">
        <img src="/app-icon.svg" alt="" />
        <nav className="hh-tabs">
          <Glyph b="lb" />
          <span className={screen.name !== "settings" ? "on" : ""}>{h.library}</span>
          <span className={screen.name === "settings" ? "on" : ""}>{h.settings}</span>
          <Glyph b="rb" />
        </nav>
        <span className="hh-head-right muted">
          {overview && (overview.cloud.kind === "none" ? h.noCloud : overview.cloud.label)}
        </span>
      </header>
      {error && <div className="hh-error">{error}</div>}
      {!overview && !error && (
        <div className="hh-center">
          <Spinner />
        </div>
      )}
      {overview && screen.name === "library" && (
        <Library overview={overview} onOpen={(key) => setScreen({ name: "game", key })} onRefresh={reload} onTab={tab} onExit={() => api.quitHandheld().then(onDesktop)} />
      )}
      {overview && screen.name === "game" && (
        <Game key={screen.key} overview={overview} gameKey={screen.key} onOpen={(key) => setScreen({ name: "game", key })} onBack={() => setScreen({ name: "library" })} onChanged={reload} />
      )}
      {overview && screen.name === "settings" && <Settings overview={overview} onTab={tab} onDesktop={onDesktop} />}
    </div>
  );
}

function Cover({ card }: { card: Card }) {
  const [failed, setFailed] = useState(false);
  const art = (card.main.game ?? card.main.cloud)!.art;
  if (art.cover && !failed) return <img src={art.cover} alt="" onError={() => setFailed(true)} />;
  return <div className="cover-fallback">{card.name}</div>;
}

function Library({
  overview,
  onOpen,
  onRefresh,
  onTab,
  onExit,
}: {
  overview: Overview;
  onOpen: (key: string) => void;
  onRefresh: () => Promise<void>;
  onTab: (s: "library" | "settings") => void;
  onExit: () => void;
}) {
  const t = useT();
  const h = t.handheld;
  const cards = useMemo(() => {
    const lang = currentLang();
    return cardsOf(overview).sort((a, b) => Number(b.playable) - Number(a.playable) || a.name.localeCompare(b.name, lang));
  }, [overview]);
  useEffect(() => {
    const id = requestAnimationFrame(focusFirst);
    return () => cancelAnimationFrame(id);
  }, []);
  useButtons({ y: onRefresh, rb: () => onTab("settings"), lb: () => onTab("settings"), b: onExit });
  const at = cards.findIndex((c) => !c.playable);

  const card = (c: Card) => {
    const g = c.main.game;
    const status = g?.running
      ? { tone: "info" as const, label: t.library.running }
      : g?.lastCheck
        ? statusInfo(g.lastCheck.status)
        : g
          ? { tone: "muted" as const, label: t.library.notChecked }
          : { tone: "muted" as const, label: t.savelib.cloudOnly };
    return (
      <button key={c.key} data-nav className={`hh-card${c.playable ? "" : " missing"}`} onClick={() => onOpen(c.key)}>
        <div className="cover">
          <Cover card={c} />
        </div>
        <div className="hh-card-name">{c.name}</div>
        <div className="hh-card-sub">
          <Pill tone={status.tone}>{status.label}</Pill>
          {c.secs > 0 && <span className="muted">{duration(c.secs)}</span>}
        </div>
      </button>
    );
  };

  return (
    <>
      <main className="hh-main">
        {cards.length === 0 && <p className="hh-center muted">{h.noGames}</p>}
        <div className="hh-grid">{(at < 0 ? cards : cards.slice(0, at)).map(card)}</div>
        {at >= 0 && (
          <>
            <div className="lib-divider">
              {t.library.missingTitle}
              <span className="chip-count">{cards.length - at}</span>
            </div>
            <div className="hh-grid">{cards.slice(at).map(card)}</div>
          </>
        )}
      </main>
      <Hints items={[["a", h.open], ["y", h.refresh], ["rb", h.settings], ["b", h.exit]]} />
    </>
  );
}

/** A full-screen dialog run with the controller. */
function Dialog({
  title,
  children,
  buttons,
  onClose,
}: {
  title: string;
  children?: React.ReactNode;
  buttons: Array<{ b: Button; label: string; onPress: () => void; disabled?: boolean }>;
  onClose: () => void;
}) {
  // Focus the first button again once it can be pressed (e.g. after loading).
  const ready = buttons.map((x) => (x.disabled ? 0 : 1)).join("");
  useEffect(() => {
    const id = requestAnimationFrame(focusFirst);
    return () => cancelAnimationFrame(id);
  }, [ready]);
  const by = (b: Button) => buttons.find((x) => x.b === b && !x.disabled)?.onPress;
  useButtons({ b: by("b") ?? onClose, x: by("x"), y: by("y") });
  return (
    <div className="hh-dialog-back" data-nav-scope>
      <div className="hh-dialog">
        <h2>{title}</h2>
        {children}
        <div className="hh-dialog-buttons">
          {buttons.map((x, i) => (
            <button key={x.label} data-nav data-autofocus={i === 0 || undefined} disabled={x.disabled} onClick={x.onPress}>
              <Glyph b={x.b} /> {x.label}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function Game({
  overview,
  gameKey,
  onOpen,
  onBack,
  onChanged,
}: {
  overview: Overview;
  gameKey: string;
  onOpen: (key: string) => void;
  onBack: () => void;
  onChanged: () => Promise<void>;
}) {
  const t = useT();
  const h = t.handheld;
  const ui = useUi();
  const game = overview.games.find((g) => g.key === gameKey);
  const cloud = overview.cloudOnly.find((g) => g.key === gameKey);
  const name = game?.name ?? cloud?.name ?? "";
  const art = (game ?? cloud)?.art;
  const members = familyOf(overview, gameKey);
  const [data, setData] = useState<SnapshotList | null>(null);
  const [autoOpen, setAutoOpen] = useState(false);
  // The other sources of the game's saves, and the newer progress one of them has.
  const [sources, setSources] = useState<SaveSources | null>(null);
  const [newerGone, setNewerGone] = useState(false);
  const [busy, setBusy] = useState(false);
  const [picked, setPicked] = useState<SnapshotSummary | null>(null);
  const [pickedElsewhere, setPickedElsewhere] = useState<{ source: SaveSource; snapshot: SnapshotSummary } | null>(null);
  const [applying, setApplying] = useState<{ source: string; sourceName: string; snapshot: SnapshotSummary; target?: string } | null>(null);
  const conflict = !!game?.conflict || game?.lastCheck?.status === "conflict";
  const newer = newerGone ? undefined : sources?.newer;

  const load = useCallback(() => {
    api.snapshots(gameKey).then(setData, () => setData({ snapshots: [], headId: null, lastSyncedId: null, platforms: {} }));
    if (game && overview.cloud.kind !== "none") api.saveSources(gameKey).then(setSources, () => setSources(null));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [gameKey]);
  useEffect(() => {
    load();
    const id = requestAnimationFrame(focusFirst);
    return () => cancelAnimationFrame(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [gameKey]);

  const sync = async (resolution?: "cloud" | "local") => {
    if (!game) return;
    setBusy(true);
    try {
      const o = await api.sync(game.key, resolution);
      ui.toast(outcomeText(o, game.name), o.kind === "conflict" ? "warn" : "ok");
      await onChanged();
      load();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setBusy(false);
    }
  };

  const makeCurrent = async (s: SnapshotSummary) => {
    if (!game) return;
    setPicked(null);
    setBusy(true);
    try {
      const o = await api.restore(game.key, s.id);
      const done = o.kind === "pulled" || o.kind === "uploaded" || o.kind === "queued";
      ui.toast(done ? msg().saves.madeCurrent : outcomeText(o, game.name));
      await onChanged();
      load();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setBusy(false);
    }
  };

  const undo = async () => {
    if (!game) return;
    setBusy(true);
    try {
      await api.undoReplace(game.key);
      ui.toast(msg().saves.undone);
      await onChanged();
      load();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setBusy(false);
    }
  };

  /**
   * Take a save of another source here: one of a line that keeps the same
   * files as it is (the dialog it was picked in asked), another version's
   * through the dialog that shows where its files go.
   */
  const takeHere = async (src: SaveSource, s: SnapshotSummary) => {
    if (!game) return;
    setPickedElsewhere(null);
    if (src.key) return setApplying({ source: src.key, sourceName: src.name, snapshot: s, target: gameKey });
    if (!src.line) return;
    setBusy(true);
    try {
      const o = await api.takeSave(game.key, src.line, s.id);
      const done = o.kind === "pulled" || o.kind === "uploaded" || o.kind === "queued";
      ui.toast(done ? msg().sources.taken : outcomeText(o, game.name));
      await onChanged();
      load();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setBusy(false);
    }
  };

  // The same rows, in the same order, as the desktop's Saves tab.
  const { top, auto, current } = useMemo(() => (data ? arrange(data, !game) : { top: [], auto: [], current: null }), [data, game]);
  const here = data?.snapshots.find((s) => s.id === current);
  const groups = sources?.sources ?? [];
  const currentHash = data?.snapshots.find((s) => s.id === current)?.contentHash;
  const isCurrent = (s: SnapshotSummary) => s.id === current || (!!currentHash && s.contentHash === currentHash);
  const isSide = (s: SnapshotSummary) => s.side ?? (s.trigger === "pre-restore" || s.trigger === "conflict-keep");
  // Steam Cloud keeps this game's saves too; writing into the Xbox app's containers is experimental.
  const note = game?.store && game.store.mode !== "managed" ? t.steamCloud.restoreNote : game?.xboxContainers ? t.saves.xboxNote : null;
  const marks = { ...(sources?.platforms ?? {}), ...(data?.platforms ?? {}) };
  const row = (s: SnapshotSummary) => {
    const named = s.locked && !!s.comment;
    const current = isCurrent(s);
    return (
      <button key={s.id} data-nav className={`hh-list-row hh-save${current ? " on" : ""}`} onClick={() => setPicked(s)}>
        <PlatformMark mark={marks[s.platform ?? ""]} size={22} />
        <span className="hh-save-when">
          {named ? <b>{s.comment}</b> : <When at={s.createdAt} strong />}
          {named && <When at={s.createdAt} />}
          {s.locked && <Icon name="star" size={16} />}
        </span>
        <DeviceChip name={s.deviceName} here={s.deviceId === overview.deviceId} />
        <span className="hh-save-mark">
          {current ? (
            <Pill tone="ok">{t.saves.current}</Pill>
          ) : s.incomplete ? (
            <Pill tone="warn">{t.saves.incomplete}</Pill>
          ) : isSide(s) && s.trigger !== "shared" ? (
            <Badge tone="muted">{t.saves.replaced}</Badge>
          ) : (
            game && <Delta cmp={cmpOf(s, here)} quiet />
          )}
        </span>
        <span className="muted hh-save-size">{bytes(s.totalSize)}</span>
      </button>
    );
  };

  useButtons({ b: onBack, y: game ? () => sync() : undefined });

  return (
    <>
      <main className="hh-main hh-game">
        <div className="hh-game-head">
          <div className={`cover${game?.playable ? "" : " missing-cover"}`}>
            {art?.cover ? <img src={art.cover} alt="" /> : <div className="cover-fallback">{name}</div>}
          </div>
          <div className="hh-game-info">
            <h1>{name}</h1>
            {game && (
              <div className="hh-facts">
                {game.lastCheck ? (
                  <Pill tone={statusInfo(game.lastCheck.status).tone}>{statusInfo(game.lastCheck.status).label}</Pill>
                ) : (
                  <Pill tone="muted">{t.library.notChecked}</Pill>
                )}
                <span className="hh-fact" title={t.kit.lastSync}>
                  <Icon name="cloud" size={19} />
                  <When at={game.lastSyncAt} />
                  {game.lastSyncedDevice && <DeviceChip name={game.lastSyncedDevice} here={game.lastSyncedDevice === overview.deviceName} />}
                </span>
                {game.playtime.totalSecs > 0 && <Stat icon="clock">{duration(game.playtime.totalSecs)}</Stat>}
              </div>
            )}
            {!game && <p className="muted">{h.cloudOnly}</p>}
            {game && !game.playable && <p className="muted">{h.notInstalled}</p>}
            {newer && (
              <div className="hh-banner">
                <SaveCompare
                  there={snapSide(newer.source.name, newer.snapshot, newer.here, marks, overview.deviceId, true)}
                  here={newer.here ? { ...snapSide(newer.hereName, newer.here, null, marks, overview.deviceId), tag: t.sources.here } : { name: newer.hereName, empty: true }}
                />
                {newer.source.take.kind === "convert" && (
                  <div className="hh-row">
                    <ConvertChip hint={t.sources.convertHint(newer.source.take.how)} />
                  </div>
                )}
                <div className="hh-row">
                  <button
                    data-nav
                    onClick={() =>
                      newer.source.key ? takeHere(newer.source, newer.snapshot) : setPickedElsewhere({ source: newer.source, snapshot: newer.snapshot })
                    }
                    disabled={busy || game?.running}
                  >
                    {t.sources.take}
                  </button>
                  <button
                    data-nav
                    onClick={() => {
                      api.dismissNewer(gameKey, newer.snapshot.id);
                      setNewerGone(true);
                    }}
                  >
                    {t.sources.notNow}
                  </button>
                </div>
              </div>
            )}
            {game && conflict && (
              <div className="hh-banner warn">
                <p>
                  <b>{t.game.conflictTitle}</b>
                </p>
                {game.xboxContainers && <p className="muted">{t.saves.xboxNote}</p>}
                <div className="hh-row">
                  <button data-nav data-autofocus onClick={() => sync("cloud")} disabled={busy}>
                    {h.useCloud}
                  </button>
                  <button data-nav onClick={() => sync("local")} disabled={busy}>
                    {h.useLocal}
                  </button>
                </div>
              </div>
            )}
            {game && !conflict && (
              <div className="hh-row">
                <button data-nav data-autofocus className="primary" onClick={() => sync()} disabled={busy || game.running}>
                  {busy ? h.syncing : h.syncNow}
                </button>
              </div>
            )}
            {game?.replaced && (
              <div className="hh-row">
                <span className="hh-fact">
                  <Icon name="restore" size={19} />
                  <DeviceChip name={game.replaced.device} />
                  <When at={game.replaced.at} />
                </span>
                <button data-nav onClick={undo} disabled={busy || game.running}>
                  {t.saves.undo}
                </button>
              </div>
            )}
          </div>
        </div>

        {members.length > 1 && (
          <section>
            <h2>{h.versions}</h2>
            <div className="hh-row">
              {members.map((m) => (
                <button
                  key={m.key}
                  data-nav
                  className={`hh-chip${m.key === gameKey ? " on" : ""}${m.playable ? "" : " missing"}`}
                  onClick={() => onOpen(m.key)}
                  title={m.cloud ? t.savelib.cloudOnly : m.playable ? t.savelib.here : t.savelib.notInstalled}
                >
                  <PlatformMark mark={m.source.logo ? { label: variantLabel(m), logo: m.source.logo } : null} label={variantLabel(m)} size={22} />
                  {m.cloud && <Icon name="cloud" size={18} />}
                </button>
              ))}
            </div>
          </section>
        )}

        <section>
          <h2 className="hh-source-head">
            {groups.length > 0 ? (
              <>
                <PlatformMark mark={marks[sources?.herePlatform ?? ""]} label={t.sources.hereName(sources?.hereName ?? "")} size={24} strong />
                <DeviceChip name={overview.deviceName} here />
              </>
            ) : (
              h.history
            )}
          </h2>
          {data === null && <Spinner />}
          {data?.snapshots.length === 0 && <p className="muted">{t.saves.empty}</p>}
          <div className="hh-list">
            {top.map((r) => row(r.s))}
            {auto.length > 0 && (
              <button data-nav className="hh-list-row" onClick={() => setAutoOpen(!autoOpen)}>
                <span className="muted">
                  {autoOpen ? "▾" : "▸"} {t.saves.auto(auto.length)}
                </span>
              </button>
            )}
            {autoOpen && auto.map((r) => row(r.s))}
          </div>
        </section>

        {groups.map((src) => (
          <section key={src.id}>
            <h2 className="hh-source-head">
              {src.kind === "device" ? <DeviceChip name={src.name} /> : <PlatformMark mark={marks[src.platform]} label={src.name} size={24} strong />}
              {src.kind !== "version" && <span className="line-tag">{src.kind === "device" ? t.sources.ownTag : t.sources.sharedTag}</span>}
              {src.devices
                .filter((d) => src.kind !== "device" || d !== src.name)
                .map((d) => (
                  <DeviceChip key={d} name={d} />
                ))}
              {src.take.kind === "convert" && <ConvertChip hint={t.sources.convertHint(src.take.how)} />}
            </h2>
            <div className="hh-list">
              {src.snapshots.map((s) => {
                const named = s.locked && !!s.comment;
                const version = versionOf(s);
                return (
                  <button key={s.id} data-nav className="hh-list-row hh-save" onClick={() => setPickedElsewhere({ source: src, snapshot: s })}>
                    <PlatformMark mark={marks[s.platform ?? ""]} size={22} />
                    <span className="hh-save-when">
                      {named ? <b>{s.comment}</b> : <When at={s.createdAt} strong />}
                      {named && <When at={s.createdAt} />}
                      {version && <span className="version-tag">{version}</span>}
                    </span>
                    <DeviceChip name={s.deviceName} />
                    <span className="hh-save-mark">
                      {s.incomplete ? <Pill tone="warn">{t.saves.incomplete}</Pill> : <Delta cmp={cmpOf(s, here)} />}
                      <PlayDelta secs={playedOf(s, here)} quiet />
                    </span>
                    <span className="muted hh-save-size">{bytes(s.totalSize)}</span>
                  </button>
                );
              })}
            </div>
          </section>
        ))}
      </main>
      <Hints items={[["a", h.select], ...(game ? ([["y", h.syncNow]] as Array<[Button, string]>) : []), ["b", h.back]]} />

      {picked && (
        <Dialog
          title={h.versionActions(picked.deviceName, dateTime(picked.createdAt))}
          onClose={() => setPicked(null)}
          buttons={[
            ...(game && !isCurrent(picked)
              ? [{ b: "a" as Button, label: t.saves.makeCurrent, onPress: () => makeCurrent(picked), disabled: game.running || !!picked.incomplete }]
              : []),
            {
              b: "x",
              label: t.saves.sendTo,
              onPress: () => (setApplying({ source: gameKey, sourceName: name, snapshot: picked }), setPicked(null)),
              disabled: !!picked.incomplete,
            },
            { b: "b", label: h.cancel, onPress: () => setPicked(null) },
          ]}
        >
          <SaveCard side={snapSide(picked.locked && picked.comment ? picked.comment : variantLabel(members.find((m) => m.key === gameKey) ?? members[0]), picked, isCurrent(picked) ? null : here, marks, overview.deviceId)} />
          {picked.incomplete && <p className="muted">{t.saves.incompleteHint}</p>}
          {game && !isCurrent(picked) && !picked.incomplete && (
            <p className="hh-caption">
              <Icon name="shield" size={20} />
              {t.kit.keptFirst}
            </p>
          )}
          {game && !isCurrent(picked) && (data?.tooNew ?? []).includes(picked.id) && <p className="muted">{t.saves.tooNew(picked.gameVersion ?? "?")}</p>}
          {game && !isCurrent(picked) && note && <p className="muted">{note}</p>}
        </Dialog>
      )}
      {pickedElsewhere && (
        <ElsewhereDialog
          picked={pickedElsewhere}
          here={here}
          hereName={sources?.hereName ?? name}
          marks={marks}
          deviceId={overview.deviceId}
          game={game}
          note={note}
          tooNew={(sources?.tooNew ?? []).includes(pickedElsewhere.snapshot.id)}
          busy={busy}
          onTake={() => takeHere(pickedElsewhere.source, pickedElsewhere.snapshot)}
          onSend={(src) => (setApplying({ source: src, sourceName: pickedElsewhere.source.name, snapshot: pickedElsewhere.snapshot }), setPickedElsewhere(null))}
          onClose={() => setPickedElsewhere(null)}
        />
      )}
      {applying && (
        <Apply
          overview={overview}
          {...applying}
          onClose={() => setApplying(null)}
          onDone={async (target) => {
            setApplying(null);
            await onChanged();
            if (target !== gameKey) onOpen(target);
            else load();
          }}
        />
      )}
    </>
  );
}

/** A save as one side of a comparison: `other` is what it is compared with. */
function snapSide(name: string, s: SnapshotSummary, other: SnapshotSummary | null | undefined, marks: Record<string, Mark>, deviceId: string, newer?: boolean): Side {
  return {
    name,
    mark: marks[s.platform ?? ""],
    device: s.deviceName,
    mine: s.deviceId === deviceId,
    at: s.createdAt,
    playtime: s.playtime,
    played: playedOf(s, other),
    size: s.totalSize,
    files: s.fileCount,
    newer,
  };
}

/**
 * A save of another source, picked on the game screen: A takes it here
 * (kept first, can be undone), X sends it to another version. What it is
 * compared with the save here, and why it can't be taken when it can't.
 */
function ElsewhereDialog({
  picked: { source, snapshot },
  here,
  hereName,
  marks,
  deviceId,
  game,
  note,
  tooNew,
  busy,
  onTake,
  onSend,
  onClose,
}: {
  picked: { source: SaveSource; snapshot: SnapshotSummary };
  here?: SnapshotSummary;
  hereName: string;
  marks: Record<string, Mark>;
  deviceId: string;
  game?: { name: string; running: boolean };
  note: string | null;
  tooNew: boolean;
  busy: boolean;
  onTake: () => void;
  onSend: (key: string) => void;
  onClose: () => void;
}) {
  const t = useT();
  const never = source.take.kind === "never" ? source.take.reason : null;
  const same = !!here && snapshot.contentHash === here.contentHash;
  return (
    <Dialog
      title={t.handheld.versionActions(snapshot.deviceName, dateTime(snapshot.createdAt))}
      onClose={onClose}
      buttons={[
        ...(game && !same
          ? [{ b: "a" as Button, label: t.sources.takeHere, onPress: onTake, disabled: busy || game.running || !!snapshot.incomplete || !!never }]
          : []),
        ...(source.key ? [{ b: "x" as Button, label: t.saves.sendTo, onPress: () => onSend(source.key!), disabled: !!snapshot.incomplete }] : []),
        { b: "b", label: t.handheld.cancel, onPress: onClose },
      ]}
    >
      <SaveCompare
        there={snapSide(source.name, snapshot, here, marks, deviceId, !!here && Date.parse(snapshot.createdAt) > Date.parse(here.createdAt))}
        here={here ? snapSide(hereName, here, null, marks, deviceId) : { name: hereName, empty: true }}
      />
      {source.take.kind === "convert" && (
        <div className="hh-row">
          <ConvertChip hint={t.sources.convertHint(source.take.how)} />
        </div>
      )}
      {never && <p className="hh-warn">{never}</p>}
      {snapshot.incomplete && <p className="muted">{t.saves.incompleteHint}</p>}
      {game && !same && !never && !snapshot.incomplete && (
        <p className="hh-caption">
          <Icon name="shield" size={20} />
          {t.kit.keptFirst}
        </p>
      )}
      {game && tooNew && <p className="muted">{t.saves.tooNew(snapshot.gameVersion ?? "?")}</p>}
      {game && !source.key && note && <p className="muted">{note}</p>}
    </Dialog>
  );
}

/** Apply a save version to a game on this device: pick it, look, confirm. */
function Apply({
  overview,
  source,
  sourceName,
  snapshot,
  target: preset,
  onClose,
  onDone,
}: {
  overview: Overview;
  source: string;
  sourceName: string;
  snapshot: SnapshotSummary;
  target?: string;
  onClose: () => void;
  onDone: (target: string) => Promise<void>;
}) {
  const t = useT();
  const h = t.handheld;
  const s = t.savelib;
  const ui = useUi();
  const [target, setTarget] = useState<string | null>(preset ?? null);
  const [plan, setPlan] = useState<TransferPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const family = familyOf(overview, source).map((m) => m.key);
  const targets = overview.games.filter((g) => g.key !== source).sort((a, b) => Number(family.includes(b.key)) - Number(family.includes(a.key)));
  const tg = overview.games.find((g) => g.key === target);

  useEffect(() => {
    if (!target) return;
    setPlan(null);
    setError(null);
    api.transferPlan(source, snapshot.id, target, null).then(setPlan, (e) => setError(errorText(e)));
  }, [source, snapshot.id, target]);

  const apply = async () => {
    if (!tg) return;
    try {
      const o = await api.transferApply(source, snapshot.id, tg.key, null);
      ui.toast(o.kind === "uploaded" ? msg().handheld.applyDone(tg.name) : outcomeText(o, tg.name));
      await onDone(tg.key);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  if (!tg) {
    return (
      <Dialog title={h.applyPick} onClose={onClose} buttons={[{ b: "b", label: h.cancel, onPress: onClose }]}>
        <div className="hh-list hh-pick">
          {targets.map((g) => (
            <button key={g.key} data-nav className="hh-list-row" onClick={() => setTarget(g.key)}>
              <b>{g.name}</b>
              <span className="muted">{variantLabel({ key: g.key, name: g.name, game: g, playable: g.playable, source: g.source, family: g.family })}</span>
            </button>
          ))}
        </div>
      </Dialog>
    );
  }

  const blocked = !plan || plan.needsFolder || plan.items.length === 0;
  return (
    <Dialog
      title={s.applyTitle(sourceName)}
      onClose={onClose}
      buttons={[
        { b: "a", label: plan ? h.applyConfirm(plan.items.length) : h.apply, onPress: apply, disabled: blocked },
        { b: "b", label: h.cancel, onPress: onClose },
      ]}
    >
      <p>
        → <b>{tg.name}</b>
      </p>
      {error && <p className="hh-error">{error}</p>}
      {!plan && !error && <p className="muted">{h.planning}</p>}
      {plan && (
        <>
          <p className="muted">{plan.same ? s.modeSame : plan.toXbox ? s.modeToXbox : plan.fromXbox ? s.modeFromXbox(plan.xboxRule) : s.modeFiles}</p>
          {plan.toXbox && <p className="hh-warn">{s.xboxSteps}</p>}
          {plan.needsFolder ? <p className="hh-warn">{s.needsFolder(tg.name)}</p> : <p className="muted">{s.matched(plan.matched, plan.items.length)}</p>}
          {!plan.same && !plan.needsFolder && plan.matched === 0 && plan.items.length > 0 && <p className="hh-warn">{s.noneMatched}</p>}
          {plan.unmatched.length > 0 && <p className="hh-warn">{s.unmatched(plan.unmatched.length)}</p>}
          <p className="muted small">{s.safe}</p>
        </>
      )}
    </Dialog>
  );
}

function Settings({ overview, onTab, onDesktop }: { overview: Overview; onTab: (s: "library" | "settings") => void; onDesktop: () => void }) {
  const t = useT();
  const h = t.handheld;
  const ui = useUi();
  const [info, setInfo] = useState<HandheldInfo | null>(null);
  const [asking, setAsking] = useState(false);
  const load = () => api.handheldInfo().then(setInfo, () => undefined);
  useEffect(() => {
    load();
    const id = requestAnimationFrame(focusFirst);
    return () => cancelAnimationFrame(id);
  }, []);
  useButtons({ lb: () => onTab("library"), rb: () => onTab("library"), b: () => onTab("library") });

  const setMode = async (m: HandheldMode) => {
    await api.setHandheldMode(m);
    load();
  };

  const addToSteam = async () => {
    setAsking(false);
    try {
      const accounts = await api.steamAccounts();
      if (!accounts.selected) {
        ui.toast(msg().handheld.steamNone, "bad");
        return;
      }
      await api.steamAddLudonimbus(accounts.selected, true);
      ui.toast(msg().handheld.steamAdded);
      load();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <>
      <main className="hh-main hh-settings">
        <section>
          <h2>{h.cloud}</h2>
          <p>{overview.cloud.kind === "none" ? h.noCloud : overview.cloud.label}</p>
          <p className="muted">{h.device(overview.deviceName)}</p>
        </section>
        <section>
          <h2>{h.promptsTitle}</h2>
          <p className="muted">{h.promptsText}</p>
          <div className="hh-list">
            {(["auto", "on", "off"] as HandheldMode[]).map((m, i) => (
              <button key={m} data-nav data-autofocus={i === 0 || undefined} className={`hh-list-row${info?.mode === m ? " on" : ""}`} onClick={() => setMode(m)}>
                {info?.mode === m ? "● " : "○ "}
                {h.modes[m]}
              </button>
            ))}
          </div>
        </section>
        <section>
          <h2>{h.steamTitle}</h2>
          <p className="muted">{info?.inSteam ? h.steamIn : h.steamNot}</p>
          {!info?.inSteam && (
            <button data-nav onClick={() => setAsking(true)}>
              {h.steamAdd}
            </button>
          )}
        </section>
        <section className="hh-row">
          <button data-nav onClick={onDesktop}>
            {h.toDesktop}
          </button>
          <button data-nav onClick={() => api.quitHandheld().then(onDesktop)}>
            {h.quit}
          </button>
        </section>
      </main>
      <Hints items={[["a", h.select], ["lb", h.library], ["b", h.back]]} />
      {asking && (
        <Dialog
          title={h.steamAdd}
          onClose={() => setAsking(false)}
          buttons={[
            { b: "a", label: h.steamAdd, onPress: addToSteam },
            { b: "b", label: h.cancel, onPress: () => setAsking(false) },
          ]}
        >
          <p>{h.steamAddConfirm}</p>
        </Dialog>
      )}
    </>
  );
}
