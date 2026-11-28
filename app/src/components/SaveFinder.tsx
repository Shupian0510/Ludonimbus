import { useEffect, useState } from "react";
import { api, errorText, SaveCandidate, SaveTitle } from "../api";
import { bytes } from "../format";
import { msg, useT } from "../i18n";
import { Pill, Stat, When } from "./kit";
import { Badge, Button, Hint, Icon, PathField, Spinner, useUi } from "./ui";

/** A few of the files a folder holds, as names. */
function Samples({ names }: { names: string[] }) {
  return (
    <>
      {names.slice(0, 3).map((n) => (
        <code key={n} className="sample">
          {n}
        </code>
      ))}
    </>
  );
}

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
        <label key={c.pattern} className={`finder-item${isPicked(c.path) ? " picked" : ""}`}>
          <input type="checkbox" checked={isPicked(c.path)} onChange={() => onToggle(c)} />
          <div className="add-info">
            <div className="row">
              <code className="path small">{c.path}</code>
              {!c.pattern.endsWith("/**") && <code className="sample">{c.pattern.split("/").pop()}</code>}
              {c.likely && <Pill tone="ok">{f.likely}</Pill>}
            </div>
            <span className="add-facts">
              <span className="reason-tag">{c.reason}</span>
              {c.files > 0 ? (
                <>
                  <Stat icon="file">{t.library.files(c.files)}</Stat>
                  <span className="stat">{bytes(c.bytes)}</span>
                </>
              ) : (
                <span className="muted small">{f.noFilesYet}</span>
              )}
              {c.modified && (
                <Stat icon="clock" title={f.modifiedTitle}>
                  <When at={c.modified} />
                </Stat>
              )}
              <Samples names={c.sample} />
            </span>
          </div>
        </label>
      ))}
    </>
  );
}

/** The last folder name of a path, to name a game after its save folder. */
const folderName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? "";

/**
 * "Find the saves": one list of suggestions for a game whose save location
 * Ludonimbus doesn't know yet (its entry in Ludusavi's database, what its
 * engine and Steam say, folders named like it), best first, to tick. Nothing
 * is used until the user ticks it and adds the game.
 */
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
  /** Ticked folders, by path: a suggestion, or `null` for one the user chose. */
  const [picked, setPicked] = useState<Map<string, SaveCandidate | null>>(new Map());
  const [suggestions, setSuggestions] = useState<SaveCandidate[] | null>(null);
  const [titles, setTitles] = useState<SaveTitle[] | null>(null);
  const [searched, setSearched] = useState<SaveTitle[]>([]);
  const [title, setTitle] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [watching, setWatching] = useState(false);
  const [watched, setWatched] = useState<SaveCandidate[]>([]);

  // By value: callers build `game` anew on every render, and a search for the
  // same game shouldn't run again (nor an older one land after a newer one).
  useEffect(() => {
    let live = true;
    api.saveSuggest(game.name, game.exe, game.install).then(
      (s) => live && setSuggestions(s),
      () => live && setSuggestions([]),
    );
    if (game.name.trim() || game.exe)
      api.saveTitles(game.name, game.exe, game.install).then(
        (t) => live && setTitles(t),
        () => live && setTitles([]),
      );
    else setTitles([]);
    return () => void (live = false);
  }, [game.name, game.exe, game.install]);

  /** The game's entry in Ludusavi's database and folders exclude each other. */
  const pickTitle = (tt: string) => {
    const on = title !== tt;
    setTitle(on ? tt : null);
    if (on) {
      setPicked(new Map());
      if (!name.trim()) setName(tt);
    }
  };

  const toggle = (c: SaveCandidate) => {
    const next = new Map(picked);
    if (next.has(c.path)) next.delete(c.path);
    else next.set(c.path, c);
    setPicked(next);
    setTitle(null);
  };

  const search = async () => {
    if (!query.trim()) return;
    try {
      const found = await api.ludusaviSearch(query);
      const known = new Set((titles ?? []).map((x) => x.title));
      setSearched(found.filter((x) => !known.has(x)).slice(0, 6).map((x) => ({ title: x, likely: false, reason: f.searched, files: 0 })));
      if (found.length === 0) ui.toast(f.noneFound, "warn");
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const startWatch = async () => {
    try {
      await api.saveWatchStart(game.exe, game.args ?? null, game.install);
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
    if (!dir) return;
    setPicked(new Map(picked).set(dir, null));
    setTitle(null);
  };

  const submit = async () => {
    try {
      const saves = await Promise.all([...picked.entries()].map(([p, c]) => (c ? c.pattern : api.folderToPattern(p, game.exe))));
      const ludusaviTitle = saves.length ? null : title;
      // A name typed, or the database's title, or the save folder's.
      const first = [...picked.keys()][0];
      const fallback = ludusaviTitle ?? (first ? folderName(first) : "");
      await onDone({ name: name.trim() || fallback, saves, ludusaviTitle });
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const big = [...picked.values()].some((c) => c && c.bytes > BIG);
  const allTitles = [...(titles ?? []).filter((x) => x.likely), ...(titles ?? []).filter((x) => !x.likely).slice(0, 3), ...searched];
  const extra = [...watched, ...[...picked.entries()].filter(([, c]) => c === null).map(([p]) => chosen(p, f.chosen))];
  const loading = !suggestions || !titles;
  const nothing = !loading && allTitles.length === 0 && suggestions.length === 0 && extra.length === 0;

  return (
    <div className="finder">
      <button className="link" onClick={onBack}>
        {f.backToList}
      </button>
      <div className="form compact">
        <label>
          <span>{f.name}</span>
          <input className="input" value={name} placeholder={title ?? f.namePlaceholder} onChange={(e) => setName(e.target.value)} />
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
          {f.title} <Hint text={f.titleHint} />
        </h4>
        {allTitles.map((x) => (
          <label key={x.title} className={`finder-item${title === x.title ? " picked" : ""}`}>
            <input type="checkbox" checked={title === x.title} onChange={() => pickTitle(x.title)} />
            <div className="add-info">
              <div className="row">
                <b>{x.title}</b>
                <Badge tone="info">{f.ludusavi}</Badge>
                {x.likely && <Pill tone="ok">{f.likely}</Pill>}
              </div>
              <span className="add-facts">
                <span className="reason-tag">{x.reason}</span>
                {x.likely &&
                  (x.files > 0 ? (
                    <Stat icon="file" title={f.filesHere(x.files)}>
                      {t.library.files(x.files)}
                    </Stat>
                  ) : (
                    <span className="muted small">{f.noFilesYet}</span>
                  ))}
              </span>
            </div>
          </label>
        ))}
        {suggestions && <SaveCandidates list={[...suggestions, ...extra]} isPicked={(p) => picked.has(p)} onToggle={toggle} />}
        {loading && <Spinner />}
        {nothing && <p className="muted small">{f.none}</p>}
        {watching && (
          <div className="banner banner-info">
            <span className="spinner" />
            <span>{f.watching}</span>
            <Button small kind="primary" icon="check" onClick={stopWatch}>
              {f.done}
            </Button>
          </div>
        )}
        <div className="row">
          <input className="input" value={query} placeholder={f.searchPlaceholder} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.key === "Enter" && search()} />
          <button className="icon-btn" title={t.art.search} aria-label={t.art.search} onClick={search}>
            <Icon name="search" size={16} />
          </button>
        </div>
        <div className="row">
          {!watching && (
            <>
              <Button small icon="play" onClick={startWatch}>
                {f.watch}
              </Button>
              <Hint text={f.watchHint(!!game.exe)} />
            </>
          )}
          <Button small icon="folder" onClick={pickManual}>
            {f.pickFolder}
          </Button>
        </div>
      </section>

      <div className="finder-foot">
        <span className="muted small">{picked.size > 0 ? f.picked(picked.size) : title ? f.usingLudusavi(title) : f.nothingPicked}</span>
        {big && <span className="health health-warn small">{f.big}</span>}
        <Button kind="primary" icon="plus" onClick={submit} disabled={picked.size === 0 && !title}>
          {submitText ?? f.add}
        </Button>
      </div>
    </div>
  );
}

/** A folder the user chose, shown as a ticked row. */
function chosen(path: string, reason: string): SaveCandidate {
  return { path, pattern: `${path}/**`, files: 0, bytes: 0, modified: null, sample: [], reason, likely: false };
}
