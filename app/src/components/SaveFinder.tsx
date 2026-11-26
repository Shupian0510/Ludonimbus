import { useEffect, useState } from "react";
import { api, errorText, SaveCandidate } from "../api";
import { bytes, relTime } from "../format";
import { msg, useT } from "../i18n";
import { Button, Hint, Icon, PathField, Spinner, useUi } from "./ui";

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

/** Folders that may hold saves, each with a checkbox. */
export function SaveCandidates({ list, isPicked, onToggle }: { list: SaveCandidate[]; isPicked: (path: string) => boolean; onToggle: (c: SaveCandidate) => void }) {
  const t = useT();
  const f = t.saveFinder;
  return (
    <>
      {list.map((c) => (
        <label key={c.path} className={`finder-item${isPicked(c.path) ? " picked" : ""}`}>
          <input type="checkbox" checked={isPicked(c.path)} onChange={() => onToggle(c)} />
          <div className="add-info">
            <code className="path small">{c.path}</code>
            <span className="muted small">
              {t.library.files(c.files)} · {bytes(c.bytes)}
              {c.modified ? ` · ${f.modified(relTime(c.modified))}` : ""} · {c.reason}
            </span>
            {c.sample.length > 0 && <span className="muted small">{f.sample(c.sample)}</span>}
          </div>
        </label>
      ))}
    </>
  );
}

/** Guided setup for a game whose save location Ludonimbus does not know. */
export function SaveFinder({
  game,
  onBack,
  onDone,
  submitText,
  onChangeExe,
}: {
  game: FinderGame;
  onBack: () => void;
  onDone: (c: FinderChoice) => Promise<void>;
  submitText?: string;
  /** Lets the user pick the game's program again. */
  onChangeExe?: (exe: string) => void;
}) {
  const ui = useUi();
  const t = useT();
  const f = t.saveFinder;
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
      if (found.length === 0) ui.toast(msg().saveFinder.nothingChanged, "bad");
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setWatching(false);
    }
  };

  const pickManual = async () => {
    const start = game.install ?? (game.exe ? game.exe.replace(/[\\/][^\\/]*$/, "") : null);
    const dir = await api.pickFolder(msg().saveFinder.pickTitle, start);
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

  const candidateList = (list: SaveCandidate[]) => <SaveCandidates list={list} isPicked={(p) => picked.has(p)} onToggle={toggle} />;

  return (
    <div className="finder">
      <button className="link" onClick={onBack}>
        {f.backToList}
      </button>
      <div className="form compact">
        <label>
          <span>{f.name}</span>
          <input className="input" value={name} placeholder={f.namePlaceholder} onChange={(e) => setName(e.target.value)} />
        </label>
        {(game.exe || onChangeExe) && (
          <div className="field">
            <span>{f.program}</span>
            <PathField
              path={game.exe}
              empty={f.noProgram}
              onPick={
                onChangeExe &&
                (async () => {
                  const exe = await api.pickExe(game.exe ?? game.install);
                  if (exe) onChangeExe(exe);
                })
              }
              pickText={game.exe ? undefined : t.game.pickExe}
            />
          </div>
        )}
      </div>

      <section className="finder-section">
        <h4>
          {f.s1Title} <Hint text={f.s1Text} />
        </h4>
        <div className="row">
          <input className="input" value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.key === "Enter" && search()} />
          <Button small icon="search" onClick={search}>
            {t.art.search}
          </Button>
        </div>
        {!titles ? (
          <Spinner />
        ) : titles.length === 0 ? (
          <p className="muted small">{f.noneFound}</p>
        ) : (
          <div className="chips">
            {titles.slice(0, 8).map((tt) => (
              <button key={tt} className={`chip${title === tt ? " on" : ""}`} onClick={() => setTitle(title === tt ? null : tt)}>
                {title === tt && <Icon name="check" size={12} />} {tt}
              </button>
            ))}
          </div>
        )}
      </section>

      <section className="finder-section">
        <h4>
          {f.s2Title} <Hint text={f.s2Text} />
        </h4>
        {!suggestions ? <Spinner /> : suggestions.length === 0 ? <p className="muted small">{f.s2None}</p> : candidateList(suggestions)}
      </section>

      <section className="finder-section">
        <h4>
          {f.s3Title} <Hint text={f.s3Text(!!game.exe)} />
        </h4>
        {watching ? (
          <div className="banner banner-info">
            <span className="spinner" />
            <span>{f.watching}</span>
            <Button small kind="primary" icon="check" onClick={stopWatch}>
              {f.done}
            </Button>
          </div>
        ) : (
          <div className="row">
            <Button small icon="play" onClick={startWatch}>
              {game.exe ? f.startWithGame : f.start}
            </Button>
          </div>
        )}
        {watched && watched.length > 0 && candidateList(watched)}
      </section>

      <section className="finder-section">
        <h4>{f.s4Title}</h4>
        <div className="row">
          <Button small icon="folder" onClick={pickManual}>
            {f.pickFolder}
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
          {picked.size > 0 ? f.picked(picked.size) : title ? f.usingLudusavi(title) : f.nothingPicked}
        </span>
        {big && <span className="health health-warn small">{f.big}</span>}
        <Button kind="primary" icon="plus" onClick={submit} disabled={(picked.size === 0 && !title) || !name.trim()}>
          {submitText ?? f.add}
        </Button>
      </div>
    </div>
  );
}
