import { useEffect, useMemo, useState } from "react";
import { api, errorText, GameView, LocalFile, NewerVersion, Outcome, Overview, playtimePlatforms, SaveCandidate, StatusReport } from "../api";
import { bytes, dateTime, duration, healthText, outcomeText, relTime } from "../format";
import { msg, useT } from "../i18n";
import { sceneIs } from "../scene";
import { detectedGroup, groupBySource } from "../source";
import { AddToSteam } from "./AddToSteam";
import { ArtPicker } from "./ArtPicker";
import { GameBackupPanel } from "./GameBackup";
import { PlatformFolders } from "./PlatformFolders";
import { StatusBadge } from "./Library";
import { withSteamClosed } from "./steamAction";
import { SaveCandidates } from "./SaveFinder";
import { Badge, Button, Hint, Icon, Menu, Modal, PathField, samePath, Spinner, Toggle, useUi } from "./ui";
import { Screenshots } from "./Screenshots";
import { Saves } from "./Saves";
import { ApplyDialog, familyOf, UndoRow, VersionBar } from "./SaveLibrary";
import { handBack, StorePanel, TakeOverDialog } from "./SteamCloud";
import { GameAchievements } from "./Achievements";
import { useShelves } from "./Collection";

type Tab = "versions" | "files" | "config" | "screenshots" | "achievements";

/** Xbox app games start through this; their own programs are protected. */
const XBOX_HELPER = /gamelaunchhelper\.exe$/i;
const XBOX_DIR = /[\\/](XboxGames|WindowsApps|ModifiableWindowsApps)[\\/]/i;

export function GameDetail({
  game,
  overview,
  check,
  checking,
  onBack,
  onOpen,
  onChanged,
  onRemoved,
}: {
  game: GameView;
  overview: Overview;
  check?: StatusReport;
  checking: boolean;
  onBack: () => void;
  /** Open another game's page (another version of this game). */
  onOpen: (key: string) => void;
  onChanged: () => Promise<void>;
  onRemoved: () => Promise<void>;
}) {
  const ui = useUi();
  const t = useT();
  const g = t.game;
  // Another version of this game (on another device or platform) has newer saves.
  const [newer, setNewer] = useState<NewerVersion | null>(null);
  const [applyNewer, setApplyNewer] = useState(false);
  const versionCount = familyOf(overview, game.key).length;
  const shelves = useShelves(overview, game.key, onChanged);
  useEffect(() => {
    if (versionCount < 2 || overview.cloud.kind === "none") return;
    api.newerVersion(game.key).then(setNewer, () => setNewer(null));
  }, [game.key, versionCount, overview.cloud.kind]);
  const [tab, setTab] = useState<Tab>(
    sceneIs("game-settings") ? "config" : sceneIs("game-screenshots") ? "screenshots" : sceneIs("game-achievements") ? "achievements" : "versions",
  );
  // Docs screenshots: open on the game files card, or on the tabs.
  useEffect(() => {
    const target = sceneIs("game-files")
      ? () => document.getElementById("game-backup")
      : sceneIs("game-screenshots", "game-achievements", "versions")
        ? () => document.querySelector(".detail-body .tabs")
        : null;
    if (!target) return;
    setTimeout(() => {
      const el = target();
      if (!el) return;
      el.scrollIntoView({ block: "start" });
      // Leave room for the sticky action bar above the content.
      let p = el.parentElement;
      while (p && !(p.scrollHeight > p.clientHeight && getComputedStyle(p).overflowY !== "visible")) p = p.parentElement;
      (p ?? document.scrollingElement)?.scrollBy(0, -110);
    }, 600);
  }, []);
  const [versionsKey, setVersionsKey] = useState(0);
  const [heroFailed, setHeroFailed] = useState(false);
  const [logoFailed, setLogoFailed] = useState(false);
  const [dialog, setDialog] = useState<"art" | "steam" | "takeover" | null>(sceneIs("art-picker") ? "art" : null);
  useEffect(() => setHeroFailed(false), [game.art.hero]);
  useEffect(() => setLogoFailed(false), [game.art.logo]);

  const xboxProtected = !!game.exe && XBOX_DIR.test(game.exe) && !XBOX_HELPER.test(game.exe);
  const switchToXboxHelper = async () => {
    const content = game.exe?.match(/^(.*?[\\/]Content)[\\/]/i)?.[1];
    try {
      if (!content) throw new Error(msg().game.noContent);
      await api.setGameExe(game.key, `${content}\\gamelaunchhelper.exe`);
      ui.toast(msg().game.switchedHelper);
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const after = async (o?: Outcome) => {
    if (o) ui.toast(outcomeText(o, game.name), o.kind === "conflict" ? "warn" : o.kind === "offline" || o.kind === "queued" ? "info" : "ok");
    setVersionsKey((k) => k + 1);
    await onChanged();
  };

  const run = async <T,>(f: () => Promise<T>, done?: (v: T) => Promise<void> | void) => {
    try {
      const v = await f();
      await done?.(v);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const syncNow = (res: "auto" | "local" | "cloud" = "auto") => run(() => api.sync(game.key, res), after);

  /** The cloud's saves replace the ones here: into the Xbox app's containers only once the user has read that it is experimental. */
  const takeCloud = async () => {
    if (game.xboxContainers && !(await ui.confirm({ title: g.useCloud, text: t.saves.xboxNote, confirm: g.useCloud }))) return;
    await syncNow("cloud");
  };

  /**
   * 「取回」: the cloud has newer saves of this game's own line, which a game
   * that doesn't start through Steam doesn't take by itself. A sync the
   * user starts takes them; into the Xbox app's containers, or from a newer
   * game, only once the user has read why not by itself.
   */
  const takeNewer = async () => {
    const note = [game.xboxContainers ? t.saves.xboxNote : "", check?.tooNew ? t.saves.tooNew(check.tooNew) : ""].filter(Boolean).join(" ");
    if (note && !(await ui.confirm({ title: t.policy.take, text: note, confirm: t.policy.take }))) return;
    await syncNow(note ? "cloud" : "auto");
  };
  const newerHere = check?.status === "cloudNewer" || check?.status === "canRestore";

  const backupNow = async () => {
    const comment = await ui.prompt({
      title: g.backupTitle,
      text: g.backupText,
      placeholder: g.backupPlaceholder,
      confirm: g.backup,
    });
    if (comment === null) return;
    await run(() => api.backup(game.key, comment), after);
  };

  /** Steam changes need Steam closed; ask once and let the backend close/reopen it. */
  const steamAction = async (f: (closeSteam: boolean) => Promise<void>, okText: string) => {
    try {
      await f(false);
      ui.toast(okText);
    } catch (e) {
      const text = errorText(e);
      if (!text.includes("STEAM_RUNNING")) {
        ui.toast(text, "bad");
        return;
      }
      const m = msg().steamAction;
      const ok = await ui.confirm({ title: m.title, text: m.text, confirm: m.confirm });
      if (!ok) return;
      try {
        await f(true);
        ui.toast(okText);
      } catch (e2) {
        ui.toast(errorText(e2), "bad");
      }
    }
    await onChanged();
  };

  const removeFromSteam = async () => {
    const ok = await ui.confirm({
      title: g.removeSteamTitle,
      text: g.removeSteamText,
      confirm: t.steamPlugin.remove,
      danger: true,
    });
    if (!ok) return;
    await steamAction((close) => api.steamRemoveGame(game.key, close), msg().game.removedSteam);
  };

  /** Quick scan: near the old location, or in the stores when there is none yet. */
  const findExe = async () => {
    try {
      const found = await api.relocateAuto(game.key);
      if (!found) {
        ui.toast(game.exe ? msg().game.notFoundMoved : msg().game.notFoundAnywhere, "info");
        return;
      }
      if (samePath(found, game.exe)) {
        ui.toast(msg().game.alreadyHere, "info");
        return;
      }
      const ok = await ui.confirm({
        title: game.exe ? g.foundNewTitle : g.foundTitle,
        text: game.exe ? (
          <>
            <p>{g.foundNear}</p>
            <code className="path">{found}</code>
            <p className="muted">{g.foundNearHint}</p>
          </>
        ) : (
          <>
            <p>{g.foundExe(game.name)}</p>
            <code className="path">{found}</code>
            <p className="muted">{g.foundExeHint}</p>
          </>
        ),
        confirm: g.useLocation,
      });
      if (!ok) return;
      await api.setGameExe(game.key, found);
      ui.toast(msg().game.locationUpdated);
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const pickExe = async () => {
    try {
      const picked = await api.pickExe(game.exe ?? game.install);
      if (!picked) return;
      await api.setGameExe(game.key, picked);
      ui.toast(msg().game.locationUpdated);
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const health = healthText(game.health);
  const conflict = check?.status === "conflict" || !!game.conflict;
  const hero = game.art.hero && !heroFailed ? game.art.hero : null;

  return (
    <div className="detail">
      <div className={`hero${hero ? "" : " hero-plain"}`} style={!hero && game.art.cover ? { backgroundImage: `url(${game.art.cover})` } : undefined}>
        {hero && <img className="hero-img" src={hero} alt="" onError={() => setHeroFailed(true)} />}
        <div className="hero-shade" />
        <button className="back" onClick={onBack}>
          <Icon name="back" /> {t.library.title}
        </button>
        <button className="hero-edit" onClick={() => setDialog("art")}>
          <Icon name="edit" size={14} /> {g.changeArt}
        </button>
        <div className="hero-title">
          {game.art.logo && !logoFailed ? (
            <img className="logo" src={game.art.logo} alt={game.name} onError={() => setLogoFailed(true)} />
          ) : (
            <h1>{game.name}</h1>
          )}
        </div>
      </div>

      <div className="actionbar">
        {newerHere ? (
          <Button kind="primary" icon="download" onClick={takeNewer} disabled={game.running || conflict} busyText={g.syncing} title={t.policy.takeTitle}>
            {t.policy.take}
          </Button>
        ) : (
          <Button kind="primary" icon="sync" onClick={() => syncNow()} disabled={game.running || conflict} busyText={g.syncing}>
            {g.syncNow}
          </Button>
        )}
        <Button icon="upload" onClick={backupNow} disabled={game.running}>
          {g.backupTitle}
        </Button>
        <Button icon="folder" onClick={() => run(() => api.openSaveFolder(game.key))}>
          {g.openSaves}
        </Button>
        <Menu title={t.saves.more} items={shelves.items} />
        <div className="actionbar-status">
          <StatusBadge game={game} check={check} checking={checking} />
          {newerHere && check?.head && (
            <span className="muted" title={dateTime(check.head.createdAt)}>
              {check.head.deviceName} · {relTime(check.head.createdAt)}
            </span>
          )}
          {check?.incomplete && (
            <span className="status-note">
              <Badge tone="warn">{g.incomplete}</Badge>
              <Hint text={g.incompleteHint} />
            </span>
          )}
          {shelves.marks}
          <span className="muted">
            {g.lastSync(relTime(game.lastSyncAt))}
            {game.lastSyncedDevice ? ` · ${game.lastSyncedDevice}` : ""}
            {check ? ` · ${g.local(check.localFiles, bytes(check.localSize))}` : ""}
          </span>
          {game.playtime.totalSecs > 0 && (
            <span className="muted playtime" title={playtimeTip(game)}>
              <Icon name="clock" size={13} /> {g.playtime(duration(game.playtime.totalSecs))}
            </span>
          )}
        </div>
      </div>

      <div className="detail-body">
        {conflict && check?.head && (
          <div className="banner banner-warn conflict">
            <Icon name="warn" />
            <div>
              <b>{g.conflictTitle}</b>
              <p>
                {g.conflictText(
                  check.head.deviceName,
                  dateTime(check.head.createdAt),
                  bytes(check.head.totalSize),
                  check.localFiles,
                  bytes(check.localSize),
                )}
              </p>
            </div>
            <div className="conflict-actions">
              <Button kind="primary" onClick={takeCloud}>
                {g.useCloud}
              </Button>
              <Button onClick={() => syncNow("local")}>{g.useLocal}</Button>
            </div>
          </div>
        )}
        {check?.tooNew && check.status === "cloudNewer" && (
          <div className="banner banner-info">
            <Icon name="warn" /> {g.tooNew(check.tooNew)} <Hint text={g.tooNewHint} />
          </div>
        )}
        {check?.message && check.status !== "conflict" && (
          <div className={`banner ${check.status === "offline" ? "banner-info" : "banner-bad"}`}>
            <Icon name="warn" /> {check.message}
          </div>
        )}
        {game.lastError && check?.status !== "offline" && (
          <div className="banner banner-bad">
            <Icon name="warn" /> {g.lastError(game.lastError)}
          </div>
        )}
        {game.pending > 0 && (
          <div className="banner banner-info">
            <Icon name="cloud" /> {g.pending(game.pending)}
          </div>
        )}
        {!game.playable && (
          <div className="banner banner-info not-here">
            <Icon name="locate" />
            <div>
              <b>{!game.exe ? g.notHere.noExe : !game.exeExists ? g.notHere.moved : g.notHere.noFile}</b> <Hint text={g.notHere.hint} />
            </div>
            <Button small onClick={findExe}>
              {g.findAuto}
            </Button>
          </div>
        )}

        {newer && (
          <div className="banner banner-info not-here">
            <Icon name="transfer" />
            <div>
              <b>{t.savelib.newerTitle(newer.label)}</b> <Hint text={t.savelib.newerText(newer.snapshot.deviceName, dateTime(newer.snapshot.createdAt))} />
            </div>
            <Button small kind="primary" onClick={() => setApplyNewer(true)}>
              {t.savelib.applyHere}
            </Button>
            <Button
              small
              kind="ghost"
              onClick={() => {
                api.dismissNewer(game.key, newer.snapshot.id).catch(() => undefined);
                setNewer(null);
              }}
            >
              {t.savelib.notNow}
            </Button>
          </div>
        )}
        {applyNewer && newer && (
          <ApplyDialog
            overview={overview}
            sourceKey={newer.key}
            sourceName={newer.name}
            snapshot={newer.snapshot}
            target={game.key}
            onClose={() => setApplyNewer(false)}
            onApplied={async () => {
              setApplyNewer(false);
              setNewer(null);
              await onChanged();
            }}
          />
        )}

        <VersionBar
          overview={overview}
          current={game.key}
          disabled={game.running}
          onOpen={onOpen}
          onChanged={() => after()}
          onEditSaves={() => {
            setTab("config");
            setTimeout(() => document.querySelector(".detail-body .tabs")?.scrollIntoView({ block: "start" }), 0);
          }}
          store={game.store ? { onTakeOver: () => setDialog("takeover"), onHandBack: () => handBack(ui, game, () => after()) } : undefined}
        />
        <UndoRow game={game} onChanged={() => after()} />

        <div className="cards2">
          {game.store && <StorePanel game={game} onChanged={() => after()} onTakeOver={() => setDialog("takeover")} />}
          {!game.store && (
            <section className="panel">
              <div className="panel-head">
                <Icon name="steam" />
                <h3>{g.steamTitle}</h3>
                {game.steam ? (
                  <Hint text={g.steamHint} />
                ) : (
                  game.health.state !== "noSteam" && <Hint text={game.exe && game.exeExists ? g.addSteamHint : g.addSteamNeedsExe} />
                )}
                {game.steam && (
                  <Toggle
                    checked={game.steam.enabled}
                    onChange={(v) =>
                      steamAction(
                        (close) => api.steamSetEnabled(game.key, v, close),
                        v ? msg().game.steamOn : msg().game.steamOff,
                      )
                    }
                  />
                )}
              </div>
              <p className={`health health-${health.tone}`}>{health.text}</p>
              {health.fixable && (
                <Button kind="primary" small icon="check" onClick={() => steamAction((close) => api.steamRepair(game.key, close), msg().game.repaired)}>
                  {g.repair}
                </Button>
              )}
              {!game.steam && game.health.state !== "noSteam" && (
                <div className="row">
                  <Button small kind="primary" icon="steam" onClick={() => setDialog("steam")} disabled={!game.exe || !game.exeExists}>
                    {g.addSteam}
                  </Button>
                </div>
              )}
              {game.steam?.created && (
                <div className="row">
                  <Button small kind="ghost" icon="trash" onClick={removeFromSteam}>
                    {g.removeSteam}
                  </Button>
                </div>
              )}
            </section>
          )}

          <section className="panel">
            <div className="panel-head">
              <Icon name="locate" />
              <h3>{g.location}</h3>
              {game.exe && game.exeExists && XBOX_HELPER.test(game.exe) && <Hint text={g.xboxHelper} />}
              {game.exe && (game.exeExists ? <Badge tone="ok">{g.ok}</Badge> : <Badge tone="warn">{g.missing}</Badge>)}
            </div>
            {game.exe && !game.exeExists && (
              <p className="health health-warn">{g.moved}</p>
            )}
            <PathField
              path={game.exe}
              empty={g.noExe}
              onScan={findExe}
              onPick={pickExe}
              scanText={g.findAuto}
              pickText={game.exe ? undefined : g.pickExe}
            >
              {game.exe && game.exeExists && (
                <Button small kind="ghost" icon="folder" onClick={() => run(() => api.openPath(game.exe!))}>
                  {g.openFolder}
                </Button>
              )}
            </PathField>
            {xboxProtected && game.exeExists && (
              <div className="row">
                <span className="health health-warn">{g.xboxProtectedShort}</span>
                <Hint text={g.xboxProtected} />
                <Button small kind="primary" icon="check" onClick={switchToXboxHelper}>
                  {g.useHelper}
                </Button>
              </div>
            )}
            {(!game.exe || !game.exeExists) && (
              <PlatformFolders
                onPick={async (exe) => {
                  try {
                    await api.setGameExe(game.key, exe);
                    ui.toast(msg().game.locationSet);
                    await onChanged();
                  } catch (e) {
                    ui.toast(errorText(e), "bad");
                  }
                }}
              />
            )}
          </section>
        </div>

        <GameBackupPanel game={game} onChanged={onChanged} />

        <div className="tabs">
          <button className={tab === "versions" ? "on" : ""} onClick={() => setTab("versions")}>
            {g.tabVersions}
          </button>
          <button className={tab === "files" ? "on" : ""} onClick={() => setTab("files")}>
            {g.tabFiles}
          </button>
          <button className={tab === "screenshots" ? "on" : ""} onClick={() => setTab("screenshots")}>
            {g.tabScreenshots}
          </button>
          <button className={tab === "config" ? "on" : ""} onClick={() => setTab("config")}>
            {g.tabConfig}
          </button>
          <button className={tab === "achievements" ? "on" : ""} onClick={() => setTab("achievements")}>
            {t.achievements.tab}
          </button>
        </div>

        {tab === "versions" && (
          <Saves key={versionsKey} game={game} overview={overview} disabled={game.running} onChanged={() => after()} onApplied={async (k) => onOpen(k)} />
        )}
        {tab === "files" && <Files game={game} />}
        {tab === "screenshots" && <Screenshots game={game} deviceId={overview.deviceId} />}
        {tab === "config" && <Config game={game} allGames={overview.games} onChanged={onChanged} onRemoved={onRemoved} />}
        {tab === "achievements" && <GameAchievements gameKey={game.key} />}
      </div>

      {dialog === "art" && (
        <Modal title={g.artTitle(game.name)} onClose={() => setDialog(null)} wide>
          <ArtPicker gameKey={game.key} name={game.name} current={game.art} onApplied={onChanged} />
          {game.steam && <p className="muted small">{g.artSteamNote}</p>}
        </Modal>
      )}
      {dialog === "takeover" && (
        <TakeOverDialog
          game={game}
          onClose={() => setDialog(null)}
          onDone={async () => {
            setDialog(null);
            await after();
          }}
        />
      )}
      {dialog === "steam" && (
        <Modal title={g.addSteamTitle(game.name)} onClose={() => setDialog(null)}>
          <AddToSteam
            games={[{ key: game.key, name: game.name }]}
            onCancel={() => setDialog(null)}
            onDone={async () => {
              setDialog(null);
              await onChanged();
            }}
          />
        </Modal>
      )}
    </div>
  );
}

/** Shown when hovering the playtime: per device, and per platform when there are several. */
function playtimeTip(game: GameView): string {
  const g = msg().game;
  const lines = [g.playtimeTitle];
  for (const d of game.playtime.devices) {
    lines.push(g.playtimeDevice(d.deviceName, duration(d.secs), d.thisDevice));
    if (d.steamMinutes) lines.push(g.playtimeSteam(duration(d.steamMinutes * 60)));
  }
  const platforms = playtimePlatforms(game.playtime);
  if (platforms.length > 0) {
    lines.push("", g.playtimePlatforms);
    for (const [name, secs] of platforms) lines.push(g.playtimePlatform(name, duration(secs)));
  }
  return lines.join("\n");
}

function Files({ game }: { game: GameView }) {
  const ui = useUi();
  const t = useT();
  const [data, setData] = useState<{ files: LocalFile[]; roots: Record<string, string> } | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.localFiles(game.key).then(setData, (e) => setError(errorText(e)));
  }, [game.key]);
  if (error) return <div className="banner banner-bad">{error}</div>;
  if (!data) return <div className="muted pad">{t.saves.reading}</div>;
  return (
    <div className="panel">
      <div className="roots">
        {Object.entries(data.roots).map(([k, v]) => (
          <div key={k} className="root-row">
            <code>{`{${k}}`}</code>
            <button className="link" onClick={() => api.openPath(v).catch((e) => ui.toast(errorText(e), "bad"))}>
              {v}
            </button>
          </div>
        ))}
      </div>
      {data.files.length === 0 ? (
        <p className="muted">{t.game.noFiles}</p>
      ) : (
        <table className="files">
          <thead>
            <tr>
              <th>{t.game.colFile}</th>
              <th>{t.game.colSize}</th>
              <th>{t.game.colModified}</th>
            </tr>
          </thead>
          <tbody>
            {data.files.map((f) => (
              <tr key={f.logical}>
                <td>
                  <code>{f.logical}</code>
                </td>
                <td>{bytes(f.size)}</td>
                <td>{f.mtime ? dateTime(f.mtime) : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function Config({
  game,
  allGames,
  onChanged,
  onRemoved,
}: {
  game: GameView;
  allGames: GameView[];
  onChanged: () => Promise<void>;
  onRemoved: () => Promise<void>;
}) {
  const ui = useUi();
  const t = useT();
  const c = t.game.config;
  const [name, setName] = useState(game.name);
  const [source, setSource] = useState(game.source.label ?? "");
  // Suggestions: the groups already in the library (emulators, the user's own…).
  const sourceNames = useMemo(() => groupBySource(allGames).groups.map((e) => e.group.label), [allGames]);
  const [saves, setSaves] = useState(game.customSaves ? game.saves.join("\n") : "");
  const [exclude, setExclude] = useState(game.exclude.join("\n"));
  const [waitFor, setWaitFor] = useState(game.waitFor.join("\n"));
  const [args, setArgs] = useState(game.args);

  const lines = (s: string) => s.split("\n").map((l) => l.trim()).filter(Boolean);

  const save = async () => {
    try {
      await api.updateGame(game.key, { name, saves: lines(saves), exclude: lines(exclude), waitFor: lines(waitFor), args, source });
      ui.toast(msg().game.config.saved);
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const [scanning, setScanning] = useState(false);
  const append = (set: (f: (s: string) => string) => void, items: string[]) =>
    set((s) => {
      const have = lines(s);
      const add = items.filter((i) => !have.some((h) => h.toLowerCase() === i.toLowerCase()));
      return [...have, ...add].join("\n");
    });

  const addFolder = async () => {
    const dir = await api.pickFolder(msg().saveFinder.pickTitle, game.install);
    if (!dir) return;
    append(setSaves, [await api.folderToPattern(dir, game.exe)]);
  };

  const excludeFolder = async () => {
    const dir = await api.pickFolder(msg().game.config.excludePickTitle, game.install);
    if (!dir) return;
    append(setExclude, [await api.folderToPattern(dir, game.exe)]);
  };

  const pickProcess = async () => {
    const exe = await api.pickExe(game.exe ?? game.install);
    const name = exe?.split(/[\\/]/).pop();
    if (name) append(setWaitFor, [name]);
  };

  const remove = async () => {
    const created = !!game.steam?.created;
    const ok = await ui.confirm({
      title: c.removeTitle(game.name),
      text: c.removeText + (created ? c.removeSteamToo : ""),
      confirm: t.steamPlugin.remove,
      danger: true,
    });
    if (!ok) return;
    try {
      if (created && (await withSteamClosed(ui, (close) => api.steamRemoveGame(game.key, close).then(() => true))) === null) return;
      await api.removeGame(game.key);
      ui.toast(msg().game.config.removed);
      await onRemoved();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <>
      <div className="panel form">
        <label>
          <span>{t.saveFinder.name}</span>
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label>
          <span>{t.source.field}</span>
          <div className="field-stack">
            <input
              className="input"
              list="source-names"
              value={source}
              placeholder={t.source.auto(detectedGroup(game).label)}
              onChange={(e) => setSource(e.target.value)}
            />
            <datalist id="source-names">
              {sourceNames.map((n) => (
                <option key={n} value={n} />
              ))}
            </datalist>
            <Hint text={t.source.fieldHint} />
          </div>
        </label>
        <label>
          <span>
            {c.autoSync} <Hint text={c.autoSyncHint} />
          </span>
          <div className="row">
            <Toggle
              checked={game.autoSync}
              onChange={(v) =>
                api.updateGame(game.key, { autoSync: v }).then(onChanged, (e) => ui.toast(errorText(e), "bad"))
              }
            />
          </div>
        </label>
        <label>
          <span>{c.uploadWhen}</span>
          <div className="row">
            <Toggle
              checked={game.uploadOnGameExit}
              disabled={!game.hasGameProcess}
              onChange={(v) =>
                api.updateGame(game.key, { uploadOnGameExit: v }).then(onChanged, (e) => ui.toast(errorText(e), "bad"))
              }
            />
            <span>{c.uploadOnExit}</span>
            <Hint text={game.hasGameProcess ? c.uploadOnExitHint(game.waitFor.length ? game.waitFor : game.defaultWaitFor) : c.uploadNeedsProcess} />
          </div>
        </label>
        <label>
          <span>
            {c.saves} <Hint text={c.savesHint} />
          </span>
          <div className="field-stack">
            <textarea
              className="input mono"
              rows={3}
              value={saves}
              placeholder={game.defaultSaves.length ? c.builtinSaves(game.defaultSaves) : "{install}/save/**"}
              onChange={(e) => setSaves(e.target.value)}
            />
            <div className="row">
              <Button small icon="search" onClick={() => setScanning(true)}>
                {c.scanSaves}
              </Button>
              <Button small icon="folder" onClick={addFolder}>
                {c.addFolder}
              </Button>
            </div>
          </div>
        </label>
        <label>
          <span>{c.exclude}</span>
          <div className="field-stack">
            <textarea className="input mono" rows={2} value={exclude} placeholder="{install}/save/cache/**" onChange={(e) => setExclude(e.target.value)} />
            <div className="row">
              <Button small icon="folder" onClick={excludeFolder}>
                {c.excludeFolder}
              </Button>
            </div>
          </div>
        </label>
        <label>
          <span>{c.process}</span>
          <div className="field-stack">
            <textarea
              className="input mono"
              rows={2}
              value={waitFor}
              placeholder={game.defaultWaitFor.length ? c.builtinProcess(game.defaultWaitFor) : c.processPlaceholder}
              onChange={(e) => setWaitFor(e.target.value)}
            />
            <div className="row">
              <Button small icon="folder" onClick={pickProcess}>
                {c.pickProcess}
              </Button>
            </div>
          </div>
        </label>
        <label>
          <span>{c.args}</span>
          <input className="input mono" value={args} onChange={(e) => setArgs(e.target.value)} />
        </label>
        <div className="row space">
          <Button kind="primary" onClick={save}>
            {c.save}
          </Button>
          <Button kind="danger" icon="trash" onClick={remove}>
            {c.remove}
          </Button>
        </div>
        <p className="muted small">{c.gameId(game.key)}</p>
      </div>
      {/* Outside the form: its label grid would squash the candidate rows. */}
      {scanning && <SaveScan game={game} onAdd={(p) => append(setSaves, p)} onClose={() => setScanning(false)} />}
    </>
  );
}

/** Quick scan for the save folder: likely folders to tick and add. */
function SaveScan({ game, onAdd, onClose }: { game: GameView; onAdd: (patterns: string[]) => void; onClose: () => void }) {
  const ui = useUi();
  const t = useT();
  const c = t.game.config;
  const [list, setList] = useState<SaveCandidate[] | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  useEffect(() => {
    api.saveSuggest(game.name, game.exe, game.install).then(setList, (e) => {
      ui.toast(errorText(e), "bad");
      setList([]);
    });
  }, [game.key]);

  const toggle = (s: SaveCandidate) => {
    const next = new Set(picked);
    if (!next.delete(s.path)) next.add(s.path);
    setPicked(next);
  };

  const add = async () => {
    try {
      onAdd(await Promise.all([...picked].map((p) => list?.find((c) => c.path === p)?.pattern ?? api.folderToPattern(p, game.exe))));
      onClose();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <Modal
      title={c.scanTitle}
      onClose={onClose}
      wide
      footer={
        <>
          <Button kind="ghost" onClick={onClose}>
            {t.common.cancel}
          </Button>
          <Button kind="primary" icon="plus" onClick={add} disabled={picked.size === 0}>
            {c.scanAdd(picked.size)}
          </Button>
        </>
      }
    >
      <div className="finder">
        <p className="muted small">{c.scanText}</p>
        {!list ? <Spinner /> : list.length === 0 ? <p className="muted">{c.scanNone}</p> : <SaveCandidates list={list} isPicked={(p) => picked.has(p)} onToggle={toggle} />}
      </div>
    </Modal>
  );
}
