import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { AccountInfo, api, BaiduDeviceCode, CloudConfig, errorText, MigrateProgress, S3Config, Settings, WebDavConfig } from "../api";
import { bytes } from "../format";
import { Button, Toggle, useUi } from "./ui";

type Preset<T> = { label: string; apply: (c: T) => T; hint: string };

const WEBDAV_PRESETS: Preset<WebDavConfig>[] = [
  {
    label: "坚果云",
    apply: (c) => ({ ...c, url: "https://dav.jianguoyun.com/dav/" }),
    hint: "用户名是坚果云账号邮箱；密码要用「账户信息 → 安全选项 → 第三方应用管理」里生成的应用密码，不是登录密码。",
  },
  {
    label: "群晖 NAS",
    apply: (c) => ({ ...c, url: "https://<NAS 地址>:5006/", folder: "<共享文件夹>/Ludonimbus" }),
    hint: "先在套件中心安装并启用「WebDAV Server」。文件夹要放在一个你有读写权限的共享文件夹下面。",
  },
  {
    label: "OpenList / AList",
    apply: (c) => ({ ...c, url: "http://<地址>:5244/dav/", folder: "<挂载的网盘>/Ludonimbus" }),
    hint: "可以把百度网盘、阿里云盘、夸克等挂载到 OpenList / AList，再用它的 WebDAV 接入。文件夹写在挂载的网盘下面。速度和稳定性取决于对应网盘。",
  },
  {
    label: "Nextcloud",
    apply: (c) => ({ ...c, url: "https://<服务器>/remote.php/dav/files/<用户名>/" }),
    hint: "开启了两步验证时，请在 Nextcloud 的安全设置里生成一个应用密码。",
  },
];

const S3_PRESETS: Preset<S3Config>[] = [
  {
    label: "MinIO（NAS）",
    apply: (c) => ({ ...c, endpoint: "http://<NAS 地址>:9000", region: "", pathStyle: true }),
    hint: "MinIO 通常需要打开「路径风格访问」。",
  },
  {
    label: "阿里云 OSS",
    apply: (c) => ({ ...c, endpoint: "https://oss-cn-hangzhou.aliyuncs.com", pathStyle: false }),
    hint: "Endpoint 换成 Bucket 所在地域的地址。建议用只允许访问这个 Bucket 的 RAM 子账号密钥。",
  },
  {
    label: "腾讯云 COS",
    apply: (c) => ({ ...c, endpoint: "https://cos.ap-guangzhou.myqcloud.com", region: "ap-guangzhou", pathStyle: false }),
    hint: "Endpoint 和区域换成 Bucket 所在地域。Bucket 名称要带上 -APPID 后缀。",
  },
  {
    label: "Cloudflare R2",
    apply: (c) => ({ ...c, endpoint: "https://<账号 ID>.r2.cloudflarestorage.com", region: "auto", pathStyle: false }),
    hint: "在 R2 里创建 API 令牌，得到 Access Key 和 Secret Key。",
  },
];

function useConnect(settings: Settings, done: (msg: string) => Promise<void>) {
  const ui = useUi();
  const [busy, setBusy] = useState<"test" | "connect" | null>(null);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

  const describe = (a: AccountInfo) =>
    `连接成功：${a.label}` + (a.used != null && a.total != null ? ` · 已用 ${bytes(a.used)} / ${bytes(a.total)}` : "");

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
      const ok = await ui.confirm({
        title: `改用${name}？`,
        text: "之后会同步到新的云盘。原来云盘里的存档不会被删除，切换后可以一键复制过来。",
        confirm: "切换",
      });
      if (!ok) return;
    }
    setBusy("connect");
    setResult(null);
    try {
      const label = await api.cloudConnect(cloud, secret);
      await done(`已连接${name}：${label}`);
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
  return (
    <div className="preset-row">
      <span className="muted small">常用：</span>
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

export function WebDavOption({ settings, done }: { settings: Settings; done: (msg: string) => Promise<void> }) {
  const current = settings.cloud.kind === "webDav" ? settings.cloud : null;
  const [open, setOpen] = useState(!!current);
  const [cfg, setCfg] = useState<WebDavConfig>(current ?? { kind: "webDav", url: "", username: "", folder: "Ludonimbus" });
  const [password, setPassword] = useState("");
  const [hint, setHint] = useState<string | null>(null);
  const { busy, result, test, connect } = useConnect(settings, done);
  const saved = current != null && current.url === cfg.url && current.username === cfg.username;
  const ready = cfg.url.trim() !== "" && !cfg.url.includes("<") && !cfg.folder.includes("<") && (password !== "" || saved);

  return (
    <div className="cloud-option">
      <div className="cloud-option-head">
        <b>WebDAV</b>
        <span className="muted small">群晖、威联通等 NAS，坚果云、Nextcloud，或通过 OpenList / AList 接入百度网盘、阿里云盘、夸克等。</span>
      </div>
      {!open ? (
        <div className="row">
          <Button icon="cloud" onClick={() => setOpen(true)}>
            填写 WebDAV 信息…
          </Button>
        </div>
      ) : (
        <>
          <PresetRow
            presets={WEBDAV_PRESETS}
            onPick={(p) => {
              setCfg(p.apply(cfg));
              setHint(p.hint);
            }}
          />
          {hint && <p className="muted small">{hint}</p>}
          <div className="form compact">
            <label>
              <span>地址</span>
              <input className="input mono" value={cfg.url} onChange={(e) => setCfg({ ...cfg, url: e.target.value })} placeholder="https://example.com/dav/" />
            </label>
            <label>
              <span>用户名</span>
              <input className="input" value={cfg.username} onChange={(e) => setCfg({ ...cfg, username: e.target.value })} autoComplete="off" />
            </label>
            <label>
              <span>密码</span>
              <input
                className="input"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={saved ? "已保存，不修改可以留空" : ""}
                autoComplete="new-password"
              />
            </label>
            <label>
              <span>文件夹</span>
              <input className="input mono" value={cfg.folder} onChange={(e) => setCfg({ ...cfg, folder: e.target.value })} placeholder="Ludonimbus" />
            </label>
          </div>
          <p className="muted small">密码保存在 Windows 凭据管理器里。局域网 NAS 没有受信任的 HTTPS 证书时，请用 http:// 地址。</p>
          <Result result={result} />
          <div className="row">
            <Button kind="primary" icon="cloud" onClick={() => connect(cfg, password, " WebDAV")} disabled={!ready || busy !== null || (current != null && isSame(current, cfg) && password === "")}>
              {busy === "connect" ? "正在连接…" : current ? "保存修改" : "连接"}
            </Button>
            <Button onClick={() => test(cfg, password)} disabled={!ready || busy !== null}>
              {busy === "test" ? "正在测试…" : "测试连接"}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

export function S3Option({ settings, done }: { settings: Settings; done: (msg: string) => Promise<void> }) {
  const current = settings.cloud.kind === "s3" ? settings.cloud : null;
  const [open, setOpen] = useState(!!current);
  const [cfg, setCfg] = useState<S3Config>(
    current ?? { kind: "s3", endpoint: "", region: "", bucket: "", prefix: "Ludonimbus", accessKeyId: "", pathStyle: false },
  );
  const [secret, setSecret] = useState("");
  const [hint, setHint] = useState<string | null>(null);
  const { busy, result, test, connect } = useConnect(settings, done);
  const saved = current != null && current.endpoint === cfg.endpoint && current.accessKeyId === cfg.accessKeyId;
  const ready =
    cfg.endpoint.trim() !== "" && !cfg.endpoint.includes("<") && cfg.bucket.trim() !== "" && cfg.accessKeyId.trim() !== "" && (secret !== "" || saved);

  return (
    <div className="cloud-option">
      <div className="cloud-option-head">
        <b>S3 兼容存储</b>
        <span className="muted small">NAS 上的 MinIO，或阿里云 OSS、腾讯云 COS、Cloudflare R2 等对象存储。</span>
      </div>
      {!open ? (
        <div className="row">
          <Button icon="cloud" onClick={() => setOpen(true)}>
            填写 S3 信息…
          </Button>
        </div>
      ) : (
        <>
          <PresetRow
            presets={S3_PRESETS}
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
              <span>区域</span>
              <input className="input mono" value={cfg.region} onChange={(e) => setCfg({ ...cfg, region: e.target.value })} placeholder="可留空（默认 us-east-1）" />
            </label>
            <label>
              <span>Bucket</span>
              <input className="input mono" value={cfg.bucket} onChange={(e) => setCfg({ ...cfg, bucket: e.target.value })} />
            </label>
            <label>
              <span>路径前缀</span>
              <input className="input mono" value={cfg.prefix} onChange={(e) => setCfg({ ...cfg, prefix: e.target.value })} placeholder="Ludonimbus" />
            </label>
            <label>
              <span>Access Key</span>
              <input className="input mono" value={cfg.accessKeyId} onChange={(e) => setCfg({ ...cfg, accessKeyId: e.target.value })} autoComplete="off" />
            </label>
            <label>
              <span>Secret Key</span>
              <input
                className="input mono"
                type="password"
                value={secret}
                onChange={(e) => setSecret(e.target.value)}
                placeholder={saved ? "已保存，不修改可以留空" : ""}
                autoComplete="new-password"
              />
            </label>
            <label>
              <span>路径风格</span>
              <div className="row">
                <Toggle checked={cfg.pathStyle} onChange={(v) => setCfg({ ...cfg, pathStyle: v })} />
                <span className="muted small">用 Endpoint/Bucket 的形式访问（MinIO 等需要打开）</span>
              </div>
            </label>
          </div>
          <p className="muted small">Secret Key 保存在 Windows 凭据管理器里。</p>
          <Result result={result} />
          <div className="row">
            <Button kind="primary" icon="cloud" onClick={() => connect(cfg, secret, " S3 存储")} disabled={!ready || busy !== null || (current != null && isSame(current, cfg) && secret === "")}>
              {busy === "connect" ? "正在连接…" : current ? "保存修改" : "连接"}
            </Button>
            <Button onClick={() => test(cfg, secret)} disabled={!ready || busy !== null}>
              {busy === "test" ? "正在测试…" : "测试连接"}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

export function BaiduOption({ settings, done }: { settings: Settings; done: (msg: string) => Promise<void> }) {
  const ui = useUi();
  const current = settings.cloud.kind === "baiduPan" ? settings.cloud : null;
  const needApp = !settings.baiduBuiltin;
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
      const ok = await ui.confirm({
        title: "改用百度网盘？",
        text: "之后会同步到百度网盘。原来云盘里的存档不会被删除，切换后可以一键复制过来。",
        confirm: "切换",
      });
      if (!ok) return;
    }
    setWaiting(true);
    setCode(null);
    setError(null);
    try {
      const name = await api.cloudConnectBaidu(appKey, secretKey, appName);
      await done(`已连接百度网盘：${name}`);
    } catch (e) {
      const text = errorText(e);
      if (!text.includes("已取消")) setError(text);
    } finally {
      setWaiting(false);
      setCode(null);
    }
  };

  return (
    <div className="cloud-option">
      <div className="cloud-option-head">
        <b>百度网盘</b>
        <span className="muted small">
          用百度账号授权登录。存档放在你网盘的「我的应用数据/{folder}」文件夹里，Ludonimbus 只能访问这个文件夹。非会员下载会被百度限速，拉取较大的存档时要多等一会儿。
        </span>
      </div>
      {(needApp || showAdvanced) && !waiting && (
        <div className="form compact">
          {needApp && (
            <p className="muted small">
              当前是开发版，还没有内置百度网盘应用。请按 docs/baidu-netdisk-setup.md 在百度网盘开放平台创建应用，把 AppKey、SecretKey 和应用名称填在这里（正式发布版会内置，用户不需要填）。
            </p>
          )}
          <label>
            <span>AppKey</span>
            <input className="input mono" value={appKey} onChange={(e) => setAppKey(e.target.value)} autoComplete="off" />
          </label>
          <label>
            <span>SecretKey</span>
            <input className="input mono" type="password" value={secretKey} onChange={(e) => setSecretKey(e.target.value)} autoComplete="new-password" />
          </label>
          <label>
            <span>应用名称</span>
            <input className="input" value={appName} onChange={(e) => setAppName(e.target.value)} placeholder={settings.baiduAppName} />
          </label>
        </div>
      )}
      {waiting && (
        <div className="baidu-login">
          {code ? (
            <>
              {code.qrcodeUrl && <img className="qr" src={code.qrcodeUrl} alt="百度网盘授权二维码" />}
              <div className="baidu-login-text">
                <span className="muted small">已在浏览器中打开百度授权页面。登录后如果要求输入授权码，请输入：</span>
                <span className="user-code">{code.userCode}</span>
                <span className="muted small">也可以用百度网盘 App 扫描二维码。授权完成后这里会自动继续。</span>
                <button className="link small" onClick={() => api.openPath(pageUrl(code)).catch(() => undefined)}>
                  重新打开授权页面
                </button>
              </div>
            </>
          ) : (
            <span className="muted small">
              <span className="spinner" /> 正在向百度申请授权码…
            </span>
          )}
        </div>
      )}
      {error && <div className="health health-warn">{error}</div>}
      <div className="row">
        {/* Distinct keys: the connect button stays "busy" for the whole sign-in,
            and the cancel button must not inherit that state. */}
        {waiting ? (
          <Button key="cancel" onClick={() => void api.cloudCancelLogin()}>
            取消
          </Button>
        ) : (
          <Button key="connect" kind="primary" icon="cloud" onClick={connect} disabled={needApp && (!appKey.trim() || !secretKey.trim())}>
            {current ? "重新连接百度网盘" : "连接百度网盘"}
          </Button>
        )}
        {!needApp && !waiting && (
          <button className="link small" onClick={() => setShowAdvanced((v) => !v)}>
            {showAdvanced ? "收起" : "使用自己的百度应用（高级）"}
          </button>
        )}
      </div>
    </div>
  );
}

/** Offer to copy the saves of the cloud used before the last switch. */
export function MigrateBanner({ settings, done }: { settings: Settings; done: (msg: string) => Promise<void> }) {
  const ui = useUi();
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
      await done(
        r.copied === 0 && r.skipped === 0
          ? "原来的云盘里没有存档，不需要复制"
          : `已复制 ${r.copied} 个版本（${bytes(r.bytes)}）` + (r.skipped ? `，${r.skipped} 个已经存在` : ""),
      );
    } catch (e) {
      ui.toast(`复制没有完成：${errorText(e)}。可以稍后重试，已复制的不会重复复制。`, "bad");
    } finally {
      setRunning(false);
    }
  };

  const skip = async () => {
    const ok = await ui.confirm({
      title: "不复制原来的存档？",
      text: "原来云盘里的存档会留在原处，Ludonimbus 以后只使用新的云盘。",
      confirm: "不复制",
    });
    if (!ok) return;
    try {
      await api.cloudDismissPrevious();
      await done("好的，原来云盘里的存档保持不变");
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const pct = progress && progress.total > 0 ? Math.floor((progress.done / progress.total) * 100) : 0;

  return (
    <div className="migrate-banner">
      <div>
        <b>要把原来云盘里的存档复制过来吗？</b>
        <div className="muted small">
          你刚从 {prev.name} 换到了新的云盘。复制后历史版本都会在新云盘里，原来的云盘不会被修改。
        </div>
        {running && (
          <div className="update-progress">
            <div className="bar">
              <span style={{ width: `${pct}%` }} />
            </div>
            <span className="muted small">
              {progress && progress.total > 0 ? `正在复制 ${Math.min(progress.done + 1, progress.total)} / ${progress.total}：${progress.game}` : "正在准备…"}
            </span>
          </div>
        )}
      </div>
      <div className="row">
        <Button kind="primary" icon="upload" onClick={copy} disabled={running}>
          {running ? "正在复制…" : "复制存档"}
        </Button>
        <Button kind="ghost" onClick={skip} disabled={running}>
          不用了
        </Button>
      </div>
    </div>
  );
}
