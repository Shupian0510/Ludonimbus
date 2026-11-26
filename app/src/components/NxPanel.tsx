// Settings → Switch (docs/plan-platforms.md §4.3, S1): the Switch homebrew
// client syncs its saves through this PC over the local network. Off by
// default; pairing with a 6-digit code; the connection is encrypted and the
// Switch trusts only this PC's certificate.
import { useEffect, useState } from "react";
import { api, errorText, NxStatus } from "../api";
import { dateTime, relTime } from "../format";
import { useT } from "../i18n";
import { Badge, Button, Icon, Toggle, useUi } from "./ui";

export function NxPanel() {
  const t = useT();
  const n = t.nx;
  const ui = useUi();
  const [st, setSt] = useState<NxStatus | null>(null);

  const load = () => api.nxStatus().then(setSt, () => undefined);
  useEffect(() => {
    load();
  }, []);
  // While a code is shown, see when the Switch has paired.
  useEffect(() => {
    if (!st?.pairingCode) return;
    const id = setInterval(load, 2000);
    return () => clearInterval(id);
  }, [st?.pairingCode]);

  const run = async (f: () => Promise<NxStatus>) => {
    try {
      setSt(await f());
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  return (
    <section className="panel form" id="settings-switch">
      <div className="panel-head">
        <Icon name="devices" />
        <h3>{n.title}</h3>
        {st?.enabled && (st.running ? <Badge tone="ok">{n.on}</Badge> : <Badge tone="warn">{n.notRunning}</Badge>)}
      </div>
      <p className="muted small">{n.intro}</p>
      <label className="row space">
        <span>{n.allow}</span>
        <Toggle checked={!!st?.enabled} onChange={(v) => run(() => api.nxSetEnabled(v))} />
      </label>
      {st?.enabled && (
        <>
          {st.error && <div className="banner banner-warn">{st.error}</div>}
          <label>
            <span>{n.address}</span>
            <code className="path small">{st.address ? `${st.address}:${st.port}` : `:${st.port}`}</code>
          </label>
          {st.fingerprint && (
            <label>
              <span>{n.fingerprint}</span>
              <code className="path small">{st.fingerprint.slice(0, 16).replace(/(....)/g, "$1 ").trim()}</code>
            </label>
          )}
          <p className="muted small">{n.firewall}</p>
          {st.pairingCode ? (
            <div className="banner banner-info nx-code">
              <div>
                <b>{n.codeTitle}</b>
                <p className="nx-digits">{st.pairingCode}</p>
                <p className="muted small">{n.codeText}</p>
              </div>
              <Button small onClick={() => run(api.nxCancelPairing)}>
                {t.common.cancel}
              </Button>
            </div>
          ) : (
            <div className="row">
              <Button small kind="primary" icon="plus" onClick={() => run(api.nxStartPairing)}>
                {n.pair}
              </Button>
            </div>
          )}
          {st.devices.length > 0 && (
            <div className="nx-devices">
              {st.devices.map((d) => (
                <div key={d.id} className="row space">
                  <span>
                    <b>{d.name || "Switch"}</b>{" "}
                    <span className="muted small">
                      {n.paired(dateTime(d.pairedAt))}
                      {d.lastSeen ? ` · ${n.lastSeen(relTime(d.lastSeen))}` : ""}
                    </span>
                  </span>
                  <Button small kind="ghost" icon="trash" onClick={() => run(() => api.nxRemoveDevice(d.id))}>
                    {n.remove}
                  </Button>
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </section>
  );
}
