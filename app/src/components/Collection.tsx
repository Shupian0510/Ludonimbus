import { useEffect, useMemo, useState } from "react";
import { api, CollectionStats, errorText, Overview } from "../api";
import { duration } from "../format";
import { currentLang, msg, useT } from "../i18n";
import { type Card, cardsOf, Cover } from "./Library";
import { familyOf } from "./SaveLibrary";
import { Badge, Hint, Icon, type MenuItem, useUi } from "./ui";

/**
 * The shelves of the game `key` (any of its versions), the game page's menu
 * items that put it on one or take it off, and the marks for its status
 * line. All its versions go together.
 */
export function useShelves(overview: Overview, key: string, onChanged: () => Promise<void>) {
  const t = useT();
  const c = t.collection;
  const ui = useUi();
  const family = familyOf(overview, key);
  const on = new Set(family.flatMap((m) => (m.game ?? m.cloud)?.shelves ?? []));
  const toggle = async (tag: "finished" | "archived") => {
    const was = on.has(tag);
    try {
      await api.setShelf(
        family.map((m) => m.key),
        tag,
        !was,
      );
      const m = msg().collection;
      ui.toast(tag === "finished" ? (was ? m.unmarkedFinished : m.markedFinished) : was ? m.unarchivedDone : m.archivedDone, "ok");
      await onChanged();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };
  const items: MenuItem[] = [
    { label: on.has("finished") ? c.unmarkFinished : c.markFinished, icon: "check", onClick: () => toggle("finished") },
    { label: on.has("archived") ? c.unarchive : c.archive, icon: "star", onClick: () => toggle("archived") },
  ];
  const marks = (
    <>
      {on.has("finished") && <Badge tone="ok">{c.finished}</Badge>}
      {on.has("archived") && <Badge tone="muted">{c.archived}</Badge>}
    </>
  );
  return { items, marks };
}

/**
 * The collection (docs/plan-v2.md 10.5): what the library adds up to, and
 * two shelves: games played to the end, and games put away (out of the
 * library; their saves, play time and achievements stay in the cloud). A
 * shelf is a tag on the game's card in the cloud, so every device shows the
 * same ones; the game page's menu puts a game on one.
 */
export function Collection({ overview, onOpen }: { overview: Overview; onOpen: (key: string) => void }) {
  const t = useT();
  const c = t.collection;
  const [stats, setStats] = useState<CollectionStats | null>(null);
  useEffect(() => {
    api.collectionStats().then(setStats, () => setStats(null));
  }, [overview]);

  const cards = useMemo(() => {
    const lang = currentLang();
    return cardsOf(overview).sort((a, b) => a.name.localeCompare(b.name, lang));
  }, [overview]);
  const secs = cards.reduce((n, card) => n + card.secs, 0);
  const finished = cards.filter((card) => card.shelves.includes("finished"));
  const archived = cards.filter((card) => card.shelves.includes("archived"));
  const pct = stats && stats.achievementsTotal > 0 ? Math.round((stats.achievementsUnlocked / stats.achievementsTotal) * 100) : null;

  const shelf = (title: string, hint: string, list: Card[], empty: string) => (
    <section className="shelf">
      <div className="lib-divider">
        {title}
        <span className="chip-count">{list.length}</span>
        <Hint text={hint} />
      </div>
      {list.length === 0 ? (
        <p className="muted small">{empty}</p>
      ) : (
        <div className="grid">
          {list.map((card) => (
            <button key={card.key} className={card.playable ? "card" : "card missing"} onClick={() => onOpen(card.key)}>
              <div className="cover">
                <Cover name={card.name} cover={(card.main.game ?? card.main.cloud)!.art.cover} />
              </div>
              <div className="card-meta">
                <div className="card-name">{card.name}</div>
                <div className="card-sub">
                  <span className="card-time muted">{card.secs > 0 && duration(card.secs)}</span>
                </div>
              </div>
            </button>
          ))}
        </div>
      )}
    </section>
  );

  return (
    <div className="page collection">
      <header className="page-head">
        <div>
          <h1>{c.title}</h1>
        </div>
      </header>
      <div className="stats">
        <div className="stat">
          <span className="muted small">
            <Icon name="clock" size={13} /> {c.playtime}
          </span>
          <b>{secs > 0 ? duration(secs) : "—"}</b>
        </div>
        <div className="stat">
          <span className="muted small">{c.games}</span>
          <b>{cards.length}</b>
        </div>
        <div className="stat">
          <span className="muted small">
            {c.achievements} {stats && stats.achievementsTotal > 0 && <Hint text={c.achievementsHint(stats.achievementsUnlocked, stats.achievementsTotal)} />}
          </span>
          <b>{pct === null ? "—" : `${pct}%`}</b>
        </div>
      </div>
      {shelf(c.finished, c.finishedHint, finished, c.finishedEmpty)}
      {shelf(c.archived, c.archivedHint, archived, c.archivedEmpty)}
    </div>
  );
}
