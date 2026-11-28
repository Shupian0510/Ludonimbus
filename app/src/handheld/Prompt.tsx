// A question of the launch wrapper, full screen (`ludonimbus-app --prompt`,
// docs/plan-phase5.md §1.2): each answer has its controller button (A / B /
// X / Y) and can also be picked with the d-pad. Closing the window answers
// with the B button.
import { useEffect } from "react";
import { api, PromptRequest } from "../api";
import { useT } from "../i18n";
import { Icon } from "../components/ui";
import { type Button, focusFirst, Glyph, NavRoot, useButtons } from "./nav";

export function Prompt({ request }: { request: PromptRequest }) {
  return (
    <NavRoot>
      <Question request={request} />
    </NavRoot>
  );
}

function Question({ request }: { request: PromptRequest }) {
  const t = useT();
  const answer = (id: string | null) => api.promptAnswer(id);
  const by = (b: Button) => {
    const x = request.buttons.find((x) => x.key === b);
    return x ? () => answer(x.id) : undefined;
  };
  useButtons({ b: by("b") ?? (() => answer(null)), x: by("x"), y: by("y") });
  useEffect(() => {
    const id = requestAnimationFrame(focusFirst);
    return () => cancelAnimationFrame(id);
  }, []);

  return (
    <div className="hh hh-prompt">
      <div className={`hh-prompt-card level-${request.level}`}>
        <div className="hh-prompt-icon">
          <Icon name={request.level === "info" ? "cloud" : "warn"} size={40} />
        </div>
        <h1>{request.title.replace(/^Ludonimbus · /, "")}</h1>
        <p className="hh-prompt-text">{request.text}</p>
        <div className="hh-dialog-buttons">
          {request.buttons.map((b) => (
            <button key={b.id} data-nav data-autofocus={b.key === "a" || undefined} onClick={() => answer(b.id)}>
              <Glyph b={b.key} />{" "}
              {b.detail ? (
                <span className="hh-button-label">
                  {b.label}
                  <small>{b.detail}</small>
                </span>
              ) : (
                b.label
              )}
            </button>
          ))}
        </div>
        <p className="muted small">{t.handheld.promptHint}</p>
      </div>
    </div>
  );
}
