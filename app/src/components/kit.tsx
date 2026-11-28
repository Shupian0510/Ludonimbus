// The component vocabulary (docs/ui-design.md): the facts the interface
// shows about saves — where they were made, on which device, when, how they
// compare with the ones here — each as one small component in a fixed place,
// the same on every page and on the handheld screen. Facts are never words
// joined by dots.
import type { ReactNode } from "react";
import { useState } from "react";
import type { PlatformMark as Mark, SnapshotSummary } from "../api";
import { bytes, dateTime, duration, relTime } from "../format";
import { msg, useT } from "../i18n";
import { Hint, Icon } from "./ui";

export type Tone = "ok" | "info" | "warn" | "bad" | "muted";

/** A state: a dot in its colour and its name. */
export function Pill({ tone, children, title }: { tone: Tone; children: ReactNode; title?: string }) {
  return (
    <span className={`pill pill-${tone}`} title={title}>
      <i />
      {children}
    </span>
  );
}

/**
 * What saves were made on: the program's own icon, read from its
 * executable on this device (crates/core/src/logos.rs), and its short name.
 * The name alone when the program isn't here; never a drawn icon.
 */
export function PlatformMark({ mark, label, size = 16, strong }: { mark?: Mark | null; label?: string | null; size?: number; strong?: boolean }) {
  const [failed, setFailed] = useState(false);
  const name = label || mark?.label;
  if (!name) return null;
  return (
    <span className={`pmark${strong ? " strong" : ""}`} title={name}>
      {mark?.logo && !failed && <img className="src-logo" src={mark.logo} alt="" width={size} height={size} onError={() => setFailed(true)} />}
      <span className="pmark-name">{name}</span>
    </span>
  );
}

export type DeviceKind = "desktop" | "laptop" | "handheld" | "deck" | "switch";

/**
 * What kind of device a name (and the OS its record gives) suggests.
 * Windows names new PCs DESKTOP-… or LAPTOP-…; handhelds and the Steam
 * Deck go by their makers' names.
 */
export function deviceKind(name: string, os?: string | null): DeviceKind {
  const n = name.toLowerCase();
  const o = (os ?? "").toLowerCase();
  if (o.startsWith("switch") || /(^|[^a-z])(switch|nintendo)([^a-z]|$)/.test(n)) return "switch";
  if (/steam ?deck|steamdeck|(^|[^a-z])deck([^a-z]|$)/.test(n)) return "deck";
  if (/(^|[^a-z])(rog|ally|gpd|legion[ _-]?go|claw|ayaneo|onexplayer|aokzoe|zotac|loki|win ?mini|win ?max|win ?4|handheld|掌机)/.test(n)) return "handheld";
  if (/laptop|notebook|book|笔记本/.test(n)) return "laptop";
  return "desktop";
}

/** A device: its kind and its name. `here`: this device. */
export function DeviceChip({ name, os, here, label }: { name: string; os?: string | null; here?: boolean; label?: string }) {
  useT();
  if (!name) return null;
  return (
    <span className={`dev${here ? " here" : ""}`} title={here ? msg().kit.thisDevice(name) : name}>
      <Icon name={`dev-${deviceKind(name, os)}`} size={15} />
      <span className="dev-name">{label ?? name}</span>
    </span>
  );
}

/** A moment: how long ago, the date and time on hover. */
export function When({ at, strong }: { at: string | null | undefined; strong?: boolean }) {
  useT();
  if (!at) return <span className="when muted">—</span>;
  return (
    <time className={`when${strong ? " strong" : ""}`} dateTime={at} title={dateTime(at)}>
      {relTime(at)}
    </time>
  );
}

/** A span of time: "30 分钟", "5 小时", "2 天". */
export function span(ms: number): string {
  const f = msg().format;
  const min = Math.max(1, Math.round(ms / 60000));
  if (min < 60) return f.minutes(min);
  if (min < 60 * 48) return f.hours(String(Math.round(min / 60)));
  return msg().kit.days(Math.round(min / (60 * 24)));
}

/** Hours and minutes: "1 小时 20 分". */
export function hm(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.round((secs % 3600) / 60);
  return msg().kit.hm(h, h === 0 ? Math.max(1, m) : m);
}

/** How a save compares with the one here. */
export interface Cmp {
  kind: "newer" | "older" | "same";
  ms: number;
}

export function cmpOf(s: SnapshotSummary, here?: SnapshotSummary | null): Cmp | null {
  if (!here) return null;
  if (s.contentHash === here.contentHash) return { kind: "same", ms: 0 };
  const ms = Date.parse(s.createdAt) - Date.parse(here.createdAt);
  return { kind: ms > 0 ? "newer" : "older", ms: Math.abs(ms) };
}

/** How much more the game had been played when `s` was made than `here`, where both say. */
export function playedOf(s: { playtime?: number | null }, here?: { playtime?: number | null } | null): number | null {
  if (s.playtime == null || here?.playtime == null) return null;
  return s.playtime - here.playtime;
}

/**
 * Newer (an arrow up, in the accent colour) or older than the save here,
 * by how much; the same saves. `quiet`: nothing for an older one (a list
 * of the saves here, where older is what history is).
 */
export function Delta({ cmp, quiet }: { cmp: Cmp | null; quiet?: boolean }) {
  useT();
  const k = msg().kit;
  if (!cmp) return null;
  if (cmp.kind === "same")
    return (
      <span className="delta same" title={k.same}>
        <Icon name="check" size={12} />
        {k.sameShort}
      </span>
    );
  if (cmp.kind === "older" && quiet) return null;
  const s = span(cmp.ms);
  return (
    <span className={`delta ${cmp.kind}`} title={cmp.kind === "newer" ? k.newerBy(s) : k.olderBy(s)}>
      <Icon name={cmp.kind === "newer" ? "arrow-up" : "arrow-down"} size={12} />
      {s}
    </span>
  );
}

/** More (or less) played than the save here: "+1 小时 20 分". `quiet`: nothing for less. */
export function PlayDelta({ secs, quiet }: { secs: number | null; quiet?: boolean }) {
  useT();
  if (secs == null || Math.abs(secs) < 60 || (quiet && secs < 0)) return null;
  const d = hm(Math.abs(secs));
  return (
    <span className={`playdelta${secs < 0 ? " less" : ""}`} title={secs > 0 ? msg().kit.playedMore(d) : msg().kit.playedLess(d)}>
      <Icon name="clock" size={12} />
      {secs > 0 ? "+" : "−"}
      {d}
    </span>
  );
}

/** An icon and a value; what it is, on hover. */
export function Stat({ icon, title, children }: { icon: string; title?: string; children: ReactNode }) {
  return (
    <span className="stat" title={title}>
      <Icon name={icon} size={14} />
      {children}
    </span>
  );
}

/** Amounts as bars against the largest (playtime per device, storage used). */
export function Bars({ rows, format }: { rows: Array<{ key: string; label: ReactNode; value: number }>; format: (v: number) => string }) {
  const max = Math.max(1, ...rows.map((r) => r.value));
  return (
    <div className="bars">
      {rows.map((r) => (
        <div key={r.key} className="bars-row">
          <span className="bars-label">{r.label}</span>
          <span className="bars-track">
            <i style={{ width: `${Math.max(2, (r.value / max) * 100)}%` }} />
          </span>
          <span className="bars-value">{format(r.value)}</span>
        </div>
      ))}
    </div>
  );
}

/** How full something is: a bar and "used / total". */
export function Meter({ used, total, title }: { used: number; total: number; title?: string }) {
  const share = total > 0 ? Math.min(1, used / total) : 0;
  return (
    <span className={`meter${share > 0.9 ? " full" : ""}`} title={title}>
      <span className="meter-track">
        <i style={{ width: `${Math.max(2, share * 100)}%` }} />
      </span>
      <span className="meter-text">
        {bytes(used)} / {bytes(total)}
      </span>
    </span>
  );
}

/** "Needs converting", with how in the "?". */
export function ConvertChip({ hint }: { hint: string }) {
  const t = useT();
  return (
    <span className="convert-chip">
      <Icon name="transfer" size={13} />
      {t.sources.convert}
      <Hint text={hint} />
    </span>
  );
}

/** One save of a comparison. */
export interface Side {
  /** What it is called: "GOG 版", "云端", "本机". */
  name: string;
  mark?: Mark | null;
  device?: string;
  /** The device is this one. */
  mine?: boolean;
  at?: string | null;
  playtime?: number | null;
  /** More played than the other side (seconds). */
  played?: number | null;
  size?: number | null;
  files?: number | null;
  /** The newer one: stressed. */
  newer?: boolean;
  /** A small tag at the top right ("这里"). */
  tag?: ReactNode;
  /** Shown when there is no save ("还没有存档"). */
  empty?: boolean;
  /** Its action (a button under the facts). */
  action?: ReactNode;
}

/** A save as a card: where, when (large), which device, how far played, how big. */
export function SaveCard({ side }: { side: Side }) {
  const t = useT();
  const k = t.kit;
  return (
    <div className={`savecard${side.newer ? " newer" : ""}${side.empty ? " empty" : ""}`}>
      <div className="savecard-head">
        <PlatformMark mark={side.mark} label={side.name} size={20} strong />
        {side.newer ? (
          <span className="savecard-tag newer">
            <Icon name="arrow-up" size={12} />
            {k.newer}
          </span>
        ) : (
          side.tag && <span className="savecard-tag">{side.tag}</span>
        )}
      </div>
      {side.empty ? (
        <div className="savecard-when muted">{k.noSaves}</div>
      ) : (
        <>
          {side.at && (
            <div className="savecard-when">
              <When at={side.at} />
              <span className="savecard-date">{dateTime(side.at)}</span>
            </div>
          )}
          <div className="savecard-facts">
            {side.device && <DeviceChip name={side.device} here={side.mine} />}
            {side.playtime != null && side.playtime > 0 && (
              <Stat icon="clock" title={k.playtime}>
                {duration(side.playtime)}
              </Stat>
            )}
            <PlayDelta secs={side.played ?? null} quiet />
            {side.size != null && (
              <Stat icon="file" title={side.files != null ? k.size(side.files, bytes(side.size)) : undefined}>
                {bytes(side.size)}
              </Stat>
            )}
          </div>
        </>
      )}
      {side.action && <div className="savecard-action">{side.action}</div>}
    </div>
  );
}

/**
 * Two saves side by side: the one offered on the left, the one here on the
 * right, an arrow between when one goes to the other.
 */
export function SaveCompare({ there, here, arrow = true }: { there: Side; here: Side; arrow?: boolean }) {
  return (
    <div className={`compare${arrow ? "" : " no-arrow"}`}>
      <SaveCard side={there} />
      {arrow && (
        <span className="compare-arrow" aria-hidden>
          <Icon name="arrow" size={22} />
        </span>
      )}
      <SaveCard side={here} />
    </div>
  );
}

/**
 * The head of a group of saves: the program's icon and the group's name,
 * the devices, its newest save, whether its saves need converting.
 */
export function SourceHead({
  mark,
  name,
  device,
  devices = [],
  mine,
  latest,
  convert,
  hint,
}: {
  mark?: Mark | null;
  name: string;
  /** The group is a device (another device's own saves): its name as a device. */
  device?: boolean;
  devices?: string[];
  /** Which of `devices` is this one. */
  mine?: string;
  latest?: string | null;
  /** How its saves are converted to be taken here (the chip's "?"). */
  convert?: string | null;
  hint: string;
}) {
  return (
    <div className="save-group">
      {device ? <DeviceChip name={name} /> : <PlatformMark mark={mark} label={name} size={18} strong />}
      {devices
        .filter((d) => !device || d !== name)
        .map((d) => (
          <DeviceChip key={d} name={d} here={d === mine} />
        ))}
      {latest && <When at={latest} />}
      {convert && <ConvertChip hint={convert} />}
      <Hint text={hint} />
    </div>
  );
}
