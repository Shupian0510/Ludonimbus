import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorText, Gallery, GameView, ScreenshotMode, Shot } from "../api";
import { bytes, dateTime } from "../format";
import { msg, useT } from "../i18n";
import { DeviceChip, Stat } from "./kit";
import { Badge, Button, Hint, Icon, Modal, Spinner, useUi } from "./ui";

/** At most this many thumbnails are downloaded from the cloud at once. */
const PARALLEL = 3;

function queue() {
  let running = 0;
  const waiting: (() => void)[] = [];
  return async function <T>(job: () => Promise<T>): Promise<T> {
    if (running >= PARALLEL) await new Promise<void>((r) => waiting.push(r));
    running++;
    try {
      return await job();
    } finally {
      running--;
      waiting.shift()?.();
    }
  };
}

export function Screenshots({ game, deviceId }: { game: GameView; deviceId: string }) {
  const t = useT();
  const s = t.screenshots;
  const ui = useUi();
  const [data, setData] = useState<Gallery | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<Shot | null>(null);
  const limit = useRef(queue()).current;

  const load = useCallback(() => {
    setError(null);
    return api.screenshots(game.key).then(setData, (e) => setError(errorText(e)));
  }, [game.key]);

  useEffect(() => {
    load();
  }, [load]);

  const setMode = async (mode: ScreenshotMode) => {
    await api.saveSettings({ screenshots: mode });
    await load();
  };

  const syncNow = async () => {
    try {
      const r = await api.screenshotSync(game.key);
      ui.toast(s.synced(r.uploaded, r.downloaded) + (r.failed ? s.failed(r.failed) : ""), r.failed ? "warn" : "ok");
      await load();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  if (error) return <div className="banner banner-bad">{error}</div>;
  if (!data) return <div className="muted pad">{s.loading}</div>;

  const deviceLabel = (shot: Shot) =>
    shot.device === deviceId ? s.thisPc : s.fromDevice(shot.deviceName ?? shot.device.slice(0, 10));

  return (
    <div className="panel shots-panel">
      <div className="shots-head">
        <label className="shots-mode">
          <span>{s.mode}</span>
          <select className="input select" value={data.mode} onChange={(e) => setMode(e.target.value as ScreenshotMode)}>
            {(["off", "upload", "sync"] as const).map((m) => (
              <option key={m} value={m}>
                {s.modes[m]}
              </option>
            ))}
          </select>
        </label>
        <div className="row">
          <Button icon="sync" onClick={syncNow} disabled={data.mode === "off" || !data.linked || game.running}>
            {s.sync}
          </Button>
          {data.folder && (
            <Button icon="folder" onClick={() => api.openPath(data.folder!).catch((e) => ui.toast(errorText(e), "bad"))}>
              {s.openFolder}
            </Button>
          )}
        </div>
      </div>

      {data.mode === "off" && (
        <div className="banner banner-info">
          <Icon name="image" />
          <span>
            <b>{s.offTitle}</b> <Hint text={s.offText} />
          </span>
        </div>
      )}
      {!data.linked && <div className="banner banner-info">{s.notLinked}</div>}
      {data.cloudError && <div className="banner banner-info">{s.cloudError(data.cloudError)}</div>}

      {data.shots.length === 0 ? (
        <div className="empty-inline">
          <b>{s.emptyTitle}</b>
          <p className="muted">{s.emptyText}</p>
        </div>
      ) : (
        <>
          <p className="add-facts">
            <Stat icon="image" title={s.summary(data.shots.length, bytes(data.cloudBytes))}>
              {data.shots.length}
            </Stat>
            <Stat icon="cloud">{bytes(data.cloudBytes)}</Stat>
            {data.mode === "sync" && <Hint text={s.steamNote} />}
          </p>
          <div className="shots">
            {data.shots.map((shot) => (
              <Tile key={`${shot.device}/${shot.name}`} game={game} shot={shot} here={shot.device === deviceId} limit={limit} onOpen={() => setOpen(shot)} />
            ))}
          </div>
        </>
      )}

      {open && <Viewer game={game} shot={open} label={deviceLabel(open)} onClose={() => setOpen(null)} />}
    </div>
  );
}

function Tile({
  game,
  shot,
  here,
  limit,
  onOpen,
}: {
  game: GameView;
  shot: Shot;
  /** Taken on this device. */
  here: boolean;
  limit: <T>(job: () => Promise<T>) => Promise<T>;
  onOpen: () => void;
}) {
  const s = msg().screenshots;
  const [url, setUrl] = useState(shot.thumbUrl);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    if (url || !shot.inCloud) return;
    let live = true;
    limit(() => api.screenshotFetch(game.key, shot.device, shot.name, true)).then(
      (u) => live && setUrl(u),
      () => live && setFailed(true),
    );
    return () => {
      live = false;
    };
  }, [game.key, shot, url, limit]);
  return (
    <button className="shot" onClick={onOpen} title={shot.takenAt ? dateTime(shot.takenAt) : shot.name}>
      <div className="shot-img">
        {url ? <img src={url} alt="" loading="lazy" /> : failed ? <Icon name="image" size={28} /> : <Spinner />}
      </div>
      <div className="shot-meta">
        <span className="muted small">{shot.takenAt ? dateTime(shot.takenAt) : shot.name}</span>
        <span className="shot-badges">
          <DeviceChip name={shot.deviceName ?? shot.device.slice(0, 10)} here={here} />
          {!shot.inCloud && <Badge tone="info">{s.onlyHere}</Badge>}
          {!shot.onThisPc && <Badge tone="muted">{s.onlyCloud}</Badge>}
        </span>
      </div>
    </button>
  );
}

function Viewer({ game, shot, label, onClose }: { game: GameView; shot: Shot; label: string; onClose: () => void }) {
  const s = msg().screenshots;
  const ui = useUi();
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.screenshotFetch(game.key, shot.device, shot.name, false).then(setUrl, (e) => setError(errorText(e)));
  }, [game.key, shot]);
  return (
    <Modal title={`${shot.takenAt ? dateTime(shot.takenAt) : shot.name} · ${label}`} onClose={onClose} wide>
      <div className="shot-view">{url ? <img src={url} alt={shot.name} /> : error ? <div className="banner banner-bad">{error}</div> : <Spinner />}</div>
      <div className="row shot-view-foot">
        <span className="add-facts">
          <code className="sample">{shot.name}</code>
          <Stat icon="file">{bytes(shot.size)}</Stat>
        </span>
        {(shot.onThisPc || url) && (
          <Button icon="folder" small onClick={() => api.screenshotReveal(game.key, shot.device, shot.name).catch((e) => ui.toast(errorText(e), "bad"))}>
            {s.reveal}
          </Button>
        )}
      </div>
    </Modal>
  );
}
