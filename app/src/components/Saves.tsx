// The "Saves" tab of a game (docs/plan-v2.md, 10.2): the save this device
// is at, the newest one of each platform, the ones the user keeps and the
// newest of every other save line come first; everything else is folded
// away under "automatic saves". One action per row: make it the current one.
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, errorText, GameView, OtherSave, Overview, PlatformMark, PlatformMarks, SnapshotList, SnapshotMeta, SnapshotSummary, TrashedSnapshot } from "../api";
import { bytes, dateTime, outcomeText, relTime, triggerLabel } from "../format";
import { msg, useT } from "../i18n";
import { ApplyDialog, membersOf } from "./SaveLibrary";
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
  /** It is the newest save of another save line. */
  other?: OtherSave;
}

/** What the saves were made on: the program's own logo when it is on this PC, and its name. */
export function PlatformTag({ mark }: { mark?: PlatformMark }) {
  if (!mark) return <span className="platform-tag" />;
  return (
    <span className="platform-tag">
      {mark.logo && <img className="src-logo" src={mark.logo} alt="" width={15} height={15} />}
      {mark.label}
    </span>
  );
}

/** The rows shown first, and the ones folded away. */
export function arrange(data: SnapshotList, others: OtherSave[], cloudOnly: boolean): { top: Row[]; auto: Row[]; current: string | null } {
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
  for (const o of others) top.push({ s: o.snapshot, other: o });
  return { top, auto: list.filter((s) => !shown.has(s.id)).map((s) => ({ s })), current };
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
  onChanged,
  onApplied,
}: {
  /** `xboxContainers`: its saves are written into the Xbox app's containers, which is experimental. */
  game: { key: string; name: string; store?: GameView["store"]; xboxContainers?: boolean };
  overview: Overview;
  /** The game is running: nothing that changes its saves. */
  disabled: boolean;
  cloudOnly?: boolean;
  onChanged: () => Promise<void>;
  /** After saves were sent to `target` (another version of the game). */
  onApplied?: (target: string) => Promise<void>;
}) {
  const ui = useUi();
  const t = useT();
  const v = t.saves;
  const [data, setData] = useState<SnapshotList | null>(null);
  const [others, setOthers] = useState<{ saves: OtherSave[]; platforms: PlatformMarks; tooNew?: string[] }>({ saves: [], platforms: {} });
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [meta, setMeta] = useState<Record<string, SnapshotMeta>>({});
  const [sending, setSending] = useState<SnapshotSummary | null>(null);
  const [autoOpen, setAutoOpen] = useState(false);
  const [trashOpen, setTrashOpen] = useState(false);
  const [trash, setTrash] = useState<TrashedSnapshot[] | null>(null);

  const load = useCallback(async () => {
    try {
      setData(await api.snapshots(game.key));
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }, [game.key]);

  useEffect(() => {
    load();
    // The other lines are an extra: the tab works without them.
    if (!cloudOnly) api.otherSaves(game.key).then(setOthers, () => undefined);
  }, [load, game.key, cloudOnly]);

  const loadTrash = useCallback(async () => {
    try {
      setTrash(await api.deletedSnapshots(game.key));
    } catch (e) {
      setTrash([]);
      ui.toast(errorText(e), "bad");
    }
  }, [game.key, ui]);

  const { top, auto, current } = useMemo(
    () => (data ? arrange(data, others.saves, !!cloudOnly) : { top: [], auto: [], current: null }),
    [data, others, cloudOnly],
  );
  const marks = useMemo(() => ({ ...others.platforms, ...(data?.platforms ?? {}) }), [data, others]);
  const canSend = membersOf(overview).length > 1;

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
  const makeCurrent = async ({ s, other }: Row) => {
    const text = v.makeCurrentText(game.name, s.deviceName, dateTime(s.createdAt));
    // Steam Cloud keeps this game's saves too, and will notice they changed.
    const steamToo = !!game.store && game.store.mode !== "managed";
    // Writing into the Xbox app's containers is experimental.
    const note = steamToo ? t.steamCloud.restoreNote : game.xboxContainers ? v.xboxNote : null;
    // Saved on another platform by a newer game than the one here.
    const tooNew = (data?.tooNew ?? []).includes(s.id) || (others.tooNew ?? []).includes(s.id) ? v.tooNew(s.gameVersion ?? "?") : null;
    const ok = await ui.confirm({
      title: v.makeCurrentTitle,
      text:
        note || tooNew ? (
          <>
            <p>{text}</p>
            {tooNew && <p className="muted">{tooNew}</p>}
            {note && <p className="muted">{note}</p>}
          </>
        ) : (
          text
        ),
      confirm: v.makeCurrent,
    });
    if (!ok) return;
    try {
      const o = other ? await api.takeSave(game.key, other.line, s.id) : await api.restore(game.key, s.id);
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
  if (data.snapshots.length === 0 && others.saves.length === 0) return <Empty icon="cloud" title={v.empty} />;

  const more = (s: SnapshotSummary, isHead: boolean): MenuItem[] => {
    const items: MenuItem[] = [];
    if (!cloudOnly) items.push({ label: v.rename, icon: "edit", onClick: () => rename(s) });
    items.push({ label: open === s.id ? v.hideFiles : v.showFiles, icon: "folder", onClick: () => toggle(s.id) });
    items.push({ label: v.export, icon: "download", onClick: () => exportZip(s) });
    if (canSend) items.push({ label: v.sendTo, icon: "transfer", onClick: () => setSending(s) });
    if (!cloudOnly) {
      const why = s.locked ? v.deleteKept : isHead ? v.deleteNewest : undefined;
      items.push({ label: v.delete, icon: "trash", danger: true, divider: true, disabled: !!why, title: why, onClick: () => remove(s) });
    }
    return items;
  };

  // A save with the same content as the one this PC is at is that save.
  const currentHash = data.snapshots.find((s) => s.id === current)?.contentHash;
  const row = ({ s, other }: Row) => {
    const isCurrent = !other && (s.id === current || (!!currentHash && s.contentHash === currentHash));
    const isHead = !other && (s.head ?? s.id === data.headId);
    const mine = s.deviceId === overview.deviceId;
    const named = s.locked && !!s.comment;
    const id = other ? `${other.line}/${s.id}` : s.id;
    return (
      <div key={id} className={`save${isSide(s) && !isShared(s) ? " side" : ""}${open === s.id && !other ? " open" : ""}`}>
        <div className="save-row">
          <button className="save-main" onClick={() => !other && toggle(s.id)} disabled={!!other} title={`${dateTime(s.createdAt)} · ${triggerLabel(s.trigger)}`}>
            <PlatformTag mark={marks[s.platform ?? ""]} />
            <span className="save-when">
              <b>{named ? s.comment : relTime(s.createdAt)}</b>
              {(named || s.comment) && <span className="muted">{named ? relTime(s.createdAt) : s.comment}</span>}
              {other && <Badge tone="muted">{other.shared ? v.sharedLine : other.name || v.ownLine}</Badge>}
              {isSide(s) && !isShared(s) && <Badge tone="muted">{v.replaced}</Badge>}
              {isHead && !isCurrent && !cloudOnly && <Badge tone="info">{v.newest}</Badge>}
            </span>
            <span className={`device${mine ? " mine" : ""}`}>{s.deviceName}</span>
            <span className="muted save-size">{bytes(s.totalSize)}</span>
          </button>
          <div className="save-actions">
            {isCurrent ? (
              <Badge tone="ok">{v.current}</Badge>
            ) : (
              !cloudOnly && (
                <Button small onClick={() => makeCurrent({ s, other })} disabled={disabled} title={disabled ? v.running : undefined}>
                  {v.makeCurrent}
                </Button>
              )
            )}
            {!other && !cloudOnly && (
              <button
                className={`icon-btn star${s.locked ? " on" : ""}`}
                title={s.locked ? v.unkeep : v.keep}
                aria-pressed={s.locked}
                onClick={() => act(() => api.setSnapshot(game.key, s.id, { locked: !s.locked }), s.locked ? msg().saves.unkept : msg().saves.kept)}
              >
                <Icon name="star" size={16} />
              </button>
            )}
            {!other && <Menu title={v.more} items={more(s, isHead)} />}
          </div>
        </div>
        {open === s.id && !other && (
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

  return (
    <div className="saves">
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
                  <PlatformTag mark={marks[d.snapshot.platform ?? ""]} />
                  <span className="save-when">
                    <b>{relTime(d.snapshot.createdAt)}</b>
                    <Badge tone="muted">{v.daysLeft(daysLeft(d))}</Badge>
                  </span>
                  <span className="device">{d.snapshot.deviceName}</span>
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
      {sending && (
        <ApplyDialog
          overview={overview}
          sourceKey={game.key}
          sourceName={game.name}
          snapshot={sending}
          onClose={() => setSending(null)}
          onApplied={async (target) => {
            setSending(null);
            await onApplied?.(target);
          }}
        />
      )}
    </div>
  );
}
