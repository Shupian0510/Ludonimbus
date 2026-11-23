import { useEffect, useState } from "react";
import { api, ArtKind, errorText, GameView, SgdbGame, SgdbImage } from "../api";
import { Button, Icon, Spinner, useUi } from "./ui";

const KINDS: { kind: ArtKind; label: string; hint: string }[] = [
  { kind: "cover", label: "竖版封面", hint: "游戏库网格里的封面" },
  { kind: "wide", label: "横版封面", hint: "最近游玩等横向位置" },
  { kind: "hero", label: "背景大图", hint: "游戏页顶部的横幅" },
  { kind: "logo", label: "Logo", hint: "叠在背景大图上的标题" },
  { kind: "icon", label: "图标", hint: "列表和任务栏里的小图标" },
];

/** Enter the SteamGridDB API key (shown when none is saved). */
export function SgdbKeyForm({ keyPage, onSaved }: { keyPage: string; onSaved: () => void }) {
  const ui = useUi();
  const [key, setKey] = useState("");
  const save = async () => {
    try {
      await api.sgdbSetKey(key);
      ui.toast("SteamGridDB API Key 已保存");
      onSaved();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };
  return (
    <div className="sgdb-key">
      <p className="muted small">
        封面来自 SteamGridDB 社区。需要一个免费的 API Key：登录 SteamGridDB 后在{" "}
        <button className="link small" onClick={() => api.openPath(keyPage)}>
          偏好设置 → API
        </button>{" "}
        里生成，粘贴到这里。Key 保存在 Windows 凭据管理器里。
      </p>
      <div className="row">
        <input className="input mono" type="password" value={key} onChange={(e) => setKey(e.target.value)} placeholder="SteamGridDB API Key" autoComplete="off" />
        <Button kind="primary" onClick={save} disabled={!key.trim()}>
          保存
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

  const search = async (term: string) => {
    setGames(null);
    setError(null);
    try {
      const list = await api.sgdbSearch(term);
      setGames(list);
      setGame(list[0] ?? null);
    } catch (e) {
      setError(errorText(e));
      setGames([]);
    }
  };

  useEffect(() => {
    if (status?.hasKey) search(name);
  }, [status?.hasKey, name]);

  useEffect(() => {
    if (!game) return setImages([]);
    setImages(null);
    api.sgdbImages(game.id, kind).then(setImages, (e) => {
      setError(errorText(e));
      setImages([]);
    });
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
          <button key={k.kind} className={kind === k.kind ? "on" : ""} onClick={() => setKind(k.kind)}>
            {k.label}
            {applied[k.kind] !== undefined && <Icon name="check" size={12} />}
          </button>
        ))}
      </div>
      <div className="row space">
        <span className="muted small">
          {KINDS.find((k) => k.kind === kind)?.hint}
          {currentUrl ? "，当前：" : "，当前还没有"}
        </span>
        <Button small icon="folder" onClick={local}>
          使用本地图片…
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
            <input className="input" value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.key === "Enter" && search(query)} placeholder="在 SteamGridDB 搜索游戏" />
            <Button small icon="search" onClick={() => search(query)}>
              搜索
            </Button>
          </div>
          {games && games.length > 1 && (
            <div className="chips">
              {games.slice(0, 8).map((g) => (
                <button key={g.id} className={`chip${game?.id === g.id ? " on" : ""}`} onClick={() => setGame(g)}>
                  {g.name}
                  {g.year ? `（${g.year}）` : ""}
                </button>
              ))}
            </div>
          )}
          {error && <div className="health health-warn small">{error}</div>}
          {games && games.length === 0 && !error && <p className="muted small">SteamGridDB 上没有找到，换个名字（例如英文名）再试。</p>}
          {!images ? (
            <Spinner />
          ) : images.length === 0 ? (
            game && <p className="muted small">这个游戏还没有这类图片。</p>
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
