// Achievements in the app: the game page's "Achievements" tab, the
// Settings panel (RetroAchievements account, popups) and the small fallback
// popup window (`index.html?toast`). Backend: app/src-tauri/src/achievements.rs.
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorText, inTauri } from "../api";
import { AchievementItem, achievementsApi, AchievementSettings, AchievementsView, Candidate, RaGame, ToastPayload } from "../achievementsApi";
import { relTime } from "../format";
import { msg, useT } from "../i18n";
import { Badge, Button, Hint, Icon, Spinner, Toggle, useUi } from "./ui";

const RA_KEYS_URL = "https://retroachievements.org/settings";

function useAchievementsChanged(key: string | null, reload: () => void) {
  useEffect(() => {
    if (!inTauri) return;
    let off: (() => void) | undefined;
    listen<string>("achievements-changed", (e) => {
      if (!key || e.payload === key) reload();
    }).then((u) => (off = u));
    return () => off?.();
  }, [key, reload]);
}

function open(url: string) {
  if (inTauri) api.openPath(url).catch(() => {});
  else window.open(url, "_blank");
}

// ---------------------------------------------------------------------------
// Game page tab

export function GameAchievements({ gameKey }: { gameKey: string }) {
  const t = useT();
  const a = t.achievements;
  const ui = useUi();
  const [view, setView] = useState<AchievementsView | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    achievementsApi.view(gameKey).then(setView, (e) => ui.toast(errorText(e), "bad"));
  }, [gameKey, ui]);
  useEffect(load, [load]);
  useAchievementsChanged(gameKey, load);

  // The game's icon, for achievements without one: undefined while loading.
  const [gameIcon, setGameIcon] = useState<string | null | undefined>(undefined);
  const wantGameIcon = !!view?.gameIcon;
  useEffect(() => {
    if (!wantGameIcon) return;
    let alive = true;
    setGameIcon(undefined);
    achievementsApi.gameIcon(gameKey).then(
      (u) => alive && setGameIcon(u ?? null),
      () => alive && setGameIcon(null),
    );
    return () => {
      alive = false;
    };
  }, [gameKey, wantGameIcon]);

  if (!view) return <Spinner />;
  if (!view.source) return <LinkSource gameKey={gameKey} onLinked={setView} />;

  const pct = Math.floor((100 * view.unlocked) / Math.max(1, view.total));
  const noNames = view.source === "lantern" && view.items.every((i) => !i.description);

  const refresh = async () => {
    setBusy(true);
    try {
      setView(await achievementsApi.refresh(gameKey));
      ui.toast(msg().achievements.refreshed);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setBusy(false);
    }
  };
  const unlink = async () => {
    const ok = await ui.confirm({ title: a.unlink, text: a.unlinkConfirm, confirm: a.unlink, danger: true });
    if (!ok) return;
    await achievementsApi.unlink(gameKey);
    ui.toast(msg().achievements.unlinked);
    load();
  };
  const setPopup = async (v: boolean) => {
    await achievementsApi.setPopup(gameKey, v);
    setView({ ...view, popup: v });
  };

  return (
    <div className="ach">
      <section className="panel">
        <div className="panel-head">
          <Icon name="star" />
          <h3>{a.progress(view.unlocked, view.total)}</h3>
          {view.pointsTotal > 0 && <Badge tone="info">{a.points(view.points, view.pointsTotal)}</Badge>}
          <div className="grow" />
          <Button small kind="ghost" icon="sync" onClick={refresh} disabled={busy}>
            {a.refresh}
          </Button>
          <Button small kind="ghost" icon="x" onClick={unlink}>
            {a.unlink}
          </Button>
        </div>
        <div className="ach-progress">
          <div style={{ width: `${pct}%` }} />
        </div>
        <p className="muted small">
          {view.sourceLabel && a.from(view.sourceLabel + (view.title ? ` · ${view.title}` : ""))}
          {view.checkedAt && ` · ${a.checked(relTime(view.checkedAt))}`}
          {noNames && (
            <>
              {" "}
              <Hint text={a.noNames} />
            </>
          )}
        </p>
        {view.lastError && <p className="health health-warn">{view.lastError}</p>}
        <div className="ach-popup">
          <Toggle checked={view.popup} onChange={setPopup} />
          <span>{a.popup}</span>
          <Hint text={`${a.popupHint} ${a.popupHints[view.source] ?? ""}`.trim()} />
        </div>
      </section>
      {view.items.length === 0 ? (
        <p className="muted">{a.empty}</p>
      ) : (
        <div className="ach-list">
          {view.items.map((i) => (
            <Row key={i.id} gameKey={gameKey} item={i} fallback={view.gameIcon ? gameIcon : null} />
          ))}
        </div>
      )}
    </div>
  );
}

/** `fallback`: the game's icon, for achievements without one (undefined while loading). */
function Row({ gameKey, item, fallback }: { gameKey: string; item: AchievementItem; fallback: string | null | undefined }) {
  const a = useT().achievements;
  const [src, setSrc] = useState<string | null>(item.iconUrl);
  const [failed, setFailed] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  // Local icons are read when the row scrolls into view.
  useEffect(() => {
    if (item.iconUrl || !item.hasIcon || !ref.current) return;
    const el = ref.current;
    const io = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        io.disconnect();
        achievementsApi.icon(gameKey, item.id).then(
          (u) => (u ? setSrc(u) : setFailed(true)),
          () => setFailed(true),
        );
      }
    });
    io.observe(el);
    return () => io.disconnect();
  }, [gameKey, item.id, item.iconUrl, item.hasIcon]);

  // Its own icon, else the game's, else a star.
  const own = !!(item.iconUrl || item.hasIcon) && !failed;
  const shown = own ? src : fallback;
  const locked = !item.unlockedAt;
  const secret = locked && item.hidden;
  return (
    <div className={`ach-row${locked ? " locked" : ""}`} ref={ref}>
      {shown ? (
        <img src={shown} alt="" onError={() => setFailed(true)} />
      ) : (
        <div className="ach-noicon">{!own && fallback === null && <Icon name="star" size={24} />}</div>
      )}
      <div className="ach-text">
        <div className="ach-name">
          {secret ? a.hidden : item.name}
          {item.grade && <span className={`ach-tag ach-${item.grade}`}>{a.grades[item.grade]}</span>}
          {item.points != null && <span className="ach-tag">{a.pts(item.points)}</span>}
          {item.hardcore && <span className="ach-tag">{a.hardcore}</span>}
        </div>
        <div className="muted small">{secret ? a.hiddenDesc : item.description}</div>
      </div>
      {item.unlockedAt && (
        <div className="ach-meta muted small">
          <div>{new Date(item.unlockedAt).toLocaleString()}</div>
          {item.deviceName && <div>{a.onDevice(item.deviceName)}</div>}
        </div>
      )}
    </div>
  );
}

function LinkSource({ gameKey, onLinked }: { gameKey: string; onLinked: (v: AchievementsView) => void }) {
  const t = useT();
  const a = t.achievements;
  const ui = useUi();
  const [candidates, setCandidates] = useState<Candidate[] | null>(null);
  const [settings, setSettings] = useState<AchievementSettings | null>(null);
  const [raGames, setRaGames] = useState<RaGame[] | null>(null);
  const [raId, setRaId] = useState("");
  const [working, setWorking] = useState(false);

  useEffect(() => {
    achievementsApi.candidates(gameKey).then(setCandidates, () => setCandidates([]));
    achievementsApi.settings().then(setSettings, () => {});
  }, [gameKey]);

  const link = async (fn: () => Promise<AchievementsView>) => {
    setWorking(true);
    try {
      onLinked(await fn());
      ui.toast(msg().achievements.linked);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setWorking(false);
    }
  };
  const linkRa = (id: string) =>
    link(() => achievementsApi.link(gameKey, { source: "retroachievements", sourceId: id.trim(), path: null, popup: null }));
  const pick = async () => {
    const path = await achievementsApi.pickFile();
    if (path) await link(() => achievementsApi.linkFile(gameKey, path));
  };
  const loadRa = async () => {
    try {
      setRaGames(await achievementsApi.raRecentGames());
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <div className="ach">
      <section className="panel">
        <div className="panel-head">
          <Icon name="star" />
          <h3>{a.noSource}</h3>
          <Hint text={a.noSourceHint} />
        </div>
        {candidates === null && <Spinner />}
        {candidates && candidates.length > 0 && (
          <>
            <h4>{a.found}</h4>
            {candidates.map((c) => (
              <div className="ach-candidate" key={`${c.binding.source}:${c.binding.sourceId}:${c.location}`}>
                <div>
                  <div>
                    {c.title ?? c.binding.sourceId} <Badge tone="muted">{c.binding.source}</Badge>
                  </div>
                  <code className="path">{c.location}</code>
                </div>
                <Button small kind="primary" icon="check" disabled={working} onClick={() => link(() => achievementsApi.link(gameKey, c.binding))}>
                  {a.link}
                </Button>
              </div>
            ))}
          </>
        )}
        <div className="row">
          <Button small icon="folder" onClick={pick} disabled={working}>
            {a.pickFile}
          </Button>
          <Hint text={a.pickFileHint} />
        </div>
      </section>

      <section className="panel">
        <div className="panel-head">
          <Icon name="star" />
          <h3>{a.ra}</h3>
          <Hint text={a.raHint} />
        </div>
        {settings && !settings.raConnected ? (
          <p className="muted small">{a.raNeedsAccount}</p>
        ) : (
          <>
            {raGames === null ? (
              <div className="row">
                <Button small icon="search" onClick={loadRa}>
                  {a.raLoad}
                </Button>
              </div>
            ) : (
              <>
                <h4>{a.raRecent}</h4>
                {raGames.map((g) => (
                  <div className="ach-candidate" key={g.id}>
                    <div>
                      <div>
                        {g.title} <Badge tone="muted">{g.console}</Badge>
                      </div>
                      <span className="muted small">
                        {a.raPlayed(g.achieved, g.total)}
                        {g.lastPlayed && ` · ${relTime(g.lastPlayed)}`}
                      </span>
                    </div>
                    <Button small kind="primary" icon="check" disabled={working} onClick={() => linkRa(g.id)}>
                      {a.link}
                    </Button>
                  </div>
                ))}
              </>
            )}
            <label className="ach-raid">
              <span className="muted small">{a.raId}</span>
              <div className="row">
                <input className="input num" value={raId} inputMode="numeric" onChange={(e) => setRaId(e.target.value.replace(/\D/g, ""))} />
                <Button small kind="primary" disabled={!raId || working} onClick={() => linkRa(raId)}>
                  {a.link}
                </Button>
              </div>
            </label>
          </>
        )}
      </section>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Settings panel

export function AchievementSettingsPanel() {
  const t = useT();
  const a = t.achievements;
  const ui = useUi();
  const [s, setS] = useState<AchievementSettings | null>(null);
  const [user, setUser] = useState("");
  const [key, setKey] = useState("");
  const [working, setWorking] = useState(false);
  const [ps4Key, setPs4Key] = useState("");

  const load = useCallback(() => achievementsApi.settings().then(setS, () => setS(null)), []);
  useEffect(() => {
    load();
  }, [load]);
  if (!s) return null;

  const save = async (next: Partial<AchievementSettings>) => {
    const v = { ...s, ...next };
    setS(v);
    await achievementsApi.saveSettings(v.steamPopup, v.fallbackPopup);
  };
  const connect = async () => {
    setWorking(true);
    try {
      const p = await achievementsApi.raConnect(user, key);
      ui.toast(msg().achievements.raConnected(p.user));
      setKey("");
      await load();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setWorking(false);
    }
  };
  const disconnect = async () => {
    await achievementsApi.raDisconnect();
    ui.toast(msg().achievements.raDisconnected);
    load();
  };
  const savePs4Key = async (key: string) => {
    try {
      await achievementsApi.setPs4Key(key);
      ui.toast(key ? msg().achievements.ps4KeySaved : msg().achievements.ps4KeyCleared);
      setPs4Key("");
      await load();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };
  const test = async (delay = 0) => {
    try {
      await achievementsApi.testPopup(delay);
      ui.toast(delay ? msg().achievements.testLater(delay) : msg().achievements.testSent);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <section className="panel form" id="settings-achievements">
      <div className="panel-head">
        <Icon name="star" />
        <h3>{a.settingsTitle}</h3>
        <Hint text={a.settingsHint} />
      </div>
      <label className="ach-toggle">
        <Toggle checked={s.steamPopup} onChange={(v) => save({ steamPopup: v })} />
        <span>{a.steamPopupDefault}</span>
      </label>
      <label className="ach-toggle">
        <Toggle checked={s.fallbackPopup} onChange={(v) => save({ fallbackPopup: v })} />
        <span>
          {a.fallback} <Hint text={a.fallbackHint} />
        </span>
      </label>
      <div className="row">
        <Button small icon="play" onClick={() => test()}>
          {a.test}
        </Button>
        <Button small kind="ghost" icon="play" onClick={() => test(8)}>
          {a.testInGame}
        </Button>
      </div>

      <h4>
        {a.ps4Key} <Hint text={a.ps4KeyHint} />
      </h4>
      <p className="muted small">
        {s.ps4KeySource === "shadps4" ? a.ps4KeyFromShadps4 : s.ps4KeySource === "settings" ? a.ps4KeySet : a.ps4KeyNone}
      </p>
      {s.ps4KeySource !== "shadps4" && (
        <>
          <label>
            <span>{a.ps4KeyLabel}</span>
            <input className="input" type="password" value={ps4Key} onChange={(e) => setPs4Key(e.target.value)} autoComplete="off" spellCheck={false} />
          </label>
          <div className="row">
            <Button small kind="primary" icon="check" disabled={!ps4Key.trim()} onClick={() => savePs4Key(ps4Key)}>
              {a.ps4KeySave}
            </Button>
            {s.ps4KeySource === "settings" && (
              <Button small kind="ghost" icon="x" onClick={() => savePs4Key("")}>
                {a.ps4KeyClear}
              </Button>
            )}
          </div>
        </>
      )}

      <h4>
        {a.raAccount} {!(s.raConnected && s.raUser) && <Hint text={a.raKeyHelp} />}
      </h4>
      {s.raConnected && s.raUser ? (
        <div className="row">
          <Badge tone="ok">{a.raConnected(s.raUser)}</Badge>
          <Button small kind="ghost" icon="x" onClick={disconnect}>
            {a.raDisconnect}
          </Button>
        </div>
      ) : (
        <>
          <div className="row">
            <Button small kind="ghost" icon="locate" onClick={() => open(RA_KEYS_URL)}>
              {a.raOpenKeys}
            </Button>
          </div>
          <label>
            <span>{a.raUser}</span>
            <input className="input" value={user} onChange={(e) => setUser(e.target.value)} autoComplete="off" />
          </label>
          <label>
            <span>{a.raKey}</span>
            <input className="input" type="password" value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" />
          </label>
          <div className="row">
            <Button small kind="primary" icon="check" disabled={!user.trim() || !key.trim() || working} onClick={connect}>
              {a.raConnect}
            </Button>
          </div>
        </>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Fallback popup window (index.html?toast)

export function AchievementToast() {
  const [toast, setToast] = useState<ToastPayload | null>(null);
  const [leaving, setLeaving] = useState(false);

  useEffect(() => {
    document.documentElement.classList.add("toast-window");
    let alive = true;
    const next = async () => {
      const t = await achievementsApi.toastNext().catch(() => null);
      if (!alive) return;
      if (!t) {
        setToast(null);
        return;
      }
      setLeaving(false);
      setToast(t);
      window.setTimeout(() => setLeaving(true), 5200);
      window.setTimeout(next, 5800);
    };
    next();
    return () => {
      alive = false;
    };
  }, []);

  if (!toast) return null;
  const en = toast.lang === "en";
  const title = en ? "Achievement unlocked" : "解锁成就";
  return (
    <div className={`ach-toast${leaving ? " leaving" : ""}`}>
      {toast.icon ? (
        <img src={toast.icon} alt="" />
      ) : (
        <div className="ach-noicon">
          <Icon name="star" size={30} />
        </div>
      )}
      <div className="ach-toast-text">
        <div className="ach-toast-title">
          {title}
          {toast.test && <span className="ach-tag">{en ? "Test" : "测试"}</span>}
        </div>
        <div className="ach-toast-name">{toast.name}</div>
        <div className="ach-toast-game">{toast.game}</div>
      </div>
    </div>
  );
}
