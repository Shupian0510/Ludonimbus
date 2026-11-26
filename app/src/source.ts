// Where games come from (an emulator, a recomp, a PC game…): the library's
// source tags, filter and groups. The backend detects the source from the
// game's save rule; the user can put a game in a group of their own.
import type { GameSource, GameView, SourceKind } from "./api";

/** Anything shown in the library: a game, or a card standing for a game's versions. */
export interface Sourced {
  key: string;
  source: GameSource;
}
import { currentLang, msg } from "./i18n";

export interface SourceGroup {
  /** Games with the same id are shown together (the label, ignoring case). */
  id: string;
  label: string;
  kind: SourceKind;
  /** A group the user named that matches no detected one. */
  named: boolean;
  /** The emulator's platform (`GameCube / Wii emulator`). */
  detail: string | null;
  /**
   * The logo of what the group's games run on (the emulator's or the Xbox
   * app's icon, read from the program on this PC); null for groups that are a
   * kind of game (recomps, PC games) and when the program isn't here.
   */
  logo: string | null;
}

export interface Grouped<T extends Sourced = GameView> {
  group: SourceGroup;
  games: T[];
}

const idOf = (label: string) => label.trim().toLocaleLowerCase();

/** The group worked out from the game's save rule, ignoring the user's choice. */
export function detectedGroup(g: Sourced): SourceGroup {
  const s = g.source;
  const label = s.name ?? msg().source.kinds[s.kind];
  return { id: idOf(label), label, kind: s.kind, named: false, detail: s.kind === "emulator" ? s.detail : null, logo: null };
}

/** Emulators and the user's own groups first (by name), then consoles, recomps, PC, Xbox, other. */
const RANK: Record<SourceKind, number> = { emulator: 0, console: 1, recomp: 2, pc: 3, xbox: 4, custom: 5 };
const rank = (g: SourceGroup) => (g.named ? 0 : RANK[g.kind]);

/**
 * Each game's group (by game key) and the groups in display order. A group
 * the user named after a detected one (`Dolphin`) joins it.
 */
export function groupBySource<T extends Sourced>(games: T[]): { of: Map<string, SourceGroup>; groups: Grouped<T>[] } {
  const detected = new Map<string, SourceGroup>();
  for (const g of games) {
    const d = detectedGroup(g);
    if (!detected.has(d.id)) detected.set(d.id, d);
  }
  const of = new Map<string, SourceGroup>();
  const byId = new Map<string, Grouped<T>>();
  for (const g of games) {
    const label = g.source.label?.trim();
    const group = label
      ? detected.get(idOf(label)) ?? { id: idOf(label), label, kind: g.source.kind, named: true, detail: null, logo: null }
      : detected.get(detectedGroup(g).id)!;
    of.set(g.key, group);
    const entry = byId.get(group.id);
    if (entry) entry.games.push(g);
    else byId.set(group.id, { group, games: [g] });
  }
  for (const e of byId.values()) e.group.logo = logoOf(e);
  const lang = currentLang();
  const groups = [...byId.values()].sort(
    (a, b) => rank(a.group) - rank(b.group) || a.group.label.localeCompare(b.group.label, lang),
  );
  return { of, groups };
}

/**
 * A group's logo: an emulator's or the Xbox app's (from any of its games), or
 * the one logo all games of a group the user named share.
 */
function logoOf<T extends Sourced>({ group, games }: Grouped<T>): string | null {
  const logos = games.map((g) => g.source.logo);
  if (group.named) return logos[0] && logos.every((l) => l === logos[0]) ? logos[0] : null;
  if (group.kind !== "emulator" && group.kind !== "xbox") return null;
  return logos.find((l) => l) ?? null;
}

/** The name on a game's badge; null for an ungrouped hand-added game, which gets no badge. */
export function tagText(group: SourceGroup): string | null {
  if (group.named || group.kind === "emulator" || group.kind === "console") return group.label;
  if (group.kind === "custom") return null;
  return msg().source.tags[group.kind];
}
