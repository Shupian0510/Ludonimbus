import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { api, errorText, inTauri, SteamPluginProgress, SteamPluginStatus } from "../api";
import { bytes } from "../format";
import { msg, useT } from "../i18n";
import { Badge, Button, Icon, useUi } from "./ui";

export const MILLENNIUM_URL = "https://github.com/SteamClientHomebrew/Millennium";

/**
 * Status of the Steam plugin (Millennium + the Ludonimbus plugin) and the
 * install/remove actions, shared by Settings and the first-run guide.
 * The actions don't ask first; callers explain what happens before calling.
 */
export function useSteamPlugin() {
  const ui = useUi();
  const p = useT().steamPlugin;
  const [st, setSt] = useState<SteamPluginStatus | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [progress, setProgress] = useState<SteamPluginProgress | null>(null);
  const [working, setWorking] = useState(false);

  const load = useCallback(
    () =>
      api
        .steamPluginStatus()
        .then(setSt, () => setSt(null))
        .finally(() => setLoaded(true)),
    [],
  );

  useEffect(() => {
    load();
    if (!inTauri) return;
    let off: (() => void) | undefined;
    let gone = false;
    listen<SteamPluginProgress>("steam-plugin-progress", (e) => setProgress(e.payload)).then((u) => (gone ? u() : (off = u)));
    return () => {
      gone = true;
      off?.();
    };
  }, [load]);

  const ready = !!st && st.millennium === "installed" && st.pluginInstalled && st.pluginEnabled;
  const needsMillennium = !!st && st.millennium !== "installed";
  /** Steam found, the plugin is bundled, and Millennium isn't the unsupported 2.x. */
  const canInstall = !!st?.steamRoot && st.bundled && st.millennium !== "legacy";

  const install = async () => {
    setWorking(true);
    setProgress(null);
    try {
      const r = await api.steamPluginInstall(true);
      ui.toast(msg().steamPlugin.installed(r.millenniumInstalled, r.steamRestarted));
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setWorking(false);
      setProgress(null);
      load();
    }
  };

  const remove = async () => {
    setWorking(true);
    try {
      await api.steamPluginRemove(true);
      ui.toast(msg().steamPlugin.removed);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setWorking(false);
      load();
    }
  };

  const progressText = !progress
    ? p.preparing
    : progress.stage === "download"
      ? progress.total > 0
        ? p.downloading(Math.floor((progress.done / progress.total) * 100), bytes(progress.done), bytes(progress.total))
        : p.fetching
      : st?.steamRunning
        ? p.installingSteam
        : p.installing;

  return { st, loaded, working, progressText, ready, needsMillennium, canInstall, install, remove };
}

export type SteamPlugin = ReturnType<typeof useSteamPlugin>;

/** "Millennium: …" and "Ludonimbus plugin: …" rows; put them inside a `.form`. */
export function SteamPluginRows({ st }: { st: SteamPluginStatus }) {
  const p = useT().steamPlugin;
  return (
    <>
      <label>
        <span>Millennium</span>
        <span className={st.millennium === "legacy" ? "health health-warn" : "muted"}>
          {st.millennium === "installed" ? p.mInstalled : st.millennium === "legacy" ? p.mLegacy : p.mMissing}
        </span>
      </label>
      <label>
        <span>{p.plugin}</span>
        <span className="muted">
          {!st.pluginInstalled
            ? p.pMissing
            : !st.pluginEnabled
              ? p.pDisabled
              : st.pluginCurrent || !st.bundled
                ? p.enabled
                : p.pUpdated}
        </span>
      </label>
    </>
  );
}

/** Settings panel: show the cloud save status inside Steam (Millennium plugin). */
export function SteamPluginPanel() {
  const ui = useUi();
  const p = useT().steamPlugin;
  const plugin = useSteamPlugin();
  const { st, working, ready, needsMillennium } = plugin;

  if (!st) return null;

  const install = async () => {
    const ok = await ui.confirm({
      title: needsMillennium ? p.installTitle : p.enableTitle,
      text: (
        <>
          {needsMillennium && <p>{p.installMillennium}</p>}
          <p>
            {p.installPlugin}
            {st.steamRunning ? p.closesSteam : ""}
          </p>
          <p className="muted small">{p.removeLater}</p>
        </>
      ),
      confirm: needsMillennium ? p.downloadInstall : p.enable,
    });
    if (ok) await plugin.install();
  };

  const remove = async () => {
    const ok = await ui.confirm({
      title: p.removeTitle,
      text: p.removeText + (st.steamRunning ? p.closesSteam : ""),
      confirm: p.remove,
      danger: true,
    });
    if (ok) await plugin.remove();
  };

  const badge = !st.steamRoot ? (
    <Badge tone="muted">{p.noSteam}</Badge>
  ) : ready ? (
    <Badge tone="ok">{p.enabled}</Badge>
  ) : st.millennium === "legacy" ? (
    <Badge tone="warn">{p.needsUpdate}</Badge>
  ) : (
    <Badge tone="muted">{p.off}</Badge>
  );

  return (
    <section className="panel form" id="settings-steam-plugin">
      <div className="panel-head">
        <Icon name="steam" />
        <h3>{p.title}</h3>
        {badge}
      </div>
      <p className="muted small">
        {p.introBefore}{" "}
        <button className="link small" onClick={() => api.openPath(MILLENNIUM_URL)}>
          Millennium
        </button>{" "}
        {p.introAfter}
      </p>
      {st.steamRoot && <SteamPluginRows st={st} />}
      {st.steamRoot && !st.bundled && <div className="health health-warn">{p.notBundled}</div>}
      {working && (
        <div className="row muted small">
          <span className="spinner" /> {plugin.progressText}
        </div>
      )}
      {plugin.canInstall && (
        <div className="row">
          {!ready && (
            <Button kind="primary" icon="download" onClick={install} disabled={working}>
              {needsMillennium ? p.oneClick : p.enablePlugin}
            </Button>
          )}
          {st.pluginInstalled && (
            <Button kind="danger" small onClick={remove} disabled={working}>
              {p.removePlugin}
            </Button>
          )}
        </div>
      )}
    </section>
  );
}
