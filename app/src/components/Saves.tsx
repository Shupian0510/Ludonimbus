// The "Saves" tab of a game (docs/plan-v2.md, 10.2), grouped by where the
// saves come from (docs/plan-positioning.md §7, S1). First "here": the save
// this device is at, the newest one of each platform and the ones the user
// keeps, everything else folded away under "automatic saves". Then one group
// per other source on the game's card (another version, the Switch, an
// emulator, a device that keeps its own), each save compared with the one
// here. One action per row: make it the current one, or take it here.
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, errorText, GameView, Overview, PlatformMark as Mark, SaveSource, SaveSources, SnapshotList, SnapshotMeta, SnapshotSummary, TrashedSnapshot, UnsentSave } from "../api";
import { bytes, dateTime, outcomeText, triggerLabel } from "../format";
import { msg, useT } from "../i18n";
import { cmpOf, Delta, DeviceChip, PlatformMark, PlayDelta, playedOf, SourceHead, When } from "./kit";
import { ApplyDialog, membersOf } from "./SaveLibrary";
import { sourceWhat, useTakeHere, versionOf } from "./SaveSources";
import { Badge, Button, Empty, Hint, Icon, Menu, MenuItem, useUi } from "./ui";

/** Days a deleted save stays in 「最近删除」 (cloud::TRASH_DAYS). */
const TRASH_DAYS = 30;

function daysLeft(d: TrashedSnapshot): number {
  return Math.max(0, TRASH_DAYS - Math.floor((Date.now() - Date.parse(d.deletedAt)) / 86_400_000));
}

/** Kept beside the history: the saves a restore replaced, the losing side of a conflict. */
const isSide = (s: SnapshotSummary) => s.side ?? (s.trigger === "pre-restore" || s.trigger === "conflict-keep");
/** A save someone else exported, taken in here: beside the history as well, but nothing it replaced. Its device column says so. */
const isShared = (s: SnapshotSummary) => s.trigger === "shared";

interface Row {
  s: SnapshotSummary;
}

/** The rows of the saves here shown first, and the ones folded away. */
export function arrange(data: SnapshotList, cloudOnly: boolean): { top: Row[]; auto: Row[]; current: string | null } {
  const list = data.snapshots;
  const current = cloudOnly ? null : (list.find((s) => s.id === data.lastSyncedId)?.id ?? null);
  const top: Row[] = [];
  const shown = new Set<string>();
  const add = (s: SnapshotSummary) => {
    if (shown.has(s.id)) return;
    shown.add(s.id);
    top.push({ s });
  };
  const at = list.find((s) => s.id === current);
  if (at) add(at);
  // The newest of each platform; the line's newest save is one of them.
  const platforms = new Set<string>();
  for (const s of list) {
    const p = s.platform ?? "";
    if (isSide(s) || platforms.has(p)) continue;
    platforms.add(p);
    add(s);
  }
  for (const s of list) if (s.locked) add(s);
  return { top, auto: list.filter((s) => !shown.has(s.id)).map((s) => ({ s })), current };
}

/** A game's list of saves, read when `key` or `stamp` changes (not at all while `skip`). */
export interface SnapshotsState {
  data: SnapshotList | null;
  error: string | null;
  reload: () => Promise<void>;
}

export function useSnapshotList(key: string, stamp: unknown = 0, skip = false): SnapshotsState {
  const [data, setData] = useState<SnapshotList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    try {
      setData(await api.snapshots(key));
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }, [key]);
  useEffect(() => {
    if (!skip) reload();
  }, [reload, stamp, skip]);
  return { data, error, reload };
}

/** The save this device is at, else the newest one that isn't kept beside the history. */
export function currentOf(data: SnapshotList | null): SnapshotSummary | null {
  if (!data) return null;
  return data.snapshots.find((s) => s.id === data.lastSyncedId) ?? data.snapshots.find((s) => !isSide(s)) ?? null;
}

/**
 * A game's saves in the cloud. `cloudOnly`: a game this PC doesn't have
 * (only exporting and sending to another version work).
 */
export function Saves({
  game,
  overview,
  disabled,
  cloudOnly,
  sources,
  onChanged,
  onApplied,
  list,
}: {
  /** `xboxContainers`: its saves are written into the Xbox app's containers, which is experimental. */
  game: { key: string; name: string; store?: GameView["store"]; xboxContainers?: boolean };
  overview: Overview;
  /** The game is running: nothing that changes its saves. */
  disabled: boolean;
  cloudOnly?: boolean;
  /** The other sources of the game's saves (read by the game page, which shows the hint row from them too). An extra: the tab works without. */
  sources?: SaveSources | null;
  onChanged: () => Promise<void>;
  /** After saves were sent to `target` (another version of the game). */
  onApplied?: (target: string) => Promise<void>;
  /** The list, when the page reads it already (the game page shows the current save at its top). */
  list?: SnapshotsState;
}) {
  const ui = useUi();
  const t = useT();
  const v = t.saves;
  const own = useSnapshotList(game.key, 0, !!list);
  const { data, error, reload: load } = list ?? own;
  const [open, setOpen] = useState<string | null>(null);
  const [meta, setMeta] = useState<Record<string, SnapshotMeta>>({});
  const [sending, setSending] = useState<{ key: string; name: string; snapshot: SnapshotSummary } | null>(null);
  const [autoOpen, setAutoOpen] = useState(false);
  const [trashOpen, setTrashOpen] = useState(false);
  const [trash, setTrash] = useState<TrashedSnapshot[] | null>(null);
  const groups = cloudOnly ? [] : (sources?.sources ?? []);
  const tooNew = useMemo(() => [...(data?.tooNew ?? []), ...(sources?.tooNew ?? [])], [data, sources]);
  const takeHere = useTakeHere(game, overview, onChanged, tooNew);

  const loadTrash = useCallback(async () => {
    try {
      setTrash(await api.deletedSnapshots(game.key));
    } catch (e) {
      setTrash([]);
      ui.toast(errorText(e), "bad");
    }
  }, [game.key, ui]);

  const { top, auto, current } = useMemo(() => (data ? arrange(data, !!cloudOnly) : { top: [], auto: [], current: null }), [data, cloudOnly]);
  const marks: Record<string, Mark> = useMemo(() => ({ ...(sources?.platforms ?? {}), ...(data?.platforms ?? {}) }), [data, sources]);
  // Also with one game in the library: the dialog offers the game's versions installed here that aren't added yet.
  const canSend = membersOf(overview).length > 1 || !cloudOnly;

  const toggle = async (id: string) => {
    if (open === id) return setOpen(null);
    setOpen(id);
    if (!meta[id]) {
      try {
        const m = await api.snapshot(game.key, id);
        setMeta((x) => ({ ...x, [id]: m }));
      } catch (e) {
        ui.toast(errorText(e), "bad");
      }
    }
  };

  const act = async (f: () => Promise<unknown>, ok?: string) => {
    try {
      await f();
      if (ok) ui.toast(ok);
      await load();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const undo = async () => {
    try {
      await api.undoReplace(game.key);
      ui.toast(msg().saves.undone);
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  /** The one action that replaces the saves on this PC, so the one that asks first. */
  const makeCurrent = async ({ s }: Row) => {
    const text = v.makeCurrentText(game.name, s.deviceName, dateTime(s.createdAt));
    // Steam Cloud keeps this game's saves too, and will notice they changed.
    const steamToo = !!game.store && game.store.mode !== "managed";
    // Writing into the Xbox app's containers is experimental.
    const note = steamToo ? t.steamCloud.restoreNote : game.xboxContainers ? v.xboxNote : null;
    // Saved on another platform by a newer game than the one here.
    const tooNewText = tooNew.includes(s.id) ? v.tooNew(s.gameVersion ?? "?") : null;
    const ok = await ui.confirm({
      title: v.makeCurrentTitle,
      text:
        note || tooNewText ? (
          <>
            <p>{text}</p>
            {tooNewText && <p className="muted">{tooNewText}</p>}
            {note && <p className="muted">{note}</p>}
          </>
        ) : (
          text
        ),
      confirm: v.makeCurrent,
    });
    if (!ok) return;
    try {
      const o = await api.restore(game.key, s.id);
      const done = o.kind === "pulled" || o.kind === "uploaded" || o.kind === "queued";
      const m = msg().saves;
      ui.toast(done ? m.madeCurrent : outcomeText(o, game.name), done ? "ok" : "info", done ? { label: m.undo, run: undo } : undefined);
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const rename = async (s: SnapshotSummary) => {
    const name = await ui.prompt({ title: v.renameTitle, text: v.renameText, value: s.locked ? s.comment : "", placeholder: v.renamePlaceholder });
    if (name === null) return;
    await act(() => api.setSnapshot(game.key, s.id, { comment: name, locked: true }), msg().saves.kept);
  };

  const remove = async (s: SnapshotSummary) => {
    const ok = await ui.confirm({ title: v.deleteTitle, text: v.deleteText(s.deviceName, dateTime(s.createdAt)), confirm: v.delete, danger: true });
    if (!ok) return;
    await act(() => api.deleteSnapshot(game.key, s.id), msg().saves.deleted);
    if (trashOpen) await loadTrash();
  };

  const exportZip = async (s: SnapshotSummary) => {
    try {
      const p = await api.exportSnapshot(game.key, s.id);
      if (p) ui.toast(msg().saves.exported(p));
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  /** A version this PC couldn't upload complete: what it has of it, as a zip. */
  const exportUnsent = async (u: UnsentSave) => {
    try {
      const p = await api.exportUnsent(game.key, u.id);
      if (p) ui.toast(msg().saves.exported(p));
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const dismissUnsent = (u: UnsentSave) => act(() => api.dismissUnsent(game.key, u.id));

  /** Take in a save someone exported: it shows among the kept ones, and nothing else changes. */
  const importShared = async () => {
    try {
      if ((await api.importSharedSave(game.key)) === null) return;
      ui.toast(msg().saves.imported);
      await load();
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const toggleTrash = async () => {
    setTrashOpen(!trashOpen);
    if (!trashOpen) await loadTrash();
  };

  const restoreDeleted = async (d: TrashedSnapshot) => {
    try {
      await api.restoreDeletedSnapshot(game.key, d.snapshot.id);
      ui.toast(msg().saves.putBack);
      await Promise.all([load(), loadTrash()]);
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const purge = async (d: TrashedSnapshot) => {
    const ok = await ui.confirm({ title: v.purgeTitle, text: v.purgeText(d.snapshot.deviceName, dateTime(d.snapshot.createdAt)), confirm: v.purge, danger: true });
    if (!ok) return;
    try {
      await api.purgeDeletedSnapshot(game.key, d.snapshot.id);
      ui.toast(msg().saves.purged);
      await loadTrash();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  if (overview.cloud.kind === "none") return <Empty icon="cloud" title={v.noCloud} />;
  if (error) return <div className="banner banner-bad">{error}</div>;
  if (!data) return <div className="muted pad">{v.loading}</div>;
  const unsent = cloudOnly ? [] : (data.unsent ?? []);
  if (data.snapshots.length === 0 && groups.length === 0 && unsent.length === 0) return <Empty icon="cloud" title={v.empty} />;

  const more = (s: SnapshotSummary, isHead: boolean): MenuItem[] => {
    const items: MenuItem[] = [];
    // Nothing can be taken from a version that misses a pack.
    const missing = s.incomplete ? v.incompleteNoUse : undefined;
    if (!cloudOnly) items.push({ label: v.rename, icon: "edit", onClick: () => rename(s) });
    items.push({ label: open === s.id ? v.hideFiles : v.showFiles, icon: "folder", onClick: () => toggle(s.id) });
    items.push({ label: v.export, icon: "download", disabled: !!missing, title: missing, onClick: () => exportZip(s) });
    if (canSend) items.push({ label: v.sendTo, icon: "transfer", disabled: !!missing, title: missing, onClick: () => setSending({ key: game.key, name: game.name, snapshot: s }) });
    if (!cloudOnly) {
      // An incomplete newest version is nobody's current progress, and may go.
      const why = s.locked ? v.deleteKept : isHead && !s.incomplete ? v.deleteNewest : undefined;
      items.push({ label: v.delete, icon: "trash", danger: true, divider: true, disabled: !!why, title: why, onClick: () => remove(s) });
    }
    return items;
  };

  /** A version this PC made and couldn't upload complete: listed as this PC's, once, until dismissed. */
  const unsentRow = (u: UnsentSave) => (
    <div key={`unsent/${u.id}`} className="save side">
      <div className="save-row">
        <div className="save-main" title={dateTime(u.createdAt)}>
          <PlatformMark mark={marks[u.platform ?? ""]} />
          <span className="save-when">
            <When at={u.createdAt} strong />
            <Badge tone="warn">{v.unsent}</Badge>
            <Hint text={v.unsentHint} />
          </span>
          <DeviceChip name={overview.deviceName} here />
          <span />
          <span className="muted save-size">{bytes(u.totalSize)}</span>
        </div>
        <div className="save-actions">
          <Button small onClick={() => exportUnsent(u)}>
            {v.export}
          </Button>
          <button className="icon-btn" title={v.dismiss} onClick={() => dismissUnsent(u)}>
            <Icon name="x" size={16} />
          </button>
        </div>
      </div>
    </div>
  );

  // A save with the same content as the one this PC is at is that save.
  const here = data.snapshots.find((s) => s.id === current);
  const currentHash = here?.contentHash;
  const row = ({ s }: Row) => {
    const isCurrent = s.id === current || (!!currentHash && s.contentHash === currentHash);
    const isHead = s.head ?? s.id === data.headId;
    const mine = s.deviceId === overview.deviceId;
    const named = s.locked && !!s.comment;
    const id = s.id;
    const incomplete = !!s.incomplete;
    const main = (
      <>
        <PlatformMark mark={marks[s.platform ?? ""]} />
        <span className="save-when">
          {named ? <b>{s.comment}</b> : <When at={s.createdAt} strong />}
          {named ? <When at={s.createdAt} /> : s.comment && <span className="muted">{s.comment}</span>}
          {isSide(s) && !isShared(s) && <Badge tone="muted">{v.replaced}</Badge>}
          {incomplete && (
            <>
              <Badge tone="warn">{v.incomplete}</Badge>
              <Hint text={v.incompleteHint} />
            </>
          )}
        </span>
        <DeviceChip name={s.deviceName} here={mine} />
        <span className="save-delta">
          {/* Newer than where this device is (another device played on): how much. */}
          {!isCurrent && !cloudOnly && !isSide(s) && (
            <>
              <Delta cmp={cmpOf(s, here)} quiet />
              <PlayDelta secs={playedOf(s, here)} quiet />
            </>
          )}
        </span>
        <span className="muted save-size">{bytes(s.totalSize)}</span>
      </>
    );
    const title = `${dateTime(s.createdAt)} · ${triggerLabel(s.trigger)}`;
    return (
      <div key={id} className={`save${(isSide(s) && !isShared(s)) || incomplete ? " side" : ""}${open === s.id ? " open" : ""}`}>
        <div className="save-row">
          {incomplete ? (
            // Its "?" can't be inside a button; its files are in "More".
            <div className="save-main" title={title}>
              {main}
            </div>
          ) : (
            <button className="save-main" onClick={() => toggle(s.id)} title={title}>
              {main}
            </button>
          )}
          <div className="save-actions">
            {isCurrent ? (
              <Badge tone="ok">{v.current}</Badge>
            ) : (
              !cloudOnly && (
                <Button
                  small
                  onClick={() => makeCurrent({ s })}
                  disabled={disabled || incomplete}
                  title={incomplete ? v.incompleteNoUse : disabled ? v.running : undefined}
                >
                  {v.makeCurrent}
                </Button>
              )
            )}
            {!cloudOnly && (
              <button
                className={`icon-btn star${s.locked ? " on" : ""}`}
                title={s.locked ? v.unkeep : v.keep}
                aria-pressed={s.locked}
                onClick={() => act(() => api.setSnapshot(game.key, s.id, { locked: !s.locked }), s.locked ? msg().saves.unkept : msg().saves.kept)}
              >
                <Icon name="star" size={16} />
              </button>
            )}
            <Menu title={v.more} items={more(s, isHead)} />
          </div>
        </div>
        {open === s.id && (
          <div className="save-files">
            {!meta[s.id] ? (
              <span className="muted">{v.reading}</span>
            ) : (
              <table className="files">
                <tbody>
                  {meta[s.id].files.map((f) => (
                    <tr key={f.path}>
                      <td>
                        <code>{f.path}</code>
                      </td>
                      <td>{bytes(f.size)}</td>
                      <td>{f.mtime ? dateTime(f.mtime) : ""}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        )}
      </div>
    );
  };

  /** A save of another source: how it compares with the one here, and taking it here. */
  const sourceRow = (src: SaveSource, s: SnapshotSummary) => {
    const same = !!here && s.contentHash === here.contentHash;
    const named = s.locked && !!s.comment;
    const version = versionOf(s);
    const never = src.take.kind === "never" ? src.take.reason : null;
    const incomplete = !!s.incomplete;
    return (
      <div key={`${src.id}/${s.id}`} className={`save${incomplete ? " side" : ""}`}>
        <div className="save-row">
          <div className="save-main" title={`${dateTime(s.createdAt)}\n${triggerLabel(s.trigger)}`}>
            <PlatformMark mark={marks[s.platform ?? ""]} />
            <span className="save-when">
              {named ? <b>{s.comment}</b> : <When at={s.createdAt} strong />}
              {named && <When at={s.createdAt} />}
              {version && <span className="version-tag">{version}</span>}
              {incomplete && (
                <>
                  <Badge tone="warn">{v.incomplete}</Badge>
                  <Hint text={v.incompleteHint} />
                </>
              )}
            </span>
            <DeviceChip name={s.deviceName} here={s.deviceId === overview.deviceId} />
            <span className="save-delta">
              {!same && <Delta cmp={cmpOf(s, here)} />}
              {!same && <PlayDelta secs={playedOf(s, here)} />}
            </span>
            <span className="muted save-size">{bytes(s.totalSize)}</span>
          </div>
          <div className="save-actions">
            {same ? (
              <Badge tone="ok">{v.current}</Badge>
            ) : (
              <Button
                small
                onClick={() => takeHere.take(src, s)}
                disabled={disabled || incomplete || !!never}
                title={incomplete ? v.incompleteNoUse : disabled ? v.running : undefined}
              >
                {t.sources.takeHere}
              </Button>
            )}
            {never && <Hint text={never} />}
            {src.key && (
              <Menu
                title={v.more}
                items={[
                  {
                    label: v.sendTo,
                    icon: "transfer",
                    disabled: incomplete,
                    title: incomplete ? v.incompleteNoUse : undefined,
                    onClick: () => setSending({ key: src.key!, name: src.name, snapshot: s }),
                  },
                ]}
              />
            )}
          </div>
        </div>
      </div>
    );
  };

  // "Here" only needs saying when there are other groups.
  const newestHere = data.snapshots.find((s) => !isSide(s))?.createdAt;
  return (
    <div className="saves">
      {groups.length > 0 && sources && (
        <SourceHead
          mark={marks[sources.herePlatform]}
          name={t.sources.hereName(sources.hereName)}
          devices={[overview.deviceName]}
          mine={overview.deviceName}
          latest={newestHere}
          hint={t.sources.hereHint}
        />
      )}
      {unsent.map(unsentRow)}
      {top.map(row)}
      <div className="saves-foot">
        {auto.length > 0 && (
          <button className="link" onClick={() => setAutoOpen(!autoOpen)} aria-expanded={autoOpen}>
            <Icon name={autoOpen ? "down" : "chevron"} size={14} /> {v.auto(auto.length)}
          </button>
        )}
        {auto.length > 0 && <Hint text={v.autoHint} />}
        {!cloudOnly && (
          <span className="saves-more">
            <button className="link" onClick={importShared}>
              <Icon name="upload" size={14} /> {v.import}
            </button>
            <Hint text={v.importHint} />
            <button className="link" onClick={toggleTrash} aria-expanded={trashOpen}>
              <Icon name="trash" size={14} /> {trashOpen && trash ? v.trashCount(trash.length) : v.trash}
            </button>
          </span>
        )}
      </div>
      {autoOpen && auto.map(row)}
      {trashOpen &&
        (!trash ? (
          <span className="muted small">{v.reading}</span>
        ) : trash.length === 0 ? (
          <span className="muted small">{v.trashEmpty}</span>
        ) : (
          trash.map((d) => (
            <div key={d.snapshot.id} className="save side">
              <div className="save-row">
                <div className="save-main" title={v.deletedInfo(dateTime(d.deletedAt), d.deletedBy || "?")}>
                  <PlatformMark mark={marks[d.snapshot.platform ?? ""]} />
                  <span className="save-when">
                    <When at={d.snapshot.createdAt} strong />
                    {d.reason === "incomplete" ? (
                      // Why it is here comes first; how long it stays is in the "?".
                      <>
                        <Badge tone="warn">{v.retired}</Badge>
                        <Hint text={v.retiredHint(daysLeft(d))} />
                      </>
                    ) : (
                      <Badge tone="muted">{v.daysLeft(daysLeft(d))}</Badge>
                    )}
                  </span>
                  <DeviceChip name={d.snapshot.deviceName} here={d.snapshot.deviceId === overview.deviceId} />
                  <span />
                  <span className="muted save-size">{bytes(d.snapshot.totalSize)}</span>
                </div>
                <div className="save-actions">
                  <Button small onClick={() => restoreDeleted(d)}>
                    {v.putBackAction}
                  </Button>
                  <button className="icon-btn danger" title={v.purge} onClick={() => purge(d)}>
                    <Icon name="trash" size={16} />
                  </button>
                </div>
              </div>
            </div>
          ))
        ))}
      {groups.map((src) => (
        <div key={src.id} className="save-source">
          <SourceHead
            mark={marks[src.platform]}
            name={src.name}
            device={src.kind === "device"}
            devices={src.devices}
            latest={src.snapshots[0]?.createdAt}
            convert={src.take.kind === "convert" ? t.sources.convertHint(src.take.how) : null}
            tag={src.kind === "device" ? t.sources.ownTag : src.kind === "shared" ? t.sources.sharedTag : null}
            hint={sourceWhat(src)}
          />
          {src.snapshots.map((s) => sourceRow(src, s))}
        </div>
      ))}
      {sending && (
        <ApplyDialog
          overview={overview}
          sourceKey={sending.key}
          sourceName={sending.name}
          snapshot={sending.snapshot}
          onClose={() => setSending(null)}
          onAdded={onChanged}
          onApplied={async (target) => {
            setSending(null);
            if (target === game.key) await onChanged();
            else await onApplied?.(target);
          }}
        />
      )}
      {takeHere.dialog}
    </div>
  );
}
