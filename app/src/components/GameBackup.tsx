// Game page → "Game files": back up the game's parts (program, game files,
// settings, firmware, or the whole folder) to the cloud, restore them to
// folders of choice, see and delete versions (docs/plan-game-backup.md).
import { useCallback, useEffect, useState } from "react";
import { api, errorText, GameView } from "../api";
import { bytes, dateTime } from "../format";
import "../gameBackup.css";
import {
  BAIDU_FREE_DOWN,
  BackupFinished,
  BackupInfo,
  BackupJob,
  etaSecs,
  gameBackupApi,
  PartKind,
  PartSummary,
  PreviewPart,
  SettingsPatch,
  Targets,
  VersionSummary,
} from "../gameBackupApi";
import { msg, useT } from "../i18n";
import { useTauriEvent } from "../useTauriEvent";
import { duration } from "./SpeedTest";
import { withSteamClosed } from "./steamAction";
import { DeviceChip, Stat, When } from "./kit";
import { Badge, Button, Hint, Icon, Modal, Spinner, useUi } from "./ui";

const startJob = (kind: BackupJob["kind"], target: string | null = null): BackupJob => ({
  kind,
  phase: "starting",
  done: 0,
  total: 0,
  uploaded: 0,
  elapsedSecs: 0,
  target,
});

/** "Program Cemu 2.6 · Game files v208 · Settings and mods" */
function partsText(parts: PartSummary[]): string {
  const k = msg().gameBackup.card.kind;
  return parts
    .map((p) => {
      const label = p.kind === "program" ? `${k.program} ${p.name}` : p.kind === "content" ? k.content : k[p.kind];
      return [label, p.version, bytes(p.totalSize)].filter(Boolean).join(" ");
    })
    .join(" · ");
}

export function GameBackupPanel({ game, onChanged }: { game: GameView; onChanged: () => Promise<void> }) {
  const ui = useUi();
  const t = useT();
  const c = t.gameBackup.card;
  const key = game.key;
  const [info, setInfo] = useState<BackupInfo | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [job, setJob] = useState<BackupJob | null>(null);
  const [preview, setPreview] = useState<PreviewPart[] | null>(null);
  const [restoreOpen, setRestoreOpen] = useState(false);
  const [showVersions, setShowVersions] = useState(false);
  const [showOptions, setShowOptions] = useState(false);
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

  useTauriEvent<{ key: string; job: BackupJob }>("game-backup-progress", (p) => {
    if (p.key === key) setJob(p.job);
  });
  useTauriEvent<BackupFinished>("game-backup-finished", (f) => {
    if (f.key !== key) return;
    setJob(null);
    ui.toast(f.message, f.ok ? "ok" : f.cancelled ? "info" : "bad");
    const fix = (f.outcome as { fix?: { shortcut?: string } } | null)?.fix;
    if (f.kind === "restore" && fix?.shortcut === "needsSteamClosed") setNeedShortcut(true);
    load();
    if (f.kind === "restore") onChanged();
  });
  useTauriEvent<string>("game-backup-install", (k) => {
    if (k === key) {
      gameBackupApi.takeInstall(key);
      setRestoreOpen(true);
    }
  });

  const run = async (f: () => Promise<unknown>) => {
    try {
      await f();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const save = async (patch: SettingsPatch) => {
    await gameBackupApi.set(key, patch);
    setPreview(null);
    await load();
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
  const previewBytes = preview?.reduce((n, p) => n + p.bytes, 0);
  const size = previewBytes ?? latest?.totalSize ?? 0;
  // Baidu Netdisk without membership: restoring takes long.
  const slowText = info.slow && size > 0 ? c.slowBaidu(bytes(size), duration(etaSecs(size, BAIDU_FREE_DOWN))) : null;
  const busy = !!job;

  const startBackup = async () => {
    // First backup on Baidu Netdisk: say how long restoring would take, ask once.
    if (info.slow && info.versions.length === 0) {
      const p = preview ?? (await gameBackupApi.preview(key));
      setPreview(p);
      const total = p.reduce((n, x) => n + x.bytes, 0);
      const text = c.slowBaidu(bytes(total), duration(etaSecs(total, BAIDU_FREE_DOWN)));
      const ok = await ui.confirm({ title: c.slowTitle, text, confirm: c.slowConfirm });
      if (!ok) return;
    }
    await gameBackupApi.start(key);
    setJob(startJob("backup"));
  };

  const pickRoot = async () => {
    const dir = await api.pickFolder(msg().gameBackup.card.pickTitle, info.root);
    if (dir) await save({ root: dir });
  };

  const remove = async (v: VersionSummary) => {
    const ok = await ui.confirm({ title: c.deleteTitle, text: c.deleteText(dateTime(v.createdAt)), confirm: c.delete, danger: true });
    if (!ok) return;
    await gameBackupApi.remove(key, v.id);
    setJob(startJob("delete"));
  };

  const toggleSkip = (k: PartKind, on: boolean) =>
    run(() => save({ skip: on ? info.skip.filter((x) => x !== k) : [...info.skip, k] }));

  const badge =
    info.exeMissing && latest ? (
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
        <Hint text={c.intro} />
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
          {info.error && <div className="health health-warn">{c.cloudError(info.error)}</div>}

          <div className="row">
            <span className="muted small">{info.byParts ? c.programFolder : c.folder}</span>
            {info.root ? <code className="path grow">{info.root}</code> : <span className="muted small grow">{c.noFolder}</span>}
            <Button small onClick={() => run(pickRoot)} disabled={busy}>
              {c.change}
            </Button>
            {info.rootCustom && (
              <Button small kind="ghost" disabled={busy} onClick={() => run(() => save({ root: "" }))}>
                {c.useDefault}
              </Button>
            )}
          </div>
          {info.root && !info.rootExists && !info.exeMissing && <div className="health health-warn">{c.folderMissing}</div>}

          {info.rootExists && (
            <div className="gb-parts">
              {preview ? (
                preview.map((p) => (
                  <div key={p.kind} className="small">
                    <b>{c.kind[p.kind]}</b> {p.kind === "program" ? p.name : ""} {p.version && <span className="version-tag">{p.version}</span>}{" "}
                    <Stat icon="file" title={c.previewLine(p.files, bytes(p.bytes))}>
                      {bytes(p.bytes)}
                    </Stat>
                    {p.excludedFiles > 0 && <span className="muted">{c.previewExcluded(p.excludedFiles, bytes(p.excludedBytes))}</span>}
                  </div>
                ))
              ) : (
                <Button small kind="ghost" disabled={busy} onClick={() => run(async () => setPreview(await gameBackupApi.preview(key)))}>
                  {c.size}
                </Button>
              )}
            </div>
          )}

          {latest && (
            // The newest backup: when, from which device, how big; what the cloud holds in all.
            <div className="gb-latest">
              <Stat icon="upload" title={c.latestTitle}>
                <When at={latest.createdAt} />
              </Stat>
              <DeviceChip name={latest.deviceName} />
              <Stat icon="file" title={partsText(latest.components)}>
                {bytes(latest.totalSize)}
              </Stat>
              <Stat icon="cloud" title={c.stored(bytes(info.storedBytes))}>
                {bytes(info.storedBytes)}
              </Stat>
            </div>
          )}
          {slowText && <div className="health health-warn">{slowText}</div>}
          {info.pendingRestore && !busy && (
            <div className="gb-warn row">
              <span className="grow">{c.pending(info.pendingRestore.targets.content ?? info.pendingRestore.targets.program)}</span>
              <Button
                small
                kind="primary"
                onClick={() =>
                  run(async () => {
                    const p = info.pendingRestore!;
                    await gameBackupApi.restore(key, p.version, p.targets, true);
                    setJob(startJob("restore", p.targets.content ?? p.targets.program));
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
              <Button icon="upload" disabled={!info.rootExists || game.running} onClick={() => run(startBackup)}>
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
              <button className="link" onClick={() => setShowOptions((v) => !v)}>
                {c.options}
              </button>
            </div>
          )}

          {showVersions && (
            <div className="gb-versions">
              {info.versions.map((v) => (
                <div key={v.id} className="gb-version">
                  <span className="grow gb-latest" title={partsText(v.components)}>
                    <When at={v.createdAt} />
                    <DeviceChip name={v.deviceName} />
                    <Stat icon="file">{bytes(v.totalSize)}</Stat>
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
                  onChange={(e) => run(() => save({ keep: Number(e.target.value) }))}
                >
                  {[1, 2, 3, 5].map((n) => (
                    <option key={n} value={n}>
                      {n}
                    </option>
                  ))}
                  <option value={0}>{c.keepAll}</option>
                </select>
                <span className="muted small">{info.keep === 0 ? "" : c.keepUnit}</span>
                <Hint text={c.keepHint} />
              </div>
            </div>
          )}

          {showOptions && (
            <div className="gb-versions">
              {info.byParts ? (
                <>
                  <b className="small">
                    {c.parts} <Hint text={info.firmwareAvailable ? `${c.partsHint}\n\n${c.firmwareHint}` : c.partsHint} />
                  </b>
                  {(["program", "content", "config"] as PartKind[]).map((k) => (
                    <label key={k} className="row gb-check-inline">
                      <input type="checkbox" checked={!info.skip.includes(k)} disabled={busy} onChange={(e) => toggleSkip(k, e.target.checked)} />
                      <span className="small">{c.kind[k]}</span>
                    </label>
                  ))}
                  {info.firmwareAvailable && (
                    <label className="row gb-check-inline">
                      <input type="checkbox" checked={info.firmware} disabled={busy} onChange={(e) => run(() => save({ firmware: e.target.checked }))} />
                      <span className="small">{c.firmware}</span>
                    </label>
                  )}
                </>
              ) : (
                <div className="muted small">{c.wholeFolder}</div>
              )}

              <b className="small">
                {c.extra} <Hint text={c.extraHint} />
              </b>
              {info.extraContent.map((p) => (
                <div key={p} className="row">
                  <code className="path grow">{p}</code>
                  <Button small kind="ghost" disabled={busy} onClick={() => run(() => save({ extraContent: info.extraContent.filter((x) => x !== p) }))}>
                    {c.remove}
                  </Button>
                </div>
              ))}
              <div className="row">
                <Button
                  small
                  disabled={busy}
                  onClick={() =>
                    run(async () => {
                      const f = await gameBackupApi.pickFile(info.root);
                      if (f) await save({ extraContent: [...info.extraContent, f] });
                    })
                  }
                >
                  {c.addFile}
                </Button>
                <Button
                  small
                  disabled={busy}
                  onClick={() =>
                    run(async () => {
                      const d = await api.pickFolder(c.addFolder, info.root);
                      if (d) await save({ extraContent: [...info.extraContent, d] });
                    })
                  }
                >
                  {c.addFolder}
                </Button>
              </div>

              <b className="small">
                {c.excludeTitle} <Hint text={`${c.excludeHint}\n\n${c.defaults(info.defaultExclude.join("  "))}`} />
              </b>
              <textarea className="input" rows={3} value={excludeText} onChange={(e) => setExcludeText(e.target.value)} />
              <div className="row">
                <Button
                  small
                  kind="primary"
                  disabled={busy}
                  onClick={() =>
                    run(async () => {
                      await save({ exclude: excludeText.split("\n") });
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
            setJob(startJob("restore", target));
          }}
        />
      )}
    </section>
  );
}

function JobProgress({ job, onStop }: { job: BackupJob; onStop: () => void }) {
  const c = useT().gameBackup.card;
  const withBytes = ["uploading", "downloading", "verifying", "checking"].includes(job.phase);
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
        <span style={{ width: `${pct}%` }} />
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
  const d = info.restore;
  const [versionId, setVersionId] = useState(info.versions[0].id);
  const [useInstalled, setUseInstalled] = useState(!!d?.installedProgram);
  const [programChosen, setProgramChosen] = useState(d?.installedProgram ? "" : d?.programDir ?? "");
  const [programTarget, setProgramTarget] = useState<string | null>(null);
  const [contentDir, setContentDir] = useState(d?.contentDir ?? "");
  const [fix, setFix] = useState(true);
  const version = info.versions.find((v) => v.id === versionId) ?? info.versions[0];
  const hasProgram = version.components.some((p) => p.kind === "program" || p.kind === "folder");
  const hasContent = version.components.some((p) => p.kind === "content");
  const programPart = version.components.find((p) => p.kind === "program" || p.kind === "folder");
  const original = d?.programDir ?? programChosen;

  useEffect(() => {
    if (!programChosen) {
      setProgramTarget(null);
      return;
    }
    gameBackupApi.resolveTarget(programChosen, original ?? programChosen).then(setProgramTarget, () => setProgramTarget(programChosen));
  }, [programChosen, original]);

  const program = useInstalled && d?.installedProgram ? d.installedProgram : programTarget;
  const ready = (!hasProgram || !!program) && (!hasContent || !!contentDir);

  const start = async () => {
    const targets: Targets = {
      // Settings kept next to the program need its folder even when the program isn't restored.
      program: program ?? d?.installedProgram ?? contentDir,
      restoreProgram: hasProgram && !(useInstalled && d?.installedProgram),
      content: hasContent ? contentDir : null,
    };
    try {
      await gameBackupApi.restore(gameKey, version.id, targets, fix);
      onStarted(targets.content ?? targets.program);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const slow =
    info.cloudKind === "baiduPan"
      ? t.gameBackup.card.slowBaidu(bytes(version.totalSize), duration(etaSecs(version.totalSize, BAIDU_FREE_DOWN)))
      : null;

  return (
    <Modal
      title={r.title}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>{t.common.cancel}</Button>
          <Button kind="primary" icon="download" disabled={!ready} onClick={start}>
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
        <div className="muted small">{r.parts(partsText(version.components))}</div>

        {hasProgram && (
          <div className="field">
            <span>{r.program}</span>
            <div className="gb-versions">
              {d?.installedProgram && (
                <>
                  <label className="row gb-check-inline">
                    <input type="radio" checked={useInstalled} onChange={() => setUseInstalled(true)} />
                    <span className="small">{r.useInstalled(programPart?.name ?? "", d.installedProgram)}</span>
                  </label>
                  <div className="muted small">{d.installedSame ? r.installedSame : r.installedDiff}</div>
                  <label className="row gb-check-inline">
                    <input type="radio" checked={!useInstalled} onChange={() => setUseInstalled(false)} />
                    <span className="small">{r.restoreProgram(programPart?.name ?? "", programPart?.version ?? null)}</span>
                  </label>
                </>
              )}
              {(!useInstalled || !d?.installedProgram) && (
                <div className="row">
                  {programChosen ? <code className="path grow">{programChosen}</code> : <span className="muted small grow">{r.noProgramDir}</span>}
                  <Button
                    small
                    icon="folder"
                    onClick={async () => {
                      const dir = await api.pickFolder(msg().gameBackup.restore.pickTitle, programChosen || null);
                      if (dir) setProgramChosen(dir);
                    }}
                  >
                    {r.pick}
                  </Button>
                </div>
              )}
              {!useInstalled && programTarget && <div className="small">{r.willGo(programTarget)}</div>}
            </div>
          </div>
        )}

        {hasContent && (
          <div className="field">
            <span>{r.contentTo}</span>
            <div className="row">
              {contentDir ? <code className="path grow">{contentDir}</code> : <span className="muted small grow">{r.noContentDir}</span>}
              <Button
                small
                icon="folder"
                onClick={async () => {
                  const dir = await api.pickFolder(msg().gameBackup.restore.pickTitle, contentDir || null);
                  if (dir) setContentDir(dir);
                }}
              >
                {r.pick}
              </Button>
            </div>
          </div>
        )}
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
