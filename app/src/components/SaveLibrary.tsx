// The save library (docs/plan-phase5.md §5): versions of one game (a Steam,
// Xbox, emulator version… on this PC or only in the cloud), linking them by
// hand, and applying a save version from one to another.
import { useEffect, useMemo, useState } from "react";
import { api, CloudOnlyView, errorText, GameSource, GameView, JoinSuggestion, LineMember, Outcome, Overview, PlatformMarks, SaveLine, SnapshotSummary, TransferPlan } from "../api";
import { bytes, dateTime, outcomeText, relTime } from "../format";
import { currentLang, msg, useT } from "../i18n";
import { Badge, Button, Hint, Icon, Menu, MenuItem, Modal, Spinner, useUi } from "./ui";

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
  if (m.game?.store && s.kind !== "emulator" && s.kind !== "recomp") return "Steam";
  if (s.kind === "pc" && m.game?.steam) return "Steam";
  if (s.kind === "recomp") return m.game?.platform ?? m.cloud?.platform ?? msg().source.kinds.recomp;
  return msg().source.kinds[s.kind];
}

/**
 * Who has this game's saves, on its page (docs/plan-v2.md, 10.2): one mark
 * for each platform on each device. The ones joined together share one
 * progress; a device that keeps its saves to itself stands apart. This
 * device's mark opens what can be changed about it. Other versions of the
 * game (other keys) follow; click one to open its page.
 */
export function VersionBar({
  overview,
  current,
  disabled,
  onOpen,
  onChanged,
  onEditSaves,
  store,
}: {
  overview: Overview;
  current: string;
  /** The game is running: its save line can't be changed now. */
  disabled?: boolean;
  onOpen: (key: string) => void;
  onChanged: () => Promise<void>;
  /** Show where the game's save folders are set. */
  onEditSaves?: () => void;
  /** For a game Steam installed: taking it over from Steam Cloud, giving it back. */
  store?: { onTakeOver: () => void; onHandBack: () => void | Promise<unknown> };
}) {
  const t = useT();
  const s = t.savelib;
  const ui = useUi();
  const [linking, setLinking] = useState(false);
  const [lines, setLines] = useState<{ lines: SaveLine[]; platforms: PlatformMarks } | null>(null);
  const members = familyOf(overview, current);
  const me = members.find((m) => m.key === current);
  const game = me?.game;
  const hasCloud = overview.cloud.kind !== "none";
  // Read again when a sync changed where the game is, and after a change made here.
  const stamp = game?.lastSyncAt ?? "";
  const [tick, setTick] = useState(0);

  useEffect(() => {
    if (!game || !hasCloud) return setLines(null);
    let live = true;
    api.lineMembers(current).then(
      (l) => live && setLines(l),
      () => live && setLines(null),
    );
    return () => void (live = false);
  }, [current, hasCloud, !game, stamp, tick]);

  const done = async (f: () => Promise<Outcome | null | void>, ok: string) => {
    try {
      const o = await f();
      const plain = !o || o.kind === "upToDate" || o.kind === "uploaded";
      ui.toast(plain ? ok : outcomeText(o, me?.name), !o || plain ? "ok" : o.kind === "conflict" ? "warn" : "info");
      setTick((n) => n + 1);
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  /** Not destructive, so no question: it is done, and can be taken back. */
  const split = async () => {
    const other = members.find((m) => m.key !== current);
    try {
      await api.unlinkGame(current);
      const m = msg().savelib;
      ui.toast(m.unlinked, "ok", other && { label: m.undo, run: () => api.linkGames([current, other.key]).then(onChanged, (e) => ui.toast(errorText(e), "bad")) });
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const mine = lines?.lines.find((l) => l.current);
  // Named like the other devices' marks, by what the saves are made on.
  const hereLabel = (me && lines?.platforms[mine?.members.find((m) => m.here)?.platform ?? ""]?.label) || (me ? variantLabel(me) : "");
  const menu: MenuItem[] = [];
  if (game) {
    const why = disabled ? s.running : undefined;
    if (lines && mine) {
      menu.push({ label: s.lineShared, checked: mine.shared, disabled, title: why, onClick: () => (mine.shared ? undefined : done(() => api.useLine(current, { kind: "shared" }), msg().savelib.lineChanged)) });
      // On a line with other devices that isn't the shared one: this device follows theirs (or they follow its own).
      const alone = !mine.shared && !mine.members.some((m) => !m.here);
      menu.push({ label: s.lineOwn, checked: alone, disabled, title: why, onClick: () => (alone ? undefined : done(() => api.useLine(current, { kind: "independent" }), msg().savelib.lineChanged)) });
      if (!mine.shared && !alone) menu.push({ label: s.lineFollow(mine.name || mine.devices.join(", ")), checked: true, onClick: () => undefined });
      for (const l of lines.lines.filter((l) => !l.shared && !l.current)) {
        menu.push({ label: s.lineFollow(l.name || l.devices.join(", ")), disabled, title: why, onClick: () => done(() => api.useLine(current, { kind: "follow", line: l.id }), msg().savelib.lineChanged) });
      }
    }
    const auto = (on: boolean, ok: string) => done(() => api.updateGame(current, { autoSync: on }), ok);
    const mode = game.store?.mode;
    if (mode && store) {
      // Three ways for a game Steam installed: Ludonimbus syncs it, stores versions beside Steam Cloud, or does nothing by itself.
      menu.push({
        label: s.autoSync,
        checked: mode === "managed" && game.autoSync,
        divider: menu.length > 0,
        disabled: disabled && mode !== "managed",
        title: disabled && mode !== "managed" ? why : undefined,
        onClick: () => (mode !== "managed" ? store.onTakeOver() : game.autoSync ? undefined : auto(true, msg().savelib.autoOn)),
      });
      menu.push({
        label: s.backupOnly,
        checked: mode === "backup",
        disabled: disabled && mode === "managed",
        title: disabled && mode === "managed" ? why : undefined,
        onClick: () => (mode === "managed" ? store.onHandBack() : mode === "steam" ? auto(true, msg().savelib.backupOn) : undefined),
      });
      menu.push({ label: s.manualSync, checked: !game.autoSync, onClick: () => (game.autoSync ? auto(false, msg().savelib.autoOff) : undefined) });
    } else {
      menu.push({
        label: s.autoSync,
        checked: game.autoSync,
        divider: menu.length > 0,
        onClick: () => auto(!game.autoSync, game.autoSync ? msg().savelib.autoOff : msg().savelib.autoOn),
      });
    }
    if (onEditSaves) menu.push({ label: s.editSaves, onClick: onEditSaves });
    if (game.joined) menu.push({ label: s.leaveJoin, disabled, title: why, onClick: () => done(() => api.joinAnswer(current, "leave"), msg().savelib.left) });
    if (members.length > 1) menu.push({ label: s.split, onClick: split });
  }

  const hereMark = me && (
    <>
      {me.source.logo && <img className="src-logo" src={me.source.logo} alt="" width={15} height={15} />}
      {hereLabel} · {s.here}
      {!game?.autoSync && game && <span className="chip-count">{s.manual}</span>}
      {game?.autoSync && game.store?.mode === "backup" && <span className="chip-count">{s.backupOnly}</span>}
      <Icon name="down" size={12} />
    </>
  );
  const mark = (m: LineMember, i: number) =>
    m.here && game ? (
      <Menu key="here" className="member here" title={s.changeHere} items={menu}>
        {hereMark}
      </Menu>
    ) : (
      <span key={`${m.device}/${m.platform}/${i}`} className="member">
        {lines?.platforms[m.platform]?.logo && <img className="src-logo" src={lines.platforms[m.platform].logo!} alt="" width={15} height={15} />}
        {lines?.platforms[m.platform]?.label ? `${lines.platforms[m.platform].label} · ` : ""}
        {m.device}
      </span>
    );
  // Lines somebody is on; before the cloud answers, just this device.
  const groups = (lines?.lines.filter((l) => l.members.length > 0) ?? []).sort((a, b) => Number(b.current) - Number(a.current));
  const others = members.filter((m) => m.key !== current);

  return (
    <div className="version-bar">
      {game && groups.length === 0 && (
        <span className="line-group">
          <Menu className="member here" title={s.changeHere} items={menu}>
            {hereMark}
          </Menu>
        </span>
      )}
      {groups.map((l) => (
        <span key={l.id} className={`line-group${l.shared ? "" : " own"}`} title={l.shared ? s.lineSharedTitle : s.lineOwnTitle(l.name)}>
          {[...l.members].sort((a, b) => Number(b.here) - Number(a.here)).map(mark)}
          {!l.shared && <span className="line-name">{s.independent}</span>}
        </span>
      ))}
      {(game || groups.length > 0) && <Hint text={s.linesHint} />}
      {others.length > 0 && (
        <div className="chips">
          {(game ? others : members).map((m) => (
            <button key={m.key} className={`chip${m.key === current ? " on" : ""}${m.playable ? "" : " chip-missing"}`} onClick={() => onOpen(m.key)} title={s.openVersion}>
              {m.source.logo && <img className="src-logo" src={m.source.logo} alt="" width={15} height={15} />}
              {variantLabel(m)}
              <span className="chip-count">{m.cloud ? s.cloudOnly : m.playable ? s.here : s.notInstalled}</span>
            </button>
          ))}
        </div>
      )}
      <div className="version-bar-actions">
        <button className="link" onClick={() => setLinking(true)}>
          <Icon name="link" size={14} /> {s.link}
        </button>
        {!game && members.length > 1 && (
          <button className="link" onClick={split}>
            {s.split}
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

/**
 * One line under the strip for a day after saves of the cloud replaced the
 * ones here (a sync that brought another device's progress, "make
 * current"): what happened, and taking it back.
 */
export function UndoRow({ game, onChanged }: { game: GameView; onChanged: () => Promise<void> }) {
  const t = useT();
  const ui = useUi();
  const [busy, setBusy] = useState(false);
  const r = game.replaced;
  if (!r || Date.now() - Date.parse(r.at) > 24 * 3600 * 1000) return null;
  const undo = async () => {
    setBusy(true);
    try {
      const o = await api.undoReplace(game.key);
      const done = o.kind === "pulled" || o.kind === "uploaded" || o.kind === "queued";
      ui.toast(done ? msg().saves.undone : outcomeText(o, game.name), done ? "ok" : "info");
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="banner banner-info join-row">
      <Icon name="restore" />
      <span title={dateTime(r.at)}>{t.saves.replacedBy(relTime(r.at), r.device)}</span>
      <Button small kind="ghost" onClick={undo} disabled={busy || game.running} title={game.running ? t.savelib.running : undefined}>
        {t.saves.undo}
      </Button>
    </div>
  );
}

/**
 * One line under the strip when the cloud has a save line this version
 * could share although it keeps its saves in other folders ("asked once",
 * docs/plan-v2.md section 6). The details are behind the "?".
 */
export function JoinRow({ gameKey, name, disabled, onChanged }: { gameKey: string; name: string; disabled?: boolean; onChanged: () => Promise<void> }) {
  const t = useT();
  const j = t.join;
  const ui = useUi();
  const [s, setS] = useState<JoinSuggestion | null>(null);
  useEffect(() => {
    let live = true;
    api.joinSuggestion(gameKey).then(
      (v) => live && setS(v),
      () => live && setS(null),
    );
    return () => void (live = false);
  }, [gameKey]);
  if (!s || s.fit !== "ready") return null;

  const answer = async (a: "accept" | "decline") => {
    try {
      const o = await api.joinAnswer(gameKey, a, s);
      setS(null);
      const m = msg().join;
      if (!o) ui.toast(m.declined, "info");
      else ui.toast(o.kind === "upToDate" || o.kind === "uploaded" ? m.shared : outcomeText(o, name), o.kind === "conflict" ? "warn" : "ok");
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };
  const detail = [
    ...s.binds.map((b) => b.local),
    j.matched(s.matched),
    ...(s.outside.length > 0 ? [j.outside(s.outside.length), ...s.outside.slice(0, 5)] : []),
    ...(s.confirmed ? [j.confirmed] : []),
  ].join("\n");

  return (
    <div className="banner banner-info join-row">
      <Icon name="link" />
      <span>
        {j.text(s.device)} <Hint text={detail} />
      </span>
      <Button small kind="primary" onClick={() => answer("accept")} disabled={disabled} title={disabled ? t.savelib.running : undefined}>
        {j.share}
      </Button>
      <Button small kind="ghost" onClick={() => answer("decline")}>
        {j.no}
      </Button>
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
      <p className="muted">
        {s.linkShort(current.name)} <Hint text={s.linkText(current.name)} />
      </p>
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
              {plan.toXbox && (
                <div className="banner banner-warn">
                  {s.xboxShort} <Hint text={s.xboxSteps} />
                </div>
              )}
              {plan.crossPlatform && (
                <div className="banner banner-warn">
                  {s.crossShort} <Hint text={s.crossPlatform} />
                </div>
              )}
              {plan.cloudTarget && (
                <div className="banner banner-info">
                  {s.cloudTargetShort} <Hint text={s.cloudTarget} />
                </div>
              )}
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
