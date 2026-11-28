// A game Steam installed, on its page (docs/plan-v2.md, section 8): who
// keeps its saves (Steam Cloud alone, Ludonimbus beside it, Ludonimbus), and
// changing that.
import { useEffect, useState } from "react";
import { api, errorText, GameView, SteamCloudApp, SteamCloudDone, SteamCloudStatus } from "../api";
import { msg, useT } from "../i18n";
import { withSteamClosed } from "./steamAction";
import { Badge, Button, Hint, Icon, Menu, Modal, useUi } from "./ui";

type Ui = ReturnType<typeof useUi>;

/** The backend's errors about Steam come with a code in front ("STEAM_CLOUD_ON: …"). */
const plain = (e: unknown) => errorText(e).replace(/^[A-Z_]+:\s*/, "");

/** Steam's own properties window of the game: its Steam Cloud switch is there. */
export const openSteamProperties = (ui: Ui, key: string) => api.steamCloudOpenProperties(key).catch((e) => ui.toast(plain(e), "bad"));

/**
 * Give the game back to Steam Cloud. Nothing of the saves changes, so
 * without a Steam plugin it is done at once and turning Steam Cloud on is
 * the user's to do in Steam. With a plugin that can, Steam turns it on
 * itself, once the user confirmed.
 */
export async function handBack(ui: Ui, game: GameView, onChanged: () => Promise<void>) {
  const m = msg().steamCloud;
  try {
    const before = await api.steamCloudStatus(game.key).catch(() => null);
    const viaPlugin = !!before?.plugin?.cloud;
    const turnsOn = viaPlugin && !!before?.cloudUsed && !before.cloudEnabled;
    if (turnsOn && !(await ui.confirm({ title: m.handBackTitle(game.name), text: m.handBackText, confirm: m.handBack }))) return;
    const s = await withSteamClosed(ui, (close) => api.steamCloudHandBack(game.key, close, viaPlugin));
    if (!s) return;
    // "Backup only" means versions are stored again by themselves.
    if (!game.autoSync) await api.updateGame(game.key, { autoSync: true });
    if (viaPlugin && s.cloudEnabled && s.globalEnabled) ui.toast(m.handedBackOn);
    else ui.toast(m.handedBack, "ok", { label: m.openProperties, run: () => openSteamProperties(ui, game.key) });
    await onChanged();
  } catch (e) {
    ui.toast(plain(e), "bad");
  }
}

/** The panel in the place of "add to Steam": the game is in Steam already. */
export function StorePanel({ game, onChanged, onTakeOver }: { game: GameView; onChanged: () => Promise<void>; onTakeOver: () => void }) {
  const t = useT();
  const c = t.steamCloud;
  const ui = useUi();
  const store = game.store!;
  const [status, setStatus] = useState<SteamCloudStatus | null>(null);
  useEffect(() => {
    let live = true;
    api.steamCloudStatus(game.key).then(
      (s) => live && setStatus(s),
      () => live && setStatus(null),
    );
    return () => void (live = false);
  }, [game.key, store.mode, store.wrapper, game.lastSyncAt]);

  const act = async (f: () => Promise<unknown>, ok: string) => {
    try {
      if ((await f()) === null) return;
      ui.toast(ok);
      await onChanged();
    } catch (e) {
      ui.toast(plain(e), "bad");
    }
  };
  /** Put the wrapper (back) into the launch options of a game Ludonimbus keeps already. */
  const wrap = (ok: string) => act(() => withSteamClosed(ui, (close) => api.steamCloudTakeOver(game.key, true, close)), ok);

  const why = game.running ? t.savelib.running : undefined;
  const noCloud = status?.cloudUsed === false;
  const broken = store.wrapper && status?.wrapper && status.wrapper !== "ok" ? status.wrapper : null;
  let tone = "ok";
  let text = c.textBackup;
  let action = (
    <Button small onClick={onTakeOver} disabled={game.running} title={why}>
      {c.takeOver}
    </Button>
  );
  if (store.mode === "steam") {
    tone = "muted";
    text = c.textSteam;
    action = (
      <Button small kind="primary" onClick={() => act(() => api.updateGame(game.key, { autoSync: true }), msg().steamCloud.backupOn)}>
        {c.backup}
      </Button>
    );
  } else if (store.mode === "managed") {
    text = noCloud ? c.textNoCloud : store.wrapper ? c.textManaged : c.textBackground;
    action = (
      <Button small kind="ghost" onClick={() => handBack(ui, game, onChanged)} disabled={game.running} title={why}>
        {c.handBack}
      </Button>
    );
    if (broken) {
      tone = "warn";
      text = c.wrapper[broken];
      action = (
        <Button small kind="primary" icon="check" onClick={() => wrap(msg().steamCloud.repaired)} disabled={game.running} title={why}>
          {c.repair}
        </Button>
      );
    } else if (noCloud && !store.wrapper) {
      // Nothing to give back to; what can be had is a sync before every launch.
      action = (
        <Button small onClick={() => wrap(msg().steamCloud.taken)} disabled={game.running} title={why}>
          {c.beforeLaunch}
        </Button>
      );
    }
  }

  return (
    <section className="panel">
      <div className="panel-head">
        <Icon name="steam" />
        <h3>{c.title}</h3>
        <Hint text={c.hint} />
        <Badge tone={store.mode === "steam" ? "muted" : "ok"}>{c.state[store.mode]}</Badge>
      </div>
      <p className={`health health-${tone}`}>{text}</p>
      <div className="row">{action}</div>
    </section>
  );
}

/**
 * Taking a game over from Steam Cloud. With a Steam plugin that can, this
 * is the one confirmation that Steam Cloud gets turned off for the game:
 * Steam does it, and sets the launch options, while it runs. Without one,
 * the two things Steam's files have to show are the user's to bring about
 * in Steam, so they are listed as they read now; the check that counts is
 * the backend's when "take over" is pressed (with Steam closed its files
 * are as it left them).
 */
export function TakeOverDialog({ game, onClose, onDone }: { game: GameView; onClose: () => void; onDone: () => Promise<void> }) {
  const t = useT();
  const c = t.steamCloud;
  const ui = useUi();
  const [status, setStatus] = useState<SteamCloudStatus | null>(null);
  const [noWrapper, setNoWrapper] = useState(false);
  useEffect(() => {
    let live = true;
    const read = () =>
      api.steamCloudStatus(game.key).then(
        (s) => live && setStatus(s),
        () => undefined,
      );
    read();
    const timer = setInterval(read, 2000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [game.key]);

  const off = !!status && (!status.cloudEnabled || !status.globalEnabled);
  // A Steam plugin reads and changes Steam's settings live; when the switch
  // is on, this dialog asks to let it turn it off.
  const viaPlugin = !!status?.plugin?.cloud;
  const ask = viaPlugin && !!status?.cloudUsed && !off;

  const take = async () => {
    try {
      const s = await withSteamClosed(ui, (close) => api.steamCloudTakeOver(game.key, !noWrapper, close, viaPlugin));
      if (!s) return;
      ui.toast(noWrapper ? msg().steamCloud.takenBackground : msg().steamCloud.taken);
      await onDone();
    } catch (e) {
      ui.toast(plain(e), "bad");
    }
  };

  const step = (done: boolean, text: string, hint: string, extra?: JSX.Element) => (
    <li className={done ? "done" : ""}>
      <Icon name={done ? "check" : "clock"} size={16} />
      <span>
        {text} <Hint text={hint} />
      </span>
      {extra}
    </li>
  );

  return (
    <Modal
      title={c.takeOverTitle(game.name)}
      onClose={onClose}
      footer={
        <>
          <Button kind="ghost" onClick={onClose}>
            {t.common.cancel}
          </Button>
          <Button kind="primary" onClick={take} disabled={!status}>
            {c.takeOverDo}
          </Button>
        </>
      }
    >
      {ask ? (
        <>
          <p>
            {c.pluginOff} <Hint text={c.pluginOffHint} />
          </p>
          {!status?.synchronized && <ul className="steps-list">{step(false, c.stepSyncedTodo, c.stepSyncedHint)}</ul>}
        </>
      ) : (
        <ul className="steps-list">
          {status?.cloudUsed !== false && step(!!status?.synchronized, status?.synchronized ? c.stepSynced : c.stepSyncedTodo, c.stepSyncedHint)}
          {status?.cloudUsed !== false &&
            step(
              off,
              off ? c.stepOff : c.stepOffTodo,
              c.stepOffHint,
              <Button small kind="ghost" onClick={() => openSteamProperties(ui, game.key)}>
                {c.openProperties}
              </Button>,
            )}
        </ul>
      )}
      <div className="check-line">
        <label>
          <input type="checkbox" checked={noWrapper} onChange={(e) => setNoWrapper(e.target.checked)} />
          {c.noWrapper}
        </label>
        <Hint text={c.noWrapperHint} />
      </div>
    </Modal>
  );
}

/** The games of a batch that weren't done, by name, each with its reason. */
function notDone(apps: SteamCloudApp[], done: SteamCloudDone[]): string[] {
  const name = (appid: number) => apps.find((a) => a.appid === appid)?.name ?? String(appid);
  return done.filter((d) => d.error).map((d) => `${name(d.appid)}：${(d.error ?? "").replace(/^[A-Z_]+:\s*/, "")}`);
}

/**
 * Settings: every game Steam installed at once. One line: how many are in
 * the library, "back up all" for the rest, and "take over all" in the menu.
 */
export function StoreGamesRow({ onChanged }: { onChanged: () => void | Promise<void> }) {
  const t = useT();
  const c = t.steamCloud;
  const ui = useUi();
  const [apps, setApps] = useState<SteamCloudApp[] | null>(null);
  const [left, setLeft] = useState<string[]>([]);
  const [dialog, setDialog] = useState(false);
  const load = () =>
    api.steamCloudApps().then(
      (a) => (setApps(a), a),
      () => (setApps([]), [] as SteamCloudApp[]),
    );
  useEffect(() => void load(), []);
  if (!apps || apps.length === 0) return null;

  const added = apps.filter((a) => a.key).length;
  const fresh = apps.length - added;
  const backedUp = apps.filter((a) => a.key && a.state === "backup");

  /** Say what a batch did, keep what it left, read the list again. */
  const report = async (done: SteamCloudDone[], ok: (n: number) => string) => {
    const n = done.filter((d) => !d.error).length;
    setLeft(notDone(apps, done));
    ui.toast(ok(n), n > 0 || done.length === 0 ? "ok" : "bad");
    await load();
    await onChanged();
  };

  const backupAll = async () => {
    try {
      await report(await api.steamCloudBackupAll(), c.allAdded);
    } catch (e) {
      ui.toast(plain(e), "bad");
    }
  };

  return (
    <div className="field">
      <span>
        {c.all} <Hint text={c.allHint} />
      </span>
      <div className="row">
        <span className="muted small">{c.allCount(added, fresh)}</span>
        <Button small onClick={backupAll} disabled={fresh === 0} busyText={c.allAdding}>
          {c.backupAll}
        </Button>
        <Menu title={t.saves.more} items={[{ label: c.takeOverAll, onClick: () => setDialog(true), disabled: backedUp.length === 0, title: backedUp.length === 0 ? c.takeOverAllNone : undefined }]} />
        {left.length > 0 && (
          <span className="muted small">
            {c.allLeft(left.length)} <Hint text={left.join("\n")} />
          </span>
        )}
      </div>
      {dialog && (
        <TakeOverAllDialog
          games={backedUp}
          onClose={() => setDialog(false)}
          onDone={async (done) => {
            setDialog(false);
            await report(done, c.allTaken);
          }}
        />
      )}
    </div>
  );
}

/**
 * Taking over every backed-up game. With a Steam plugin that can, this is
 * the one confirmation that Steam Cloud gets turned off for these games,
 * and Steam does it. Without one, Steam Cloud has to be off for all games
 * (the account's switch), which is the user's to do in Steam.
 */
function TakeOverAllDialog({ games, onClose, onDone }: { games: SteamCloudApp[]; onClose: () => void; onDone: (done: SteamCloudDone[]) => Promise<void> }) {
  const t = useT();
  const c = t.steamCloud;
  const ui = useUi();
  const [status, setStatus] = useState<SteamCloudStatus | null>(null);
  const [noWrapper, setNoWrapper] = useState(false);
  // The account's switch as Steam has it now, read through any of the games.
  const probe = games[0]?.key;
  useEffect(() => {
    if (!probe) return;
    let live = true;
    const read = () =>
      api.steamCloudStatus(probe).then(
        (s) => live && setStatus(s),
        () => undefined,
      );
    read();
    const timer = setInterval(read, 2000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [probe]);
  const off = status ? !status.globalEnabled : null;
  // The launch options of all of them go through Steam too (or aren't changed).
  const viaPlugin = !!status?.plugin?.cloud && (noWrapper || status.plugin.launchOptions);

  const take = async () => {
    try {
      const done = await withSteamClosed(ui, (close) => api.steamCloudTakeOverAll(!noWrapper, close, viaPlugin));
      if (done) await onDone(done);
    } catch (e) {
      // Refused as a whole (Steam Cloud still on): the dialog stays.
      ui.toast(plain(e), "bad");
    }
  };

  return (
    <Modal
      title={c.takeOverAllTitle(games.length)}
      onClose={onClose}
      footer={
        <>
          <Button kind="ghost" onClick={onClose}>
            {t.common.cancel}
          </Button>
          <Button kind="primary" onClick={take} disabled={off === null} busyText={c.allTaking}>
            {c.takeOverDo}
          </Button>
        </>
      }
    >
      {viaPlugin && !off ? (
        <p>
          {c.pluginAllOff(games.length)} <Hint text={c.pluginAllOffHint} />
        </p>
      ) : (
        <ul className="steps-list">
          <li className={off ? "done" : ""}>
            <Icon name={off ? "check" : "clock"} size={16} />
            <span>
              {off ? c.stepAllOff : c.stepAllOffTodo} <Hint text={c.stepAllOffHint} />
            </span>
          </li>
        </ul>
      )}
      <div className="check-line">
        <label>
          <input type="checkbox" checked={noWrapper} onChange={(e) => setNoWrapper(e.target.checked)} />
          {c.noWrapper}
        </label>
        <Hint text={c.noWrapperHint} />
      </div>
    </Modal>
  );
}
