// Settings → Switch (docs/plan-platforms.md §4.3, S1; docs/nx-protocol.md):
// the Switch homebrew client backs up its saves through this PC over the
// local network. Off by default; pairing with a 6-digit code; the
// connection is encrypted and the Switch trusts only this PC's certificate.
// Each paired Switch can be renamed, and each of its users left out or kept
// apart from users of the same name on other Switches, so nothing but the
// pairing code is typed on the Switch.
import { useEffect, useState } from "react";
import { api, errorText, NxStatus } from "../api";
import { dateTime, relTime } from "../format";
import { useT } from "../i18n";
import { Badge, Button, Hint, Icon, Toggle, useUi } from "./ui";

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

  const rename = async (id: string, name: string) => {
    const next = await ui.prompt({ title: n.renameTitle, text: n.renameText, value: name });
    if (next && next.trim() && next.trim() !== name) run(() => api.nxRenameDevice(id, next.trim()));
  };

  return (
    <section className="panel form" id="settings-switch">
      <div className="panel-head">
        <Icon name="devices" />
        <h3>{n.title}</h3>
        <Hint text={n.intro} />
        {st?.enabled && (st.running ? <Badge tone="ok">{n.on}</Badge> : <Badge tone="warn">{n.notRunning}</Badge>)}
      </div>
      <label className="row space">
        <span>{n.allow}</span>
        <Toggle checked={!!st?.enabled} onChange={(v) => run(() => api.nxSetEnabled(v))} />
      </label>
      {st?.enabled && (
        <>
          {st.error && <div className="banner banner-warn">{st.error}</div>}
          <label>
            <span>
              {n.address} <Hint text={n.firewall} />
            </span>
            <code className="path small">{st.address ? `${st.address}:${st.port}` : `:${st.port}`}</code>
          </label>
          {st.fingerprint && (
            <label>
              <span>{n.fingerprint}</span>
              <code className="path small">{st.fingerprint.slice(0, 16).replace(/(....)/g, "$1 ").trim()}</code>
            </label>
          )}
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
                <div key={d.id} className="nx-device">
                  <div className="row space">
                    <span>
                      <b>{d.name || "Switch"}</b>{" "}
                      <span className="muted small">
                        {n.paired(dateTime(d.pairedAt))}
                        {d.lastSeen ? ` · ${n.lastSeen(relTime(d.lastSeen))}` : ""}
                      </span>
                    </span>
                    <span className="row">
                      <Button small kind="ghost" icon="edit" onClick={() => rename(d.id, d.name)}>
                        {n.rename}
                      </Button>
                      <Button small kind="ghost" icon="trash" onClick={() => run(() => api.nxRemoveDevice(d.id))}>
                        {n.remove}
                      </Button>
                    </span>
                  </div>
                  <div className="nx-users">
                    <span className="muted small">
                      {n.users} <Hint text={n.usersHint} />
                    </span>
                    {d.users.length === 0 && <span className="muted small">{n.noUsers}</span>}
                    {d.users.map((u) => (
                      <div key={u.uid} className="row space">
                        <span>
                          {u.nickname}
                          {u.sync && <span className="muted small"> · {u.line}</span>}
                        </span>
                        <select
                          className="input select"
                          value={!u.sync ? "off" : u.separate ? "separate" : "shared"}
                          onChange={(e) => run(() => api.nxSetUser(d.id, u.uid, e.target.value !== "off", e.target.value === "separate"))}
                        >
                          <option value="shared">{n.userShared}</option>
                          <option value="separate">{n.userSeparate}</option>
                          <option value="off">{n.userOff}</option>
                        </select>
                      </div>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </section>
  );
}
