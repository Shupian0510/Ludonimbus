import { useEffect, useState } from "react";
import { api, errorText, GameView, LocalFile, Outcome, Overview, StatusReport } from "../api";
import { bytes, dateTime, healthText, outcomeText, relTime } from "../format";
import { StatusBadge } from "./Library";
import { Badge, Button, Icon, Toggle, useUi } from "./ui";
import { Versions } from "./Versions";

type Tab = "versions" | "files" | "config";

export function GameDetail({
  game,
  overview,
  check,
  checking,
  onBack,
  onChanged,
  onRemoved,
}: {
  game: GameView;
  overview: Overview;
  check?: StatusReport;
  checking: boolean;
  onBack: () => void;
  onChanged: () => Promise<void>;
  onRemoved: () => Promise<void>;
}) {
  const ui = useUi();
  const [tab, setTab] = useState<Tab>("versions");
  const [versionsKey, setVersionsKey] = useState(0);
  const [heroFailed, setHeroFailed] = useState(false);
  const [logoFailed, setLogoFailed] = useState(false);

  const after = async (o?: Outcome) => {
    if (o) ui.toast(outcomeText(o, game.name), o.kind === "conflict" ? "warn" : o.kind === "offline" || o.kind === "queued" ? "info" : "ok");
    setVersionsKey((k) => k + 1);
    await onChanged();
  };

  const run = async <T,>(f: () => Promise<T>, done?: (v: T) => Promise<void> | void) => {
    try {
      const v = await f();
      await done?.(v);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const syncNow = (res: "auto" | "local" | "cloud" = "auto") => run(() => api.sync(game.key, res), after);

  const backupNow = async () => {
    const comment = await ui.prompt({
      title: "备份当前存档",
      text: "手动备份会永久保留（不受自动清理影响）。可以写一句备注，比如「打 Boss 前」。",
      placeholder: "备注（可选）",
      confirm: "备份",
    });
    if (comment === null) return;
    await run(() => api.backup(game.key, comment), after);
  };

  /** Steam changes need Steam closed; ask once and let the backend close/reopen it. */
  const steamAction = async (f: (closeSteam: boolean) => Promise<void>, okText: string) => {
    try {
      await f(false);
      ui.toast(okText);
    } catch (e) {
      const msg = errorText(e);
      if (!msg.includes("STEAM_RUNNING")) {
        ui.toast(msg, "bad");
        return;
      }
      const ok = await ui.confirm({
        title: "需要暂时关闭 Steam",
        text: "Steam 运行时会覆盖对快捷方式的修改。Ludonimbus 会先关闭 Steam，改好后再自动重新打开。",
        confirm: "关闭 Steam 并继续",
      });
      if (!ok) return;
      try {
        await f(true);
        ui.toast(okText);
      } catch (e2) {
        ui.toast(errorText(e2), "bad");
      }
    }
    await onChanged();
  };

  const relocate = async () => {
    try {
      const found = await api.relocateAuto(game.key);
      if (found) {
        const ok = await ui.confirm({
          title: "找到了新的位置",
          text: (
            <>
              <p>在原位置附近找到了：</p>
              <code className="path">{found}</code>
              <p className="muted">改用这个位置吗？如果新文件夹里没有存档，下次启动时会自动从云端恢复。</p>
            </>
          ),
          confirm: "使用这个位置",
        });
        if (ok) {
          await api.setGameExe(game.key, found);
          ui.toast("已更新游戏位置");
          await onChanged();
          return;
        }
      }
      const picked = await api.pickExe(game.exe);
      if (picked) {
        await api.setGameExe(game.key, picked);
        ui.toast("已更新游戏位置");
        await onChanged();
      } else if (!found) {
        ui.toast("没有自动找到新位置，也没有选择文件", "info");
      }
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const health = healthText(game.health);
  const conflict = check?.status === "conflict" || !!game.conflict;
  const hero = game.art.hero && !heroFailed ? game.art.hero : null;

  return (
    <div className="detail">
      <div className={`hero${hero ? "" : " hero-plain"}`} style={!hero && game.art.cover ? { backgroundImage: `url(${game.art.cover})` } : undefined}>
        {hero && <img className="hero-img" src={hero} alt="" onError={() => setHeroFailed(true)} />}
        <div className="hero-shade" />
        <button className="back" onClick={onBack}>
          <Icon name="back" /> 游戏库
        </button>
        <div className="hero-title">
          {game.art.logo && !logoFailed ? (
            <img className="logo" src={game.art.logo} alt={game.name} onError={() => setLogoFailed(true)} />
          ) : (
            <h1>{game.name}</h1>
          )}
        </div>
      </div>

      <div className="actionbar">
        <Button kind="primary" icon="sync" onClick={() => syncNow()} disabled={game.running || conflict} busyText="同步中…">
          立即同步
        </Button>
        <Button icon="upload" onClick={backupNow} disabled={game.running}>
          备份当前存档
        </Button>
        <Button icon="folder" onClick={() => run(() => api.openSaveFolder(game.key))}>
          打开存档文件夹
        </Button>
        <div className="actionbar-status">
          <StatusBadge game={game} check={check} checking={checking} />
          <span className="muted">
            上次同步 {relTime(game.lastSyncAt)}
            {game.lastSyncedDevice ? ` · ${game.lastSyncedDevice}` : ""}
            {check ? ` · 本地 ${check.localFiles} 个文件，${bytes(check.localSize)}` : ""}
          </span>
        </div>
      </div>

      <div className="detail-body">
        {conflict && check?.head && (
          <div className="banner banner-warn conflict">
            <Icon name="warn" />
            <div>
              <b>本机和云端都有新的存档改动</b>
              <p>
                云端最新：{check.head.deviceName} 于 {dateTime(check.head.createdAt)} 保存（{bytes(check.head.totalSize)}）。本机：
                {check.localFiles} 个文件，{bytes(check.localSize)}。没选的那份也会留在版本历史里。
              </p>
            </div>
            <div className="conflict-actions">
              <Button kind="primary" onClick={() => syncNow("cloud")}>
                使用云端存档
              </Button>
              <Button onClick={() => syncNow("local")}>使用本机存档</Button>
            </div>
          </div>
        )}
        {check?.message && check.status !== "conflict" && (
          <div className={`banner ${check.status === "offline" ? "banner-info" : "banner-bad"}`}>
            <Icon name="warn" /> {check.message}
          </div>
        )}
        {game.lastError && check?.status !== "offline" && (
          <div className="banner banner-bad">
            <Icon name="warn" /> 上次同步出错：{game.lastError}
          </div>
        )}
        {game.pending > 0 && (
          <div className="banner banner-info">
            <Icon name="cloud" /> 有 {game.pending} 个版本等待上传（云端恢复访问后自动上传）。
          </div>
        )}

        <div className="cards2">
          <section className="panel">
            <div className="panel-head">
              <Icon name="steam" />
              <h3>Steam 自动同步</h3>
              {game.steam && (
                <Toggle
                  checked={game.steam.enabled}
                  onChange={(v) =>
                    steamAction(
                      (close) => api.steamSetEnabled(game.key, v, close),
                      v ? "已启用：从 Steam 启动时会自动同步存档" : "已恢复原来的 Steam 快捷方式",
                    )
                  }
                />
              )}
            </div>
            <p className={`health health-${health.tone}`}>{health.text}</p>
            {health.fixable && (
              <Button kind="primary" small icon="check" onClick={() => steamAction((close) => api.steamRepair(game.key, close), "快捷方式已修复")}>
                一键修复
              </Button>
            )}
            {game.steam && (
              <p className="muted small">
                开启后，Steam 快捷方式会通过 Ludonimbus 启动游戏：启动前拉取云端存档，退出后上传。快捷方式里只记录游戏 id，游戏路径由 Ludonimbus 管理，模拟器更新不需要改 Steam。
              </p>
            )}
          </section>

          <section className="panel">
            <div className="panel-head">
              <Icon name="locate" />
              <h3>游戏位置</h3>
              {game.exe && (game.exeExists ? <Badge tone="ok">正常</Badge> : <Badge tone="warn">找不到</Badge>)}
            </div>
            {game.exe ? <code className="path">{game.exe}</code> : <p className="muted">未设置游戏程序</p>}
            {game.exe && !game.exeExists && (
              <p className="health health-warn">
                原来的位置不存在了（比如解压了新版本到别的文件夹）。从 Steam 启动时也会提示你，或者现在处理：
              </p>
            )}
            <div className="row">
              <Button small icon="locate" onClick={relocate}>
                {game.exeExists ? "更换位置…" : "重新定位"}
              </Button>
              {game.exe && game.exeExists && (
                <Button small kind="ghost" icon="folder" onClick={() => run(() => api.openPath(game.exe!))}>
                  打开所在文件夹
                </Button>
              )}
            </div>
          </section>
        </div>

        <div className="tabs">
          <button className={tab === "versions" ? "on" : ""} onClick={() => setTab("versions")}>
            版本历史
          </button>
          <button className={tab === "files" ? "on" : ""} onClick={() => setTab("files")}>
            存档文件
          </button>
          <button className={tab === "config" ? "on" : ""} onClick={() => setTab("config")}>
            设置
          </button>
        </div>

        {tab === "versions" && (
          <Versions key={versionsKey} game={game} overview={overview} disabled={game.running} onChanged={() => after()} />
        )}
        {tab === "files" && <Files game={game} />}
        {tab === "config" && <Config game={game} onChanged={onChanged} onRemoved={onRemoved} />}
      </div>
    </div>
  );
}

function Files({ game }: { game: GameView }) {
  const ui = useUi();
  const [data, setData] = useState<{ files: LocalFile[]; roots: Record<string, string> } | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.localFiles(game.key).then(setData, (e) => setError(errorText(e)));
  }, [game.key]);
  if (error) return <div className="banner banner-bad">{error}</div>;
  if (!data) return <div className="muted pad">读取中…</div>;
  return (
    <div className="panel">
      <div className="roots">
        {Object.entries(data.roots).map(([k, v]) => (
          <div key={k} className="root-row">
            <code>{`{${k}}`}</code>
            <button className="link" onClick={() => api.openPath(v).catch((e) => ui.toast(errorText(e), "bad"))}>
              {v}
            </button>
          </div>
        ))}
      </div>
      {data.files.length === 0 ? (
        <p className="muted">没有找到存档文件。可以在「设置」里检查存档位置。</p>
      ) : (
        <table className="files">
          <thead>
            <tr>
              <th>文件</th>
              <th>大小</th>
              <th>修改时间</th>
            </tr>
          </thead>
          <tbody>
            {data.files.map((f) => (
              <tr key={f.logical}>
                <td>
                  <code>{f.logical}</code>
                </td>
                <td>{bytes(f.size)}</td>
                <td>{f.mtime ? dateTime(f.mtime) : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function Config({ game, onChanged, onRemoved }: { game: GameView; onChanged: () => Promise<void>; onRemoved: () => Promise<void> }) {
  const ui = useUi();
  const [name, setName] = useState(game.name);
  const [saves, setSaves] = useState(game.customSaves ? game.saves.join("\n") : "");
  const [exclude, setExclude] = useState(game.exclude.join("\n"));
  const [waitFor, setWaitFor] = useState(game.waitFor.join("\n"));
  const [args, setArgs] = useState(game.args);

  const lines = (s: string) => s.split("\n").map((l) => l.trim()).filter(Boolean);

  const save = async () => {
    try {
      await api.updateGame(game.key, { name, saves: lines(saves), exclude: lines(exclude), waitFor: lines(waitFor), args });
      ui.toast("已保存");
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const addFolder = async () => {
    const dir = await api.pickFolder("选择存档文件夹", game.install);
    if (!dir) return;
    const p = await api.folderToPattern(dir, game.exe);
    setSaves((s) => (s.trim() ? `${s.trim()}\n${p}` : p));
  };

  const remove = async () => {
    const ok = await ui.confirm({
      title: `从 Ludonimbus 移除「${game.name}」？`,
      text: "只会从本机的 Ludonimbus 里移除，云端的存档和版本历史都会保留，以后重新添加就能找回。",
      confirm: "移除",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.removeGame(game.key);
      ui.toast("已移除");
      await onRemoved();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <div className="panel form">
      <label>
        <span>名称</span>
        <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      <label>
        <span>自动同步</span>
        <div className="row">
          <Toggle
            checked={game.autoSync}
            onChange={(v) =>
              api.updateGame(game.key, { autoSync: v }).then(onChanged, (e) => ui.toast(errorText(e), "bad"))
            }
          />
          <span className="muted small">关闭后，启动/退出游戏时不再同步（手动操作仍可用）。</span>
        </div>
      </label>
      <label>
        <span>上传时机</span>
        <div>
          <div className="row">
            <Toggle
              checked={game.uploadOnGameExit}
              disabled={!game.hasGameProcess}
              onChange={(v) =>
                api.updateGame(game.key, { uploadOnGameExit: v }).then(onChanged, (e) => ui.toast(errorText(e), "bad"))
              }
            />
            <span>游戏进程退出就上传，不等启动器关闭</span>
          </div>
          <p className="muted small">
            {game.hasGameProcess
              ? `适合启动器会一直开着的游戏。开启后，${(game.waitFor.length ? game.waitFor : game.defaultWaitFor).join("、")} 一退出就上传；关闭启动器时会再检查一次。`
              : "需要先在下面的「游戏进程」里填写游戏真正的进程名。"}
          </p>
        </div>
      </label>
      <label>
        <span>存档位置</span>
        <textarea
          className="input mono"
          rows={3}
          value={saves}
          placeholder={game.defaultSaves.length ? `使用内置规则：\n${game.defaultSaves.join("\n")}` : "{install}/save/**"}
          onChange={(e) => setSaves(e.target.value)}
        />
        <div className="row">
          <Button small icon="folder" onClick={addFolder}>
            添加文件夹…
          </Button>
          <span className="muted small">
            每行一个，以 {"{install}"}（游戏目录）、{"{appdata}"}、{"{localappdata}"}、{"{documents}"}、{"{home}"} 开头。留空则使用内置规则。
          </span>
        </div>
      </label>
      <label>
        <span>排除</span>
        <textarea className="input mono" rows={2} value={exclude} placeholder="{install}/save/cache/**" onChange={(e) => setExclude(e.target.value)} />
      </label>
      <label>
        <span>游戏进程</span>
        <textarea className="input mono" rows={2} value={waitFor} placeholder={
            game.defaultWaitFor.length
              ? `使用内置规则：${game.defaultWaitFor.join("、")}`
              : "例如 game.exe（游戏真正的进程名，用来判断游戏是否还在运行）"
          } onChange={(e) => setWaitFor(e.target.value)} />
      </label>
      <label>
        <span>启动参数</span>
        <input className="input mono" value={args} onChange={(e) => setArgs(e.target.value)} />
      </label>
      <div className="row space">
        <Button kind="primary" onClick={save}>
          保存设置
        </Button>
        <Button kind="danger" icon="trash" onClick={remove}>
          从 Ludonimbus 移除
        </Button>
      </div>
      <p className="muted small">游戏 id：{game.key}（多台设备靠它找到同一份云端存档）</p>
    </div>
  );
}
