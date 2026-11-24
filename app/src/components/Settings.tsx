import { useCallback, useEffect, useState } from "react";
import { AccountInfo, api, CloudKind, errorText, Settings, SteamAccount, UpdateInfo } from "../api";
import { bytes } from "../format";
import { SgdbKeyForm } from "./ArtPicker";
import { CloudOptions, MigrateBanner } from "./CloudForms";
import { SteamPluginPanel } from "./SteamPlugin";
import { SpeedTestPanel } from "./SpeedTest";
import { Badge, Button, Icon, PathChoice, PathField, samePath, Spinner, useUi } from "./ui";
import { UpdateDialog } from "./Update";
import { LANGS, LangPref, msg, useI18n, useT } from "../i18n";
import { DISCORD_URL, docsUrl, GITHUB_URL } from "../links";
import { scene, SCENE_SETTINGS } from "../scene";

export function cloudName(kind: CloudKind): string {
  return msg().cloudNames[kind];
}

function currentLabel(s: Settings): string {
  const c = s.cloud;
  switch (c.kind) {
    case "localFolder":
      return c.path;
    case "googleDrive":
      return c.account ?? "";
    case "webDav":
      return `${c.url.replace(/\/+$/, "")}/${c.folder}`;
    case "s3":
      return `${c.endpoint} / ${c.bucket}${c.prefix ? ` / ${c.prefix}` : ""}`;
    case "baiduPan":
      return c.account ?? "";
    case "quark":
      return c.folder;
    default:
      return "";
  }
}

export function SettingsPage({ onChanged, onGuide }: { onChanged: () => Promise<void>; onGuide: () => void }) {
  const ui = useUi();
  const { t, lang } = useI18n();
  const st = t.settings;
  const [s, setS] = useState<Settings | null>(null);
  const [account, setAccount] = useState<AccountInfo | null>(null);
  const [accountError, setAccountError] = useState<string | null>(null);
  const [steamAccounts, setSteamAccounts] = useState<SteamAccount[]>([]);
  const [deviceName, setDeviceName] = useState("");
  const [keepRecent, setKeepRecent] = useState(20);
  const [keepDaily, setKeepDaily] = useState(30);
  const [timeout, setTimeoutSecs] = useState(8);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);

  const load = useCallback(async () => {
    const v = await api.settings();
    setS(v);
    setDeviceName(v.deviceName);
    setKeepRecent(v.retention.keepRecent);
    setKeepDaily(v.retention.keepDailyDays);
    setTimeoutSecs(v.networkTimeoutSecs);
    setAccount(null);
    setAccountError(null);
    if (v.cloud.kind !== "none") api.cloudAccount().then(setAccount, (e) => setAccountError(errorText(e)));
    api.steamAccounts().then((r) => setSteamAccounts(r.accounts), () => undefined);
  }, []);

  useEffect(() => {
    load().catch((e) => ui.toast(errorText(e), "bad"));
  }, [load, ui]);

  // Browser preview: ?scene=… scrolls to its section (docs screenshots).
  useEffect(() => {
    const target = SCENE_SETTINGS[scene() ?? ""];
    if (s && target) setTimeout(() => document.getElementById(target)?.scrollIntoView({ block: "start" }), 400);
  }, [s]);

  if (!s) {
    return (
      <div className="center">
        <Spinner />
      </div>
    );
  }

  const done = async (msg: string) => {
    ui.toast(msg);
    await load();
    await onChanged();
  };

  const disconnect = async () => {
    const ok = await ui.confirm({
      title: st.disconnectTitle,
      text: st.disconnectText,
      confirm: st.disconnect,
      danger: true,
    });
    if (!ok) return;
    try {
      await api.cloudDisconnect();
      await done(msg().settings.disconnected);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const saveGeneral = async () => {
    try {
      await api.saveSettings({ deviceName, keepRecent, keepDailyDays: keepDaily, networkTimeoutSecs: timeout });
      await done(msg().settings.saved);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const checkUpdate = async () => {
    try {
      const u = await api.checkUpdate();
      if (u.available) setUpdate(u);
      else ui.toast(msg().settings.upToDate(u.current));
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <div className="page settings">
      <header className="page-head">
        <div>
          <h1>{st.title}</h1>
          <p className="muted">Ludonimbus {s.version}</p>
        </div>
      </header>

      <section className="panel" id="settings-cloud">
        <div className="panel-head">
          <Icon name="cloud" />
          <h3>{st.cloud}</h3>
          {s.cloud.kind === "none" ? <Badge tone="muted">{t.cloudNames.none}</Badge> : accountError ? <Badge tone="warn">{t.library.needsAttention}</Badge> : <Badge tone="ok">{st.connected}</Badge>}
        </div>

        <MigrateBanner settings={s} done={done} />

        {s.cloud.kind !== "none" && (
          <div className="cloud-current">
            <div>
              <b>{t.cloudNames[s.cloud.kind]}</b>
              <div className="muted small">
                {account ? account.label : currentLabel(s)}
                {account?.used != null && account.total != null && ` · ${st.used(bytes(account.used), bytes(account.total))}`}
              </div>
              {accountError && <div className="health health-warn">{accountError}</div>}
              {s.cloud.kind === "googleDrive" && <div className="muted small">{st.googleWhere(s.cloud.folder)}</div>}
              {s.cloud.kind === "baiduPan" && (
                <div className="muted small">{st.baiduWhere(s.cloud.appName || s.baiduAppName)}</div>
              )}
            </div>
            <Button kind="danger" onClick={disconnect}>
              {st.disconnect}
            </Button>
          </div>
        )}

        <CloudOptions settings={s} done={done} />
      </section>

      <SpeedTestPanel hasCloud={s.cloud.kind !== "none"} />

      <LanguagePanel />

      <section className="panel form">
        <div className="panel-head">
          <Icon name="devices" />
          <h3>{st.general}</h3>
        </div>
        <label>
          <span>{st.deviceName}</span>
          <input className="input" value={deviceName} onChange={(e) => setDeviceName(e.target.value)} />
        </label>
        <label>
          <span>{st.keepRecent}</span>
          <div className="row">
            <input className="input num" type="number" min={1} max={500} value={keepRecent} onChange={(e) => setKeepRecent(Number(e.target.value))} />
            <span className="muted small">{st.keepMid}</span>
            <input className="input num" type="number" min={0} max={3650} value={keepDaily} onChange={(e) => setKeepDaily(Number(e.target.value))} />
            <span className="muted small">{st.keepEnd}</span>
          </div>
        </label>
        <label>
          <span>{st.timeout}</span>
          <div className="row">
            <input className="input num" type="number" min={3} max={120} value={timeout} onChange={(e) => setTimeoutSecs(Number(e.target.value))} />
            <span className="muted small">{st.timeoutHint}</span>
          </div>
        </label>
        <div className="row">
          <Button kind="primary" onClick={saveGeneral}>
            {t.common.save}
          </Button>
        </div>
      </section>

      <section className="panel form">
        <div className="panel-head">
          <Icon name="steam" />
          <h3>Steam</h3>
        </div>
        <SteamRootField settings={s} done={done} />
        {steamAccounts.length > 0 && (
          <label>
            <span>{t.addToSteam.account}</span>
            <select
              className="input select"
              value={s.steamAccount ?? ""}
              onChange={async (e) => {
                await api.saveSettings({ steamAccount: e.target.value });
                await done(msg().settings.steamSwitched);
              }}
            >
              {steamAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.persona ?? a.accountName ?? a.id}{st.shortcutCount(a.shortcutCount)}
                </option>
              ))}
            </select>
          </label>
        )}
        <label>
          <span>{st.runner}</span>
          <div>
            <code className="path">{s.runnerPath}</code>
            {!s.runnerExists && <div className="health health-warn">{st.runnerMissing}</div>}
          </div>
        </label>
      </section>

      <SteamPluginPanel key={s.steamRoot ?? ""} />

      <SgdbPanel />

      <section className="panel form">
        <div className="panel-head">
          <Icon name="settings" />
          <h3>{st.about}</h3>
        </div>
        <label>
          <span>{st.version}</span>
          <div className="row">
            <span>Ludonimbus {s.version}</span>
            <Button small icon="download" onClick={checkUpdate}>
              {st.checkUpdate}
            </Button>
          </div>
        </label>
        <label>
          <span>{st.links}</span>
          <div className="row">
            <button className="link" onClick={() => api.openPath(docsUrl(lang))}>
              {st.docs}
            </button>
            <button className="link" onClick={() => api.openPath(GITHUB_URL)}>
              GitHub
            </button>
            <button className="link" onClick={() => api.openPath(`${GITHUB_URL}/issues`)}>
              {st.feedback}
            </button>
            {DISCORD_URL && (
              <button className="link" onClick={() => api.openPath(DISCORD_URL)}>
                Discord
              </button>
            )}
            <button className="link" onClick={onGuide}>
              {st.reopenGuide}
            </button>
          </div>
        </label>
        <label>
          <span>{st.dataDir}</span>
          <button className="link" onClick={() => api.openPath(s.dataDir)}>
            {s.dataDir}
          </button>
        </label>
        <label>
          <span>Ludusavi</span>
          <span className="muted">{s.ludusavi ?? st.ludusaviMissing}</span>
        </label>
        <label>
          <span>{st.deviceId}</span>
          <code>{s.deviceId}</code>
        </label>
      </section>
      {update && <UpdateDialog info={update} onClose={() => setUpdate(null)} />}
    </div>
  );
}

/** Where Steam is installed: auto-detected, found by a scan, or picked by hand. */
function SteamRootField({ settings: s, done }: { settings: Settings; done: (msg: string) => Promise<void> }) {
  const ui = useUi();
  const t = useT();
  const st = t.settings;
  const [choices, setChoices] = useState<PathChoice[] | null>(null);

  const use = async (path: string | null) => {
    try {
      const now = await api.steamSetRoot(path);
      setChoices(null);
      await done(path === null ? msg().settings.steamBackedAuto : msg().settings.steamSet(now ?? path));
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const scan = async () => {
    try {
      const found = await api.steamFindRoots();
      if (found.length === 0) {
        setChoices(null);
        ui.toast(msg().settings.steamNoneFound, "warn");
      } else if (found.length === 1 && samePath(found[0], s.steamRoot)) {
        setChoices(null);
        ui.toast(msg().settings.steamOnlyCurrent, "info");
      } else if (found.length === 1) {
        await use(found[0]);
      } else {
        setChoices(found.map((path) => ({ path })));
      }
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const pick = async () => {
    const dir = await api.pickFolder(msg().settings.steamPickTitle, s.steamRoot);
    if (dir) await use(dir);
  };

  return (
    <div className="field">
      <span>{st.steamRoot}</span>
      <PathField
        path={s.steamRoot}
        empty={t.steamPlugin.noSteam}
        badge={s.steamRoot && <Badge tone={s.steamRootCustom ? "info" : "muted"}>{s.steamRootCustom ? st.steamCustom : st.steamAuto}</Badge>}
        onScan={scan}
        onPick={pick}
        choices={choices}
        onChoose={(c) => use(c.path)}
      >
        {s.steamRoot && (
          <Button small kind="ghost" onClick={() => api.openPath(s.steamRoot!).catch((e) => ui.toast(errorText(e), "bad"))}>
            {st.open}
          </Button>
        )}
        {s.steamRootCustom && (
          <Button small kind="ghost" onClick={() => use(null)}>
            {st.steamBackAuto}
          </Button>
        )}
      </PathField>
    </div>
  );
}

/** SteamGridDB API key for picking artwork. */
function SgdbPanel() {
  const ui = useUi();
  const t = useT();
  const st = t.settings;
  const [status, setStatus] = useState<{ hasKey: boolean; keyPage: string } | null>(null);
  const load = useCallback(() => api.sgdbStatus().then(setStatus, () => setStatus(null)), []);
  useEffect(() => {
    load();
  }, [load]);
  if (!status) return null;
  return (
    <section className="panel" id="settings-sgdb">
      <div className="panel-head">
        <Icon name="library" />
        <h3>{st.sgdbTitle}</h3>
        {status.hasKey && <Badge tone="ok">{st.sgdbSet}</Badge>}
      </div>
      {status.hasKey ? (
        <div className="row space">
          <p className="muted small">{st.sgdbOn}</p>
          <Button
            small
            kind="ghost"
            icon="trash"
            onClick={async () => {
              try {
                await api.sgdbClearKey();
                ui.toast(msg().settings.sgdbCleared);
                await load();
              } catch (e) {
                ui.toast(errorText(e), "bad");
              }
            }}
          >
            {st.sgdbClear}
          </Button>
        </div>
      ) : (
        <SgdbKeyForm keyPage={status.keyPage} onSaved={load} />
      )}
    </section>
  );
}

/** Interface language; applies right away and is shared with the tray, the Steam wrapper and the Steam plugin. */
function LanguagePanel() {
  const ui = useUi();
  const { t, pref, setPref } = useI18n();
  const change = async (p: LangPref) => {
    try {
      await api.setLanguage(p);
      setPref(p);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };
  return (
    <section className="panel form" id="settings-language">
      <div className="panel-head">
        <Icon name="settings" />
        <h3>{t.settings.language}</h3>
      </div>
      <label>
        <span>{t.settings.languageLabel}</span>
        <select className="input select" value={pref} onChange={(e) => change(e.target.value as LangPref)}>
          <option value="system">{t.settings.languageSystem}</option>
          {LANGS.map((l) => (
            <option key={l.id} value={l.id}>
              {l.label}
            </option>
          ))}
        </select>
      </label>
    </section>
  );
}
