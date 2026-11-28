import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { api, LaunchInfo, PlatformMark, PromptRequest } from "./api";
import App from "./App";
import { AchievementToast } from "./components/Achievements";
import { UiProvider } from "./components/ui";
import { Handheld } from "./handheld/Handheld";
import { Prompt } from "./handheld/Prompt";
import { asPref, I18nProvider, LangPref } from "./i18n";
import { useTauriEvent } from "./useTauriEvent";
import "./styles.css";

/** The handheld view and the launch questions are zoomed to the screen (handheld.rs). */
function useHandheldZoom(on: boolean) {
  useEffect(() => {
    api.handheldZoom(on).catch(() => undefined);
  }, [on]);
}

/** The desktop view or the handheld view (the window can switch between them). */
function Shell({ start }: { start: "desktop" | "handheld" }) {
  const [view, setView] = useState(start);
  useHandheldZoom(view === "handheld");
  // Opened again from the Xbox full screen experience or Steam Big Picture.
  useTauriEvent("show-handheld", () => setView("handheld"));
  if (view === "handheld") return <Handheld onDesktop={() => setView("desktop")} />;
  return (
    <App
      onHandheld={() => {
        api.setFullscreen(true).catch(() => undefined);
        setView("handheld");
      }}
    />
  );
}

function Question({ request, platforms }: { request: PromptRequest; platforms: Record<string, PlatformMark> }) {
  // Full screen it is zoomed to the screen; a window on the desktop isn't.
  useHandheldZoom(!request.dialog);
  return <Prompt request={request} platforms={platforms} />;
}

function render(lang: LangPref, launch: LaunchInfo) {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <I18nProvider initial={lang}>
        <UiProvider>{launch.mode === "prompt" && launch.prompt ? <Question request={launch.prompt} platforms={launch.platforms ?? {}} /> : <Shell start={launch.mode === "handheld" ? "handheld" : "desktop"} />}</UiProvider>
      </I18nProvider>
    </React.StrictMode>,
  );
}

// index.html?toast: the small achievement popup window (achievements.rs).
const toastWindow = new URLSearchParams(location.search).has("toast");

if (toastWindow) {
  ReactDOM.createRoot(document.getElementById("root")!).render(<AchievementToast />);
} else {
  // Read the saved language first so the window never flashes the other one.
  const desktop: LaunchInfo = { mode: "desktop", prompt: null, platforms: {} };
  Promise.all([api.language().then(asPref, () => "system" as LangPref), api.launchMode().catch(() => desktop)]).then(([lang, launch]) =>
    render(lang, launch),
  );
}
