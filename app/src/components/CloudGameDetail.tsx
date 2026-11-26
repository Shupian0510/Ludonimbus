// A game the cloud has but this PC doesn't (docs/plan-phase5.md §5, V2): its
// versions and playtime from the cloud, adding it here, installing it from
// a backup of the game itself, and applying its saves to another version.
import { useEffect, useState } from "react";
import { api, CloudGameInfo, CloudOnlyView, errorText, Overview, ShortcutView } from "../api";
import { bytes, dateTime, duration } from "../format";
import { msg, useT } from "../i18n";
import { AccountPicker, useAccounts } from "./AddGame";
import { VersionBar } from "./SaveLibrary";
import { Saves } from "./Saves";
import { Button, Icon, Modal, useUi } from "./ui";

export function CloudGameDetail({
  game,
  overview,
  onBack,
  onOpen,
  onChanged,
  onAdded,
}: {
  game: CloudOnlyView;
  overview: Overview;
  onBack: () => void;
  onOpen: (key: string) => void;
  onChanged: () => Promise<void>;
  /** Added to this PC: show its game page. */
  onAdded: (key: string) => Promise<void>;
}) {
  const t = useT();
  const s = t.savelib;
  const ui = useUi();
  const [info, setInfo] = useState<CloudGameInfo | null>(null);
  const [adding, setAdding] = useState(false);
  const [heroFailed, setHeroFailed] = useState(false);
  const hero = !heroFailed ? game.art.hero : null;

  useEffect(() => {
    api.cloudGameInfo(game.key).then(setInfo, () => setInfo({ playtime: game.playtime, gameBackup: null }));
  }, [game.key, game.playtime]);

  const install = async () => {
    try {
      // Added without a program; its page opens the restore dialog.
      const key = await api.addFromCloud(game.key, null, null, null);
      await api.requestInstall(key);
      await onAdded(key);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const playtime = info?.playtime ?? game.playtime;
  const played = playtime.devices.filter((d) => d.secs > 0);
  return (
    <div className="detail">
      <div className={`hero${hero ? "" : " hero-plain"}`} style={!hero && game.art.cover ? { backgroundImage: `url(${game.art.cover})` } : undefined}>
        {hero && <img className="hero-img missing-art" src={hero} alt="" onError={() => setHeroFailed(true)} />}
        <div className="hero-shade" />
        <button className="back" onClick={onBack}>
          <Icon name="back" /> {t.library.title}
        </button>
        <div className="hero-title">
          <h1>{game.name}</h1>
        </div>
      </div>

      <div className="actionbar">
        <Button kind="primary" icon="plus" onClick={() => setAdding(true)}>
          {s.addHere}
        </Button>
        {info?.gameBackup && (
          <Button icon="download" onClick={install}>
            {s.installFromCloud}
          </Button>
        )}
        <div className="actionbar-status">
          {playtime.totalSecs > 0 && (
            <span className="muted playtime">
              <Icon name="clock" size={13} /> {t.game.playtime(duration(playtime.totalSecs))}
            </span>
          )}
        </div>
      </div>

      <div className="detail-body">
        <div className="banner banner-info not-here">
          <Icon name="cloud" />
          <div>
            <b>{s.cloudBannerTitle}</b>
            <p>{s.cloudBannerText}</p>
            {info && (
              <p className="muted small">
                {info.gameBackup
                  ? s.installHint(bytes(info.gameBackup.size), info.gameBackup.deviceName, dateTime(info.gameBackup.createdAt))
                  : s.noGameBackup}
              </p>
            )}
          </div>
        </div>

        <VersionBar overview={overview} current={game.key} onOpen={onOpen} onChanged={onChanged} />

        {played.length > 0 && (
          <section className="panel">
            <div className="panel-head">
              <Icon name="clock" />
              <h3>{s.playedOn}</h3>
            </div>
            {played.map((d) => (
              <div key={d.device} className="row space">
                <span>{d.deviceName}</span>
                <span className="muted">{duration(d.secs)}</span>
              </div>
            ))}
          </section>
        )}

        <div className="tabs">
          <button className="on">{t.game.tabVersions}</button>
        </div>
        <Saves game={game} overview={overview} disabled cloudOnly onChanged={async () => undefined} onApplied={async (k) => onOpen(k)} />
      </div>

      {adding && <AddHere game={game} onClose={() => setAdding(false)} onAdded={onAdded} />}
    </div>
  );
}

/** Add a game only the cloud has to this PC: a Steam shortcut, a program, or nothing yet. */
function AddHere({ game, onClose, onAdded }: { game: CloudOnlyView; onClose: () => void; onAdded: (key: string) => Promise<void> }) {
  const t = useT();
  const s = t.savelib;
  const ui = useUi();
  const { accounts, account, setAccount } = useAccounts();
  const [shortcuts, setShortcuts] = useState<ShortcutView[]>([]);

  useEffect(() => {
    if (account) api.steamShortcuts(account).then(setShortcuts, () => setShortcuts([]));
  }, [account]);

  const add = async (sc: ShortcutView | null, pick: boolean) => {
    try {
      let exe: string | null = null;
      if (pick) {
        exe = await api.pickExe();
        if (!exe) return;
      }
      const key = await api.addFromCloud(game.key, sc?.account ?? null, sc?.appid ?? null, exe);
      ui.toast(msg().savelib.added(game.name));
      await onAdded(key);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const free = shortcuts.filter((x) => !x.addedKey && !x.wrappedKey);
  return (
    <Modal
      title={s.addTitle}
      onClose={onClose}
      footer={
        <>
          <Button onClick={() => add(null, false)}>{s.addWithout}</Button>
          <Button kind="primary" icon="folder" onClick={() => add(null, true)}>
            {s.pickExe}
          </Button>
        </>
      }
    >
      <p className="muted">{s.addText}</p>
      <div className="row space">
        <h3>{s.pickShortcut}</h3>
        <AccountPicker accounts={accounts} account={account} setAccount={setAccount} />
      </div>
      {free.length === 0 && <p className="muted small">{s.noShortcuts}</p>}
      <div className="pick-list">
        {free.map((sc) => (
          <button key={sc.appid} className="pick-row" onClick={() => add(sc, false)}>
            <b>{sc.suggestedName}</b>
            <code className="path small">{sc.exe}</code>
          </button>
        ))}
      </div>
    </Modal>
  );
}
