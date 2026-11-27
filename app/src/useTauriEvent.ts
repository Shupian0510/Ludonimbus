// Listening to events from the Rust side, the one way every component does it.
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import { inTauri } from "./api";

/**
 * Calls `handler` with the payload of every `name` event while the component
 * is mounted (nothing in the browser preview). The handler can be a new
 * function on every render: only a change of `name` registers again.
 *
 * `listen` resolves later than it is called, so a cleanup that runs first
 * (StrictMode, a quick unmount) unlistens as soon as it resolves instead of
 * leaving the listener behind.
 */
export function useTauriEvent<T>(name: string, handler: (payload: T) => void): void {
  const ref = useRef(handler);
  useEffect(() => {
    ref.current = handler;
  });
  useEffect(() => {
    if (!inTauri) return;
    let gone = false;
    let off: (() => void) | undefined;
    listen<T>(name, (e) => {
      if (!gone) ref.current(e.payload);
    }).then(
      (u) => {
        if (gone) u();
        else off = u;
      },
      () => undefined,
    );
    return () => {
      gone = true;
      off?.();
    };
  }, [name]);
}
