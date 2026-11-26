import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { AccountInfo, api, BaiduDeviceCode, CloudConfig, errorText, MigrateProgress, OpenListInfo, S3Config, Settings, WebDavConfig } from "../api";
import { bytes } from "../format";
import { Messages, msg, useI18n, useT } from "../i18n";
import { BAIDU_AUTHORIZATIONS_URL, docsUrl } from "../links";
import { scene, sceneIs } from "../scene";
import { Button, Hint, Menu, PathChoice, PathField, Toggle, useUi } from "./ui";

type Preset<T> = { label: string; apply: (c: T) => T; hint: string };

function webdavPresets(t: Messages): Preset<WebDavConfig>[] {
  const p = t.cloud.presets;
  return [
    { label: p.jianguoyun.label, apply: (c) => ({ ...c, url: "https://dav.jianguoyun.com/dav/" }), hint: p.jianguoyun.hint },
    {
      label: p.synology.label,
      apply: (c) => ({ ...c, url: `https://<${p.nasAddress}>:5006/`, folder: `<${p.sharedFolder}>/Ludonimbus` }),
      hint: p.synology.hint,
    },
    {
      label: "OpenList / AList",
      apply: (c) => ({ ...c, url: `http://<${p.address}>:5244/dav/`, folder: `<${p.mountedDrive}>/Ludonimbus` }),
      hint: p.openlist.hint,
    },
    {
      label: "Nextcloud",
      apply: (c) => ({ ...c, url: `https://<${p.server}>/remote.php/dav/files/<${p.username}>/` }),
      hint: p.nextcloud.hint,
    },
  ];
}

function s3Presets(t: Messages): Preset<S3Config>[] {
  const p = t.cloud.presets;
  return [
    { label: p.minio.label, apply: (c) => ({ ...c, endpoint: `http://<${p.nasAddress}>:9000`, region: "", pathStyle: true }), hint: p.minio.hint },
    { label: p.oss.label, apply: (c) => ({ ...c, endpoint: "https://oss-cn-hangzhou.aliyuncs.com", pathStyle: false }), hint: p.oss.hint },
    {
      label: p.cos.label,
      apply: (c) => ({ ...c, endpoint: "https://cos.ap-guangzhou.myqcloud.com", region: "ap-guangzhou", pathStyle: false }),
      hint: p.cos.hint,
    },
    {
      label: "Cloudflare R2",
      apply: (c) => ({ ...c, endpoint: `https://<${p.accountId}>.r2.cloudflarestorage.com`, region: "auto", pathStyle: false }),
      hint: p.r2.hint,
    },
  ];
}

function useConnect(settings: Settings, done: (msg: string) => Promise<void>) {
  const ui = useUi();
  const [busy, setBusy] = useState<"test" | "connect" | null>(null);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

  const describe = (a: AccountInfo) => {
    const m = msg();
    return m.cloud.testOk(a.label) + (a.used != null && a.total != null ? ` · ${m.settings.used(bytes(a.used), bytes(a.total))}` : "");
  };

  const test = async (cloud: WebDavConfig | S3Config, secret: string) => {
    setBusy("test");
    setResult(null);
    try {
      setResult({ ok: true, text: describe(await api.cloudTest(cloud, secret)) });
    } catch (e) {
      setResult({ ok: false, text: errorText(e) });
    } finally {
      setBusy(null);
    }
  };

  const connect = async (cloud: WebDavConfig | S3Config, secret: string, name: string) => {
    if (settings.cloud.kind !== "none") {
      const m = msg().cloud;
      const ok = await ui.confirm({ title: m.switchTitle(name), text: m.switchText, confirm: m.switch });
      if (!ok) return;
    }
    setBusy("connect");
    setResult(null);
    try {
      const label = await api.cloudConnect(cloud, secret);
      await done(msg().cloud.connected(name, label));
    } catch (e) {
      setResult({ ok: false, text: errorText(e) });
    } finally {
      setBusy(null);
    }
  };

  return { busy, result, test, connect };
}

function isSame(a: CloudConfig, b: CloudConfig) {
  return JSON.stringify(a) === JSON.stringify(b);
}

function PresetRow<T>({ presets, onPick }: { presets: Preset<T>[]; onPick: (p: Preset<T>) => void }) {
  const t = useT();
  return (
    <div className="preset-row">
      <span className="muted small">{t.cloud.common}</span>
      {presets.map((p) => (
        <button key={p.label} className="chip" onClick={() => onPick(p)}>
          {p.label}
        </button>
      ))}
    </div>
  );
}

function Result({ result }: { result: { ok: boolean; text: string } | null }) {
  if (!result) return null;
  return <div className={`health ${result.ok ? "health-ok" : "health-warn"}`}>{result.text}</div>;
}

export function WebDavOption({
  settings,
  done,
  preset,
}: {
  settings: Settings;
  done: (msg: string) => Promise<void>;
  /** Start with this preset (its label) filled in, e.g. Jianguoyun in the first-run guide. */
  preset?: string;
}) {
  const t = useT();
  const c = t.cloud;
  const current = settings.cloud.kind === "webDav" ? settings.cloud : null;
  const start = preset ? webdavPresets(t).find((p) => p.label === preset) : undefined;
  const blank: WebDavConfig = { kind: "webDav", url: "", username: "", folder: "Ludonimbus" };
  // Browser preview (docs screenshot): the form filled in with the first preset.
  const demo = !preset && sceneIs("settings-cloud-webdav") ? webdavPresets(t)[0] : undefined;
  const [open, setOpen] = useState(!!current || !!demo);
  const [cfg, setCfg] = useState<WebDavConfig>(
    current ?? (start ? start.apply(blank) : demo ? { ...demo.apply(blank), username: "player@example.com" } : blank),
  );
  const [password, setPassword] = useState("");
  const [hint, setHint] = useState<string | null>(start && !current ? start.hint : demo ? demo.hint : null);
  const { busy, result, test, connect } = useConnect(settings, done);
  const saved = current != null && current.url === cfg.url && current.username === cfg.username;
  const ready = cfg.url.trim() !== "" && !cfg.url.includes("<") && !cfg.folder.includes("<") && (password !== "" || saved);

  return (
    <div className="cloud-option" id={preset ? undefined : "cloud-webdav"}>
      <div className="cloud-option-head">
        <b>{start ? `${start.label} (WebDAV)` : "WebDAV"}</b>
        <Hint text={start ? start.hint : c.webdavIntro} />
        <span className="grow" />
        {!open && (
          <Button small icon="cloud" onClick={() => setOpen(true)}>
            {c.fill}
          </Button>
        )}
      </div>
      {open && (
        <>
          <PresetRow
            presets={webdavPresets(t)}
            onPick={(p) => {
              setCfg(p.apply(cfg));
              setHint(p.hint);
            }}
          />
          {hint && <p className="muted small">{hint}</p>}
          <div className="form compact">
            <label>
              <span>{c.url}</span>
              <input className="input mono" value={cfg.url} onChange={(e) => setCfg({ ...cfg, url: e.target.value })} placeholder="https://example.com/dav/" />
            </label>
            <label>
              <span>{c.username}</span>
              <input className="input" value={cfg.username} onChange={(e) => setCfg({ ...cfg, username: e.target.value })} autoComplete="off" />
            </label>
            <label>
              <span>
                {c.password} <Hint text={c.webdavNote} />
              </span>
              <input
                className="input"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={saved ? c.savedKeep : ""}
                autoComplete="new-password"
              />
            </label>
            <label>
              <span>{c.folder}</span>
              <input className="input mono" value={cfg.folder} onChange={(e) => setCfg({ ...cfg, folder: e.target.value })} placeholder="Ludonimbus" />
            </label>
          </div>
          <Result result={result} />
          <div className="row">
            <Button kind="primary" icon="cloud" onClick={() => connect(cfg, password, "WebDAV")} disabled={!ready || busy !== null || (current != null && isSame(current, cfg) && password === "")}>
              {busy === "connect" ? c.connecting : current ? c.saveChanges : c.connect}
            </Button>
            <Button onClick={() => test(cfg, password)} disabled={!ready || busy !== null}>
              {busy === "test" ? c.testing : c.test}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

export function S3Option({ settings, done }: { settings: Settings; done: (msg: string) => Promise<void> }) {
  const t = useT();
  const c = t.cloud;
  const current = settings.cloud.kind === "s3" ? settings.cloud : null;
  // Browser preview (docs screenshot): the form filled in with the R2 preset.
  const demo = sceneIs("settings-cloud-s3") ? s3Presets(t)[3] : undefined;
  const blankS3: S3Config = { kind: "s3", endpoint: "", region: "", bucket: "", prefix: "Ludonimbus", accessKeyId: "", pathStyle: false };
  const [open, setOpen] = useState(!!current || !!demo);
  const [cfg, setCfg] = useState<S3Config>(
    current ?? (demo ? { ...demo.apply(blankS3), bucket: "ludonimbus-saves", accessKeyId: "0123456789abcdef" } : blankS3),
  );
  const [secret, setSecret] = useState("");
  const [hint, setHint] = useState<string | null>(demo ? demo.hint : null);
  const { busy, result, test, connect } = useConnect(settings, done);
  const saved = current != null && current.endpoint === cfg.endpoint && current.accessKeyId === cfg.accessKeyId;
  const ready =
    cfg.endpoint.trim() !== "" && !cfg.endpoint.includes("<") && cfg.bucket.trim() !== "" && cfg.accessKeyId.trim() !== "" && (secret !== "" || saved);

  return (
    <div className="cloud-option">
      <div className="cloud-option-head">
        <b id="cloud-s3">{t.cloudNames.s3}</b>
        <Hint text={c.s3Intro} />
        <span className="grow" />
        {!open && (
          <Button small icon="cloud" onClick={() => setOpen(true)}>
            {c.fill}
          </Button>
        )}
      </div>
      {open && (
        <>
          <PresetRow
            presets={s3Presets(t)}
            onPick={(p) => {
              setCfg(p.apply(cfg));
              setHint(p.hint);
            }}
          />
          {hint && <p className="muted small">{hint}</p>}
          <div className="form compact">
            <label>
              <span>Endpoint</span>
              <input className="input mono" value={cfg.endpoint} onChange={(e) => setCfg({ ...cfg, endpoint: e.target.value })} placeholder="https://s3.example.com" />
            </label>
            <label>
              <span>{c.region}</span>
              <input className="input mono" value={cfg.region} onChange={(e) => setCfg({ ...cfg, region: e.target.value })} placeholder={c.regionPlaceholder} />
            </label>
            <label>
              <span>Bucket</span>
              <input className="input mono" value={cfg.bucket} onChange={(e) => setCfg({ ...cfg, bucket: e.target.value })} />
            </label>
            <label>
              <span>{c.prefix}</span>
              <input className="input mono" value={cfg.prefix} onChange={(e) => setCfg({ ...cfg, prefix: e.target.value })} placeholder="Ludonimbus" />
            </label>
            <label>
              <span>Access Key</span>
              <input className="input mono" value={cfg.accessKeyId} onChange={(e) => setCfg({ ...cfg, accessKeyId: e.target.value })} autoComplete="off" />
            </label>
            <label>
              <span>
                Secret Key <Hint text={c.s3Note} />
              </span>
              <input
                className="input mono"
                type="password"
                value={secret}
                onChange={(e) => setSecret(e.target.value)}
                placeholder={saved ? c.savedKeep : ""}
                autoComplete="new-password"
              />
            </label>
            <label>
              <span>
                {c.pathStyle} <Hint text={c.pathStyleHint} />
              </span>
              <div className="row">
                <Toggle checked={cfg.pathStyle} onChange={(v) => setCfg({ ...cfg, pathStyle: v })} />
              </div>
            </label>
          </div>
          <Result result={result} />
          <div className="row">
            <Button kind="primary" icon="cloud" onClick={() => connect(cfg, secret, t.app.cloud.s3)} disabled={!ready || busy !== null || (current != null && isSame(current, cfg) && secret === "")}>
              {busy === "connect" ? c.connecting : current ? c.saveChanges : c.connect}
            </Button>
            <Button onClick={() => test(cfg, secret)} disabled={!ready || busy !== null}>
              {busy === "test" ? c.testing : c.test}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

/** How to withdraw Ludonimbus's access to Baidu Netdisk, in the app and on Baidu's side. */
export function BaiduRevokeHelp({ folder }: { folder: string }) {
  const { t, lang } = useI18n();
  const b = t.cloud.baidu;
  return (
    <details className="revoke-help">
      <summary>{b.revoke}</summary>
      <ol>
        <li>{b.revokeApp}</li>
        <li>
          {b.revokeBaidu}{" "}
          <button className="link small" onClick={() => api.openPath(BAIDU_AUTHORIZATIONS_URL).catch(() => undefined)}>
            {b.revokeOpen}
          </button>
        </li>
        <li>{b.revokeData(folder)}</li>
      </ol>
      <button className="link small" onClick={() => api.openPath(docsUrl(lang, lang === "en" ? "privacy.html#baidu-netdisk" : "privacy.html#baidu")).catch(() => undefined)}>
        {b.revokeMore}
      </button>
    </details>
  );
}

export function BaiduOption({ settings, done }: { settings: Settings; done: (msg: string) => Promise<void> }) {
  const ui = useUi();
  const t = useT();
  const b = t.cloud.baidu;
  const current = settings.cloud.kind === "baiduPan" ? settings.cloud : null;
  const c = t.cloud;
  const needApp = !settings.baiduBuiltin;
  // Without a built-in app, its fields show once asked for.
  const [open, setOpen] = useState(!!current?.appKey);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [appKey, setAppKey] = useState(current?.appKey ?? "");
  const [secretKey, setSecretKey] = useState(current?.secretKey ?? "");
  const [appName, setAppName] = useState(current?.appName ?? "");
  const [code, setCode] = useState<BaiduDeviceCode | null>(null);
  const [waiting, setWaiting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let off: (() => void) | undefined;
    listen<BaiduDeviceCode>("baidu-device-code", (e) => setCode(e.payload)).then((u) => (off = u), () => undefined);
    return () => off?.();
  }, []);

  const folder = appName.trim() || settings.baiduAppName;
  const pageUrl = (c: BaiduDeviceCode) => `${c.verificationUrl}${c.verificationUrl.includes("?") ? "&" : "?"}code=${c.userCode}`;

  const connect = async () => {
    if (settings.cloud.kind !== "none" && settings.cloud.kind !== "baiduPan") {
      const m = msg().cloud;
      const ok = await ui.confirm({ title: m.switchTitle(msg().cloudNames.baiduPan), text: m.switchText, confirm: m.switch });
      if (!ok) return;
    }
    setWaiting(true);
    setCode(null);
    setError(null);
    try {
      const name = await api.cloudConnectBaidu(appKey, secretKey, appName);
      await done(msg().cloud.connected(msg().cloudNames.baiduPan, name));
    } catch (e) {
      const text = errorText(e);
      if (!text.includes("已取消") && !/cancel/i.test(text)) setError(text);
    } finally {
      setWaiting(false);
      setCode(null);
    }
  };

  return (
    <div className="cloud-option">
      <div className="cloud-option-head">
        <b>{t.cloudNames.baiduPan}</b>
        <Hint text={b.intro(folder)} />
        <span className="grow" />
        {/* Distinct keys: the connect button stays "busy" for the whole sign-in,
            and the cancel button must not inherit that state. */}
        {waiting ? (
          <Button key="cancel" small onClick={() => void api.cloudCancelLogin()}>
            {t.common.cancel}
          </Button>
        ) : needApp && !open ? (
          <Button key="fill" small icon="cloud" onClick={() => setOpen(true)}>
            {c.fill}
          </Button>
        ) : (
          <Button key="connect" small icon="cloud" onClick={connect} disabled={needApp && (!appKey.trim() || !secretKey.trim())}>
            {current ? c.reconnect : c.connect}
          </Button>
        )}
        {!needApp && !waiting && <Menu title={t.saves.more} items={[{ label: b.own, checked: showAdvanced, onClick: () => setShowAdvanced((v) => !v) }]} />}
      </div>
      {(needApp ? open : showAdvanced) && !waiting && (
        <div className="form compact">
          <label>
            <span>AppKey {needApp && <Hint text={b.dev} />}</span>
            <input className="input mono" value={appKey} onChange={(e) => setAppKey(e.target.value)} autoComplete="off" />
          </label>
          <label>
            <span>
              SecretKey <Hint text={b.secretNote} />
            </span>
            <input className="input mono" type="password" value={secretKey} onChange={(e) => setSecretKey(e.target.value)} autoComplete="new-password" />
          </label>
          <label>
            <span>{b.appName}</span>
            <input className="input" value={appName} onChange={(e) => setAppName(e.target.value)} placeholder={settings.baiduAppName} />
          </label>
        </div>
      )}
      {waiting && (
        <div className="baidu-login">
          {code ? (
            <>
              {code.qrcodeUrl && <img className="qr" src={code.qrcodeUrl} alt={b.qrAlt} />}
              <div className="baidu-login-text">
                <span className="muted small">{b.opened}</span>
                <span className="user-code">{code.userCode}</span>
                <span className="muted small">{b.scan}</span>
                <button className="link small" onClick={() => api.openPath(pageUrl(code)).catch(() => undefined)}>
                  {b.reopen}
                </button>
              </div>
            </>
          ) : (
            <span className="muted small">
              <span className="spinner" /> {b.requesting}
            </span>
          )}
        </div>
      )}
      {error && <div className="health health-warn">{error}</div>}
    </div>
  );
}

/** Offer to copy the saves of the cloud used before the last switch. */
export function MigrateBanner({ settings, done }: { settings: Settings; done: (msg: string) => Promise<void> }) {
  const ui = useUi();
  const t = useT();
  const mg = t.cloud.migrate;
  const [progress, setProgress] = useState<MigrateProgress | null>(null);
  const [running, setRunning] = useState(false);

  useEffect(() => {
    let off: (() => void) | undefined;
    listen<MigrateProgress>("cloud-migrate-progress", (e) => setProgress(e.payload)).then((u) => (off = u), () => undefined);
    return () => off?.();
  }, []);

  const prev = settings.previousCloud;
  if (!prev) return null;

  const copy = async () => {
    setRunning(true);
    setProgress(null);
    try {
      const r = await api.cloudMigrate();
      const m = msg().cloud.migrate;
      await done(r.copied === 0 && r.skipped === 0 ? m.nothing : m.copied(r.copied, bytes(r.bytes), r.skipped));
    } catch (e) {
      ui.toast(msg().cloud.migrate.failed(errorText(e)), "bad");
    } finally {
      setRunning(false);
    }
  };

  const skip = async () => {
    const m = msg().cloud.migrate;
    const ok = await ui.confirm({ title: m.skipTitle, text: m.skipText, confirm: m.skip });
    if (!ok) return;
    try {
      await api.cloudDismissPrevious();
      await done(msg().cloud.migrate.skipped);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const pct = progress && progress.total > 0 ? Math.floor((progress.done / progress.total) * 100) : 0;

  return (
    <div className="migrate-banner">
      <div>
        <span>
          <b>{mg.title}</b> <Hint text={mg.text(prev.name)} />
        </span>
        {running && (
          <div className="update-progress">
            <div className="bar">
              <span style={{ width: `${pct}%` }} />
            </div>
            <span className="muted small">
              {progress && progress.total > 0 ? mg.progress(Math.min(progress.done + 1, progress.total), progress.total, progress.game) : t.steamPlugin.preparing}
            </span>
          </div>
        )}
      </div>
      <div className="row">
        <Button small kind="primary" icon="upload" onClick={copy} disabled={running}>
          {running ? mg.copying : mg.copy}
        </Button>
        <Button small kind="ghost" onClick={skip} disabled={running}>
          {mg.no}
        </Button>
      </div>
    </div>
  );
}

export function GoogleOption({ settings, done }: { settings: Settings; done: (msg: string) => Promise<void> }) {
  const ui = useUi();
  const t = useT();
  const st = t.settings;
  const c = t.cloud;
  const current = settings.cloud.kind === "googleDrive" ? settings.cloud : null;
  const [clientId, setClientId] = useState(current?.clientId ?? "");
  const [clientSecret, setClientSecret] = useState(current?.clientSecret ?? "");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const needClientId = !settings.googleBuiltin;

  const connect = async () => {
    setConnecting(true);
    try {
      const label = await api.cloudConnectGoogle(clientId, clientSecret);
      await done(msg().settings.connectedGoogle(label));
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setConnecting(false);
    }
  };

  return (
    <div className="cloud-option">
      <div className="cloud-option-head">
        <b>Google Drive</b>
        <Hint text={st.googleIntro} />
        <span className="grow" />
        <Button small icon="cloud" onClick={connect} disabled={connecting || (needClientId && !clientId.trim())}>
          {connecting ? st.googleWaiting : current ? c.reconnect : c.connect}
        </Button>
        {!needClientId && <Menu title={t.saves.more} items={[{ label: st.googleOwn, checked: showAdvanced, onClick: () => setShowAdvanced((v) => !v) }]} />}
      </div>
      {(needClientId || showAdvanced) && (
        <div className="form compact">
          <label>
            <span>Client ID {needClientId && <Hint text={st.googleDev} />}</span>
            <input className="input mono" value={clientId} onChange={(e) => setClientId(e.target.value)} placeholder="xxxx.apps.googleusercontent.com" />
          </label>
          <label>
            <span>Client secret</span>
            <input className="input mono" value={clientSecret} onChange={(e) => setClientSecret(e.target.value)} placeholder="GOCSPX-…" />
          </label>
        </div>
      )}
    </div>
  );
}

export function FolderOption({ settings, done, note }: { settings: Settings; done: (msg: string) => Promise<void>; note?: string }) {
  const ui = useUi();
  const t = useT();
  const st = t.settings;
  const current = settings.cloud.kind === "localFolder" ? settings.cloud.path : null;
  const [choices, setChoices] = useState<PathChoice[] | null>(null);

  const use = async (dir: string) => {
    try {
      await api.cloudUseFolder(dir);
      setChoices(null);
      await done(msg().settings.folderSet);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const pick = async () => {
    const dir = await api.pickFolder(msg().settings.pickCloudFolder, current);
    if (dir) await use(dir);
  };

  // Folders kept in sync by OneDrive, Dropbox, Google Drive for desktop…
  const scan = async () => {
    try {
      const list = await api.cloudFolderSuggestions();
      if (list.length === 0) {
        setChoices(null);
        ui.toast(msg().settings.folderScanNone, "warn");
        return;
      }
      setChoices(list.map((f) => ({ path: f.path, label: f.label, note: f.existing ? msg().settings.folderHasData : undefined })));
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const choose = async (c: PathChoice) => {
    const m = msg().settings;
    const ok = await ui.confirm({
      title: m.folderUseTitle,
      text: (
        <>
          <code className="path">{c.path}</code>
          <p className="muted">{m.folderUseText(!!c.note)}</p>
        </>
      ),
      confirm: m.folderUse,
    });
    if (ok) await use(c.path);
  };

  return (
    <div className="cloud-option">
      <div className="cloud-option-head">
        <b>{t.cloudNames.localFolder}</b>
        <Hint text={st.folderIntro} />
        <span className="grow" />
        <Button small icon="folder" onClick={pick}>
          {current ? t.pathField.repick : t.saveFinder.pickFolder}
        </Button>
        <Menu
          title={t.saves.more}
          items={[
            { label: t.pathField.scan, icon: "search", onClick: scan },
            ...(current ? [{ label: st.open, icon: "folder", onClick: () => api.openPath(current).catch((e) => ui.toast(errorText(e), "bad")) }] : []),
          ]}
        />
      </div>
      {note && <p className="health health-warn small">{note}</p>}
      {(current || choices) && <PathField path={current} choices={choices} choicesText={st.folderFound} onChoose={choose} />}
    </div>
  );
}

/**
 * Every cloud option. In the first-run guide (`guide`), the ones that suit
 * the interface language come first and the rest fold under "More clouds".
 */
export function CloudOptions({
  settings,
  done,
  guide,
}: {
  settings: Settings;
  done: (msg: string) => Promise<void>;
  guide?: { multiDevice: boolean };
}) {
  const { t, lang } = useI18n();
  const [more, setMore] = useState(!guide && !!scene());
  const k = settings.cloud.kind;
  // There is no built-in Baidu app (it would need Baidu's review), so the
  // option only shows in development or when already connected with one's
  // own app.
  const baidu = settings.baiduBuiltin || k === "baiduPan" || (import.meta.env.DEV && !scene());
  const folderNote = guide?.multiDevice ? t.onboarding.cloud.folderMultiDevice : undefined;
  const all = {
    google: <GoogleOption key={`google-${k}`} settings={settings} done={done} />,
    folder: <FolderOption key="folder" settings={settings} done={done} note={folderNote} />,
    baidu: baidu ? <BaiduOption key={`baidu-${k}`} settings={settings} done={done} /> : null,
    quark: <QuarkOption key={`quark-${k}`} settings={settings} done={done} />,
    webdav: <WebDavOption key={`webdav-${k}`} settings={settings} done={done} />,
    jianguoyun: <WebDavOption key={`jgy-${k}`} settings={settings} done={done} preset={t.cloud.presets.jianguoyun.label} />,
    s3: <S3Option key={`s3-${k}`} settings={settings} done={done} />,
  };
  if (!guide) {
    // With a cloud connected, the rest (and its own details) are one click away.
    if (k !== "none" && !more) {
      return (
        <button className="link small cloud-change" onClick={() => setMore(true)}>
          {t.cloud.change}
        </button>
      );
    }
    return <div className="cloud-options">{[all.google, all.folder, all.baidu, all.quark, all.webdav, all.s3]}</div>;
  }
  const [first, rest] =
    lang === "zh-CN"
      ? [[all.quark, all.jianguoyun, all.google], [all.baidu, all.webdav, all.s3, all.folder]]
      : [[all.google, all.webdav, all.folder], [all.s3, all.quark, all.baidu]];
  return (
    <div className="cloud-options">
      {first}
      {more ? (
        rest
      ) : (
        <button className="link small" onClick={() => setMore(true)}>
          {t.onboarding.cloud.more}
        </button>
      )}
    </div>
  );
}

/**
 * Quark through OpenList: download the component once (with consent), then
 * sign in on Quark's own page in a private window.
 */
export function QuarkOption({ settings, done }: { settings: Settings; done: (msg: string) => Promise<void> }) {
  const ui = useUi();
  const t = useT();
  const q = t.cloud.quark;
  const c = t.cloud;
  const current = settings.cloud.kind === "quark";
  const [info, setInfo] = useState<OpenListInfo | null>(null);
  const [stage, setStage] = useState<"idle" | "download" | "login">("idle");
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.openlistStatus().then(setInfo, () => setInfo(null));
    let off: (() => void) | undefined;
    listen<{ done: number; total: number }>("openlist-progress", (e) => setProgress(e.payload)).then((u) => (off = u), () => undefined);
    return () => off?.();
  }, []);

  const confirmSwitch = async () => {
    if (settings.cloud.kind === "none" || current) return true;
    const m = msg().cloud;
    return ui.confirm({ title: m.switchTitle(msg().cloudNames.quark), text: m.switchText, confirm: m.switch });
  };

  const connect = async (zip?: string | null) => {
    if (!(await confirmSwitch())) return;
    setError(null);
    try {
      if (!info?.installed) {
        setStage("download");
        setProgress(null);
        await api.openlistInstall(zip ?? null);
        setInfo(await api.openlistStatus());
      }
      setStage("login");
      const label = await api.cloudConnectQuark();
      await done(msg().cloud.connected(msg().cloudNames.quark, label));
    } catch (e) {
      const text = errorText(e);
      if (!text.includes("已取消") && !/cancel/i.test(text)) setError(text);
    } finally {
      setStage("idle");
    }
  };

  const pickZip = async () => {
    const zip = await api.openlistPickZip();
    if (zip) await connect(zip);
  };

  const pct = progress && progress.total > 0 ? Math.floor((progress.done / progress.total) * 100) : 0;

  return (
    <div className="cloud-option" id="cloud-quark">
      <div className="cloud-option-head">
        <b>{t.cloudNames.quark}</b>
        <span className="badge badge-muted">{q.experimental}</span>
        {/* What is downloaded, from where and under which licence, with the rest. */}
        <Hint text={info && !info.installed ? `${q.intro}\n\n${q.needComponent(bytes(info.size), info.license)}` : q.intro} />
        <span className="grow" />
        {stage === "login" ? (
          <Button key="cancel" small onClick={() => void api.cloudCancelLogin()}>
            {t.common.cancel}
          </Button>
        ) : (
          <Button key="connect" small icon="cloud" onClick={() => connect()} disabled={stage !== "idle" || !info}>
            {current ? c.reconnect : info?.installed ? c.connect : q.downloadConnect}
          </Button>
        )}
        {info && stage === "idle" && (
          <Menu
            title={t.saves.more}
            items={[
              ...(info.installed ? [] : [{ label: q.pickZip, icon: "folder", onClick: pickZip }]),
              { label: q.project, onClick: () => api.openPath(info.project) },
            ]}
          />
        )}
      </div>
      {stage === "download" && (
        <div className="update-progress">
          <div className="bar">
            <span style={{ width: `${pct || 3}%` }} />
          </div>
          <span className="muted small">
            {progress && progress.total > 0 ? q.downloading(pct, bytes(progress.done), bytes(progress.total)) : q.preparing}
          </span>
        </div>
      )}
      {stage === "login" && (
        <div className="row muted small">
          <span className="spinner" /> {q.waiting}
        </div>
      )}
      {error && <div className="health health-warn small">{error}</div>}
    </div>
  );
}
