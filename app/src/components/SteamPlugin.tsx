import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { api, errorText, inTauri, SteamPluginProgress, SteamPluginStatus } from "../api";
import { bytes } from "../format";
import { Badge, Button, Icon, useUi } from "./ui";

const MILLENNIUM_URL = "https://github.com/SteamClientHomebrew/Millennium";

/** Settings panel: show the cloud save status inside Steam (Millennium plugin). */
export function SteamPluginPanel() {
  const ui = useUi();
  const [st, setSt] = useState<SteamPluginStatus | null>(null);
  const [progress, setProgress] = useState<SteamPluginProgress | null>(null);
  const [working, setWorking] = useState(false);

  const load = useCallback(() => api.steamPluginStatus().then(setSt, () => setSt(null)), []);

  useEffect(() => {
    load();
    if (!inTauri) return;
    let off: (() => void) | undefined;
    listen<SteamPluginProgress>("steam-plugin-progress", (e) => setProgress(e.payload)).then((u) => (off = u));
    return () => off?.();
  }, [load]);

  if (!st) return null;

  const ready = st.millennium === "installed" && st.pluginInstalled && st.pluginEnabled;
  const needsMillennium = st.millennium !== "installed";

  const install = async () => {
    const ok = await ui.confirm({
      title: needsMillennium ? "安装 Millennium 和 Ludonimbus 插件" : "启用 Ludonimbus 插件",
      text: (
        <>
          {needsMillennium && (
            <p>
              将从 GitHub 下载 Millennium 的最新正式版并安装到 Steam 文件夹。Millennium 是开源的 Steam 客户端插件框架，不是 Valve 官方组件；下载后会先校验 SHA-256。
            </p>
          )}
          <p>然后把 Ludonimbus 插件放进 Millennium 并启用。{st.steamRunning ? "安装时需要暂时关闭 Steam，完成后会自动重新打开。" : ""}</p>
          <p className="muted small">之后可以随时在这里移除插件。卸载 Millennium 本身请使用它的官方安装程序。</p>
        </>
      ),
      confirm: needsMillennium ? "下载并安装" : "启用",
    });
    if (!ok) return;
    setWorking(true);
    setProgress(null);
    try {
      const r = await api.steamPluginInstall(true);
      const what = r.millenniumInstalled ? `已安装 Millennium ${r.millenniumInstalled} 和 Ludonimbus 插件` : "Ludonimbus 插件已启用";
      ui.toast(`${what}，${r.steamRestarted ? "Steam 已重新打开" : "下次打开 Steam 时生效"}`);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setWorking(false);
      setProgress(null);
      load();
    }
  };

  const remove = async () => {
    const ok = await ui.confirm({
      title: "移除 Ludonimbus 插件？",
      text: `Steam 里将不再显示云存档状态，自动同步不受影响。Millennium 本身会保留。${st.steamRunning ? "需要暂时关闭 Steam，完成后会自动重新打开。" : ""}`,
      confirm: "移除",
      danger: true,
    });
    if (!ok) return;
    setWorking(true);
    try {
      await api.steamPluginRemove(true);
      ui.toast("已移除 Ludonimbus 插件");
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setWorking(false);
      load();
    }
  };

  const badge = !st.steamRoot ? (
    <Badge tone="muted">没有找到 Steam</Badge>
  ) : ready ? (
    <Badge tone="ok">已启用</Badge>
  ) : st.millennium === "legacy" ? (
    <Badge tone="warn">Millennium 需要更新</Badge>
  ) : (
    <Badge tone="muted">未启用</Badge>
  );

  const progressText = !progress
    ? "正在准备…"
    : progress.stage === "download"
      ? progress.total > 0
        ? `正在下载 Millennium… ${Math.floor((progress.done / progress.total) * 100)}%（${bytes(progress.done)} / ${bytes(progress.total)}）`
        : "正在获取 Millennium 最新版本…"
      : st.steamRunning
        ? "正在安装（Steam 会暂时关闭）…"
        : "正在安装…";

  return (
    <section className="panel form">
      <div className="panel-head">
        <Icon name="steam" />
        <h3>在 Steam 中显示云存档状态</h3>
        {badge}
      </div>
      <p className="muted small">
        在 Steam 游戏库的游戏页面（「开始游戏」那一栏）显示 Ludonimbus 的云存档状态，并可以一键同步。需要 Ludonimbus 在托盘中运行。基于开源的{" "}
        <button className="link small" onClick={() => api.openPath(MILLENNIUM_URL)}>
          Millennium
        </button>{" "}
        插件框架实现。
      </p>
      {st.steamRoot && (
        <>
          <label>
            <span>Millennium</span>
            <span className={st.millennium === "legacy" ? "health health-warn" : "muted"}>
              {st.millennium === "installed"
                ? "已安装"
                : st.millennium === "legacy"
                  ? "检测到旧版（2.x），不支持这个插件。请先用 Millennium 官方安装程序更新到最新版。"
                  : "未安装（会自动下载官方最新版）"}
            </span>
          </label>
          <label>
            <span>Ludonimbus 插件</span>
            <span className="muted">
              {!st.pluginInstalled
                ? "未安装"
                : !st.pluginEnabled
                  ? "已安装，未启用"
                  : st.pluginCurrent || !st.bundled
                    ? "已启用"
                    : "已启用（已更新到新版本，下次打开 Steam 时生效）"}
            </span>
          </label>
        </>
      )}
      {st.steamRoot && !st.bundled && <div className="health health-warn">这个版本的 Ludonimbus 没有附带 Steam 插件（开发版需要先运行 scripts/build-plugin.ps1）。</div>}
      {working && (
        <div className="row muted small">
          <span className="spinner" /> {progressText}
        </div>
      )}
      {st.steamRoot && st.bundled && st.millennium !== "legacy" && (
        <div className="row">
          {!ready && (
            <Button kind="primary" icon="download" onClick={install} disabled={working}>
              {needsMillennium ? "一键安装" : "启用插件"}
            </Button>
          )}
          {st.pluginInstalled && (
            <Button kind="danger" small onClick={remove} disabled={working}>
              移除插件
            </Button>
          )}
        </div>
      )}
    </section>
  );
}
