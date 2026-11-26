// Settings → Handheld (docs/plan-phase5.md §2): full-screen questions when a
// game starts, Ludonimbus in the Steam library (where there is no tray),
// and the handheld view itself.
import { useEffect, useState } from "react";
import { api, errorText, HandheldInfo, HandheldMode } from "../api";
import { msg, useT } from "../i18n";
import { withSteamClosed } from "./steamAction";
import { Badge, Button, Icon, useUi } from "./ui";

export function HandheldPanel({ onHandheld }: { onHandheld?: () => void }) {
  const t = useT();
  const h = t.handheld;
  const ui = useUi();
  const [info, setInfo] = useState<HandheldInfo | null>(null);
  const load = () => api.handheldInfo().then(setInfo, () => undefined);
  useEffect(() => {
    load();
  }, []);

  const setMode = async (m: HandheldMode) => {
    try {
      await api.setHandheldMode(m);
      await load();
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const addToSteam = async () => {
    try {
      const a = await api.steamAccounts();
      if (!a.selected) {
        ui.toast(msg().handheld.steamNone, "bad");
        return;
      }
      const done = await withSteamClosed(ui, (close) => api.steamAddLudonimbus(a.selected!, close));
      if (done !== null) {
        ui.toast(msg().handheld.steamAdded);
        await load();
      }
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <section className="panel form" id="settings-handheld">
      <div className="panel-head">
        <Icon name="devices" />
        <h3>{h.panelTitle}</h3>
        {info?.device && <Badge tone="info">{info.device}</Badge>}
      </div>
      <p className="muted small">{info?.device ? h.detected(info.device) : h.panelText}</p>
      <label>
        <span>{h.promptsTitle}</span>
        <select className="input select" value={info?.mode ?? "auto"} onChange={(e) => setMode(e.target.value as HandheldMode)}>
          {(["auto", "on", "off"] as HandheldMode[]).map((m) => (
            <option key={m} value={m}>
              {h.modes[m]}
            </option>
          ))}
        </select>
      </label>
      <label>
        <span>{h.steamTitle}</span>
        <div className="row">
          {info?.inSteam ? <Badge tone="ok">{h.steamIn}</Badge> : <Badge tone="muted">{h.steamNot}</Badge>}
          <Button small icon="steam" onClick={addToSteam}>
            {info?.inSteam ? h.steamUpdate : h.steamAdd}
          </Button>
        </div>
      </label>
      <p className="muted small">{h.xboxHow}</p>
      {onHandheld && (
        <div className="row">
          <Button small kind="primary" icon="play" onClick={onHandheld}>
            {h.enter}
          </Button>
        </div>
      )}
    </section>
  );
}
