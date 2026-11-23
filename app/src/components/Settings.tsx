import { useCallback, useEffect, useState } from "react";
import { AccountInfo, api, CloudKind, errorText, Settings, SteamAccount, UpdateInfo } from "../api";
import { bytes } from "../format";
import { SgdbKeyForm } from "./ArtPicker";
import { BaiduOption, MigrateBanner, S3Option, WebDavOption } from "./CloudForms";
import { SteamPluginPanel } from "./SteamPlugin";
import { Badge, Button, Icon, Spinner, useUi } from "./ui";
import { UpdateDialog } from "./Update";

export const CLOUD_NAMES: Record<CloudKind, string> = {
  none: "未连接",
  localFolder: "本地 / 同步文件夹",
  googleDrive: "Google Drive",
  webDav: "WebDAV",
  s3: "S3 兼容存储",
  baiduPan: "百度网盘",
};

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
    default:
      return "";
  }
}

export function SettingsPage({ onChanged }: { onChanged: () => Promise<void> }) {
  const ui = useUi();
  const [s, setS] = useState<Settings | null>(null);
  const [account, setAccount] = useState<AccountInfo | null>(null);
  const [accountError, setAccountError] = useState<string | null>(null);
  const [steamAccounts, setSteamAccounts] = useState<SteamAccount[]>([]);
  const [deviceName, setDeviceName] = useState("");
  const [keepRecent, setKeepRecent] = useState(20);
  const [keepDaily, setKeepDaily] = useState(30);
  const [timeout, setTimeoutSecs] = useState(8);
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);

  const load = useCallback(async () => {
    const v = await api.settings();
    setS(v);
    setDeviceName(v.deviceName);
    setKeepRecent(v.retention.keepRecent);
    setKeepDaily(v.retention.keepDailyDays);
    setTimeoutSecs(v.networkTimeoutSecs);
    if (v.cloud.kind === "googleDrive") {
      setClientId(v.cloud.clientId);
      setClientSecret(v.cloud.clientSecret);
    }
    setAccount(null);
    setAccountError(null);
    if (v.cloud.kind !== "none") api.cloudAccount().then(setAccount, (e) => setAccountError(errorText(e)));
    api.steamAccounts().then((r) => setSteamAccounts(r.accounts), () => undefined);
  }, []);

  useEffect(() => {
    load().catch((e) => ui.toast(errorText(e), "bad"));
  }, [load, ui]);

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

  const connectGoogle = async () => {
    setConnecting(true);
    try {
      const label = await api.cloudConnectGoogle(clientId, clientSecret);
      await done(`已连接 Google Drive：${label}`);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setConnecting(false);
    }
  };

  const useFolder = async () => {
    const dir = await api.pickFolder("选择用作云端的文件夹（例如 Google Drive 电脑版同步的文件夹、NAS 共享目录）");
    if (!dir) return;
    try {
      await api.cloudUseFolder(dir);
      await done("已设置云端文件夹");
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const disconnect = async () => {
    const ok = await ui.confirm({
      title: "断开云盘？",
      text: "只是让本机不再同步，云端已有的存档和版本都会保留。",
      confirm: "断开",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.cloudDisconnect();
      await done("已断开云盘");
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const saveGeneral = async () => {
    try {
      await api.saveSettings({ deviceName, keepRecent, keepDailyDays: keepDaily, networkTimeoutSecs: timeout });
      await done("设置已保存");
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const needClientId = !s.googleBuiltin;

  const checkUpdate = async () => {
    try {
      const u = await api.checkUpdate();
      if (u.available) setUpdate(u);
      else ui.toast(`已经是最新版本（${u.current}）`);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <div className="page settings">
      <header className="page-head">
        <div>
          <h1>设置</h1>
          <p className="muted">Ludonimbus {s.version}</p>
        </div>
      </header>

      <section className="panel">
        <div className="panel-head">
          <Icon name="cloud" />
          <h3>云盘</h3>
          {s.cloud.kind === "none" ? <Badge tone="muted">未连接</Badge> : accountError ? <Badge tone="warn">需要处理</Badge> : <Badge tone="ok">已连接</Badge>}
        </div>

        <MigrateBanner settings={s} done={done} />

        {s.cloud.kind !== "none" && (
          <div className="cloud-current">
            <div>
              <b>{CLOUD_NAMES[s.cloud.kind]}</b>
              <div className="muted small">
                {account ? account.label : currentLabel(s)}
                {account?.used != null && account.total != null && ` · 已用 ${bytes(account.used)} / ${bytes(account.total)}`}
              </div>
              {accountError && <div className="health health-warn">{accountError}</div>}
              {s.cloud.kind === "googleDrive" && <div className="muted small">存档在你的 Google Drive「{s.cloud.folder}」文件夹里，网页版 Drive 也能直接看到和下载。</div>}
              {s.cloud.kind === "baiduPan" && (
                <div className="muted small">存档在你百度网盘的「我的应用数据/{s.cloud.appName || s.baiduAppName}」文件夹里，网页版和 App 都能看到。</div>
              )}
            </div>
            <Button kind="danger" onClick={disconnect}>
              断开
            </Button>
          </div>
        )}

        <div className="cloud-options">
          <div className="cloud-option">
            <div className="cloud-option-head">
              <b>Google Drive</b>
              <span className="muted small">点按钮后在浏览器里登录你的 Google 账号即可。只会访问 Ludonimbus 自己创建的文件。</span>
            </div>
            {(needClientId || showAdvanced) && (
              <div className="form compact">
                {needClientId && (
                  <p className="muted small">
                    当前是开发版，还没有内置 OAuth Client ID。请按 docs/google-oauth-setup.md 创建一个「桌面应用」类型的客户端，把 Client ID 和 Client secret 填在这里（正式发布版会内置，用户不需要填）。
                  </p>
                )}
                <label>
                  <span>Client ID</span>
                  <input className="input mono" value={clientId} onChange={(e) => setClientId(e.target.value)} placeholder="xxxx.apps.googleusercontent.com" />
                </label>
                <label>
                  <span>Client secret</span>
                  <input className="input mono" value={clientSecret} onChange={(e) => setClientSecret(e.target.value)} placeholder="GOCSPX-…" />
                </label>
              </div>
            )}
            <div className="row">
              <Button kind="primary" icon="cloud" onClick={connectGoogle} disabled={connecting || (needClientId && !clientId.trim())}>
                {connecting ? "请在浏览器中完成登录…" : s.cloud.kind === "googleDrive" ? "重新连接 Google Drive" : "连接 Google Drive"}
              </Button>
              {!needClientId && (
                <button className="link small" onClick={() => setShowAdvanced((v) => !v)}>
                  {showAdvanced ? "收起" : "使用自己的 Client ID（高级）"}
                </button>
              )}
            </div>
          </div>
          <div className="cloud-option">
            <div className="cloud-option-head">
              <b>本地 / 同步文件夹</b>
              <span className="muted small">任意文件夹：Google Drive 电脑版或 OneDrive 同步的文件夹、NAS 共享目录、移动硬盘。</span>
            </div>
            <div className="row">
              <Button icon="folder" onClick={useFolder}>
                选择文件夹…
              </Button>
            </div>
          </div>
          {/* Baidu needs an app approved on the Baidu open platform: builds
              without one hide the option (except for development, or when
              this PC is already connected). */}
          {(s.baiduBuiltin || s.cloud.kind === "baiduPan" || import.meta.env.DEV) && (
            <BaiduOption key={`baidu-${s.cloud.kind}`} settings={s} done={done} />
          )}
          <WebDavOption key={`webdav-${s.cloud.kind}`} settings={s} done={done} />
          <S3Option key={`s3-${s.cloud.kind}`} settings={s} done={done} />
        </div>
      </section>

      <section className="panel form">
        <div className="panel-head">
          <Icon name="devices" />
          <h3>本机与同步</h3>
        </div>
        <label>
          <span>设备名称</span>
          <input className="input" value={deviceName} onChange={(e) => setDeviceName(e.target.value)} />
        </label>
        <label>
          <span>保留最近的自动版本</span>
          <div className="row">
            <input className="input num" type="number" min={1} max={500} value={keepRecent} onChange={(e) => setKeepRecent(Number(e.target.value))} />
            <span className="muted small">个；另外每天保留 1 个，保留</span>
            <input className="input num" type="number" min={0} max={3650} value={keepDaily} onChange={(e) => setKeepDaily(Number(e.target.value))} />
            <span className="muted small">天。手动备份和锁定的版本永久保留。</span>
          </div>
        </label>
        <label>
          <span>网络超时</span>
          <div className="row">
            <input className="input num" type="number" min={3} max={120} value={timeout} onChange={(e) => setTimeoutSecs(Number(e.target.value))} />
            <span className="muted small">秒。启动游戏前连不上云端时，等这么久就直接启动（退出后补传）。</span>
          </div>
        </label>
        <div className="row">
          <Button kind="primary" onClick={saveGeneral}>
            保存
          </Button>
        </div>
      </section>

      <section className="panel form">
        <div className="panel-head">
          <Icon name="steam" />
          <h3>Steam</h3>
        </div>
        <label>
          <span>Steam 位置</span>
          <code className="path">{s.steamRoot ?? "没有找到 Steam"}</code>
        </label>
        {steamAccounts.length > 0 && (
          <label>
            <span>Steam 账号</span>
            <select
              className="input select"
              value={s.steamAccount ?? ""}
              onChange={async (e) => {
                await api.saveSettings({ steamAccount: e.target.value });
                await done("已切换 Steam 账号");
              }}
            >
              {steamAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.persona ?? a.accountName ?? a.id}（{a.shortcutCount} 个非 Steam 游戏）
                </option>
              ))}
            </select>
          </label>
        )}
        <label>
          <span>启动包装器</span>
          <div>
            <code className="path">{s.runnerPath}</code>
            {!s.runnerExists && <div className="health health-warn">找不到 ludonimbus-run.exe，Steam 自动同步无法使用。请重新安装 Ludonimbus。</div>}
          </div>
        </label>
      </section>

      <SteamPluginPanel />

      <SgdbPanel />

      <section className="panel form">
        <div className="panel-head">
          <Icon name="settings" />
          <h3>关于</h3>
        </div>
        <label>
          <span>版本</span>
          <div className="row">
            <span>Ludonimbus {s.version}</span>
            <Button small icon="download" onClick={checkUpdate}>
              检查更新
            </Button>
          </div>
        </label>
        <label>
          <span>数据目录</span>
          <button className="link" onClick={() => api.openPath(s.dataDir)}>
            {s.dataDir}
          </button>
        </label>
        <label>
          <span>Ludusavi</span>
          <span className="muted">{s.ludusavi ?? "未找到（不影响模拟器游戏，只影响从 Ludusavi 数据库添加 PC 游戏）"}</span>
        </label>
        <label>
          <span>设备 id</span>
          <code>{s.deviceId}</code>
        </label>
      </section>
      {update && <UpdateDialog info={update} onClose={() => setUpdate(null)} />}
    </div>
  );
}

/** SteamGridDB API key for picking artwork. */
function SgdbPanel() {
  const ui = useUi();
  const [status, setStatus] = useState<{ hasKey: boolean; keyPage: string } | null>(null);
  const load = useCallback(() => api.sgdbStatus().then(setStatus, () => setStatus(null)), []);
  useEffect(() => {
    load();
  }, [load]);
  if (!status) return null;
  return (
    <section className="panel">
      <div className="panel-head">
        <Icon name="library" />
        <h3>封面（SteamGridDB）</h3>
        {status.hasKey && <Badge tone="ok">已设置</Badge>}
      </div>
      {status.hasKey ? (
        <div className="row space">
          <p className="muted small">添加游戏、加入 Steam 时可以从 SteamGridDB 自动配封面，也可以在游戏详情页里「更换封面」自己挑。</p>
          <Button
            small
            kind="ghost"
            icon="trash"
            onClick={async () => {
              try {
                await api.sgdbClearKey();
                ui.toast("已删除 SteamGridDB API Key");
                await load();
              } catch (e) {
                ui.toast(errorText(e), "bad");
              }
            }}
          >
            删除 Key
          </Button>
        </div>
      ) : (
        <SgdbKeyForm keyPage={status.keyPage} onSaved={load} />
      )}
    </section>
  );
}
