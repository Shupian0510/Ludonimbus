// A game Steam installed, on its page (docs/plan-v2.md, section 8): who
// keeps its saves (Steam Cloud alone, Ludonimbus beside it, Ludonimbus), and
// changing that.
import { useEffect, useState } from "react";
import { api, errorText, GameView, SteamCloudStatus } from "../api";
import { msg, useT } from "../i18n";
import { withSteamClosed } from "./steamAction";
import { Badge, Button, Hint, Icon, Modal, useUi } from "./ui";

type Ui = ReturnType<typeof useUi>;

/** The backend's errors about Steam come with a code in front ("STEAM_CLOUD_ON: …"). */
const plain = (e: unknown) => errorText(e).replace(/^[A-Z_]+:\s*/, "");

/** Steam's own properties window of the game: its Steam Cloud switch is there. */
export const openSteamProperties = (ui: Ui, key: string) => api.steamCloudOpenProperties(key).catch((e) => ui.toast(plain(e), "bad"));

/**
 * Give the game back to Steam Cloud. Nothing of the saves changes, so it is
 * done at once; what is left is the user's to do in Steam.
 */
export async function handBack(ui: Ui, game: GameView, onChanged: () => Promise<void>) {
  try {
    const s = await withSteamClosed(ui, (close) => api.steamCloudHandBack(game.key, close));
    if (!s) return;
    // "Backup only" means versions are stored again by themselves.
    if (!game.autoSync) await api.updateGame(game.key, { autoSync: true });
    const m = msg().steamCloud;
    ui.toast(m.handedBack, "ok", { label: m.openProperties, run: () => openSteamProperties(ui, game.key) });
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
 * Taking a game over from Steam Cloud. The two things Steam's files have to
 * show are the user's to bring about in Steam, so they are listed as they
 * read now; the check that counts is the backend's when "take over" is
 * pressed (with Steam closed its files are as it left them).
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

  const take = async () => {
    try {
      const s = await withSteamClosed(ui, (close) => api.steamCloudTakeOver(game.key, !noWrapper, close));
      if (!s) return;
      ui.toast(noWrapper ? msg().steamCloud.takenBackground : msg().steamCloud.taken);
      await onDone();
    } catch (e) {
      ui.toast(plain(e), "bad");
    }
  };

  const off = !!status && (!status.cloudEnabled || !status.globalEnabled);
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
