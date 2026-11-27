import { useEffect, useRef, useState } from "react";
import { api, ArtKind, errorText, GameView, SgdbGame, SgdbImage } from "../api";
import { msg, useT } from "../i18n";
import { Button, Icon, Spinner, useUi } from "./ui";

const KINDS: ArtKind[] = ["cover", "wide", "hero", "logo", "icon"];

/** Enter the SteamGridDB API key (shown when none is saved). */
export function SgdbKeyForm({ keyPage, onSaved }: { keyPage: string; onSaved: () => void }) {
  const ui = useUi();
  const t = useT();
  const [key, setKey] = useState("");
  const save = async () => {
    try {
      await api.sgdbSetKey(key);
      ui.toast(msg().art.keySaved);
      onSaved();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };
  return (
    <div className="sgdb-key">
      <p className="muted small">
        {t.art.keyBefore}{" "}
        <button className="link small" onClick={() => api.openPath(keyPage)}>
          {t.art.keyLink}
        </button>{" "}
        {t.art.keyAfter}
      </p>
      <div className="row">
        <input className="input mono" type="password" value={key} onChange={(e) => setKey(e.target.value)} placeholder="SteamGridDB API Key" autoComplete="off" />
        <Button kind="primary" onClick={save} disabled={!key.trim()}>
          {t.common.save}
        </Button>
      </div>
    </div>
  );
}

/**
 * Pick artwork for a game: SteamGridDB images (applied on click) or local
 * files. `current` shows what the game has now.
 */
export function ArtPicker({ gameKey, name, current, onApplied }: { gameKey: string; name: string; current: GameView["art"] | null; onApplied: () => void }) {
  const ui = useUi();
  const t = useT();
  const [status, setStatus] = useState<{ hasKey: boolean; keyPage: string } | null>(null);
  const [query, setQuery] = useState(name);
  const [games, setGames] = useState<SgdbGame[] | null>(null);
  const [game, setGame] = useState<SgdbGame | null>(null);
  const [kind, setKind] = useState<ArtKind>("cover");
  const [images, setImages] = useState<SgdbImage[] | null>(null);
  const [applied, setApplied] = useState<Partial<Record<ArtKind, number | "local">>>({});
  const [busy, setBusy] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const loadStatus = () => api.sgdbStatus().then(setStatus, (e) => setError(errorText(e)));
  useEffect(() => {
    loadStatus();
  }, []);

  // Only the latest search shows (the automatic one and a typed one can overlap).
  const searchSeq = useRef(0);
  const search = async (term: string) => {
    const seq = ++searchSeq.current;
    setGames(null);
    setError(null);
    try {
      const list = await api.sgdbSearch(term);
      if (seq !== searchSeq.current) return;
      setGames(list);
      setGame(list[0] ?? null);
    } catch (e) {
      if (seq !== searchSeq.current) return;
      setError(errorText(e));
      setGames([]);
    }
  };

  useEffect(() => {
    if (status?.hasKey) search(name);
  }, [status?.hasKey, name]);

  useEffect(() => {
    if (!game) return setImages([]);
    // Switching tabs quickly: a slower answer for the previous kind must not
    // fill this one (a click would then set, say, a cover as the logo).
    let live = true;
    setImages(null);
    api.sgdbImages(game.id, kind).then(
      (list) => live && setImages(list),
      (e) => {
        if (!live) return;
        setError(errorText(e));
        setImages([]);
      },
    );
    return () => void (live = false);
  }, [game, kind]);

  const apply = async (img: SgdbImage) => {
    setBusy(img.id);
    try {
      await api.artSetUrl(gameKey, kind, img.url);
      setApplied({ ...applied, [kind]: img.id });
      onApplied();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setBusy(null);
    }
  };

  const local = async () => {
    const file = await api.pickImage();
    if (!file) return;
    try {
      await api.artSetFile(gameKey, kind, file);
      setApplied({ ...applied, [kind]: "local" });
      onApplied();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const currentUrl = current?.[kind] ?? null;

  return (
    <div className="art-picker">
      <div className="tabs tabs-tight">
        {KINDS.map((k) => (
          <button key={k} className={kind === k ? "on" : ""} onClick={() => setKind(k)}>
            {t.art.kinds[k].label}
            {applied[k] !== undefined && <Icon name="check" size={12} />}
          </button>
        ))}
      </div>
      <div className="row space">
        <span className="muted small">
          {t.art.kinds[kind].hint}
          {currentUrl ? t.art.currentIs : t.art.currentNone}
        </span>
        <Button small icon="folder" onClick={local}>
          {t.art.local}
        </Button>
      </div>
      {currentUrl && <img className={`art-current art-${kind}`} src={currentUrl} alt="" />}

      {!status ? (
        <Spinner />
      ) : !status.hasKey ? (
        <SgdbKeyForm keyPage={status.keyPage} onSaved={loadStatus} />
      ) : (
        <>
          <div className="row">
            <input className="input" value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.key === "Enter" && search(query)} placeholder={t.art.searchPlaceholder} />
            <Button small icon="search" onClick={() => search(query)}>
              {t.art.search}
            </Button>
          </div>
          {games && games.length > 1 && (
            <div className="chips">
              {games.slice(0, 8).map((g) => (
                <button key={g.id} className={`chip${game?.id === g.id ? " on" : ""}`} onClick={() => setGame(g)}>
                  {g.name}
                  {g.year ? t.art.year(g.year) : ""}
                </button>
              ))}
            </div>
          )}
          {error && <div className="health health-warn small">{error}</div>}
          {games && games.length === 0 && !error && <p className="muted small">{t.art.notFound}</p>}
          {!images ? (
            <Spinner />
          ) : images.length === 0 ? (
            game && <p className="muted small">{t.art.noImages}</p>
          ) : (
            <div className={`art-grid art-grid-${kind}`}>
              {images.slice(0, 24).map((img) => (
                <button key={img.id} className={`art-thumb${applied[kind] === img.id ? " on" : ""}`} onClick={() => apply(img)} disabled={busy !== null} title={`${img.width}×${img.height}${img.author ? ` · ${img.author}` : ""}`}>
                  <img src={img.thumb} alt="" loading="lazy" />
                  {busy === img.id && <span className="spinner" />}
                  {applied[kind] === img.id && (
                    <span className="art-tick">
                      <Icon name="check" size={14} />
                    </span>
                  )}
                </button>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}
