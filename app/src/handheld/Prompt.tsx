// A question of the launch wrapper, full screen (`ludonimbus-app --prompt`,
// docs/plan-phase5.md §1.2): each answer has its controller button (A / B /
// X / Y) and can also be picked with the d-pad. Closing the window answers
// with the B button. A question about two saves (another version's newer
// progress, a conflict) shows them as two cards side by side; on the desktop
// it is a window of its own (`dialog`), run with the mouse or keyboard.
import { useEffect } from "react";
import { api, PlatformMark, PromptCompare, PromptRequest, PromptSide } from "../api";
import { useT } from "../i18n";
import { ConvertChip, SaveCompare, Side } from "../components/kit";
import { Icon } from "../components/ui";
import { type Button, focusFirst, Glyph, NavRoot, useButtons } from "./nav";

export function Prompt({ request, platforms = {} }: { request: PromptRequest; platforms?: Record<string, PlatformMark> }) {
  return (
    <NavRoot>
      <Question request={request} platforms={platforms} />
    </NavRoot>
  );
}

/** The two saves as cards: the one offered, an arrow, the one here. */
function Compare({ compare, platforms }: { compare: PromptCompare; platforms: Record<string, PlatformMark> }) {
  const t = useT();
  const { there, here } = compare;
  // Another version's progress goes from there to here; in a conflict neither is ahead.
  const offer = there.newer;
  const side = (s: PromptSide, mine: boolean, other?: PromptSide | null): Side => ({
    name: s.name,
    mark: platforms[s.platform],
    device: s.device,
    mine,
    at: s.at,
    playtime: s.playtime,
    played: s.playtime != null && other?.playtime != null ? s.playtime - other.playtime : null,
    size: s.size,
    files: s.files,
    newer: s.newer,
    tag: mine && offer ? t.sources.here : undefined,
  });
  return (
    <SaveCompare
      there={side(there, false, here)}
      here={here ? side(here, true, there) : { name: t.sources.here, empty: true }}
      arrow={offer}
    />
  );
}

function Question({ request, platforms }: { request: PromptRequest; platforms: Record<string, PlatformMark> }) {
  const t = useT();
  const answer = (id: string | null) => api.promptAnswer(id);
  const by = (b: Button) => {
    const x = request.buttons.find((x) => x.key === b);
    return x ? () => answer(x.id) : undefined;
  };
  useButtons({ b: by("b") ?? (() => answer(null)), x: by("x"), y: by("y") });
  useEffect(() => {
    const id = requestAnimationFrame(() => {
      focusFirst();
      // On screen now: the wrapper won't fall back to a system dialog.
      api.promptShown().catch(() => undefined);
    });
    return () => cancelAnimationFrame(id);
  }, []);
  const c = request.compare;
  const details = request.buttons.filter((b) => b.detail);

  return (
    <div className={`hh hh-prompt${request.dialog ? " as-window" : ""}`}>
      <div className={`hh-prompt-card level-${request.level}${c ? " with-compare" : ""}`}>
        <div className="hh-prompt-title">
          <span className="hh-prompt-icon">
            <Icon name={request.level === "info" ? "cloud" : "warn"} size={30} />
          </span>
          <h1>{request.title.replace(/^Ludonimbus · /, "")}</h1>
        </div>
        {c ? (
          <>
            <Compare compare={c} platforms={platforms} />
            {(!!c.convert || c.notes.length > 0) && (
              <div className="hh-prompt-notes">
                {c.convert && <ConvertChip hint={t.sources.convertHint(c.convert)} />}
                {c.notes.map((n) => (
                  <p key={n} className="hh-prompt-note">
                    <Icon name="warn" size={18} />
                    {n}
                  </p>
                ))}
              </div>
            )}
          </>
        ) : (
          <p className="hh-prompt-text">{request.text}</p>
        )}
        <div className="hh-dialog-buttons">
          {request.buttons.map((b) => (
            <button key={b.id} data-nav data-autofocus={b.key === "a" || undefined} className={c && b.key === "a" ? "primary" : undefined} onClick={() => answer(b.id)}>
              <Glyph b={b.key} /> {b.label}
            </button>
          ))}
        </div>
        {details.map((b) => (
          <p key={b.id} className="hh-prompt-caption">
            <Icon name="shield" size={18} />
            {b.detail}
          </p>
        ))}
      </div>
    </div>
  );
}
