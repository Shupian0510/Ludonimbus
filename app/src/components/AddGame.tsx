import { useEffect, useState } from "react";
import { api, CloudGame, errorText, Overview, ShortcutView, SteamAccount } from "../api";
import { Badge, Button, Empty, Icon, Modal, Spinner, useUi } from "./ui";

type Tab = "steam" | "cloud" | "ludusavi" | "manual";

export function AddGame({ overview, onClose, onAdded }: { overview: Overview; onClose: () => void; onAdded: (key: string) => void }) {
  const [tab, setTab] = useState<Tab>("steam");
  return (
    <Modal title="添加游戏" onClose={onClose} wide>
      <div className="tabs tabs-tight">
        <button className={tab === "steam" ? "on" : ""} onClick={() => setTab("steam")}>
          <Icon name="steam" size={16} /> Steam 快捷方式
        </button>
        <button className={tab === "cloud" ? "on" : ""} onClick={() => setTab("cloud")}>
          <Icon name="cloud" size={16} /> 云端已有
        </button>
        <button className={tab === "ludusavi" ? "on" : ""} onClick={() => setTab("ludusavi")}>
          <Icon name="search" size={16} /> Ludusavi 数据库
        </button>
        <button className={tab === "manual" ? "on" : ""} onClick={() => setTab("manual")}>
          <Icon name="edit" size={16} /> 手动添加
        </button>
      </div>
      {tab === "steam" && <FromSteam onAdded={onAdded} />}
      {tab === "cloud" && <FromCloud overview={overview} onAdded={onAdded} />}
      {tab === "ludusavi" && <FromLudusavi onAdded={onAdded} />}
      {tab === "manual" && <Manual onAdded={onAdded} />}
    </Modal>
  );
}

function useAccounts() {
  const [accounts, setAccounts] = useState<SteamAccount[]>([]);
  const [account, setAccount] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.steamAccounts().then(
      (r) => {
        setAccounts(r.accounts);
        setAccount(r.selected);
        if (!r.root) setError("没有找到 Steam");
      },
      (e) => setError(errorText(e)),
    );
  }, []);
  return { accounts, account, setAccount, error };
}

function AccountPicker({ accounts, account, setAccount }: { accounts: SteamAccount[]; account: string | null; setAccount: (a: string) => void }) {
  if (accounts.length < 2) return null;
  return (
    <select className="input select" value={account ?? ""} onChange={(e) => setAccount(e.target.value)}>
      {accounts.map((a) => (
        <option key={a.id} value={a.id}>
          {a.persona ?? a.accountName ?? a.id}（{a.shortcutCount} 个非 Steam 游戏）
        </option>
      ))}
    </select>
  );
}

function FromSteam({ onAdded }: { onAdded: (key: string) => void }) {
  const ui = useUi();
  const { accounts, account, setAccount, error } = useAccounts();
  const [list, setList] = useState<ShortcutView[] | null>(null);
  const [manual, setManual] = useState<ShortcutView | null>(null);

  useEffect(() => {
    if (!account) return;
    setList(null);
    api.steamShortcuts(account).then(setList, (e) => ui.toast(errorText(e), "bad"));
  }, [account, ui]);

  const add = async (s: ShortcutView) => {
    if (!s.adapter) return setManual(s);
    try {
      const key = await api.addFromShortcut({ account: s.account, appid: s.appid, name: s.suggestedName });
      ui.toast(`已添加「${s.suggestedName}」`);
      onAdded(key);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  if (error) return <div className="banner banner-bad">{error}</div>;
  if (manual) return <ManualSaves shortcut={manual} onBack={() => setManual(null)} onAdded={onAdded} />;

  return (
    <div className="add-list">
      <div className="row space">
        <p className="muted small">选择 Steam 里的非 Steam 游戏。识别出的模拟器 / 重制版会自动套用存档规则；添加后在详情页打开「Steam 自动同步」。</p>
        <AccountPicker accounts={accounts} account={account} setAccount={setAccount} />
      </div>
      {!list ? (
        <div className="center pad">
          <Spinner />
        </div>
      ) : list.length === 0 ? (
        <Empty icon="steam" title="这个账号没有非 Steam 游戏">
          <p className="muted">先在 Steam 里「添加非 Steam 游戏」，再回来添加。</p>
        </Empty>
      ) : (
        list.map((s) => (
          <div key={s.appid} className="add-item">
            {s.cover ? <img className="add-cover" src={s.cover} alt="" /> : <div className="add-cover fallback">{s.suggestedName.slice(0, 1)}</div>}
            <div className="add-info">
              <b>{s.suggestedName}</b>
              <code className="path small">{s.exe}</code>
              <div className="row">
                {s.adapterName ? <Badge tone="ok">已识别：{s.adapterName}</Badge> : <Badge tone="muted">需要手动指定存档位置</Badge>}
                {s.wrappedKey && !s.addedKey && <Badge tone="warn">已被 Ludonimbus 接管</Badge>}
              </div>
            </div>
            {s.addedKey ? (
              <Badge tone="info">已添加</Badge>
            ) : (
              <Button kind={s.adapter ? "primary" : "secondary"} icon="plus" onClick={() => add(s)} disabled={!!s.wrappedKey}>
                {s.adapter ? "添加" : "指定存档…"}
              </Button>
            )}
          </div>
        ))
      )}
    </div>
  );
}

function SavesEditor({ saves, setSaves, exe }: { saves: string[]; setSaves: (s: string[]) => void; exe: string | null }) {
  const add = async () => {
    const start = exe ? exe.replace(/[\\/][^\\/]*$/, "") : null;
    const dir = await api.pickFolder("选择存档文件夹", start);
    if (!dir) return;
    const p = await api.folderToPattern(dir, exe);
    if (!saves.includes(p)) setSaves([...saves, p]);
  };
  return (
    <div className="saves-editor">
      {saves.map((s) => (
        <div key={s} className="save-pill">
          <code>{s}</code>
          <button className="icon-btn" onClick={() => setSaves(saves.filter((x) => x !== s))}>
            <Icon name="x" size={14} />
          </button>
        </div>
      ))}
      <Button small icon="folder" onClick={add}>
        添加存档文件夹…
      </Button>
    </div>
  );
}

function ManualSaves({ shortcut, onBack, onAdded }: { shortcut: ShortcutView; onBack: () => void; onAdded: (key: string) => void }) {
  const ui = useUi();
  const [name, setName] = useState(shortcut.suggestedName);
  const [saves, setSaves] = useState<string[]>([]);
  const submit = async () => {
    try {
      const key = await api.addFromShortcut({ account: shortcut.account, appid: shortcut.appid, name, saves });
      ui.toast(`已添加「${name}」`);
      onAdded(key);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };
  return (
    <div className="form">
      <button className="link" onClick={onBack}>
        ← 返回列表
      </button>
      <label>
        <span>名称</span>
        <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      <label>
        <span>游戏程序</span>
        <code className="path">{shortcut.exe}</code>
      </label>
      <label>
        <span>存档位置</span>
        <SavesEditor saves={saves} setSaves={setSaves} exe={shortcut.exe} />
      </label>
      <p className="muted small">选中的文件夹如果在游戏目录里，会记成 {"{install}/…"}，换电脑或换版本目录后也能找到。</p>
      <div className="row">
        <Button kind="primary" onClick={submit} disabled={!saves.length || !name.trim()}>
          添加
        </Button>
      </div>
    </div>
  );
}

function FromCloud({ overview, onAdded }: { overview: Overview; onAdded: (key: string) => void }) {
  const ui = useUi();
  const [games, setGames] = useState<CloudGame[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [linking, setLinking] = useState<CloudGame | null>(null);
  const { accounts, account, setAccount } = useAccounts();
  const [shortcuts, setShortcuts] = useState<ShortcutView[]>([]);

  useEffect(() => {
    if (overview.cloud.kind === "none") return;
    api.cloudGames().then(setGames, (e) => setError(errorText(e)));
  }, [overview.cloud.kind]);

  useEffect(() => {
    if (account) api.steamShortcuts(account).then(setShortcuts, () => setShortcuts([]));
  }, [account]);

  if (overview.cloud.kind === "none") return <div className="banner banner-info">请先在设置里连接云盘。</div>;
  if (error) return <div className="banner banner-bad">{error}</div>;
  if (!games) {
    return (
      <div className="center pad">
        <Spinner />
      </div>
    );
  }
  if (games.length === 0) {
    return (
      <Empty icon="cloud" title="没有需要关联的游戏">
        <p className="muted">云端的游戏都已经在本机添加过了。</p>
      </Empty>
    );
  }

  const link = async (cg: CloudGame, s: ShortcutView | null) => {
    try {
      let exe: string | null = null;
      if (!s) {
        exe = await api.pickExe();
        if (!exe) return;
      }
      const key = await api.addFromCloud(cg.key, s?.account ?? null, s?.appid ?? null, exe);
      ui.toast(`已关联「${cg.name}」，同步一次就会把云端存档拉下来`);
      onAdded(key);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  if (linking) {
    const free = shortcuts.filter((s) => !s.addedKey && !s.wrappedKey);
    return (
      <div className="add-list">
        <button className="link" onClick={() => setLinking(null)}>
          ← 返回
        </button>
        <div className="row space">
          <p>
            把云端的 <b>{linking.name}</b> 关联到本机哪个 Steam 快捷方式？
          </p>
          <AccountPicker accounts={accounts} account={account} setAccount={setAccount} />
        </div>
        {free.map((s) => (
          <div key={s.appid} className="add-item">
            {s.cover ? <img className="add-cover" src={s.cover} alt="" /> : <div className="add-cover fallback">{s.suggestedName.slice(0, 1)}</div>}
            <div className="add-info">
              <b>{s.suggestedName}</b>
              <code className="path small">{s.exe}</code>
            </div>
            <Button kind="primary" onClick={() => link(linking, s)}>
              关联
            </Button>
          </div>
        ))}
        <div className="row">
          <Button icon="folder" onClick={() => link(linking, null)}>
            不在 Steam 里，直接选择游戏程序…
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="add-list">
      <p className="muted small">其他设备上传过、但本机还没添加的游戏。关联到本机的游戏后，就能把存档拉下来。</p>
      {games.map((g) => (
        <div key={g.key} className="add-item">
          <div className="add-cover fallback">{g.name.slice(0, 1)}</div>
          <div className="add-info">
            <b>{g.name}</b>
            <span className="muted small">{g.adapter ? `内置规则：${g.adapter}` : g.saves.join("、")}</span>
          </div>
          <Button kind="primary" onClick={() => setLinking(g)}>
            关联到本机…
          </Button>
        </div>
      ))}
    </div>
  );
}

function FromLudusavi({ onAdded }: { onAdded: (key: string) => void }) {
  const ui = useUi();
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<string[] | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [files, setFiles] = useState<string[] | null>(null);

  const search = async () => {
    if (!query.trim()) return;
    setResults(null);
    setPicked(null);
    try {
      setResults(await api.ludusaviSearch(query));
    } catch (e) {
      ui.toast(errorText(e), "bad");
      setResults([]);
    }
  };

  const pick = async (t: string) => {
    setPicked(t);
    setFiles(null);
    try {
      setFiles(await api.ludusaviPreview(t));
    } catch (e) {
      ui.toast(errorText(e), "bad");
      setFiles([]);
    }
  };

  const add = async () => {
    if (!picked) return;
    try {
      const key = await api.addCustom(picked, null, [], picked);
      ui.toast(`已添加「${picked}」`);
      onAdded(key);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <div className="add-list">
      <p className="muted small">Ludusavi 收录了上万款 PC 游戏的存档位置（来自 PCGamingWiki）。适合普通 PC 游戏；模拟器游戏请用「Steam 快捷方式」或「手动添加」。第一次搜索会下载数据库，稍等片刻。</p>
      <div className="row">
        <input className="input" placeholder="输入游戏英文名，例如 Hollow Knight" value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.key === "Enter" && search()} />
        <Button kind="primary" icon="search" onClick={search}>
          搜索
        </Button>
      </div>
      {results && results.length === 0 && <p className="muted">没有找到。</p>}
      {results?.map((t) => (
        <div key={t} className={`add-item compact${picked === t ? " picked" : ""}`}>
          <div className="add-info">
            <b>{t}</b>
          </div>
          <Button small onClick={() => pick(t)}>
            查看存档
          </Button>
        </div>
      ))}
      {picked && (
        <div className="panel">
          <h3>{picked}</h3>
          {!files ? (
            <Spinner />
          ) : files.length === 0 ? (
            <p className="muted">本机没有找到这个游戏的存档（可能还没玩过或没安装）。仍然可以添加，以后找到存档时会自动同步。</p>
          ) : (
            <ul className="file-list">
              {files.slice(0, 30).map((f) => (
                <li key={f}>
                  <code>{f}</code>
                </li>
              ))}
              {files.length > 30 && <li className="muted">…共 {files.length} 个文件</li>}
            </ul>
          )}
          <Button kind="primary" icon="plus" onClick={add}>
            添加「{picked}」
          </Button>
        </div>
      )}
    </div>
  );
}

function Manual({ onAdded }: { onAdded: (key: string) => void }) {
  const ui = useUi();
  const [name, setName] = useState("");
  const [exe, setExe] = useState<string | null>(null);
  const [saves, setSaves] = useState<string[]>([]);
  const submit = async () => {
    try {
      const key = await api.addCustom(name.trim(), exe, saves, null);
      ui.toast(`已添加「${name}」`);
      onAdded(key);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };
  return (
    <div className="form">
      <label>
        <span>名称</span>
        <input className="input" value={name} placeholder="游戏名称" onChange={(e) => setName(e.target.value)} />
      </label>
      <label>
        <span>游戏程序（可选）</span>
        <div className="row">
          {exe ? <code className="path">{exe}</code> : <span className="muted small">不设置也可以，只是无法自动判断游戏是否在运行。</span>}
          <Button small icon="folder" onClick={async () => setExe((await api.pickExe(exe)) ?? exe)}>
            选择…
          </Button>
        </div>
      </label>
      <label>
        <span>存档位置</span>
        <SavesEditor saves={saves} setSaves={setSaves} exe={exe} />
      </label>
      <div className="row">
        <Button kind="primary" onClick={submit} disabled={!name.trim() || !saves.length}>
          添加
        </Button>
      </div>
    </div>
  );
}
