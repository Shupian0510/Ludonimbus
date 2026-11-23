import { useCallback, useEffect, useState } from "react";
import { api, errorText, GameView, Overview, SnapshotMeta, SnapshotSummary } from "../api";
import { bytes, dateTime, outcomeText, relTime, triggerLabel } from "../format";
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
      title: "恢复到这个版本？",
      text: (
        <>
          <p>
            将把「{game.name}」的存档恢复为 <b>{s.deviceName}</b> 在 <b>{dateTime(s.createdAt)}</b> 保存的版本。
          </p>
          <p className="muted">当前存档如果云端还没有，会先自动备份成「恢复前保护」版本，不会丢。</p>
        </>
      ),
      confirm: "恢复",
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
    const c = await ui.prompt({ title: "版本备注", text: "给这个版本写一句备注。", value: s.comment, placeholder: "备注" });
    if (c === null) return;
    await act(() => api.setSnapshot(game.key, s.id, { comment: c }), "已保存备注");
  };

  const remove = async (s: SnapshotSummary) => {
    const ok = await ui.confirm({
      title: "删除这个版本？",
      text: `${s.deviceName} 在 ${dateTime(s.createdAt)} 保存的版本会从云端永久删除，无法恢复。`,
      confirm: "删除",
      danger: true,
    });
    if (ok) await act(() => api.deleteSnapshot(game.key, s.id), "已删除");
  };

  const exportZip = async (s: SnapshotSummary) => {
    try {
      const p = await api.exportSnapshot(game.key, s.id);
      if (p) ui.toast(`已导出到 ${p}`);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  if (overview.cloud.kind === "none") {
    return (
      <Empty icon="cloud" title="还没有连接云盘">
        <p className="muted">在「设置」里连接 Google Drive 或选择一个同步文件夹后，这里会显示所有设备上传的存档版本。</p>
      </Empty>
    );
  }
  if (error) return <div className="banner banner-bad">{error}</div>;
  if (!data) return <div className="muted pad">读取版本历史…</div>;
  if (data.snapshots.length === 0) {
    return (
      <Empty icon="cloud" title="云端还没有版本">
        <p className="muted">点「立即同步」或玩一次游戏后，就会出现第一个版本。</p>
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
                  {mine ? "（本机）" : ""}
                </span>
                <span className="muted">{triggerLabel[s.trigger] ?? s.trigger}</span>
                <span className="muted">
                  {s.fileCount} 个文件 · {bytes(s.totalSize)}
                </span>
                <span className="tags">
                  {isHead && <Badge tone="info">最新</Badge>}
                  {isCurrent && <Badge tone="ok">本机当前</Badge>}
                  {s.locked && <Badge tone="muted">已锁定</Badge>}
                </span>
              </button>
              <div className="version-actions">
                <Button small icon="restore" onClick={() => restore(s)} disabled={disabled || isCurrent} title={isCurrent ? "本机已经是这个版本" : "恢复到这个版本"}>
                  恢复
                </Button>
                <button className="icon-btn" title="导出 zip" onClick={() => exportZip(s)}>
                  <Icon name="download" size={16} />
                </button>
                <button className="icon-btn" title="备注" onClick={() => editComment(s)}>
                  <Icon name="edit" size={16} />
                </button>
                <button
                  className="icon-btn"
                  title={s.locked ? "解锁（允许被自动清理）" : "锁定（永久保留）"}
                  onClick={() => act(() => api.setSnapshot(game.key, s.id, { locked: !s.locked }), s.locked ? "已解锁" : "已锁定，会永久保留")}
                >
                  <Icon name={s.locked ? "lock" : "unlock"} size={16} />
                </button>
                <button className="icon-btn danger" title="删除" disabled={s.locked || isHead} onClick={() => remove(s)}>
                  <Icon name="trash" size={16} />
                </button>
              </div>
            </div>
            {s.comment && <div className="version-comment">“{s.comment}”</div>}
            {open === s.id && (
              <div className="version-files">
                {!meta[s.id] ? (
                  <span className="muted">读取中…</span>
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
