import { useCallback, useEffect, useState } from "react";
import { api, errorText, GameView, Overview, SnapshotMeta, SnapshotSummary } from "../api";
import { bytes, dateTime, outcomeText, relTime, triggerLabel } from "../format";
import { msg, useT } from "../i18n";
import { Badge, Button, Empty, Icon, useUi } from "./ui";

export function Versions({
  game,
  overview,
  disabled,
  onChanged,
}: {
  game: GameView;
  overview: Overview;
  disabled: boolean;
  onChanged: () => Promise<void>;
}) {
  const ui = useUi();
  const t = useT();
  const v = t.versions;
  const [data, setData] = useState<{ snapshots: SnapshotSummary[]; headId: string | null; lastSyncedId: string | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [meta, setMeta] = useState<Record<string, SnapshotMeta>>({});

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
    if (ok) await act(() => api.deleteSnapshot(game.key, s.id), msg().versions.deleted);
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
                </span>
                <span className="tags">
                  {isHead && <Badge tone="info">{v.latest}</Badge>}
                  {isCurrent && <Badge tone="ok">{v.current}</Badge>}
                  {s.locked && <Badge tone="muted">{v.locked}</Badge>}
                </span>
              </button>
              <div className="version-actions">
                <Button small icon="restore" onClick={() => restore(s)} disabled={disabled || isCurrent} title={isCurrent ? v.alreadyCurrent : v.restoreThis}>
                  {v.restore}
                </Button>
                <button className="icon-btn" title={v.exportZip} onClick={() => exportZip(s)}>
                  <Icon name="download" size={16} />
                </button>
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
    </div>
  );
}
