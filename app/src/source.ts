// Where games come from (an emulator, a recomp, a PC game…): the library's
// source badges, filter and groups. The backend detects the source from the
// game's save rule; the user can put a game in a group of their own.
import type { GameView, SourceKind } from "./api";
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
  /** An icon name from components/ui.tsx. */
  icon: string;
}

export interface Grouped {
  group: SourceGroup;
  games: GameView[];
}

const idOf = (label: string) => label.trim().toLocaleLowerCase();

/**
 * Icons an emulator rule can name (`icon:` in adapters/emulators.yaml): the
 * shape of the device it emulates, never a logo. Keep in sync with the
 * `emulator_icons` test in crates/core/src/adapters.rs.
 */
export const SOURCE_ICONS = ["cube", "tablet", "dual-screen", "hybrid", "disc", "handheld", "layers"];

const KIND_ICON: Record<SourceKind, string> = { emulator: "gamepad", recomp: "chip", pc: "monitor", xbox: "bag", custom: "folder" };

function iconOf(g: GameView): string {
  const named = g.source.kind === "emulator" ? g.source.icon : null;
  return named && SOURCE_ICONS.includes(named) ? named : KIND_ICON[g.source.kind];
}

/** The group worked out from the game's save rule, ignoring the user's choice. */
export function detectedGroup(g: GameView): SourceGroup {
  const s = g.source;
  const label = s.name ?? msg().source.kinds[s.kind];
  return { id: idOf(label), label, kind: s.kind, named: false, detail: s.kind === "emulator" ? s.detail : null, icon: iconOf(g) };
}

/** Emulators and the user's own groups first (by name), then recomps, PC, Xbox, other. */
const RANK: Record<SourceKind, number> = { emulator: 0, recomp: 1, pc: 2, xbox: 3, custom: 4 };
const rank = (g: SourceGroup) => (g.named ? 0 : RANK[g.kind]);

/**
 * Each game's group (by game key) and the groups in display order. A group
 * the user named after a detected one (`Dolphin`) joins it.
 */
export function groupBySource(games: GameView[]): { of: Map<string, SourceGroup>; groups: Grouped[] } {
  const detected = new Map<string, SourceGroup>();
  for (const g of games) {
    const d = detectedGroup(g);
    if (!detected.has(d.id)) detected.set(d.id, d);
  }
  const of = new Map<string, SourceGroup>();
  const byId = new Map<string, Grouped>();
  for (const g of games) {
    const label = g.source.label?.trim();
    const group = label
      ? detected.get(idOf(label)) ?? { id: idOf(label), label, kind: g.source.kind, named: true, detail: null, icon: "tag" }
      : detected.get(detectedGroup(g).id)!;
    of.set(g.key, group);
    const entry = byId.get(group.id);
    if (entry) entry.games.push(g);
    else byId.set(group.id, { group, games: [g] });
  }
  const lang = currentLang();
  const groups = [...byId.values()].sort(
    (a, b) => rank(a.group) - rank(b.group) || a.group.label.localeCompare(b.group.label, lang),
  );
  return { of, groups };
}

/** The name on a game's badge; null for an ungrouped hand-added game, which gets no badge. */
export function tagText(group: SourceGroup): string | null {
  if (group.named || group.kind === "emulator") return group.label;
  if (group.kind === "custom") return null;
  return msg().source.tags[group.kind];
}
