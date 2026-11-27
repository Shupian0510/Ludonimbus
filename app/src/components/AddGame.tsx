import { useEffect, useMemo, useRef, useState } from "react";
import { AdapterInfo, api, CloudGame, errorText, Found, Overview, ShortcutView, SteamAccount } from "../api";
import { bytes } from "../format";
import { currentLang, msg, useT } from "../i18n";
import { scene, SCENE_ADD, sceneIs } from "../scene";
import { AddToSteam, SteamCandidate } from "./AddToSteam";
import { PlatformFolders } from "./PlatformFolders";
import { FinderGame, SaveFinder } from "./SaveFinder";
import { Badge, Button, Empty, Hint, Icon, Modal, PathField, Spinner, useUi } from "./ui";

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
  const t = useT();
  const a = t.addGame;
  const [tab, setTab] = useState<Tab>(SCENE_ADD[scene() ?? ""] ?? "scan");
  return (
    <Modal title={t.library.add} onClose={onClose} wide>
      <div className="tabs tabs-tight">
        <button className={tab === "scan" ? "on" : ""} onClick={() => setTab("scan")}>
          <Icon name="search" size={16} /> {a.tabScan}
        </button>
        <button className={tab === "cloud" ? "on" : ""} onClick={() => setTab("cloud")}>
          <Icon name="cloud" size={16} /> {a.tabCloud}
        </button>
        <button className={tab === "ludusavi" ? "on" : ""} onClick={() => setTab("ludusavi")}>
          <Icon name="library" size={16} /> {t.library.ludusavi}
        </button>
        <button className={tab === "manual" ? "on" : ""} onClick={() => setTab("manual")}>
          <Icon name="edit" size={16} /> {a.tabManual}
        </button>
        {/* What the tab in front is for. */}
        <Hint text={{ scan: a.scanIntro, cloud: a.cloudIntro, ludusavi: a.ludusaviIntro, manual: a.manualIntro }[tab]} />
      </div>
      {tab === "scan" && <ScanLocal overview={overview} onChanged={onChanged} onOpen={onAdded} />}
      {tab === "cloud" && <FromCloud overview={overview} onAdded={onAdded} />}
      {tab === "ludusavi" && <FromLudusavi onAdded={onAdded} />}
      {tab === "manual" && <Manual onAdded={onAdded} />}
    </Modal>
  );
}

export function useAccounts() {
  const [accounts, setAccounts] = useState<SteamAccount[]>([]);
  const [account, setAccount] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.steamAccounts().then(
      (r) => {
        setAccounts(r.accounts);
        setAccount(r.selected);
        if (!r.root) setError(msg().addToSteam.noSteam);
      },
      (e) => setError(errorText(e)),
    );
  }, []);
  return { accounts, account, setAccount, error };
}

export function AccountPicker({ accounts, account, setAccount }: { accounts: SteamAccount[]; account: string | null; setAccount: (a: string) => void }) {
  const t = useT();
  if (accounts.length < 2) return null;
  return (
    <select className="input select" value={account ?? ""} onChange={(e) => setAccount(e.target.value)}>
      {accounts.map((a) => (
        <option key={a.id} value={a.id}>
          {a.persona ?? a.accountName ?? a.id}{t.settings.shortcutCount(a.shortcutCount)}
        </option>
      ))}
    </select>
  );
}

function FromCloud({ overview, onAdded }: { overview: Overview; onAdded: (key: string) => void }) {
  const ui = useUi();
  const t = useT();
  const a = t.addGame;
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

  if (overview.cloud.kind === "none") return <div className="banner banner-info">{a.connectFirst}</div>;
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
      <Empty icon="cloud" title={a.cloudNoneTitle}>
        <p className="muted">{a.cloudNoneText}</p>
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
      ui.toast(msg().addGame.linked(cg.name));
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
          {a.back}
        </button>
        <div className="row space">
          <p>{a.linkWhich(linking.name)}</p>
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
              {a.link}
            </Button>
          </div>
        ))}
        <div className="row">
          <Button icon="folder" onClick={() => link(linking, null)}>
            {a.notInSteam}
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="add-list">
      {games.map((g) => (
        <div key={g.key} className="add-item">
          <div className="add-cover fallback">{g.name.slice(0, 1)}</div>
          <div className="add-info">
            <b>{g.name}</b>
            <span className="muted small">{g.adapter ? a.builtinRule(g.adapter) : g.saves.join(", ")}</span>
          </div>
          <Button kind="primary" onClick={() => setLinking(g)}>
            {a.linkHere}
          </Button>
        </div>
      ))}
    </div>
  );
}

function FromLudusavi({ onAdded }: { onAdded: (key: string) => void }) {
  const ui = useUi();
  const t = useT();
  const a = t.addGame;
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

  const pick = async (title: string) => {
    setPicked(title);
    setFiles(null);
    try {
      setFiles(await api.ludusaviPreview(title));
    } catch (e) {
      ui.toast(errorText(e), "bad");
      setFiles([]);
    }
  };

  const add = async () => {
    if (!picked) return;
    try {
      const key = await api.addCustom(picked, null, [], picked);
      ui.toast(msg().addGame.added(picked));
      onAdded(key);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <div className="add-list">
      <div className="row">
        <input className="input" placeholder={a.ludusaviPlaceholder} value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.key === "Enter" && search()} />
        <Button kind="primary" icon="search" onClick={search}>
          {t.art.search}
        </Button>
      </div>
      {results && results.length === 0 && <p className="muted">{t.saveFinder.noneFound}</p>}
      {results?.map((title) => (
        <div key={title} className={`add-item compact${picked === title ? " picked" : ""}`}>
          <div className="add-info">
            <b>{title}</b>
          </div>
          <Button small onClick={() => pick(title)}>
            {a.viewSaves}
          </Button>
        </div>
      ))}
      {picked && (
        <div className="panel">
          <h3>{picked}</h3>
          {!files ? (
            <Spinner />
          ) : files.length === 0 ? (
            <p className="muted">{a.noLocalSaves}</p>
          ) : (
            <ul className="file-list">
              {files.slice(0, 30).map((f) => (
                <li key={f}>
                  <code>{f}</code>
                </li>
              ))}
              {files.length > 30 && <li className="muted">{a.moreFiles(files.length)}</li>}
            </ul>
          )}
          <Button kind="primary" icon="plus" onClick={add}>
            {a.addNamed(picked)}
          </Button>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Scan this PC


/** Ludusavi's full scan is slow; keep its result while the app runs. */
let ludusaviCache: Found[] | null = null;

const normExe = (p: string | null) => (p ? p.replace(/\//g, "\\").toLowerCase() : null);


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
  const t = useT();
  const a = t.addGame;
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
    // Browser preview (docs screenshot): a scanned games folder.
    if (sceneIs("add-game-scan-folder")) api.scanFolder("D:\\Games").then(setFolders, () => undefined);
  }, []);

  const scanFolder = async () => {
    const dir = await api.pickFolder(msg().addGame.pickGamesFolder);
    if (!dir) return;
    try {
      const list = await api.scanFolder(dir);
      const known = new Set(folders.map((f) => f.id));
      setFolders([...folders, ...list.filter((f) => !known.has(f.id))]);
      ui.toast(list.length ? msg().addGame.foundIn(dir, list.length) : msg().addGame.noneIn, list.length ? "ok" : "warn");
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const library = useMemo(() => new Set([...overview.games.map((g) => g.key), ...Object.values(added)]), [overview.games, added]);

  const items = useMemo(() => {
    const merged = mergeFound([quick ?? [], folders], ludusavi ?? []);
    for (const f of merged) if (added[f.id]) f.addedKey = added[f.id];
    const q = filter.trim().toLowerCase();
    return merged.filter((f) => !q || f.name.toLowerCase().includes(q)).sort((x, y) => x.name.localeCompare(y.name, currentLang()));
  }, [quick, folders, ludusavi, added, filter]);

  const groups = useMemo(() => {
    const g: Record<Group, Found[]> = { ready: [], needs: [], cloud: [], registry: [], added: [] };
    for (const f of items) g[groupOf(f, library)].push(f);
    return g;
  }, [items, library]);

  // Browser preview (docs screenshots): open the save finder / "Add to Steam" once the list is there.
  const sceneOpened = useRef(false);
  useEffect(() => {
    if (sceneOpened.current || !quick || !ludusavi) return;
    if (sceneIs("save-finder")) {
      const f = items.find((x) => groupOf(x, library) === "needs");
      if (f) setFinder(f);
    } else if (sceneIs("add-to-steam")) {
      setToSteam(items.filter(steamable).slice(0, 3).map((f) => ({ key: f.id, name: f.name })));
    } else return;
    sceneOpened.current = true;
  }, [quick, ludusavi, items, library]);

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
        failed.push(`${f.name}: ${errorText(e)}`);
      }
    }
    setSelected(new Set());
    remember(done);
    if (done.length) ui.toast(msg().addGame.addedMany(done.length));
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
          {t.saveFinder.backToList}
        </button>
        <h3>{t.addToSteam.add}</h3>
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
          ui.toast(msg().addGame.added(c.name));
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
          <Badge tone="muted">{a.source[f.source]}</Badge>
          {f.adapterName && <Badge tone="ok">{a.recognized(f.adapterName)}</Badge>}
          {!f.adapterName && f.ludusaviTitle && <Badge tone="info">{a.ludusaviKnows}</Badge>}
          {f.storeGame && <Badge tone="muted">{a.steamInstalled}</Badge>}
          {f.source === "ludusavi" && f.installStore && f.exe && <Badge tone="ok">{a.foundLocation(a.store[f.installStore] ?? f.installStore)}</Badge>}
          {f.saveBytes > BIG_SAVES && <Badge tone="warn">{a.bigSaves}</Badge>}
        </div>
        {f.exe && <code className="path small">{f.exe}</code>}
        {f.saveFiles > 0 && (
          <span className="muted small">
            {a.localSaves(f.saveFiles)} · {bytes(f.saveBytes)}
            {f.saveSample[0] ? ` · ${f.saveSample[0]}` : ""}
          </span>
        )}
        {kind === "check" && f.source === "ludusavi" && !f.exe && !f.storeGame && (
          <span className="muted small">{a.noProgram}</span>
        )}
        {group === "cloud" && (
          <span className="muted small">{f.storeGame ? a.steamCloudInstalled : a.steamCloudFolder}</span>
        )}
      </div>
      {kind === "finder" && (
        <Button small icon="search" onClick={() => setFinder(f)}>
          {a.pickSaves}
        </Button>
      )}
      {kind === "added" && f.addedKey && (
        <Button small onClick={() => onOpen(f.addedKey!)}>
          {a.view}
        </Button>
      )}
    </div>
  );

  const readyIds = groups.ready.map((f) => f.id);
  const allReady = readyIds.length > 0 && readyIds.every((id) => selected.has(id));

  return (
    <div className="add-list scan">
      <div className="row">
        <Button small icon="folder" onClick={scanFolder}>
          {a.scanFolder}
        </Button>
      </div>

      {pendingSteam.length > 0 && (
        <div className="banner banner-info">
          <Icon name="steam" />
          <span>
            {pendingSteam.length === 1 ? a.notInSteamOne(pendingSteam[0].name) : a.notInSteamMany(pendingSteam.length)}
            {a.notInSteamHint}
          </span>
          <Button small kind="primary" onClick={() => setToSteam(pendingSteam.map((r) => ({ key: r.key, name: r.name })))}>
            {t.game.addSteam}
          </Button>
          <button className="icon-btn" title={t.cloud.migrate.no} onClick={() => setRecent([])}>
            <Icon name="x" size={14} />
          </button>
        </div>
      )}

      {scanning && (
        <div className="scan-progress">
          <span className="spinner" /> {a.scanning}
        </div>
      )}
      {ludusaviError && (
        <div className="banner banner-bad">
          {a.scanFailed(ludusaviError)}
          <Button small onClick={scanLudusavi}>
            {t.common.retry}
          </Button>
        </div>
      )}

      {!quick ? (
        <div className="center pad">
          <Spinner />
        </div>
      ) : items.length === 0 && !scanning && !filter ? (
        <Empty icon="search" title={a.noneTitle}>
          <p className="muted">{a.noneText}</p>
        </Empty>
      ) : (
        <>
          {items.length > 8 && <input className="input" placeholder={a.filter} value={filter} onChange={(e) => setFilter(e.target.value)} />}

          {groups.ready.length > 0 && (
            <section className="scan-group">
              <div className="row space">
                <h4>
                  {a.groupReady(groups.ready.length)} <Hint text={a.groupReadyHint} />
                </h4>
                <button className="link small" onClick={() => setSelected(allReady ? new Set() : new Set([...selected, ...readyIds]))}>
                  {allReady ? a.selectNone : a.selectAll}
                </button>
              </div>
              {groups.ready.map((f) => row(f, "check"))}
            </section>
          )}

          {groups.needs.length > 0 && (
            <section className="scan-group">
              <h4>
                {a.groupNeeds(groups.needs.length)} <Hint text={a.groupNeedsHint} />
              </h4>
              {groups.needs.map((f) => row(f, "finder"))}
            </section>
          )}

          {groups.cloud.length > 0 && (
            <details className="scan-group">
              <summary>
                {a.groupCloud(groups.cloud.length)} <Hint text={a.groupCloudHint} />
              </summary>
              {groups.cloud.map((f) => row(f, "check", "cloud"))}
            </details>
          )}

          {groups.registry.length > 0 && (
            <details className="scan-group">
              <summary>
                {a.groupRegistry(groups.registry.length)} <Hint text={a.groupRegistryHint} />
              </summary>
              {groups.registry.map((f) => row(f, "none"))}
            </details>
          )}

          {groups.added.length > 0 && (
            <details className="scan-group">
              <summary>{a.groupAdded(groups.added.length)}</summary>
              {groups.added.map((f) => row(f, "added"))}
            </details>
          )}
        </>
      )}

      {selected.size > 0 && (
        <div className="scan-foot">
          <span>{a.selected(selected.size)}</span>
          <Button kind="primary" icon="plus" onClick={addSelected} busyText={a.adding}>
            {a.addToLudonimbus}
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
  const t = useT();
  const a = t.addGame;
  const [game, setGame] = useState<FinderGame | null>(null);
  const [rule, setRule] = useState<AdapterInfo | null>(null);
  const [name, setName] = useState("");

  const pick = async () => {
    const exe = await api.pickExe(game?.exe);
    if (exe) await chooseExe(exe);
  };

  const chooseExe = async (exe: string) => {
    const adapter = await api.detectAdapter(exe).catch(() => null);
    setRule(adapter);
    setName(adapter?.name ?? nameFromExe(exe));
    setGame({ name: adapter?.name ?? nameFromExe(exe), exe, install: exe.replace(/[\\/][^\\/]*$/, "") });
  };

  const add = async (n: string, saves: string[], title: string | null) => {
    const key = await api.addCustom(n, game?.exe ?? null, saves, title);
    ui.toast(msg().addGame.added(n));
    onAdded(key);
  };

  if (!game) {
    return (
      <div className="form">
        <div className="row">
          <Button kind="primary" icon="folder" onClick={pick}>
            {a.pickProgram}
          </Button>
          <Button onClick={() => setGame({ name: "", exe: null, install: null })}>{a.noProgramButton}</Button>
        </div>
        <PlatformFolders onPick={chooseExe} />
      </div>
    );
  }
  if (rule) {
    return (
      <div className="form">
        <button className="link" onClick={() => setGame(null)}>
          {a.back}
        </button>
        <div className="banner banner-ok">
          <Icon name="check" /> {a.recognizedAs(rule.label)}
        </div>
        {rule.emulator && <p className="muted small">{a.emulatorWhole}</p>}
        <label>
          <span>{t.saveFinder.name}</span>
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <div className="field">
          <span>{t.saveFinder.program}</span>
          <PathField path={game.exe} onPick={pick} />
        </div>
        <div className="row">
          <Button kind="primary" icon="plus" onClick={() => add(name.trim(), [], null).catch((e) => ui.toast(errorText(e), "bad"))} disabled={!name.trim()}>
            {t.saveFinder.add}
          </Button>
        </div>
      </div>
    );
  }
  // Keyed by the program so picking another one starts the search over.
  return (
    <SaveFinder
      key={game.exe ?? ""}
      game={game}
      onBack={() => setGame(null)}
      onChangeExe={(exe) => void chooseExe(exe)}
      onDone={(c) => add(c.name, c.saves, c.ludusaviTitle)}
    />
  );
}
