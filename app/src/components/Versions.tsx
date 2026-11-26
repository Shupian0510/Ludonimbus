import { useCallback, useEffect, useState } from "react";
import { api, errorText, Overview, SnapshotMeta, SnapshotSummary, TrashedSnapshot } from "../api";
import { bytes, dateTime, outcomeText, relTime, triggerLabel } from "../format";
import { msg, useT } from "../i18n";
import { ApplyDialog } from "./SaveLibrary";
import { Badge, Button, Empty, Icon, useUi } from "./ui";

/** Days a deleted version stays in 「最近删除」 (cloud::TRASH_DAYS). */
const TRASH_DAYS = 30;

function daysLeft(d: TrashedSnapshot): number {
  return Math.max(0, TRASH_DAYS - Math.floor((Date.now() - Date.parse(d.deletedAt)) / 86_400_000));
}

/**
 * A game's save versions in the cloud. `cloudOnly`: a game this PC doesn't
 * have (only exporting and applying to another version work).
 */
export function Versions({
  game,
  overview,
  disabled,
  cloudOnly,
  onChanged,
  onApplied,
}: {
  game: { key: string; name: string };
  overview: Overview;
  disabled: boolean;
  cloudOnly?: boolean;
  onChanged: () => Promise<void>;
  /** After saves were applied to `target` (another version of the game). */
  onApplied?: (target: string) => Promise<void>;
}) {
  const ui = useUi();
  const t = useT();
  const v = t.versions;
  const [data, setData] = useState<{ snapshots: SnapshotSummary[]; headId: string | null; lastSyncedId: string | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [meta, setMeta] = useState<Record<string, SnapshotMeta>>({});
  const [applying, setApplying] = useState<SnapshotSummary | null>(null);
  const [trashOpen, setTrashOpen] = useState(false);
  const [trash, setTrash] = useState<TrashedSnapshot[] | null>(null);

  const loadTrash = useCallback(async () => {
    try {
      setTrash(await api.deletedSnapshots(game.key));
    } catch (e) {
      setTrash([]);
      ui.toast(errorText(e), "bad");
    }
  }, [game.key, ui]);

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
  }, [load]);

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

  const restore = async (s: SnapshotSummary) => {
    const ok = await ui.confirm({
      title: v.restoreTitle,
      text: (
        <>
          <p>{v.restoreText(game.name, s.deviceName, dateTime(s.createdAt))}</p>
          <p className="muted">{v.restoreSafe}</p>
        </>
      ),
      confirm: v.restore,
    });
    if (!ok) return;
    try {
      const o = await api.restore(game.key, s.id);
      ui.toast(outcomeText(o, game.name));
      await load();
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
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

  const editComment = async (s: SnapshotSummary) => {
    const c = await ui.prompt({ title: v.commentTitle, text: v.commentText, value: s.comment, placeholder: v.comment });
    if (c === null) return;
    await act(() => api.setSnapshot(game.key, s.id, { comment: c }), msg().versions.commentSaved);
  };

  const remove = async (s: SnapshotSummary) => {
    const ok = await ui.confirm({
      title: v.deleteTitle,
      text: v.deleteText(s.deviceName, dateTime(s.createdAt)),
      confirm: v.delete,
      danger: true,
    });
    if (!ok) return;
    await act(() => api.deleteSnapshot(game.key, s.id), msg().versions.deleted);
    if (trashOpen) await loadTrash();
  };

  const toggleTrash = async () => {
    setTrashOpen(!trashOpen);
    if (!trashOpen) await loadTrash();
  };

  const restoreDeleted = async (d: TrashedSnapshot) => {
    try {
      const locked = await api.restoreDeletedSnapshot(game.key, d.snapshot.id);
      ui.toast(locked ? msg().versions.restoredLocked : msg().versions.restoredDeleted);
      await Promise.all([load(), loadTrash()]);
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const purge = async (d: TrashedSnapshot) => {
    const ok = await ui.confirm({
      title: v.purgeTitle,
      text: v.purgeText(d.snapshot.deviceName, dateTime(d.snapshot.createdAt)),
      confirm: v.purge,
      danger: true,
    });
    if (!ok) return;
    try {
      await api.purgeDeletedSnapshot(game.key, d.snapshot.id);
      ui.toast(msg().versions.purged);
      await loadTrash();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const exportZip = async (s: SnapshotSummary) => {
    try {
      const p = await api.exportSnapshot(game.key, s.id);
      if (p) ui.toast(msg().versions.exported(p));
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  if (overview.cloud.kind === "none") {
    return (
      <Empty icon="cloud" title={v.noCloudTitle}>
        <p className="muted">{v.noCloudText}</p>
      </Empty>
    );
  }
  if (error) return <div className="banner banner-bad">{error}</div>;
  if (!data) return <div className="muted pad">{v.loading}</div>;
  if (data.snapshots.length === 0) {
    return (
      <Empty icon="cloud" title={v.emptyTitle}>
        <p className="muted">{v.emptyText}</p>
      </Empty>
    );
  }

  return (
    <div className="versions">
      {data.snapshots.map((s) => {
        const isHead = s.id === data.headId;
        const isCurrent = s.id === data.lastSyncedId;
        const mine = s.deviceId === overview.deviceId;
        const side = s.trigger === "pre-restore" || s.trigger === "conflict-keep";
        return (
          <div key={s.id} className={`version${side ? " side" : ""}${open === s.id ? " open" : ""}`}>
            <div className="version-row">
              <button className="version-main" onClick={() => toggle(s.id)}>
                <span className="dot" data-tone={isHead ? "head" : side ? "side" : "normal"} />
                <span className="version-time">
                  <b>{dateTime(s.createdAt)}</b>
                  <small className="muted">{relTime(s.createdAt)}</small>
                </span>
                <span className={`device${mine ? " mine" : ""}`}>
                  <Icon name="devices" size={14} />
                  {s.deviceName}
                  {mine ? v.mine : ""}
                </span>
                <span className="muted">{triggerLabel(s.trigger)}</span>
                <span className="muted">
                  {t.library.files(s.fileCount)} · {bytes(s.totalSize)}
                  {s.gameVersion ? (
                    <span title={v.gameVersionTitle}> · {v.gameVersion(s.gameVersion)}</span>
                  ) : s.installedVersion ? (
                    <span title={v.installedVersionTitle}> · {v.installedVersion(s.installedVersion)}</span>
                  ) : null}
                </span>
                <span className="tags">
                  {isHead && <Badge tone="info">{v.latest}</Badge>}
                  {isCurrent && <Badge tone="ok">{v.current}</Badge>}
                  {s.locked && <Badge tone="muted">{v.locked}</Badge>}
                </span>
              </button>
              <div className="version-actions">
                {!cloudOnly && (
                  <Button small icon="restore" onClick={() => restore(s)} disabled={disabled || isCurrent} title={isCurrent ? v.alreadyCurrent : v.restoreThis}>
                    {v.restore}
                  </Button>
                )}
                <button className="icon-btn" title={t.savelib.applyToTitle} onClick={() => setApplying(s)}>
                  <Icon name="transfer" size={16} />
                </button>
                <button className="icon-btn" title={v.exportZip} onClick={() => exportZip(s)}>
                  <Icon name="download" size={16} />
                </button>
                {!cloudOnly && (
                  <>
                    <button className="icon-btn" title={v.comment} onClick={() => editComment(s)}>
                      <Icon name="edit" size={16} />
                    </button>
                    <button
                      className="icon-btn"
                      title={s.locked ? v.unlockTitle : v.lockTitle}
                      onClick={() => act(() => api.setSnapshot(game.key, s.id, { locked: !s.locked }), s.locked ? v.unlocked : v.lockedToast)}
                    >
                      <Icon name={s.locked ? "lock" : "unlock"} size={16} />
                    </button>
                    <button className="icon-btn danger" title={v.delete} disabled={s.locked || isHead} onClick={() => remove(s)}>
                      <Icon name="trash" size={16} />
                    </button>
                  </>
                )}
              </div>
            </div>
            {s.comment && <div className="version-comment">{v.quoted(s.comment)}</div>}
            {open === s.id && (
              <div className="version-files">
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
      })}
      {!cloudOnly && (
        <div className="versions-trash">
          <button className="link small" onClick={toggleTrash} aria-expanded={trashOpen}>
            <Icon name="trash" size={14} /> {trashOpen && trash ? v.trashCount(trash.length) : v.trash}
          </button>
          {trashOpen && (
            <>
              <p className="muted small">{v.trashHint}</p>
              {!trash ? (
                <span className="muted small">{v.reading}</span>
              ) : trash.length === 0 ? (
                <span className="muted small">{v.trashEmpty}</span>
              ) : (
                trash.map((d) => (
                  <div key={d.snapshot.id} className="version side deleted">
                    <div className="version-row">
                      <div className="version-main">
                        <span className="dot" data-tone="side" />
                        <span className="version-time">
                          <b>{dateTime(d.snapshot.createdAt)}</b>
                          <small className="muted">{relTime(d.snapshot.createdAt)}</small>
                        </span>
                        <span className="device">
                          <Icon name="devices" size={14} />
                          {d.snapshot.deviceName}
                        </span>
                        <span className="muted">{triggerLabel(d.snapshot.trigger)}</span>
                        <span className="muted">
                          {t.library.files(d.snapshot.fileCount)} · {bytes(d.snapshot.totalSize)}
                        </span>
                        <span />
                      </div>
                      <div className="version-actions">
                        <Button small icon="restore" onClick={() => restoreDeleted(d)}>
                          {v.restoreDeleted}
                        </Button>
                        <button className="icon-btn danger" title={v.purge} onClick={() => purge(d)}>
                          <Icon name="trash" size={16} />
                        </button>
                      </div>
                    </div>
                    <div className="version-deleted muted small">{v.deletedInfo(dateTime(d.deletedAt), d.deletedBy || "?", daysLeft(d))}</div>
                  </div>
                ))
              )}
            </>
          )}
        </div>
      )}
      {applying && (
        <ApplyDialog
          overview={overview}
          sourceKey={game.key}
          sourceName={game.name}
          snapshot={applying}
          onClose={() => setApplying(null)}
          onApplied={async (target) => {
            setApplying(null);
            await onApplied?.(target);
          }}
        />
      )}
    </div>
  );
}
