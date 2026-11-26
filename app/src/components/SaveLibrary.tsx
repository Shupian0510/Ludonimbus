// The save library (docs/plan-phase5.md §5): versions of one game (a Steam,
// Xbox, emulator version… on this PC or only in the cloud), linking them by
// hand, and applying a save version from one to another.
import { useEffect, useMemo, useState } from "react";
import { api, CloudOnlyView, errorText, GameSource, GameView, Overview, SnapshotSummary, TransferPlan } from "../api";
import { bytes, dateTime, outcomeText } from "../format";
import { currentLang, msg, useT } from "../i18n";
import { Badge, Button, Icon, Modal, Spinner, useUi } from "./ui";

/** One version of a game in the library. */
export interface Member {
  key: string;
  name: string;
  /** On this PC (`game`) or only in the cloud (`cloud`). */
  game?: GameView;
  cloud?: CloudOnlyView;
  playable: boolean;
  source: GameSource;
  family: string;
}

export function membersOf(overview: Overview): Member[] {
  return [
    ...overview.games.map((g) => ({ key: g.key, name: g.name, game: g, playable: g.playable, source: g.source, family: g.family })),
    ...(overview.cloudOnly ?? []).map((c) => ({ key: c.key, name: c.name, cloud: c, playable: false, source: c.source, family: c.family })),
  ];
}

/** The versions of the game `key` belongs to (itself included). */
export function familyOf(overview: Overview, key: string): Member[] {
  const all = membersOf(overview);
  const fam = all.find((m) => m.key === key)?.family ?? key;
  return all.filter((m) => m.family === fam);
}

/** What a version runs on: its emulator, Steam, Xbox… */
export function variantLabel(m: Member): string {
  const s = m.source;
  if (s.label) return s.label;
  if (s.kind === "emulator" && s.name) return s.name;
  if (s.kind === "pc" && m.game?.steam) return "Steam";
  if (s.kind === "recomp") return m.game?.platform ?? m.cloud?.platform ?? msg().source.kinds.recomp;
  return msg().source.kinds[s.kind];
}

/** The versions of a game, shown on its page; click one to open it. */
export function VersionBar({
  overview,
  current,
  onOpen,
  onChanged,
}: {
  overview: Overview;
  current: string;
  onOpen: (key: string) => void;
  onChanged: () => Promise<void>;
}) {
  const t = useT();
  const s = t.savelib;
  const ui = useUi();
  const [linking, setLinking] = useState(false);
  const members = familyOf(overview, current);
  const me = members.find((m) => m.key === current);

  const unlink = async () => {
    if (!me) return;
    const ok = await ui.confirm({ title: s.unlinkTitle, text: s.unlinkText(me.name), confirm: s.unlink });
    if (!ok) return;
    try {
      await api.unlinkGame(current);
      ui.toast(msg().savelib.unlinked);
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <div className="version-bar">
      <span className="muted small">{s.versionsTitle}</span>
      <div className="chips">
        {members.map((m) => (
          <button key={m.key} className={`chip${m.key === current ? " on" : ""}${m.playable ? "" : " chip-missing"}`} onClick={() => onOpen(m.key)}>
            {m.source.logo && <img className="src-logo" src={m.source.logo} alt="" width={15} height={15} />}
            {variantLabel(m)}
            <span className="chip-count">{m.cloud ? s.cloudOnly : m.playable ? s.here : s.notInstalled}</span>
          </button>
        ))}
      </div>
      <div className="version-bar-actions">
        <button className="link" onClick={() => setLinking(true)}>
          <Icon name="link" size={14} /> {s.link}
        </button>
        {members.length > 1 && (
          <button className="link" onClick={unlink}>
            {s.unlink}
          </button>
        )}
      </div>
      {linking && me && (
        <LinkDialog
          overview={overview}
          current={me}
          onClose={() => setLinking(false)}
          onLinked={async () => {
            setLinking(false);
            await onChanged();
          }}
        />
      )}
    </div>
  );
}

/** Pick another library entry that is the same game. */
function LinkDialog({ overview, current, onClose, onLinked }: { overview: Overview; current: Member; onClose: () => void; onLinked: () => Promise<void> }) {
  const t = useT();
  const s = t.savelib;
  const ui = useUi();
  const [query, setQuery] = useState("");
  const others = useMemo(() => {
    const q = query.trim().toLowerCase();
    return membersOf(overview)
      .filter((m) => m.family !== current.family && m.name.toLowerCase().includes(q))
      .sort((a, b) => a.name.localeCompare(b.name, currentLang()));
  }, [overview, current, query]);

  const link = async (m: Member) => {
    try {
      await api.linkGames([current.key, m.key]);
      ui.toast(msg().savelib.linked);
      await onLinked();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <Modal title={s.linkTitle} onClose={onClose}>
      <p className="muted">{s.linkText(current.name)}</p>
      <div className="search">
        <Icon name="search" size={16} />
        <input autoFocus placeholder={s.linkSearch} value={query} onChange={(e) => setQuery(e.target.value)} />
      </div>
      <div className="pick-list">
        {others.map((m) => (
          <button key={m.key} className="pick-row" onClick={() => link(m)}>
            <b>{m.name}</b>
            <span className="muted small">
              {variantLabel(m)} · {m.cloud ? s.cloudOnly : m.playable ? s.here : s.notInstalled}
            </span>
          </button>
        ))}
      </div>
    </Modal>
  );
}

/**
 * Apply one save version (of any version of a game, from any device) to a
 * game on this PC: pick the target, look at what goes where, apply.
 */
export function ApplyDialog({
  overview,
  sourceKey,
  sourceName,
  snapshot,
  target: preset,
  onClose,
  onApplied,
}: {
  overview: Overview;
  sourceKey: string;
  sourceName: string;
  snapshot: SnapshotSummary;
  /** Apply to this game (skips choosing). */
  target?: string;
  onClose: () => void;
  onApplied: (target: string) => Promise<void>;
}) {
  const t = useT();
  const s = t.savelib;
  const ui = useUi();
  const [target, setTarget] = useState<Member | null>(() => membersOf(overview).find((m) => m.key === preset) ?? null);
  const [base, setBase] = useState<string | null>(null);
  const [plan, setPlan] = useState<TransferPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const versions = familyOf(overview, sourceKey);
  const family = versions.map((m) => m.key);
  const locals = membersOf(overview).filter((m) => m.game && m.key !== sourceKey);
  // The Switch itself, only in the cloud: saves go into its newest version
  // (for games a save rule covers; the plan says when it isn't).
  const consoles = versions.filter((m) => m.cloud && m.key !== sourceKey && m.source.kind === "console");
  const same = [...locals.filter((m) => family.includes(m.key)), ...consoles];
  const other = locals.filter((m) => !family.includes(m.key)).sort((a, b) => a.name.localeCompare(b.name, currentLang()));

  useEffect(() => {
    if (!target) return;
    setPlan(null);
    setError(null);
    api.transferPlan(sourceKey, snapshot.id, target.key, base).then(setPlan, (e) => setError(errorText(e)));
  }, [target, base, sourceKey, snapshot.id]);

  const pickFolder = async () => {
    const f = await api.pickFolder(msg().savelib.pickFolder, plan?.base ?? null);
    if (f) setBase(f);
  };

  const apply = async () => {
    if (!target) return;
    setBusy(true);
    try {
      const o = await api.transferApply(sourceKey, snapshot.id, target.key, base);
      ui.toast(o.kind === "uploaded" ? msg().savelib.applied(target.name) : outcomeText(o, target.name));
      await onApplied(target.key);
    } catch (e) {
      ui.toast(errorText(e), "bad");
      setBusy(false);
    }
  };

  const row = (m: Member) => (
    <button key={m.key} className="pick-row" onClick={() => setTarget(m)}>
      <b>{m.name}</b>
      <span className="muted small">
        {variantLabel(m)}
        {m.cloud ? ` · ${s.cloudOnlyTarget}` : m.playable ? "" : ` · ${s.notInstalled}`}
      </span>
    </button>
  );

  const shown = plan?.items.slice(0, 200) ?? [];
  const footer = target && (
    <>
      <Button onClick={() => (setTarget(null), setBase(null))}>{s.otherTarget}</Button>
      <Button kind="primary" icon="transfer" onClick={apply} disabled={!plan || busy || plan.needsFolder || plan.items.length === 0}>
        {s.apply}
      </Button>
    </>
  );

  return (
    <Modal title={s.applyTitle(sourceName)} onClose={onClose} wide footer={footer || undefined}>
      <p className="muted small">{s.applySource(snapshot.deviceName, dateTime(snapshot.createdAt))}</p>
      {!target ? (
        <>
          <h3>{s.pickTarget}</h3>
          {same.length + other.length === 0 && <p className="muted">{s.noTargets}</p>}
          {same.length > 0 && (
            <>
              <p className="muted small">{s.sameGame}</p>
              <div className="pick-list">{same.map(row)}</div>
            </>
          )}
          {other.length > 0 && (
            <>
              <p className="muted small">{s.otherGames}</p>
              <div className="pick-list">{other.map(row)}</div>
            </>
          )}
        </>
      ) : (
        <>
          <h3>{target.name}</h3>
          {error && <div className="banner banner-bad">{error}</div>}
          {!plan && !error && (
            <div className="row">
              <Spinner /> <span className="muted">{s.planning}</span>
            </div>
          )}
          {plan && (
            <div className="apply-plan">
              <p>
                {plan.same
                  ? s.modeSame
                  : plan.formatRule
                    ? s.modeRule(plan.formatRule)
                    : plan.toXbox
                      ? s.modeToXbox
                      : plan.fromXbox
                        ? s.modeFromXbox(plan.xboxRule)
                        : s.modeFiles}
              </p>
              {plan.fromXbox && plan.toXbox && <p>{s.modeFromXbox(plan.xboxRule)}</p>}
              {plan.toXbox && <div className="banner banner-warn">{s.xboxSteps}</div>}
              {plan.crossPlatform && <div className="banner banner-warn">{s.crossPlatform}</div>}
              {plan.cloudTarget && <div className="banner banner-info">{s.cloudTarget}</div>}
              {plan.tooNew ? (
                <div className="banner banner-warn">{s.tooNew(plan.sourceVersion ?? "", plan.targetVersion ?? "")}</div>
              ) : (
                (plan.sourceVersion || plan.targetVersion) && <p className="muted small">{s.gameVersions(plan.sourceVersion, plan.targetVersion)}</p>
              )}
              {plan.needsFolder ? (
                <div className="banner banner-info">
                  <span>{s.needsFolder(target.name)}</span>
                  <Button small icon="folder" onClick={pickFolder}>
                    {s.pickFolder}
                  </Button>
                </div>
              ) : (
                <>
                  {plan.base && !plan.same && (
                    <div className="row">
                      <span className="muted small">{s.target}</span>
                      <code className="path small">{plan.base}</code>
                      {!plan.toXbox && !plan.cloudTarget && (
                        <Button small kind="ghost" icon="folder" onClick={pickFolder}>
                          {s.changeFolder}
                        </Button>
                      )}
                    </div>
                  )}
                  <p className="muted small">{s.matched(plan.matched, plan.items.length)}</p>
                  {!plan.same && !plan.formatRule && plan.matched === 0 && plan.items.length > 0 && (
                    <div className="banner banner-warn">{s.noneMatched}</div>
                  )}
                  {plan.skipped.length > 0 && (
                    <p className="muted small" title={plan.skipped.slice(0, 20).join(", ")}>
                      {s.skipped(plan.skipped.length)}
                    </p>
                  )}
                  <table className="files apply-files">
                    <tbody>
                      {shown.map((i) => (
                        <tr key={i.from + i.to}>
                          <td>
                            <code>{i.from}</code>
                          </td>
                          <td className="arrow">→</td>
                          <td>
                            <code>{i.to}</code>
                          </td>
                          <td>{bytes(i.size)}</td>
                          <td>{i.replaces ? <Badge tone="warn">{s.replaces}</Badge> : <Badge tone="muted">{s.adds}</Badge>}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  {plan.items.length > shown.length && <p className="muted small">{s.more(plan.items.length - shown.length)}</p>}
                </>
              )}
              {plan.unmatched.length > 0 && (
                <div className="banner banner-warn unmatched">
                  <span>{s.unmatched(plan.unmatched.length)}</span>
                  <code className="small">{plan.unmatched.slice(0, 20).join(", ")}</code>
                </div>
              )}
              <p className="muted small">{s.safe}</p>
            </div>
          )}
        </>
      )}
    </Modal>
  );
}
