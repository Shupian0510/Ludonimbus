// Controller navigation for the handheld screen (docs/plan-phase5.md §1.1):
// the d-pad / left stick / arrow keys move the focus to the nearest
// focusable element in that direction, A / Enter presses it, B / Escape goes
// back, X and Y (and LB / RB) do what the current screen says. Works with
// the Gamepad API (WebView2) and with keys (Steam Input's desktop layout
// turns the controller into a keyboard).
import { createContext, ReactNode, useContext, useEffect, useRef, useSyncExternalStore } from "react";

export type Button = "a" | "b" | "x" | "y" | "lb" | "rb";
export type Handlers = Partial<Record<Exclude<Button, "a">, () => void>>;

type Dir = "up" | "down" | "left" | "right";

/** Focusable elements in the topmost scope (an open dialog, else the page). */
function focusables(): HTMLElement[] {
  const scopes = document.querySelectorAll<HTMLElement>("[data-nav-scope]");
  const root: ParentNode = scopes.length ? scopes[scopes.length - 1] : document;
  return [...root.querySelectorAll<HTMLElement>("[data-nav]")].filter((e) => !(e as HTMLButtonElement).disabled && e.offsetParent !== null);
}

/**
 * The highlighted element carries `data-current`: the highlight must not
 * depend on the window having the system focus (Steam's overlay can take it).
 */
function mark(el: Element | null) {
  document.querySelectorAll("[data-current]").forEach((e) => e !== el && e.removeAttribute("data-current"));
  if (el?.hasAttribute("data-nav")) el.setAttribute("data-current", "");
}

function current(list: HTMLElement[]): HTMLElement | null {
  const marked = document.querySelector<HTMLElement>("[data-current]");
  if (marked && list.includes(marked)) return marked;
  const active = document.activeElement as HTMLElement | null;
  return active && list.includes(active) ? active : null;
}

function focus(el: HTMLElement) {
  mark(el);
  el.focus({ preventScroll: true });
  el.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
}

/** Focus the element marked `data-autofocus`, else the first one. */
export function focusFirst() {
  const list = focusables();
  const el = list.find((e) => e.hasAttribute("data-autofocus")) ?? list[0];
  if (el) focus(el);
}

function move(dir: Dir) {
  const list = focusables();
  const cur = current(list);
  if (!cur) {
    focusFirst();
    return;
  }
  const r = cur.getBoundingClientRect();
  const cx = r.left + r.width / 2;
  const cy = r.top + r.height / 2;
  let best: { el: HTMLElement; score: number } | null = null;
  for (const el of list) {
    if (el === cur) continue;
    const b = el.getBoundingClientRect();
    const x = b.left + b.width / 2;
    const y = b.top + b.height / 2;
    const ahead = dir === "up" ? r.top - y : dir === "down" ? y - r.bottom : dir === "left" ? r.left - x : x - r.right;
    if (ahead < -2) continue;
    const side = dir === "up" || dir === "down" ? Math.abs(x - cx) : Math.abs(y - cy);
    const score = ahead + side * 2.5;
    if (!best || score < best.score) best = { el, score };
  }
  if (best) focus(best.el);
}

const Ctx = createContext<{ push: (h: { current: Handlers }) => () => void } | null>(null);

/**
 * The screen's controller actions (B, X, Y, LB, RB). The most recently
 * mounted handlers win, so a dialog takes over from the page under it.
 */
export function useButtons(h: Handlers) {
  const ctx = useContext(Ctx);
  const ref = useRef(h);
  ref.current = h;
  useEffect(() => ctx?.push(ref), [ctx]);
}

const PAD: Record<number, Button | Dir> = { 0: "a", 1: "b", 2: "x", 3: "y", 4: "lb", 5: "rb", 12: "up", 13: "down", 14: "left", 15: "right" };
const KEYS: Record<string, Button | Dir> = {
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  Enter: "a",
  " ": "a",
  Escape: "b",
  Backspace: "b",
  x: "x",
  y: "y",
  q: "lb",
  e: "rb",
  PageUp: "lb",
  PageDown: "rb",
};

/** Installs the keyboard and controller listeners for everything inside. */
export function NavRoot({ children }: { children: ReactNode }) {
  const stack = useRef<Array<{ current: Handlers }>>([]);
  const value = useRef({
    push: (h: { current: Handlers }) => {
      stack.current.push(h);
      return () => {
        stack.current = stack.current.filter((x) => x !== h);
      };
    },
  });

  useEffect(() => {
    const fire = (b: Button | Dir) => {
      if (b === "up" || b === "down" || b === "left" || b === "right") return move(b);
      if (b === "a") {
        const el = current(focusables());
        if (el) el.click();
        else focusFirst();
        return;
      }
      const top = stack.current[stack.current.length - 1];
      top?.current[b]?.();
    };

    const onKey = (e: KeyboardEvent) => {
      // Typing in a field: only Escape leaves it.
      const typing = (e.target as HTMLElement)?.tagName === "INPUT" && e.key !== "Escape";
      const b = KEYS[e.key];
      if (!b || typing || e.altKey || e.ctrlKey || e.metaKey) return;
      e.preventDefault();
      fire(b);
    };
    window.addEventListener("keydown", onKey);
    // Touch and mouse move the highlight too.
    const onFocus = (e: FocusEvent) => mark(e.target as Element);
    document.addEventListener("focusin", onFocus);

    // Gamepad: edge-triggered buttons, held directions repeat.
    let raf = 0;
    const held: Record<string, number> = {};
    const loop = (t: number) => {
      const pads = navigator.getGamepads ? navigator.getGamepads() : [];
      const down = new Set<Button | Dir>();
      for (const p of pads) {
        if (!p) continue;
        if (p.buttons.some((btn) => btn.pressed)) setLayout(layoutOf(p.id));
        p.buttons.forEach((btn, i) => btn.pressed && PAD[i] && down.add(PAD[i]));
        const [ax = 0, ay = 0] = p.axes;
        if (ax < -0.6) down.add("left");
        if (ax > 0.6) down.add("right");
        if (ay < -0.6) down.add("up");
        if (ay > 0.6) down.add("down");
      }
      for (const b of down) {
        const since = held[b];
        const dir = b === "up" || b === "down" || b === "left" || b === "right";
        if (since === undefined) {
          held[b] = t;
          fire(b);
        } else if (dir && t - since > 380) {
          held[b] = t - 380 + 110;
          fire(b);
        }
      }
      for (const b of Object.keys(held)) if (!down.has(b as Button | Dir)) delete held[b];
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => {
      window.removeEventListener("keydown", onKey);
      document.removeEventListener("focusin", onFocus);
      cancelAnimationFrame(raf);
    };
  }, []);

  return <Ctx.Provider value={value.current}>{children}</Ctx.Provider>;
}

/**
 * Whose button names to show, by the controller last used: Xbox (also the
 * built-in controls of Windows handhelds and Steam Input), PlayStation or
 * Nintendo. The buttons are the same places either way ("a" is the bottom
 * one), only their names differ.
 */
type Layout = "xbox" | "playstation" | "nintendo";
let layout: Layout = "xbox";
const layoutListeners = new Set<() => void>();

/** The Gamepad API's id: "… (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)" on Windows, "054c-0ce6-…" on Linux. */
function layoutOf(id: string): Layout {
  const s = id.toLowerCase();
  if (/vendor: 054c|^054c-|dualsense|dualshock|playstation/.test(s)) return "playstation";
  if (/vendor: 057e|^057e-|pro controller|joy-con/.test(s)) return "nintendo";
  return "xbox";
}

function useLayout(): Layout {
  return useSyncExternalStore(
    (f) => {
      layoutListeners.add(f);
      return () => layoutListeners.delete(f);
    },
    () => layout,
  );
}

function setLayout(l: Layout) {
  if (l === layout) return;
  layout = l;
  layoutListeners.forEach((f) => f());
}

const GLYPHS: Record<Layout, Record<Button, string>> = {
  xbox: { a: "A", b: "B", x: "X", y: "Y", lb: "LB", rb: "RB" },
  playstation: { a: "✕", b: "○", x: "□", y: "△", lb: "L1", rb: "R1" },
  nintendo: { a: "B", b: "A", x: "Y", y: "X", lb: "L", rb: "R" },
};

/** A controller button's glyph, named as on the controller in use. */
export function Glyph({ b }: { b: Button }) {
  const l = useLayout();
  return <span className={`hh-glyph hh-glyph-${b}${l === "xbox" ? "" : ` hh-glyph-${l}`}`}>{GLYPHS[l][b]}</span>;
}

/** The bottom bar: what each button does on this screen. */
export function Hints({ items }: { items: Array<[Button, string]> }) {
  return (
    <div className="hh-hints">
      {items.map(([b, label]) => (
        <span key={b + label} className="hh-hint">
          <Glyph b={b} /> {label}
        </span>
      ))}
    </div>
  );
}
