// A game's saves from elsewhere (docs/plan-positioning.md §7, S1 and S2):
// the other sources of saves on its card (another version, the Switch, an
// emulator, a device that keeps its own), shown as groups in the Saves tab
// and on the handheld screen, each save compared with the one here; taking
// one here; and the one-line hint on the game page when another version
// has newer progress.
import { useState } from "react";
import { api, errorText, NewerElsewhere, Overview, PlatformMark, SaveSource, SnapshotSummary } from "../api";
import { dateTime, outcomeText, relTime } from "../format";
import { msg, useT } from "../i18n";
import { ApplyDialog } from "./SaveLibrary";
import { Badge, Button, Hint, Icon, useUi } from "./ui";

/** The game the saves are taken to. `xboxContainers`: written into the Xbox app's containers (experimental). */
export interface TakeTarget {
  key: string;
  name: string;
  xboxContainers?: boolean;
}

/** A span of time, the way the comparisons say it: "3 小时", "2 天". */
function span(ms: number): string {
  const f = msg().format;
  const min = Math.max(1, Math.round(ms / 60000));
  if (min < 60) return f.minutes(min);
  if (min < 60 * 48) return f.hours(String(Math.round(min / 60)));
  return msg().sources.days(Math.round(min / (60 * 24)));
}

/** How a save compares with the one here: newer or older by how much, or the same saves. */
export function compareWith(s: SnapshotSummary, here?: SnapshotSummary | null): { text: string; newer: boolean } | null {
  if (!here) return null;
  const m = msg().sources;
  if (s.contentHash === here.contentHash) return { text: m.same, newer: false };
  const ms = Date.parse(s.createdAt) - Date.parse(here.createdAt);
  return ms > 0 ? { text: m.newerBy(span(ms)), newer: true } : { text: m.olderBy(span(-ms)), newer: false };
}

/** How much more (or less) the game had been played, where both saves say. */
export function playedText(s: SnapshotSummary, here?: SnapshotSummary | null): string | null {
  if (s.playtime == null || here?.playtime == null) return null;
  const d = s.playtime - here.playtime;
  if (Math.abs(d) < 60) return null;
  const m = msg().sources;
  const secs = Math.abs(d);
  const h = Math.floor(secs / 3600);
  const text = h > 0 ? msg().format.hours(String(h)) : msg().format.minutes(Math.round(secs / 60));
  return d > 0 ? m.playedMore(text) : m.playedLess(text);
}

/** The game version that wrote a save, where it is known. */
export function versionOf(s: SnapshotSummary): string | null {
  const v = s.gameVersion ?? s.installedVersion;
  return v ? `v${v}` : null;
}

/** What one save is, in one line: "Xbox 版 · LAPTOP · 2 小时前 · 多玩了 2 小时". */
export function saveLine(name: string, s: SnapshotSummary | null | undefined, here?: SnapshotSummary | null): string {
  const line = s ? [name, s.deviceName, relTime(s.createdAt), playedText(s, here)].filter(Boolean).join(" · ") : `${name} · ${msg().sources.noSaves}`;
  // A full-width bracket has its own space.
  return line.replace("） · ", "）· ");
}

/** What the "?" of a group says. */
export function sourceHint(src: SaveSource): string {
  const m = msg().sources;
  const what = src.kind === "device" ? m.deviceHint : src.kind === "shared" ? m.sharedHint : m.versionHint;
  const how = src.take.kind === "convert" ? m.convertHint(src.take.how) : src.take.kind === "never" ? src.take.reason : null;
  return how ? `${what}\n${how}` : what;
}

/**
 * The head of a group: the program's own icon (when this PC has it), its
 * name, devices, its newest save, and whether its saves need converting to
 * be taken here (the "?" says how).
 */
export function GroupHead({
  mark,
  name,
  devices,
  latest,
  convert,
  hint,
}: {
  mark?: PlatformMark;
  name: string;
  devices: string[];
  latest?: string | null;
  convert?: boolean;
  hint: string;
}) {
  return (
    <div className="save-group">
      {mark?.logo && <img className="src-logo" src={mark.logo} alt="" width={18} height={18} />}
      <b>{name}</b>
      {devices.length > 0 && <span className="muted">{devices.join(" · ")}</span>}
      {latest && (
        <span className="muted" title={dateTime(latest)}>
          {relTime(latest)}
        </span>
      )}
      {convert && <Badge tone="muted">{msg().sources.convert}</Badge>}
      <Hint text={hint} />
    </div>
  );
}

/**
 * Taking a save of another source here ("取回到这里"). A line that keeps the
 * same files is asked once and taken as it is; another version's save goes
 * through the "send" dialog aimed at this game, which shows where each file
 * goes. Either way the saves here are kept first and it can be undone.
 */
export function useTakeHere(target: TakeTarget, overview: Overview, onDone: () => Promise<void>, tooNew: string[] = []) {
  const ui = useUi();
  const t = useT();
  const [applying, setApplying] = useState<{ source: SaveSource; snapshot: SnapshotSummary } | null>(null);

  const undo = async () => {
    try {
      await api.undoReplace(target.key);
      ui.toast(msg().saves.undone);
      await onDone();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const take = async (source: SaveSource, s: SnapshotSummary) => {
    if (source.key) return setApplying({ source, snapshot: s });
    if (!source.line) return;
    const m = t.sources;
    const notes = [tooNew.includes(s.id) ? t.saves.tooNew(s.gameVersion ?? "?") : null, target.xboxContainers ? t.saves.xboxNote : null].filter(Boolean);
    const text = m.takeText(target.name, source.name, source.name === s.deviceName ? null : s.deviceName, dateTime(s.createdAt));
    const ok = await ui.confirm({
      title: m.takeTitle,
      text: notes.length ? (
        <>
          <p>{text}</p>
          {notes.map((n) => (
            <p key={n} className="muted">
              {n}
            </p>
          ))}
        </>
      ) : (
        text
      ),
      confirm: m.takeHere,
    });
    if (!ok) return;
    try {
      const o = await api.takeSave(target.key, source.line, s.id);
      const done = o.kind === "pulled" || o.kind === "uploaded" || o.kind === "queued";
      ui.toast(done ? msg().sources.taken : outcomeText(o, target.name), done ? "ok" : "info", done ? { label: msg().saves.undo, run: undo } : undefined);
      await onDone();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const dialog = applying && (
    <ApplyDialog
      overview={overview}
      sourceKey={applying.source.key!}
      sourceName={applying.source.name}
      snapshot={applying.snapshot}
      target={target.key}
      onClose={() => setApplying(null)}
      onApplied={async () => {
        setApplying(null);
        await onDone();
      }}
    />
  );
  return { take, dialog };
}

/**
 * One line on the game page when another version has newer progress than
 * the save here: "Xbox 版有更新的进度", the two saves in the "?", take it or
 * say no (that save isn't offered again).
 */
export function NewerRow({
  target,
  overview,
  newer,
  disabled,
  tooNew,
  onDismiss,
  onDone,
}: {
  target: TakeTarget;
  overview: Overview;
  newer: NewerElsewhere;
  disabled?: boolean;
  tooNew?: string[];
  onDismiss: () => void;
  onDone: () => Promise<void>;
}) {
  const t = useT();
  const m = t.sources;
  const { take, dialog } = useTakeHere(target, overview, onDone, tooNew);
  const there = saveLine(newer.source.name, newer.snapshot, newer.here);
  const here = saveLine(m.hereName(newer.hereName), newer.here);
  const how = newer.source.take.kind === "convert" ? m.convertHint(newer.source.take.how) : null;
  return (
    <div className="banner banner-info join-row">
      <Icon name="transfer" />
      <span>
        {m.newerRow(newer.source.name)} <Hint text={[there, here, how, m.keptFirst].filter(Boolean).join("\n")} />
      </span>
      <Button small kind="primary" onClick={() => take(newer.source, newer.snapshot)} disabled={disabled} title={disabled ? t.savelib.running : undefined}>
        {m.take}
      </Button>
      <Button
        small
        kind="ghost"
        onClick={() => {
          api.dismissNewer(target.key, newer.snapshot.id).catch(() => undefined);
          onDismiss();
        }}
      >
        {m.notNow}
      </Button>
      {dialog}
    </div>
  );
}
