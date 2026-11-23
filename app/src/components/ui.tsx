import { createContext, ReactNode, useCallback, useContext, useEffect, useRef, useState } from "react";

// ---------------------------------------------------------------------------
// Icons (inline, 1.6px strokes)

const paths: Record<string, ReactNode> = {
  grid: <path d="M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z" />,
  list: <path d="M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01" />,
  plus: <path d="M12 5v14M5 12h14" />,
  sync: <path d="M20 11a8 8 0 0 0-14.3-4.9L4 8m0-4v4h4M4 13a8 8 0 0 0 14.3 4.9L20 16m0 4v-4h-4" />,
  cloud: <path d="M7 18a5 5 0 0 1-.9-9.9A6 6 0 0 1 17.7 9 4.5 4.5 0 0 1 17.5 18z" />,
  upload: <path d="M12 16V4m0 0-4 4m4-4 4 4M4 16v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2" />,
  download: <path d="M12 4v12m0 0-4-4m4 4 4-4M4 16v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2" />,
  folder: <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />,
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
    </>
  ),
  devices: <path d="M4 5h16v10H4zM2 19h20M9 15v4M15 15v4" />,
  library: <path d="M4 4h4v16H4zM10 4h4v16h-4zM16 5l4 1-3 14-4-1z" />,
  back: <path d="M15 18l-6-6 6-6" />,
  lock: (
    <>
      <rect x="5" y="11" width="14" height="10" rx="2" />
      <path d="M8 11V7a4 4 0 0 1 8 0v4" />
    </>
  ),
  unlock: (
    <>
      <rect x="5" y="11" width="14" height="10" rx="2" />
      <path d="M8 11V7a4 4 0 0 1 7.5-2" />
    </>
  ),
  trash: <path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13" />,
  restore: <path d="M3 12a9 9 0 1 0 3-6.7L3 8m0-5v5h5M12 7v5l3 3" />,
  edit: <path d="M4 20h4L19 9l-4-4L4 16zM14 6l4 4" />,
  steam: (
    <>
      <circle cx="12" cy="12" r="9" />
      <circle cx="15" cy="9" r="2.5" />
      <path d="M3.5 14.5l5 2a3 3 0 0 0 5-2.3l1.5-3" />
    </>
  ),
  warn: <path d="M12 3l9.5 17h-19zM12 10v4M12 17h.01" />,
  check: <path d="M5 12l5 5L20 7" />,
  x: <path d="M6 6l12 12M18 6L6 18" />,
  search: (
    <>
      <circle cx="11" cy="11" r="7" />
      <path d="M20 20l-4-4" />
    </>
  ),
  play: <path d="M7 4l13 8-13 8z" />,
  locate: (
    <>
      <circle cx="12" cy="12" r="7" />
      <path d="M12 2v3M12 19v3M2 12h3M19 12h3" />
    </>
  ),
};

export function Icon({ name, size = 18 }: { name: keyof typeof paths | string; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {paths[name]}
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Basic controls

type BtnProps = {
  children?: ReactNode;
  onClick?: () => void | Promise<unknown>;
  kind?: "primary" | "secondary" | "ghost" | "danger";
  icon?: string;
  disabled?: boolean;
  small?: boolean;
  title?: string;
  busyText?: string;
};

/** Button that shows a spinner while its async handler runs. */
export function Button({ children, onClick, kind = "secondary", icon, disabled, small, title, busyText }: BtnProps) {
  const [busy, setBusy] = useState(false);
  const mounted = useRef(true);
  useEffect(() => () => void (mounted.current = false), []);
  const click = async () => {
    if (!onClick || busy) return;
    const r = onClick();
    if (r instanceof Promise) {
      setBusy(true);
      try {
        await r;
      } finally {
        if (mounted.current) setBusy(false);
      }
    }
  };
  return (
    <button
      className={`btn btn-${kind}${small ? " btn-sm" : ""}`}
      onClick={click}
      disabled={disabled || busy}
      title={title}
    >
      {busy ? <span className="spinner" /> : icon ? <Icon name={icon} size={small ? 15 : 17} /> : null}
      {busy && busyText ? busyText : children}
    </button>
  );
}

export function Toggle({ checked, onChange, disabled }: { checked: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <button
      className={`toggle${checked ? " on" : ""}`}
      onClick={() => !disabled && onChange(!checked)}
      disabled={disabled}
      role="switch"
      aria-checked={checked}
    >
      <span />
    </button>
  );
}

export function Badge({ tone, children }: { tone: "ok" | "info" | "warn" | "bad" | "muted"; children: ReactNode }) {
  return <span className={`badge badge-${tone}`}>{children}</span>;
}

export function Spinner() {
  return <span className="spinner" />;
}

export function Empty({ icon, title, children }: { icon: string; title: string; children?: ReactNode }) {
  return (
    <div className="empty">
      <div className="empty-icon">
        <Icon name={icon} size={34} />
      </div>
      <h3>{title}</h3>
      {children}
    </div>
  );
}

export function Modal({
  title,
  onClose,
  children,
  wide,
  footer,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
  footer?: ReactNode;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`modal${wide ? " modal-wide" : ""}`}>
        <div className="modal-head">
          <h2>{title}</h2>
          <button className="icon-btn" onClick={onClose} aria-label="关闭">
            <Icon name="x" />
          </button>
        </div>
        <div className="modal-body">{children}</div>
        {footer && <div className="modal-foot">{footer}</div>}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Toasts + confirm / prompt dialogs

type Toast = { id: number; text: string; tone: "ok" | "info" | "warn" | "bad" };
type Ask = {
  title: string;
  text: ReactNode;
  confirm: string;
  danger?: boolean;
  input?: { placeholder: string; value: string };
  resolve: (v: string | null) => void;
};

interface UiApi {
  toast: (text: string, tone?: Toast["tone"]) => void;
  confirm: (o: { title: string; text: ReactNode; confirm?: string; danger?: boolean }) => Promise<boolean>;
  prompt: (o: { title: string; text: ReactNode; placeholder?: string; value?: string; confirm?: string }) => Promise<string | null>;
}

const UiCtx = createContext<UiApi | null>(null);

export function useUi(): UiApi {
  const v = useContext(UiCtx);
  if (!v) throw new Error("UiProvider missing");
  return v;
}

export function UiProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [ask, setAsk] = useState<Ask | null>(null);
  const [input, setInput] = useState("");
  const seq = useRef(0);

  const toast = useCallback((text: string, tone: Toast["tone"] = "ok") => {
    const id = ++seq.current;
    setToasts((t) => [...t, { id, text, tone }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), tone === "bad" ? 9000 : 4500);
  }, []);

  const confirm = useCallback<UiApi["confirm"]>(
    (o) =>
      new Promise((resolve) =>
        setAsk({ title: o.title, text: o.text, confirm: o.confirm ?? "确定", danger: o.danger, resolve: (v) => resolve(v !== null) }),
      ),
    [],
  );

  const prompt = useCallback<UiApi["prompt"]>(
    (o) =>
      new Promise((resolve) => {
        setInput(o.value ?? "");
        setAsk({
          title: o.title,
          text: o.text,
          confirm: o.confirm ?? "确定",
          input: { placeholder: o.placeholder ?? "", value: o.value ?? "" },
          resolve,
        });
      }),
    [],
  );

  const close = (v: string | null) => {
    ask?.resolve(v);
    setAsk(null);
  };

  return (
    <UiCtx.Provider value={{ toast, confirm, prompt }}>
      {children}
      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast toast-${t.tone}`}>
            <Icon name={t.tone === "ok" ? "check" : t.tone === "bad" ? "x" : t.tone === "warn" ? "warn" : "cloud"} />
            <span>{t.text}</span>
          </div>
        ))}
      </div>
      {ask && (
        <Modal
          title={ask.title}
          onClose={() => close(null)}
          footer={
            <>
              <Button kind="ghost" onClick={() => close(null)}>
                取消
              </Button>
              <Button kind={ask.danger ? "danger" : "primary"} onClick={() => close(ask.input ? input : "")}>
                {ask.confirm}
              </Button>
            </>
          }
        >
          <div className="ask-text">{ask.text}</div>
          {ask.input && (
            <input
              className="input"
              autoFocus
              placeholder={ask.input.placeholder}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && close(input)}
            />
          )}
        </Modal>
      )}
    </UiCtx.Provider>
  );
}
