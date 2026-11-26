// Settings → Cloud speed test: how fast the current cloud uploads and downloads
// game-backup-sized objects, and what that means for a 10 / 50 GB game.
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { errorText, inTauri } from "../api";
import { bytes } from "../format";
import { etaSecs, gameBackupApi, SpeedPhase, SpeedProgress, SpeedReport } from "../gameBackupApi";
import { msg, useT } from "../i18n";
import "../gameBackup.css";
import { Badge, Button, Hint, Icon } from "./ui";

const GB = 1024 ** 3;

/** "3 hours", "2 days"… for an ETA in seconds. */
export function duration(secs: number | null): string {
  const d = msg().gameBackup.duration;
  if (secs == null) return d.unknown;
  if (secs < 3600) return d.minutes(Math.max(1, Math.round(secs / 60)));
  if (secs < 86400 * 2) return d.hours(Math.round((secs / 3600) * 10) / 10);
  return d.days(Math.round((secs / 86400) * 10) / 10);
}

function rate(p: SpeedPhase | null): string {
  if (!p || p.objects === 0) return "-";
  return `${bytes(p.bytesPerSec)}/s`;
}

const verdictTone = { good: "ok", slow: "warn", tooSlow: "bad", failed: "bad" } as const;

export function SpeedTestPanel({ hasCloud }: { hasCloud: boolean }) {
  const t = useT();
  const sp = t.gameBackup.speed;
  const [mb, setMb] = useState(200);
  const [threads, setThreads] = useState(4);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<SpeedProgress | null>(null);
  const [report, setReport] = useState<SpeedReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!inTauri) return;
    let off: (() => void) | undefined;
    listen<SpeedProgress>("speed-test-progress", (e) => setProgress(e.payload)).then((u) => (off = u), () => undefined);
    return () => off?.();
  }, []);

  const start = async () => {
    setRunning(true);
    setProgress(null);
    setReport(null);
    setError(null);
    try {
      setReport(await gameBackupApi.speedTest(mb, threads));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setRunning(false);
      setProgress(null);
    }
  };

  const pct = progress && progress.totalBytes > 0 ? Math.floor((progress.doneBytes / progress.totalBytes) * 100) : 0;
  const row = (label: string, serial: SpeedPhase | null, parallel: SpeedPhase | null) => (
    <tr>
      <td>{label}</td>
      <td>
        {rate(serial)}
        {serial?.capped && <span className="muted small"> · {sp.capped}</span>}
      </td>
      <td>
        {rate(parallel)}
        {parallel?.capped && <span className="muted small"> · {sp.capped}</span>}
      </td>
    </tr>
  );

  return (
    <section className="panel form" id="settings-speed">
      <div className="panel-head">
        <Icon name="cloud" />
        <h3>{sp.title}</h3>
        <Hint text={sp.intro} />
        {report && <Badge tone={verdictTone[report.verdict]}>{sp.verdict[report.verdict]}</Badge>}
      </div>
      {!hasCloud ? (
        <p className="muted small">{sp.noCloud}</p>
      ) : (
        <div className="row">
          <span className="muted small">{sp.amount}</span>
          <select className="input select" style={{ width: "auto" }} value={mb} disabled={running} onChange={(e) => setMb(Number(e.target.value))}>
            {[50, 200, 500].map((v) => (
              <option key={v} value={v}>
                {v} MB
              </option>
            ))}
          </select>
          <span className="muted small">{sp.threads}</span>
          <select className="input select" style={{ width: "auto" }} value={threads} disabled={running} onChange={(e) => setThreads(Number(e.target.value))}>
            {[2, 4, 8].map((v) => (
              <option key={v} value={v}>
                {v}
              </option>
            ))}
          </select>
          {running ? (
            <Button onClick={() => gameBackupApi.speedTestCancel()}>{sp.cancel}</Button>
          ) : (
            <Button kind="primary" icon="sync" onClick={start}>
              {sp.start}
            </Button>
          )}
        </div>
      )}
      {running && (
        <div className="update-progress">
          <div className="muted small">
            {progress ? sp.phase[progress.phase] : sp.phase.latency}
            {progress && progress.totalBytes > 0 && progress.phase !== "cleanup" && ` · ${bytes(progress.doneBytes)} / ${bytes(progress.totalBytes)}`}
          </div>
          <div className="bar">
            <span style={{ width: `${pct}%` }} />
          </div>
        </div>
      )}
      {error && <div className="health health-bad">{sp.failed(error)}</div>}
      {report && (
        <>
          <table className="speed-table">
            <thead>
              <tr>
                <th />
                <th>{sp.serial}</th>
                <th>{sp.parallel(report.uploadParallel?.threads ?? threads)}</th>
              </tr>
            </thead>
            <tbody>
              {row(sp.upload, report.uploadSerial, report.uploadParallel)}
              {row(sp.download, report.downloadSerial, report.downloadParallel)}
            </tbody>
          </table>
          <div className="muted small">{sp.request(Math.round(report.smallWriteMs), Math.round(report.smallReadMs))}</div>
          {[10, 50].map((g) => (
            <div key={g} className="small">
              {sp.estimate(`${g} GB`, duration(etaSecs(g * GB, report.uploadBps)), duration(etaSecs(g * GB, report.downloadBps)))}
            </div>
          ))}
          {report.notes.map((n) => (
            <div key={n} className="muted small">
              {n}
            </div>
          ))}
          {report.cleanupError && <div className="health health-warn">{sp.cleanupFailed(report.cleanupError)}</div>}
        </>
      )}
    </section>
  );
}
