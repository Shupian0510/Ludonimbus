// The top of a game page (docs/ui-design.md, 4): where the game's progress
// is. When something waits for a choice it is two save cards side by side —
// both changed (cloud / this device), newer saves of this version in the
// cloud, newer progress in another version — each with its one action.
// Otherwise the save this device is at, beside the newest save of each other
// source (another version, a device that keeps its own), each one click
// from being taken here.
import { api, GameView, Overview, PlatformMark as Mark, SaveSource, SaveSources, SnapshotList, SnapshotSummary, StatusReport } from "../api";
import { useT } from "../i18n";
import { cmpOf, ConvertChip, Pill, playedOf, SaveCard, SaveCompare, Side } from "./kit";
import { currentOf } from "./Saves";
import { useTakeHere } from "./SaveSources";
import { Button, Hint, Icon } from "./ui";

export function Progress({
  game,
  overview,
  check,
  data,
  sources,
  newerGone,
  onDismissNewer,
  onTakeCloud,
  onUseLocal,
  onTakeNewer,
  onDone,
}: {
  game: GameView;
  overview: Overview;
  check?: StatusReport;
  data: SnapshotList | null;
  sources: SaveSources | null;
  /** The newer progress elsewhere was waved off on this page. */
  newerGone: boolean;
  onDismissNewer: () => void;
  /** Both changed: keep the cloud's, or this device's. */
  onTakeCloud: () => Promise<void>;
  onUseLocal: () => Promise<void>;
  /** The cloud has newer saves of this version ("取回"). */
  onTakeNewer: () => Promise<void>;
  onDone: () => Promise<void>;
}) {
  const t = useT();
  const g = t.game;
  const m = t.sources;
  const tooNew = [...(data?.tooNew ?? []), ...(sources?.tooNew ?? [])];
  const { take, dialog } = useTakeHere(game, overview, onDone, tooNew);
  const marks: Record<string, Mark> = { ...(sources?.platforms ?? {}), ...(data?.platforms ?? {}) };
  const here = currentOf(data);
  const hereName = sources?.hereName ?? marks[here?.platform ?? ""]?.label ?? game.name;
  const running = game.running ? t.savelib.running : undefined;

  /** A save as one side of a card. */
  const side = (name: string, s: SnapshotSummary, other: SnapshotSummary | null, extra: Partial<Side> = {}): Side => ({
    name,
    mark: marks[s.platform ?? ""],
    device: s.deviceName,
    mine: s.deviceId === overview.deviceId,
    at: s.createdAt,
    playtime: s.playtime,
    played: playedOf(s, other),
    size: s.totalSize,
    files: s.fileCount,
    ...extra,
  });
  const hereSide = (extra: Partial<Side> = {}): Side => (here ? side(hereName, here, null, { tag: m.here, ...extra }) : { name: hereName, empty: true, tag: m.here, ...extra });

  const conflict = check?.status === "conflict" || !!game.conflict;
  const cloudNewer = check?.status === "cloudNewer" || check?.status === "canRestore";
  const newer = !newerGone ? sources?.newer : undefined;

  // Both changed: the two saves, a button on each.
  if (conflict && check?.head) {
    return (
      <section className="progress decide warn">
        <div className="progress-head">
          <Icon name="warn" />
          <b>{g.conflictTitle}</b>
          <Hint text={g.conflictHint} />
        </div>
        <SaveCompare
          arrow={false}
          there={{
            ...side(g.cloudSide, check.head, null),
            action: (
              <Button kind="primary" onClick={onTakeCloud} disabled={game.running} title={running}>
                {g.useCloud}
              </Button>
            ),
          }}
          here={{
            name: g.localSide,
            device: overview.deviceName,
            mine: true,
            size: check.localSize,
            files: check.localFiles,
            action: (
              <Button onClick={onUseLocal} disabled={game.running} title={running}>
                {g.useLocal}
              </Button>
            ),
          }}
        />
      </section>
    );
  }

  // Newer saves of this version in the cloud (another device played on): take them.
  if (cloudNewer && check?.head) {
    return (
      <section className="progress decide">
        <SaveCompare
          there={{ ...side(g.cloudSide, check.head, here, { newer: true }), action: <Button kind="primary" icon="download" onClick={onTakeNewer} disabled={game.running} title={running}>{t.policy.take}</Button> }}
          here={hereSide()}
        />
      </section>
    );
  }

  // Newer progress in another version: take it, or say no (it isn't offered again).
  if (newer) {
    const convert = newer.source.take.kind === "convert" ? m.convertHint(newer.source.take.how) : null;
    return (
      <section className="progress decide">
        <SaveCompare
          there={{
            ...side(newer.source.name, newer.snapshot, newer.here, { newer: true }),
            action: (
              <span className="progress-actions">
                <Button kind="primary" onClick={() => take(newer.source, newer.snapshot)} disabled={game.running} title={running}>
                  {m.take}
                </Button>
                <Button
                  kind="ghost"
                  onClick={() => {
                    api.dismissNewer(game.key, newer.snapshot.id).catch(() => undefined);
                    onDismissNewer();
                  }}
                >
                  {m.notNow}
                </Button>
                {convert && <ConvertChip hint={convert} />}
              </span>
            ),
          }}
          here={newer.here ? side(newer.hereName, newer.here, null, { tag: m.here }) : hereSide()}
        />
        {dialog}
      </section>
    );
  }

  // Nothing to decide: where this device is, and the newest save of each other source.
  const others = (sources?.sources ?? []).map((src) => ({ src, s: src.snapshots.find((x) => x.head && !x.incomplete) ?? src.snapshots[0] })).filter((x) => !!x.s);
  if (!here && others.length === 0) return null;
  const takeButton = (src: SaveSource, s: SnapshotSummary) => {
    const never = src.take.kind === "never" ? src.take.reason : null;
    const same = !!here && s.contentHash === here.contentHash;
    if (same) return <Pill tone="ok">{t.saves.current}</Pill>;
    return (
      <span className="progress-actions">
        <Button small onClick={() => take(src, s)} disabled={game.running || !!never || !!s.incomplete} title={running}>
          {m.takeHere}
        </Button>
        {never && <Hint text={never} />}
      </span>
    );
  };
  return (
    <section className="progress">
      <div className="progress-cards">
        {here && <SaveCard side={side(hereName, here, null, { tag: t.saves.current, tagTone: "ok" })} />}
        {others.map(({ src, s }) => (
          <SaveCard
            key={src.id}
            side={side(src.name, s, here, {
              newer: cmpOf(s, here)?.kind === "newer" && src.kind === "version",
              tag: src.kind === "device" ? m.ownTag : src.kind === "shared" ? m.sharedTag : undefined,
              action: takeButton(src, s),
            })}
          />
        ))}
      </div>
      {dialog}
    </section>
  );
}
