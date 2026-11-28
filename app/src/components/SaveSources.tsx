// A game's saves from elsewhere (docs/plan-positioning.md §7, S1 and S2):
// the other sources of saves on its card (another version, the Switch, an
// emulator, a device that keeps its own), shown as groups in the Saves tab
// and on the handheld screen, each save compared with the one here; taking
// one here (the game page's progress cards and the Saves tab).
import { useState } from "react";
import { api, errorText, Overview, SaveSource, SnapshotSummary } from "../api";
import { dateTime, outcomeText } from "../format";
import { msg, useT } from "../i18n";
import { ApplyDialog } from "./SaveLibrary";
import { useUi } from "./ui";

/** The game the saves are taken to. `xboxContainers`: written into the Xbox app's containers (experimental). */
export interface TakeTarget {
  key: string;
  name: string;
  xboxContainers?: boolean;
}

/** The game version that wrote a save, where it is known. */
export function versionOf(s: SnapshotSummary): string | null {
  const v = s.gameVersion ?? s.installedVersion;
  return v ? `v${v}` : null;
}

/** What a group of saves is, for its "?". */
export function sourceWhat(src: SaveSource): string {
  const m = msg().sources;
  return src.kind === "device" ? m.deviceHint : src.kind === "shared" ? m.sharedHint : m.versionHint;
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
