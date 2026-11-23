import { useEffect, useState } from "react";
import { api, errorText, SaveCandidate } from "../api";
import { bytes, relTime } from "../format";
import { Button, Icon, Spinner, useUi } from "./ui";

export interface FinderGame {
  name: string;
  exe: string | null;
  install: string | null;
  args?: string;
}

export interface FinderChoice {
  name: string;
  saves: string[];
  ludusaviTitle: string | null;
}

const BIG = 200 * 1024 * 1024;

/** Guided setup for a game whose save location Ludonimbus does not know. */
export function SaveFinder({ game, onBack, onDone, submitText = "添加" }: { game: FinderGame; onBack: () => void; onDone: (c: FinderChoice) => Promise<void>; submitText?: string }) {
  const ui = useUi();
  const [name, setName] = useState(game.name);
  const [picked, setPicked] = useState<Map<string, SaveCandidate | null>>(new Map());
  const [suggestions, setSuggestions] = useState<SaveCandidate[] | null>(null);
  const [titles, setTitles] = useState<string[] | null>(null);
  const [title, setTitle] = useState<string | null>(null);
  const [query, setQuery] = useState(game.name);
  const [watching, setWatching] = useState(false);
  const [watched, setWatched] = useState<SaveCandidate[] | null>(null);

  useEffect(() => {
    api.saveSuggest(game.name, game.exe, game.install).then(setSuggestions, () => setSuggestions([]));
    if (game.name.trim()) api.ludusaviSearch(game.name).then(setTitles, () => setTitles([]));
    else setTitles([]);
  }, [game]);

  const toggle = (c: SaveCandidate) => {
    const next = new Map(picked);
    if (next.has(c.path)) next.delete(c.path);
    else next.set(c.path, c);
    setPicked(next);
  };

  const search = async () => {
    if (!query.trim()) return;
    setTitles(null);
    try {
      setTitles(await api.ludusaviSearch(query));
    } catch (e) {
      ui.toast(errorText(e), "bad");
      setTitles([]);
    }
  };

  const startWatch = async () => {
    try {
      await api.saveWatchStart(game.exe, game.args ?? null, game.install);
      setWatched(null);
      setWatching(true);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const stopWatch = async () => {
    try {
      const found = await api.saveWatchStop();
      setWatched(found);
      if (found.length === 0) ui.toast("没有发现明显的存档变化，可以再试一次，或者手动选择文件夹", "bad");
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setWatching(false);
    }
  };

  const pickManual = async () => {
    const start = game.install ?? (game.exe ? game.exe.replace(/[\\/][^\\/]*$/, "") : null);
    const dir = await api.pickFolder("选择存档文件夹", start);
    if (dir) setPicked(new Map(picked).set(dir, null));
  };

  const submit = async () => {
    try {
      const saves = await Promise.all([...picked.keys()].map((p) => api.folderToPattern(p, game.exe)));
      await onDone({ name: name.trim(), saves, ludusaviTitle: saves.length ? null : title });
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const big = [...picked.values()].some((c) => c && c.bytes > BIG);

  const candidateList = (list: SaveCandidate[]) =>
    list.map((c) => (
      <label key={c.path} className={`finder-item${picked.has(c.path) ? " picked" : ""}`}>
        <input type="checkbox" checked={picked.has(c.path)} onChange={() => toggle(c)} />
        <div className="add-info">
          <code className="path small">{c.path}</code>
          <span className="muted small">
            {c.files} 个文件 · {bytes(c.bytes)}
            {c.modified ? ` · ${relTime(c.modified)}修改` : ""} · {c.reason}
          </span>
          {c.sample.length > 0 && <span className="muted small">例如：{c.sample.join("、")}</span>}
        </div>
      </label>
    ));

  return (
    <div className="finder">
      <button className="link" onClick={onBack}>
        ← 返回列表
      </button>
      <div className="form compact">
        <label>
          <span>名称</span>
          <input className="input" value={name} placeholder="游戏名称" onChange={(e) => setName(e.target.value)} />
        </label>
        {game.exe && (
          <label>
            <span>游戏程序</span>
            <code className="path small">{game.exe}</code>
          </label>
        )}
      </div>

      <section className="finder-section">
        <h4>1. 在 Ludusavi 数据库里找</h4>
        <p className="muted small">收录了上万款 PC 游戏的存档位置。找到就不用自己选文件夹。</p>
        <div className="row">
          <input className="input" value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.key === "Enter" && search()} />
          <Button small icon="search" onClick={search}>
            搜索
          </Button>
        </div>
        {!titles ? (
          <Spinner />
        ) : titles.length === 0 ? (
          <p className="muted small">没有找到。</p>
        ) : (
          <div className="chips">
            {titles.slice(0, 8).map((t) => (
              <button key={t} className={`chip${title === t ? " on" : ""}`} onClick={() => setTitle(title === t ? null : t)}>
                {title === t && <Icon name="check" size={12} />} {t}
              </button>
            ))}
          </div>
        )}
      </section>

      <section className="finder-section">
        <h4>2. 常见的存档位置</h4>
        <p className="muted small">文档、保存的游戏、AppData 和游戏目录里名字相近的文件夹，最近修改的排在前面。</p>
        {!suggestions ? <Spinner /> : suggestions.length === 0 ? <p className="muted small">没有找到名字相近的文件夹。</p> : candidateList(suggestions)}
      </section>

      <section className="finder-section">
        <h4>3. 玩一下再找</h4>
        <p className="muted small">
          开始记录后{game.exe ? "会启动游戏" : "请自己打开游戏"}，进游戏玩一会儿并存一次档，退出游戏后点「完成」。Ludonimbus 会列出这段时间被写入的文件夹（已排除缓存、日志等）。
        </p>
        {watching ? (
          <div className="banner banner-info">
            <span className="spinner" />
            <span>正在记录文件变化…存档并退出游戏后点完成。</span>
            <Button small kind="primary" icon="check" onClick={stopWatch}>
              完成
            </Button>
          </div>
        ) : (
          <div className="row">
            <Button small icon="play" onClick={startWatch}>
              {game.exe ? "开始记录并启动游戏" : "开始记录"}
            </Button>
          </div>
        )}
        {watched && watched.length > 0 && candidateList(watched)}
      </section>

      <section className="finder-section">
        <h4>4. 自己选择文件夹</h4>
        <div className="row">
          <Button small icon="folder" onClick={pickManual}>
            选择文件夹…
          </Button>
        </div>
        {[...picked.entries()]
          .filter(([, c]) => c === null)
          .map(([p]) => (
            <div key={p} className="save-pill">
              <code>{p}</code>
              <button className="icon-btn" onClick={() => setPicked(new Map([...picked].filter(([k]) => k !== p)))}>
                <Icon name="x" size={14} />
              </button>
            </div>
          ))}
      </section>

      <div className="finder-foot">
        <span className="muted small">
          {picked.size > 0 ? `已选 ${picked.size} 个存档文件夹` : title ? `使用 Ludusavi 数据库：${title}` : "还没有选择存档位置"}
        </span>
        {big && <span className="health health-warn small">选中的文件夹超过 200 MB，可能选到了整个游戏目录</span>}
        <Button kind="primary" icon="plus" onClick={submit} disabled={(picked.size === 0 && !title) || !name.trim()}>
          {submitText}
        </Button>
      </div>
    </div>
  );
}
