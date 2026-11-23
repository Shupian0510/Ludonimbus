import { useEffect, useMemo, useState } from "react";
import { AdapterInfo, api, CloudGame, errorText, Found, Overview, ShortcutView, SteamAccount } from "../api";
import { bytes } from "../format";
import { AddToSteam, SteamCandidate } from "./AddToSteam";
import { PlatformFolders } from "./PlatformFolders";
import { FinderGame, SaveFinder } from "./SaveFinder";
import { Badge, Button, Empty, Icon, Modal, Spinner, useUi } from "./ui";

type Tab = "scan" | "cloud" | "ludusavi" | "manual";

export function AddGame({
  overview,
  onClose,
  onAdded,
  onChanged,
}: {
  overview: Overview;
  onClose: () => void;
  /** One game added: close and open it. */
  onAdded: (key: string) => void;
  /** Games added from the scan list, which stays open. */
  onChanged: (keys: string[]) => void | Promise<void>;
}) {
  const [tab, setTab] = useState<Tab>("scan");
  return (
    <Modal title="添加游戏" onClose={onClose} wide>
      <div className="tabs tabs-tight">
        <button className={tab === "scan" ? "on" : ""} onClick={() => setTab("scan")}>
          <Icon name="search" size={16} /> 扫描本机
        </button>
        <button className={tab === "cloud" ? "on" : ""} onClick={() => setTab("cloud")}>
          <Icon name="cloud" size={16} /> 云端已有
        </button>
        <button className={tab === "ludusavi" ? "on" : ""} onClick={() => setTab("ludusavi")}>
          <Icon name="library" size={16} /> Ludusavi 数据库
        </button>
        <button className={tab === "manual" ? "on" : ""} onClick={() => setTab("manual")}>
          <Icon name="edit" size={16} /> 手动添加
        </button>
      </div>
      {tab === "scan" && <ScanLocal overview={overview} onChanged={onChanged} onOpen={onAdded} />}
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

// ---------------------------------------------------------------------------
// Scan this PC

const SOURCE: Record<Found["source"], string> = {
  steam: "Steam 快捷方式",
  epic: "Epic",
  gog: "GOG",
  xbox: "Xbox",
  recomplauncher: "RecompLauncher",
  emulator: "模拟器里的存档",
  ludusavi: "找到存档",
  folder: "文件夹",
};

/** Ludusavi's full scan is slow; keep its result while the app runs. */
let ludusaviCache: Found[] | null = null;

const normExe = (p: string | null) => (p ? p.replace(/\//g, "\\").toLowerCase() : null);

const STORE: Record<string, string> = { epic: "Epic", gog: "GOG", xbox: "Xbox", registry: "已安装的程序", folder: "游戏文件夹" };

/**
 * One list from all scans: a game started the same way (exe and launch
 * options) once, one emulator game (same rule and saves) once, and a
 * Ludusavi hit folded into the store / folder entry of the same game (it
 * knows the saves, the other knows the exe).
 */
function mergeFound(lists: Found[][], ludusavi: Found[]): Found[] {
  const out: Found[] = [];
  const runs = new Map<string, Found>();
  const rules = new Map<string, Found>();
  for (const list of lists) {
    for (const f of list) {
      const exe = normExe(f.exe);
      const run = exe ? `${exe}|${f.args.trim()}` : null;
      const rule = f.adapter && f.saves?.length ? `${f.adapter}|${f.saves.join("|")}` : null;
      const prev = (run && runs.get(run)) || (rule && rules.get(rule));
      if (prev) {
        // Found in an emulator's data folder, then again with the emulator's exe: keep how to start it.
        if (!prev.exe && f.exe) {
          Object.assign(prev, { exe: f.exe, install: f.install, args: f.args });
          runs.set(run!, prev);
        }
        continue;
      }
      const copy = { ...f };
      out.push(copy);
      if (run) runs.set(run, copy);
      if (rule) rules.set(rule, copy);
    }
  }
  for (const l of ludusavi) {
    const title = l.ludusaviTitle?.toLowerCase();
    const exe = normExe(l.exe);
    const m = out.find(
      (f) => f.source !== "ludusavi" && ((exe && normExe(f.exe) === exe) || (title && f.ludusaviTitle?.toLowerCase() === title) || f.matchKey === l.matchKey),
    );
    if (!m) {
      out.push({ ...l });
      continue;
    }
    if (!m.adapter) m.ludusaviTitle = m.ludusaviTitle ?? l.ludusaviTitle;
    m.saveFiles = l.saveFiles;
    m.saveBytes = l.saveBytes;
    m.saveSample = l.saveSample;
    m.addedKey = m.addedKey ?? l.addedKey;
  }
  return out;
}

type Group = "ready" | "needs" | "cloud" | "registry" | "added";

function groupOf(f: Found, library: Set<string>): Group {
  if (f.addedKey && library.has(f.addedKey)) return "added";
  if (f.registryOnly && !f.adapter) return "registry";
  if (f.steamCloud) return "cloud";
  if (f.adapter || f.ludusaviTitle || f.saves?.length) return "ready";
  return "needs";
}

/** Xbox app games start through their launch helper; their own exe is protected. */
const xboxProtected = (f: Found) => (f.source === "xbox" || f.installStore === "xbox") && !/gamelaunchhelper\.exe$/i.test(f.exe ?? "");

/** Games that can get a Steam shortcut made by Ludonimbus. */
const steamable = (f: Found) => f.source !== "steam" && !f.storeGame && !!f.exe && !xboxProtected(f);

const BIG_SAVES = 200 * 1024 * 1024;

function ScanLocal({ overview, onChanged, onOpen }: { overview: Overview; onChanged: (keys: string[]) => void | Promise<void>; onOpen: (key: string) => void }) {
  const ui = useUi();
  const [quick, setQuick] = useState<Found[] | null>(null);
  const [folders, setFolders] = useState<Found[]>([]);
  const [ludusavi, setLudusavi] = useState<Found[] | null>(ludusaviCache);
  const [ludusaviError, setLudusaviError] = useState<string | null>(null);
  const [scanning, setScanning] = useState(false);
  const [added, setAdded] = useState<Record<string, string>>({});
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  const [finder, setFinder] = useState<Found | null>(null);
  const [recent, setRecent] = useState<{ key: string; name: string; steamable: boolean }[]>([]);
  const [toSteam, setToSteam] = useState<SteamCandidate[] | null>(null);

  const scanLudusavi = async () => {
    setScanning(true);
    setLudusaviError(null);
    try {
      const list = await api.scanLudusavi();
      ludusaviCache = list;
      setLudusavi(list);
    } catch (e) {
      setLudusaviError(errorText(e));
    } finally {
      setScanning(false);
    }
  };

  useEffect(() => {
    api.scanQuick().then(setQuick, (e) => {
      ui.toast(errorText(e), "bad");
      setQuick([]);
    });
    if (!ludusaviCache) scanLudusavi();
  }, []);

  const scanFolder = async () => {
    const dir = await api.pickFolder("选择放游戏的文件夹（例如 D:\\Games）");
    if (!dir) return;
    try {
      const list = await api.scanFolder(dir);
      const known = new Set(folders.map((f) => f.id));
      setFolders([...folders, ...list.filter((f) => !known.has(f.id))]);
      ui.toast(list.length ? `在 ${dir} 里找到 ${list.length} 个游戏` : "这个文件夹里没有找到游戏程序", list.length ? "ok" : "warn");
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const library = useMemo(() => new Set([...overview.games.map((g) => g.key), ...Object.values(added)]), [overview.games, added]);

  const items = useMemo(() => {
    const merged = mergeFound([quick ?? [], folders], ludusavi ?? []);
    for (const f of merged) if (added[f.id]) f.addedKey = added[f.id];
    const q = filter.trim().toLowerCase();
    return merged.filter((f) => !q || f.name.toLowerCase().includes(q)).sort((a, b) => a.name.localeCompare(b.name, "zh-CN"));
  }, [quick, folders, ludusavi, added, filter]);

  const groups = useMemo(() => {
    const g: Record<Group, Found[]> = { ready: [], needs: [], cloud: [], registry: [], added: [] };
    for (const f of items) g[groupOf(f, library)].push(f);
    return g;
  }, [items, library]);

  const remember = (pairs: { f: Found; key: string; name: string }[]) => {
    if (!pairs.length) return;
    setAdded((a) => ({ ...a, ...Object.fromEntries(pairs.map((p) => [p.f.id, p.key])) }));
    for (const p of pairs) {
      const c = ludusaviCache?.find((l) => l.id === p.f.id);
      if (c) c.addedKey = p.key;
    }
    setRecent((r) => [...r, ...pairs.map((p) => ({ key: p.key, name: p.name, steamable: steamable(p.f) }))]);
    onChanged(pairs.map((p) => p.key));
  };

  const addSelected = async () => {
    const list = items.filter((f) => selected.has(f.id));
    const done: { f: Found; key: string; name: string }[] = [];
    const failed: string[] = [];
    for (const f of list) {
      try {
        done.push({ f, key: await api.addFound(f), name: f.name });
      } catch (e) {
        failed.push(`${f.name}：${errorText(e)}`);
      }
    }
    setSelected(new Set());
    remember(done);
    if (done.length) ui.toast(`已添加 ${done.length} 个游戏`);
    if (failed.length) ui.toast(failed.join("\n"), "bad");
  };

  const toggle = (id: string) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setSelected(next);
  };

  if (toSteam) {
    return (
      <div className="add-list">
        <button className="link" onClick={() => setToSteam(null)}>
          ← 返回列表
        </button>
        <h3>加入 Steam</h3>
        <AddToSteam
          games={toSteam}
          onCancel={() => setToSteam(null)}
          onDone={(keys) => {
            setRecent((r) => r.map((x) => (keys.includes(x.key) ? { ...x, steamable: false } : x)));
            setToSteam(null);
            onChanged(keys);
          }}
        />
      </div>
    );
  }

  if (finder) {
    const game: FinderGame = { name: finder.name, exe: finder.exe, install: finder.install, args: finder.args };
    return (
      <SaveFinder
        game={game}
        onBack={() => setFinder(null)}
        onDone={async (c) => {
          const key = await api.addFound(finder, { name: c.name, saves: c.saves, ludusaviTitle: c.ludusaviTitle });
          ui.toast(`已添加「${c.name}」`);
          remember([{ f: finder, key, name: c.name }]);
          setFinder(null);
        }}
      />
    );
  }

  const pendingSteam = recent.filter((r) => r.steamable);
  const row = (f: Found, kind: "check" | "finder" | "added" | "none", group?: Group) => (
    <div key={f.id} className={`add-item compact${selected.has(f.id) ? " picked" : ""}`}>
      {kind === "check" && <input type="checkbox" checked={selected.has(f.id)} onChange={() => toggle(f.id)} />}
      <div className="add-info" onClick={() => kind === "check" && toggle(f.id)}>
        <div className="row">
          <b>{f.name}</b>
          <Badge tone="muted">{SOURCE[f.source]}</Badge>
          {f.adapterName && <Badge tone="ok">已识别：{f.adapterName}</Badge>}
          {!f.adapterName && f.ludusaviTitle && <Badge tone="info">Ludusavi 知道存档位置</Badge>}
          {f.storeGame && <Badge tone="muted">Steam 已安装</Badge>}
          {f.source === "ludusavi" && f.installStore && f.exe && <Badge tone="ok">找到游戏位置（{STORE[f.installStore] ?? f.installStore}）</Badge>}
          {f.saveBytes > BIG_SAVES && <Badge tone="warn">存档很大，可能含有缓存</Badge>}
        </div>
        {f.exe && <code className="path small">{f.exe}</code>}
        {f.saveFiles > 0 && (
          <span className="muted small">
            本机有 {f.saveFiles} 个存档文件 · {bytes(f.saveBytes)}
            {f.saveSample[0] ? ` · ${f.saveSample[0]}` : ""}
          </span>
        )}
        {kind === "check" && f.source === "ludusavi" && !f.exe && !f.storeGame && (
          <span className="muted small">没有找到游戏程序（可能已卸载）。存档照样能同步，以后可以在详情页设置游戏位置。</span>
        )}
        {group === "cloud" && (
          <span className="muted small">{f.storeGame ? "Steam 已安装这个游戏，Steam 云在同步它的存档" : "存档都在 Steam 云的文件夹里"}</span>
        )}
      </div>
      {kind === "finder" && (
        <Button small icon="search" onClick={() => setFinder(f)}>
          指定存档…
        </Button>
      )}
      {kind === "added" && f.addedKey && (
        <Button small onClick={() => onOpen(f.addedKey!)}>
          查看
        </Button>
      )}
    </div>
  );

  const readyIds = groups.ready.map((f) => f.id);
  const allReady = readyIds.length > 0 && readyIds.every((id) => selected.has(id));

  return (
    <div className="add-list scan">
      <div className="row space">
        <p className="muted small">Steam 里的非 Steam 游戏、Epic / GOG / Xbox 安装的游戏，以及 Ludusavi 在本机找到存档的游戏。放在别处的游戏可以扫描文件夹。</p>
        <div className="row">
          <Button small icon="folder" onClick={scanFolder}>
            扫描文件夹…
          </Button>
        </div>
      </div>

      {pendingSteam.length > 0 && (
        <div className="banner banner-info">
          <Icon name="steam" />
          <span>
            {pendingSteam.length === 1 ? `「${pendingSteam[0].name}」还不在 Steam 里。` : `刚添加的 ${pendingSteam.length} 个游戏还不在 Steam 里。`}
            加入后从 Steam 启动就会自动同步存档。
          </span>
          <Button small kind="primary" onClick={() => setToSteam(pendingSteam.map((r) => ({ key: r.key, name: r.name })))}>
            加入 Steam…
          </Button>
          <button className="icon-btn" title="不用了" onClick={() => setRecent([])}>
            <Icon name="x" size={14} />
          </button>
        </div>
      )}

      {scanning && (
        <div className="scan-progress">
          <span className="spinner" /> 正在用 Ludusavi 查找本机所有游戏的存档，大约需要半分钟…
        </div>
      )}
      {ludusaviError && (
        <div className="banner banner-bad">
          Ludusavi 扫描失败：{ludusaviError}
          <Button small onClick={scanLudusavi}>
            重试
          </Button>
        </div>
      )}

      {!quick ? (
        <div className="center pad">
          <Spinner />
        </div>
      ) : items.length === 0 && !scanning && !filter ? (
        <Empty icon="search" title="没有找到游戏">
          <p className="muted">可以扫描放游戏的文件夹，或者用「手动添加」。</p>
        </Empty>
      ) : (
        <>
          {items.length > 8 && <input className="input" placeholder="筛选游戏名" value={filter} onChange={(e) => setFilter(e.target.value)} />}

          {groups.ready.length > 0 && (
            <section className="scan-group">
              <div className="row space">
                <h4>可以直接添加（{groups.ready.length}）</h4>
                <button className="link small" onClick={() => setSelected(allReady ? new Set() : new Set([...selected, ...readyIds]))}>
                  {allReady ? "全不选" : "全选"}
                </button>
              </div>
              <p className="muted small">已经知道存档在哪里：内置规则或 Ludusavi 数据库。</p>
              {groups.ready.map((f) => row(f, "check"))}
            </section>
          )}

          {groups.needs.length > 0 && (
            <section className="scan-group">
              <h4>需要指定存档位置（{groups.needs.length}）</h4>
              <p className="muted small">Ludonimbus 还不知道这些游戏把存档放在哪里，点「指定存档」一步步找。</p>
              {groups.needs.map((f) => row(f, "finder"))}
            </section>
          )}

          {groups.cloud.length > 0 && (
            <details className="scan-group">
              <summary>
                Steam 云已经在同步（{groups.cloud.length}）<span className="muted small"> — 一般不需要再添加，想跨平台备份也可以勾选</span>
              </summary>
              {groups.cloud.map((f) => row(f, "check", "cloud"))}
            </details>
          )}

          {groups.registry.length > 0 && (
            <details className="scan-group">
              <summary>
                存档在注册表里（{groups.registry.length}）<span className="muted small"> — 暂不支持同步注册表</span>
              </summary>
              {groups.registry.map((f) => row(f, "none"))}
            </details>
          )}

          {groups.added.length > 0 && (
            <details className="scan-group">
              <summary>已经添加（{groups.added.length}）</summary>
              {groups.added.map((f) => row(f, "added"))}
            </details>
          )}
        </>
      )}

      {selected.size > 0 && (
        <div className="scan-foot">
          <span>已选 {selected.size} 个游戏</span>
          <Button kind="primary" icon="plus" onClick={addSelected} busyText="正在添加…">
            添加到 Ludonimbus
          </Button>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Manual

const GENERIC_DIRS = ["bin", "bin64", "binaries", "win64", "win32", "x64", "x86", "game", "client"];

/** A name for a game from its exe: the closest folder that is not a generic one. */
function nameFromExe(exe: string): string {
  const parts = exe.split(/[\\/]/).slice(0, -1);
  for (let i = parts.length - 1; i >= 0; i--) {
    if (parts[i] && !GENERIC_DIRS.includes(parts[i].toLowerCase()) && !parts[i].endsWith(":")) return parts[i];
  }
  return exe.split(/[\\/]/).pop()!.replace(/\.exe$/i, "");
}

function Manual({ onAdded }: { onAdded: (key: string) => void }) {
  const ui = useUi();
  const [game, setGame] = useState<FinderGame | null>(null);
  const [rule, setRule] = useState<AdapterInfo | null>(null);
  const [name, setName] = useState("");

  const pick = async () => {
    const exe = await api.pickExe();
    if (exe) await chooseExe(exe);
  };

  const chooseExe = async (exe: string) => {
    const adapter = await api.detectAdapter(exe).catch(() => null);
    setRule(adapter);
    setName(adapter?.name ?? nameFromExe(exe));
    setGame({ name: adapter?.name ?? nameFromExe(exe), exe, install: exe.replace(/[\/][^\/]*$/, "") });
  };

  const add = async (n: string, saves: string[], title: string | null) => {
    const key = await api.addCustom(n, game?.exe ?? null, saves, title);
    ui.toast(`已添加「${n}」`);
    onAdded(key);
  };

  if (!game) {
    return (
      <div className="form">
        <p className="muted small">先选择游戏程序：Ludonimbus 用它判断游戏是否在运行、在游戏目录附近找存档，以后也能一键加入 Steam。模拟器请选择模拟器程序，会自动套用内置规则。</p>
        <div className="row">
          <Button kind="primary" icon="folder" onClick={pick}>
            选择游戏程序…
          </Button>
          <Button onClick={() => setGame({ name: "", exe: null, install: null })}>没有程序，直接指定存档</Button>
        </div>
        <PlatformFolders onPick={chooseExe} />
      </div>
    );
  }
  if (rule) {
    return (
      <div className="form">
        <button className="link" onClick={() => setGame(null)}>
          ← 返回
        </button>
        <div className="banner banner-ok">
          <Icon name="check" /> 识别为 {rule.label}，会使用内置的存档规则，不用自己找存档。
        </div>
        {rule.emulator && (
          <p className="muted small">
            直接选模拟器程序时，模拟器里所有游戏的存档会一起同步。想按游戏分开同步：在 Steam 里给每个游戏建一个带游戏文件的快捷方式（例如用 Steam ROM Manager），再从「扫描本机」添加，Ludonimbus 会认出是哪个游戏。
          </p>
        )}
        <label>
          <span>名称</span>
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label>
          <span>游戏程序</span>
          <code className="path">{game.exe}</code>
        </label>
        <div className="row">
          <Button kind="primary" icon="plus" onClick={() => add(name.trim(), [], null).catch((e) => ui.toast(errorText(e), "bad"))} disabled={!name.trim()}>
            添加
          </Button>
        </div>
      </div>
    );
  }
  return <SaveFinder game={game} onBack={() => setGame(null)} onDone={(c) => add(c.name, c.saves, c.ludusaviTitle)} />;
}
