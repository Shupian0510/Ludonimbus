import { errorText } from "../api";
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
    const ok = await ui.confirm({
      title: "需要暂时关闭 Steam",
      text: "Steam 运行时会覆盖对快捷方式的修改。Ludonimbus 会先关闭 Steam，改好后再自动重新打开。",
      confirm: "关闭 Steam 并继续",
    });
    if (!ok) return null;
    return await f(true);
  }
}
