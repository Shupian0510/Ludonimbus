import { useMemo, useState } from "react";
import type { GameView, Overview, StatusReport } from "../api";
import { bytes, relTime, statusInfo } from "../format";
import { Badge, Button, Empty, Icon, Spinner } from "./ui";

function loadView(): "grid" | "list" {
  try {
    return localStorage.getItem("ln.view") === "list" ? "list" : "grid";
  } catch {
    return "grid";
  }
}

export function StatusBadge({ game, check, checking }: { game: GameView; check?: StatusReport; checking?: boolean }) {
  if (game.running) return <Badge tone="info">游戏运行中</Badge>;
  if (!check) return checking ? <Badge tone="muted">检查中…</Badge> : <Badge tone="muted">未检查</Badge>;
  const s = statusInfo[check.status];
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
        .sort((a, b) => a.name.localeCompare(b.name, "zh-CN")),
    [overview.games, query],
  );

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>游戏库</h1>
          <p className="muted">
            {overview.games.length} 个游戏 · 存档会在从 Steam 启动和退出时自动同步
          </p>
        </div>
        <div className="head-actions">
          <div className="search">
            <Icon name="search" size={16} />
            <input placeholder="搜索游戏" value={query} onChange={(e) => setQuery(e.target.value)} />
          </div>
          <div className="seg">
            <button className={view === "grid" ? "on" : ""} onClick={() => setViewSaved("grid")} title="网格">
              <Icon name="grid" />
            </button>
            <button className={view === "list" ? "on" : ""} onClick={() => setViewSaved("list")} title="列表">
              <Icon name="list" />
            </button>
          </div>
          <Button icon="sync" onClick={onRefresh} title="重新检查所有游戏">
            刷新
          </Button>
          <Button kind="primary" icon="plus" onClick={onAdd}>
            添加游戏
          </Button>
        </div>
      </header>

      {overview.cloud.kind === "none" && (
        <div className="banner banner-info">
          <Icon name="cloud" />
          <span>还没有连接云盘，存档暂时只会保存在本机。</span>
          <Button small kind="primary" onClick={onSettings}>
            去连接
          </Button>
        </div>
      )}
      {overview.cloud.error && overview.cloud.kind !== "none" && (
        <div className="banner banner-warn">
          <Icon name="warn" />
          <span>{overview.cloud.error}</span>
          <Button small onClick={onSettings}>
            打开设置
          </Button>
        </div>
      )}

      {overview.games.length === 0 ? (
        <Empty icon="library" title="还没有添加游戏">
          <p className="muted">从 Steam 里的非 Steam 游戏开始：模拟器、重制版、第三方游戏都可以。</p>
          <Button kind="primary" icon="plus" onClick={onAdd}>
            添加游戏
          </Button>
        </Empty>
      ) : view === "grid" ? (
        <div className="grid">
          {games.map((g) => (
            <button key={g.key} className="card" onClick={() => onOpen(g.key)}>
              <div className="cover">
                <Cover game={g} />
                {needsAttention(g) && (
                  <span className="attention" title="需要处理">
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
                </div>
              </div>
            </button>
          ))}
        </div>
      ) : (
        <div className="table">
          <div className="tr th">
            <span>游戏</span>
            <span>状态</span>
            <span>本地存档</span>
            <span>上次同步</span>
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
                    <small className="muted">{g.platform ?? g.adapterName ?? (g.ludusaviTitle ? "Ludusavi 数据库" : "自定义")}</small>
                  </span>
                </span>
                <span>
                  <StatusBadge game={g} check={c} checking={checking[g.key]} />
                </span>
                <span className="muted">{c ? `${c.localFiles} 个文件 · ${bytes(c.localSize)}` : "—"}</span>
                <span className="muted">
                  {relTime(g.lastSyncAt)}
                  {g.lastSyncedDevice ? ` · ${g.lastSyncedDevice}` : ""}
                </span>
                <span>
                  {needsAttention(g) ? (
                    <Badge tone="warn">需要处理</Badge>
                  ) : g.steam?.enabled ? (
                    <Badge tone="ok">自动同步</Badge>
                  ) : g.steam ? (
                    <Badge tone="muted">未启用</Badge>
                  ) : (
                    <Badge tone="muted">未关联</Badge>
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
