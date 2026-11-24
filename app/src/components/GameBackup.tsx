// Game page → "Game files": back up the game folder to the cloud, restore it
// to a folder of choice, see and delete versions (docs/plan-game-backup.md).
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { api, errorText, GameView, inTauri } from "../api";
import { bytes, dateTime } from "../format";
import "../gameBackup.css";
import { BackupFinished, BackupInfo, BackupJob, etaSecs, gameBackupApi, ScanSummary, SLOW_RATES, VersionSummary } from "../gameBackupApi";
import { msg, useT } from "../i18n";
import { duration } from "./SpeedTest";
import { withSteamClosed } from "./steamAction";
import { Badge, Button, Icon, Modal, Spinner, useUi } from "./ui";

export function GameBackupPanel({ game, onChanged }: { game: GameView; onChanged: () => Promise<void> }) {
  const ui = useUi();
  const t = useT();
  const c = t.gameBackup.card;
  const key = game.key;
  const [info, setInfo] = useState<BackupInfo | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [job, setJob] = useState<BackupJob | null>(null);
  const [preview, setPreview] = useState<ScanSummary | null>(null);
  const [restoreOpen, setRestoreOpen] = useState(false);
  const [showVersions, setShowVersions] = useState(false);
  const [showExclude, setShowExclude] = useState(false);
  const [excludeText, setExcludeText] = useState("");
  const [needShortcut, setNeedShortcut] = useState(false);

  const load = useCallback(async () => {
    try {
      const i = await gameBackupApi.info(key);
      setInfo(i);
      setJob(i.job);
      setExcludeText(i.exclude.join("\n"));
      setLoadError(null);
    } catch (e) {
      setLoadError(errorText(e));
    }
  }, [key]);

  useEffect(() => {
    setInfo(null);
    setPreview(null);
    setNeedShortcut(false);
    load();
  }, [load, game.exe, game.exeExists]);

  // The Steam plugin asked to install this game.
  useEffect(() => {
    gameBackupApi.takeInstall(key).then((yes) => yes && setRestoreOpen(true), () => undefined);
  }, [key]);

  useEffect(() => {
    if (!inTauri) return;
    const off: Array<() => void> = [];
    listen<{ key: string; job: BackupJob }>("game-backup-progress", (e) => {
      if (e.payload.key === key) setJob(e.payload.job);
    }).then((u) => off.push(u));
    listen<BackupFinished>("game-backup-finished", (e) => {
      const f = e.payload;
      if (f.key !== key) return;
      setJob(null);
      ui.toast(f.message, f.ok ? "ok" : f.cancelled ? "info" : "bad");
      const fix = (f.outcome as { fix?: { shortcut?: string } } | null)?.fix;
      if (f.kind === "restore" && fix?.shortcut === "needsSteamClosed") setNeedShortcut(true);
      load();
      if (f.kind === "restore") onChanged();
    }).then((u) => off.push(u));
    listen<string>("game-backup-install", (e) => {
      if (e.payload === key) {
        gameBackupApi.takeInstall(key);
        setRestoreOpen(true);
      }
    }).then((u) => off.push(u));
    return () => off.forEach((u) => u());
  }, [key, load, onChanged, ui]);

  const run = async (f: () => Promise<unknown>) => {
    try {
      await f();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  if (!info) {
    return (
      <section className="panel">
        <div className="panel-head">
          <Icon name="upload" />
          <h3>{c.title}</h3>
        </div>
        {loadError ? <div className="health health-bad">{loadError}</div> : <Spinner />}
      </section>
    );
  }

  const latest: VersionSummary | undefined = info.versions[0];
  const size = preview?.bytes ?? latest?.totalSize ?? 0;
  const slowRate = SLOW_RATES[info.cloudKind];
  const slowText =
    info.slow && slowRate && size > 0
      ? info.cloudKind === "baiduPan"
        ? c.slowBaidu(bytes(size), duration(etaSecs(size, slowRate.down)))
        : c.slowQuark(bytes(size), duration(etaSecs(size, slowRate.up)))
      : null;
  const busy = !!job;

  const startBackup = async () => {
    // First backup on a slow cloud: say how long it takes, ask once.
    if (info.slow && info.versions.length === 0) {
      const p = preview ?? (await gameBackupApi.preview(key));
      setPreview(p);
      const rate = SLOW_RATES[info.cloudKind];
      const text =
        info.cloudKind === "baiduPan"
          ? c.slowBaidu(bytes(p.bytes), duration(etaSecs(p.bytes, rate.down)))
          : c.slowQuark(bytes(p.bytes), duration(etaSecs(p.bytes, rate.up)));
      const ok = await ui.confirm({ title: c.slowTitle, text, confirm: c.slowConfirm });
      if (!ok) return;
    }
    await gameBackupApi.start(key);
    setJob({ kind: "backup", phase: "starting", done: 0, total: 0, uploaded: 0, elapsedSecs: 0, target: null });
  };

  const pickRoot = async () => {
    const dir = await api.pickFolder(msg().gameBackup.card.pickTitle, info.root);
    if (!dir) return;
    await gameBackupApi.set(key, { root: dir });
    setPreview(null);
    await load();
  };

  const remove = async (v: VersionSummary) => {
    const when = dateTime(v.createdAt);
    const ok = await ui.confirm({ title: c.deleteTitle, text: c.deleteText(when), confirm: c.delete, danger: true });
    if (!ok) return;
    await gameBackupApi.remove(key, v.id);
    setJob({ kind: "delete", phase: "starting", done: 0, total: 0, uploaded: 0, elapsedSecs: 0, target: null });
  };

  const badge = info.exeMissing && latest ? (
    <Badge tone="warn">{c.missing}</Badge>
  ) : latest ? (
    <Badge tone="ok">{c.backedUp}</Badge>
  ) : (
    <Badge tone="muted">{c.notBackedUp}</Badge>
  );

  return (
    <section className="panel" id="game-backup">
      <div className="panel-head">
        <Icon name="upload" />
        <h3>{c.title}</h3>
        {info.hasCloud && badge}
      </div>

      {!info.hasCloud ? (
        <p className="muted small">{c.noCloud}</p>
      ) : (
        <>
          {info.exeMissing && latest && !busy && (
            <div className="gb-warn row">
              <span className="grow">{c.missingBanner}</span>
              <Button small kind="primary" icon="download" onClick={() => setRestoreOpen(true)}>
                {c.installNow}
              </Button>
            </div>
          )}
          {!latest && <p className="muted small">{c.intro}</p>}
          {info.error && <div className="health health-warn">{c.cloudError(info.error)}</div>}

          <div className="row">
            <span className="muted small">{c.folder}</span>
            {info.root ? <code className="path grow">{info.root}</code> : <span className="muted small grow">{c.noFolder}</span>}
            <Button small onClick={() => run(pickRoot)} disabled={busy}>
              {c.change}
            </Button>
            {info.rootCustom && (
              <Button
                small
                kind="ghost"
                disabled={busy}
                onClick={() =>
                  run(async () => {
                    await gameBackupApi.set(key, { root: "" });
                    setPreview(null);
                    await load();
                  })
                }
              >
                {c.useDefault}
              </Button>
            )}
          </div>
          {info.root && !info.rootExists && !info.exeMissing && <div className="health health-warn">{c.folderMissing}</div>}
          {info.fileOutside && <div className="health health-warn">{c.fileOutside(info.fileOutside)}</div>}
          {info.rootExists && (
            <div className="row">
              {preview ? (
                <span className="muted small">
                  {c.sizeResult(preview.files, bytes(preview.bytes), preview.excludedFiles, bytes(preview.excludedBytes))}
                </span>
              ) : (
                <Button small kind="ghost" disabled={busy} onClick={() => run(async () => setPreview(await gameBackupApi.preview(key)))}>
                  {c.size}
                </Button>
              )}
            </div>
          )}

          {latest && (
            <div className="small">
              {c.latest(dateTime(latest.createdAt), latest.deviceName, bytes(latest.totalSize))}
              <span className="muted"> · {c.stored(bytes(info.storedBytes))}</span>
            </div>
          )}
          {slowText && <div className="health health-warn">{slowText}</div>}
          {info.pendingRestore && !busy && (
            <div className="gb-warn row">
              <span className="grow">{c.pending(info.pendingRestore.target)}</span>
              <Button
                small
                kind="primary"
                onClick={() =>
                  run(async () => {
                    const p = info.pendingRestore!;
                    await gameBackupApi.restore(key, p.version, p.target, true);
                    setJob({ kind: "restore", phase: "starting", done: 0, total: 0, uploaded: 0, elapsedSecs: 0, target: p.target });
                  })
                }
              >
                {c.resume}
              </Button>
            </div>
          )}
          {needShortcut && (
            <div className="row">
              <Button
                small
                icon="steam"
                onClick={() =>
                  run(async () => {
                    const done = await withSteamClosed(ui, (close) => api.steamSetEnabled(key, false, close));
                    if (done !== null) {
                      setNeedShortcut(false);
                      ui.toast(msg().gameBackup.card.shortcutUpdated);
                    }
                  })
                }
              >
                {c.updateShortcut}
              </Button>
            </div>
          )}

          {job ? (
            <JobProgress job={job} onStop={() => run(() => gameBackupApi.cancel(key))} />
          ) : (
            <div className="row">
              <Button kind="primary" icon="upload" disabled={!info.rootExists || game.running} onClick={() => run(startBackup)}>
                {latest ? c.update : c.backup}
              </Button>
              {latest && (
                <Button icon="download" disabled={game.running} onClick={() => setRestoreOpen(true)}>
                  {c.restore}
                </Button>
              )}
              {info.versions.length > 0 && (
                <button className="link" onClick={() => setShowVersions((v) => !v)}>
                  {c.versions(info.versions.length)}
                </button>
              )}
              <button className="link" onClick={() => setShowExclude((v) => !v)}>
                {c.excludeTitle}
              </button>
            </div>
          )}

          {showVersions && (
            <div className="gb-versions">
              {info.versions.map((v) => (
                <div key={v.id} className="gb-version">
                  <span className="grow">
                    {dateTime(v.createdAt)} · {v.deviceName} · {bytes(v.totalSize)}
                  </span>
                  <Button small kind="ghost" icon="trash" disabled={busy} title={c.delete} onClick={() => run(() => remove(v))} />
                </div>
              ))}
              <div className="row">
                <span className="muted small">{c.keep}</span>
                <select
                  className="input select"
                  style={{ width: "auto" }}
                  value={info.keep}
                  disabled={busy}
                  onChange={(e) => run(async () => {
                    await gameBackupApi.set(key, { keep: Number(e.target.value) });
                    await load();
                  })}
                >
                  {[1, 2, 3, 5].map((n) => (
                    <option key={n} value={n}>
                      {n}
                    </option>
                  ))}
                  <option value={0}>{c.keepAll}</option>
                </select>
                <span className="muted small">{info.keep === 0 ? "" : c.keepUnit}</span>
              </div>
              <div className="muted small">{c.keepHint}</div>
            </div>
          )}

          {showExclude && (
            <div className="gb-versions">
              <div className="muted small">{c.excludeHint}</div>
              <textarea className="input" rows={4} value={excludeText} onChange={(e) => setExcludeText(e.target.value)} />
              <div className="muted small">{c.defaults(info.defaultExclude.join("  "))}</div>
              <div className="row">
                <Button
                  small
                  kind="primary"
                  disabled={busy}
                  onClick={() =>
                    run(async () => {
                      await gameBackupApi.set(key, { exclude: excludeText.split("\n") });
                      setPreview(null);
                      await load();
                      ui.toast(msg().settings.saved);
                    })
                  }
                >
                  {t.common.save}
                </Button>
              </div>
            </div>
          )}
        </>
      )}

      {restoreOpen && info.versions.length > 0 && (
        <RestoreDialog
          gameKey={key}
          info={info}
          onClose={() => setRestoreOpen(false)}
          onStarted={(target) => {
            setRestoreOpen(false);
            setJob({ kind: "restore", phase: "starting", done: 0, total: 0, uploaded: 0, elapsedSecs: 0, target });
          }}
        />
      )}
    </section>
  );
}

function JobProgress({ job, onStop }: { job: BackupJob; onStop: () => void }) {
  const c = useT().gameBackup.card;
  const withBytes = job.phase === "uploading" || job.phase === "downloading" || job.phase === "verifying";
  const pct = job.total > 0 ? Math.min(100, Math.floor((job.done / job.total) * 100)) : 0;
  const rate = job.elapsedSecs > 2 && job.done > 0 ? job.done / job.elapsedSecs : 0;
  const left = rate > 0 && job.total > job.done ? (job.total - job.done) / rate : null;
  return (
    <div className="update-progress">
      <div className="row">
        <span className="small grow">
          {c.phase[job.phase] ?? job.phase}
          {job.target && ` · ${job.target}`}
        </span>
        <Button small onClick={onStop} title={c.stopHint}>
          {c.stop}
        </Button>
      </div>
      <div className="bar">
        <span style={{ width: `${withBytes ? pct : job.total > 0 ? pct : 0}%` }} />
      </div>
      {withBytes && job.total > 0 && (
        <div className="muted small">
          {job.kind === "backup" ? c.read(bytes(job.done), bytes(job.total)) : c.got(bytes(job.done), bytes(job.total))}
          {job.kind === "backup" && job.uploaded > 0 && ` · ${c.uploaded(bytes(job.uploaded))}`}
          {rate > 0 && ` · ${c.eta(bytes(rate), duration(left))}`}
        </div>
      )}
    </div>
  );
}

function RestoreDialog({
  gameKey,
  info,
  onClose,
  onStarted,
}: {
  gameKey: string;
  info: BackupInfo;
  onClose: () => void;
  onStarted: (target: string) => void;
}) {
  const ui = useUi();
  const t = useT();
  const r = t.gameBackup.restore;
  const [versionId, setVersionId] = useState(info.versions[0].id);
  const [chosen, setChosen] = useState(info.pendingRestore?.target ?? info.defaultTarget ?? "");
  const [target, setTarget] = useState<string | null>(null);
  const [fix, setFix] = useState(true);
  const version = info.versions.find((v) => v.id === versionId) ?? info.versions[0];

  useEffect(() => {
    if (!chosen) {
      setTarget(null);
      return;
    }
    gameBackupApi.resolveTarget(chosen, version.root, version.id).then(setTarget, () => setTarget(chosen));
  }, [chosen, version]);

  const pick = async () => {
    const dir = await api.pickFolder(msg().gameBackup.restore.pickTitle, chosen || null);
    if (dir) setChosen(dir);
  };

  const start = async () => {
    if (!target) return;
    try {
      await gameBackupApi.restore(gameKey, version.id, target, fix);
      onStarted(target);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const slow = info.cloudKind === "baiduPan" ? t.gameBackup.card.slowBaidu(bytes(version.totalSize), duration(etaSecs(version.totalSize, SLOW_RATES.baiduPan.down))) : null;

  return (
    <Modal
      title={r.title}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>{t.common.cancel}</Button>
          <Button kind="primary" icon="download" disabled={!target} onClick={start}>
            {r.start}
          </Button>
        </>
      }
    >
      <div className="form gb-restore">
        <label>
          <span>{r.version}</span>
          <select className="input select" value={versionId} onChange={(e) => setVersionId(e.target.value)}>
            {info.versions.map((v) => (
              <option key={v.id} value={v.id}>
                {r.versionLabel(dateTime(v.createdAt), v.deviceName, bytes(v.totalSize))}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>{r.target}</span>
          <div className="row">
            {chosen ? <code className="path grow">{chosen}</code> : <span className="muted small grow">{r.noTarget}</span>}
            <Button small icon="folder" onClick={pick}>
              {r.pick}
            </Button>
          </div>
        </label>
        {target && <div className="small">{r.willGo(target)}</div>}
        <div className="muted small">{r.need(bytes(version.totalSize))}</div>
        {slow && <div className="health health-warn">{slow}</div>}
      </div>
      <div className="row gb-check">
        <input id="gb-fix" type="checkbox" checked={fix} onChange={(e) => setFix(e.target.checked)} />
        <span className="small" onClick={() => setFix(!fix)}>
          {r.fix}
        </span>
      </div>
    </Modal>
  );
}
