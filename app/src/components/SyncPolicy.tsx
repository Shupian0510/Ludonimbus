// Writing into a game's saves only when the user says so, for games that
// don't start through Steam (docs/plan-positioning.md, sections 2 and 6):
// the library's line about games that differ from the cloud, the dialog
// that settles them game by game, and 「全部同步」, which uploads and then
// lists what needs a choice.
import { useState } from "react";
import { api, errorText, type GameView, type Outcome, type StatusReport } from "../api";
import { outcomeText, relTime } from "../format";
import { msg, useT } from "../i18n";
import { Button, Hint, Icon, Menu, Modal, useUi } from "./ui";

/**
 * Whether a game needs the user to choose: both sides changed, or the cloud
 * has newer saves of its own line that won't come by themselves (it doesn't
 * start through Steam, `GameView.autoPull`).
 */
export function needsChoice(game: GameView, check?: StatusReport): boolean {
  if (!check || game.running) return false;
  return check.status === "conflict" || (check.status === "cloudNewer" && !game.autoPull);
}

/** The saves here are just an older version of the line, which 「全部取回」 may take without anything here being lost. */
function plainlyOlder(game: GameView, check: StatusReport): boolean {
  return check.status === "cloudNewer" && !game.xboxContainers && !check.tooNew;
}

/** What identifies the set of differences, so that "Not now" holds until it changes. */
export function differenceKey(list: { game: GameView; check: StatusReport }[]): string {
  return list
    .map(({ game, check }) => `${game.key}:${check.status}:${check.head?.id ?? ""}`)
    .sort()
    .join("|");
}

/** One line in the library: how many games differ from the cloud, and 「处理」. */
export function DifferBanner({ count, onReview, onDismiss }: { count: number; onReview: () => void; onDismiss: () => void }) {
  const t = useT();
  return (
    <div className="banner banner-warn join-row">
      <Icon name="cloud" />
      <span>{t.policy.differ(count)}</span>
      <Button small kind="primary" onClick={onReview}>
        {t.policy.review}
      </Button>
      <button className="icon-btn" onClick={onDismiss} title={t.policy.notNow} aria-label={t.policy.notNow}>
        <Icon name="x" size={14} />
      </button>
    </div>
  );
}

/**
 * The games that differ from the cloud, each with 「取回」 and, under
 * "more", 「用本机的」 and 「这台设备单独存」; 「全部取回」 for the ones whose
 * saves here are just an older version. Replacing saves is what this
 * dialog is for, so no further question is asked; what is replaced is kept
 * first and can be undone on the game's page.
 */
export function DifferDialog({
  games,
  checks,
  onClose,
  onChanged,
}: {
  games: GameView[];
  checks: Record<string, StatusReport>;
  onClose: () => void;
  /** After a game was settled: look at it again. */
  onChanged: (keys: string[]) => Promise<void>;
}) {
  const t = useT();
  const ui = useUi();
  // Settled here; gone from the list before the next check comes back.
  const [done, setDone] = useState<string[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const rows = games.filter((g) => !done.includes(g.key) && needsChoice(g, checks[g.key]));
  const older = rows.filter((g) => plainlyOlder(g, checks[g.key]));

  const act = async (g: GameView, run: () => Promise<Outcome>, ok: string) => {
    setBusy(g.key);
    try {
      const o = await run();
      if (o.kind === "conflict" || o.kind === "offline") ui.toast(outcomeText(o, g.name), o.kind === "conflict" ? "warn" : "info");
      else ui.toast(ok, "ok");
      if (o.kind !== "offline" && o.kind !== "conflict") setDone((d) => [...d, g.key]);
      await onChanged([g.key]);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setBusy(null);
    }
  };

  /** 「取回」: the newer version of the line, as "立即同步" takes it; the cloud's newest in a conflict, or where a sync wouldn't write by itself (the Xbox app's containers, a save from a newer game). */
  const take = (g: GameView) => {
    const check = checks[g.key];
    const resolution = plainlyOlder(g, check) ? "auto" : "cloud";
    return act(g, () => api.sync(g.key, resolution), msg().policy.taken(g.name));
  };

  const takeAll = async () => {
    for (const g of older) await take(g);
  };

  const row = (g: GameView) => {
    const check = checks[g.key];
    const head = check.head;
    const when = head ? relTime(head.createdAt) : "";
    const device = head?.deviceName ?? "";
    const notes = [g.xboxContainers ? t.saves.xboxNote : "", check.tooNew ? t.saves.tooNew(check.tooNew) : ""].filter(Boolean).join("\n");
    return (
      <div key={g.key} className="differ-row">
        <span className="differ-name">
          <b>{g.name}</b>
          <small className="muted">
            {check.status === "conflict" ? t.policy.bothChanged(device, when) : t.policy.cloudNewer(device, when)}
            {notes && <Hint text={notes} />}
          </small>
        </span>
        <Button small kind="primary" onClick={() => take(g)} disabled={!!busy} title={t.policy.takeTitle}>
          {t.policy.take}
        </Button>
        <Menu
          title={t.saves.more}
          disabled={!!busy}
          items={[
            { label: t.policy.keepLocal, title: t.policy.keepLocalTitle, onClick: () => act(g, () => api.sync(g.key, "local"), msg().policy.kept(g.name)) },
            { label: t.policy.ownLine, title: t.policy.ownLineTitle, onClick: () => act(g, () => api.useLine(g.key, { kind: "independent" }), msg().policy.separate(g.name)) },
          ]}
        />
      </div>
    );
  };

  return (
    <Modal
      title={t.policy.dialogTitle}
      onClose={onClose}
      wide
      footer={
        <>
          <Button kind="ghost" onClick={onClose}>
            {t.common.close}
          </Button>
          {older.length > 1 && (
            <Button kind="primary" icon="download" onClick={takeAll} disabled={!!busy} title={t.policy.takeAllTitle}>
              {t.policy.takeAll(older.length)}
            </Button>
          )}
        </>
      }
    >
      <p className="muted small">
        {t.policy.dialogLead} <Hint text={t.policy.dialogHint} />
      </p>
      {rows.length === 0 ? <p className="muted">{t.policy.allDone}</p> : <div className="differ-list">{rows.map(row)}</div>}
    </Modal>
  );
}

/**
 * 「全部同步」 over `games`: each uploads what changed here and writes
 * nothing into its saves; the statuses that come back say which need a
 * choice. `onProgress` is told "3/12" as it goes. Running games are left out.
 */
export async function uploadAll(
  games: GameView[],
  onProgress: (text: string) => void,
): Promise<{ reports: Record<string, StatusReport>; done: number; conflicts: number; failed: string[]; skipped: number }> {
  const todo = games.filter((g) => !g.running);
  const r = { reports: {} as Record<string, StatusReport>, done: 0, conflicts: 0, failed: [] as string[], skipped: games.length - todo.length };
  for (const [i, g] of todo.entries()) {
    onProgress(msg().source.progress(i + 1, todo.length));
    try {
      const { outcome, check } = await api.uploadGame(g.key);
      r.reports[g.key] = check;
      if (outcome.kind === "offline" || outcome.kind === "noCloud") r.failed.push(g.name);
      else if (check.status === "conflict") r.conflicts++;
      else r.done++;
    } catch {
      r.failed.push(g.name);
    }
  }
  return r;
}
