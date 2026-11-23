import { errorText } from "../api";
import { msg } from "../i18n";
import { useUi } from "./ui";

type Ui = ReturnType<typeof useUi>;

/**
 * Run a change to Steam's shortcuts. If Steam is running, ask once and let
 * the backend close and reopen it. Resolves to null when the user declines.
 */
export async function withSteamClosed<T>(ui: Ui, f: (closeSteam: boolean) => Promise<T>): Promise<T | null> {
  try {
    return await f(false);
  } catch (e) {
    if (!errorText(e).includes("STEAM_RUNNING")) throw e;
    const m = msg().steamAction;
    const ok = await ui.confirm({ title: m.title, text: m.text, confirm: m.confirm });
    if (!ok) return null;
    return await f(true);
  }
}
