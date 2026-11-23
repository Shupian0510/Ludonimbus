import { useEffect, useState } from "react";
import { api, DeviceInfo, errorText, Overview } from "../api";
import { dateTime, relTime } from "../format";
import { useT } from "../i18n";
import { Badge, Empty, Icon, Spinner } from "./ui";

export function Devices({ overview }: { overview: Overview | null }) {
  const t = useT();
  const [devices, setDevices] = useState<DeviceInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!overview || overview.cloud.kind === "none") return;
    api.devices().then(setDevices, (e) => setError(errorText(e)));
  }, [overview]);

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>{t.devices.title}</h1>
          <p className="muted">{t.devices.subtitle}</p>
        </div>
      </header>
      {overview?.cloud.kind === "none" ? (
        <Empty icon="cloud" title={t.devices.noCloud} />
      ) : error ? (
        <div className="banner banner-bad">{error}</div>
      ) : !devices ? (
        <div className="center pad">
          <Spinner />
        </div>
      ) : (
        <div className="device-list">
          {devices.map((d) => (
            <div key={d.id} className="panel device-card">
              <Icon name="devices" size={26} />
              <div>
                <b>{d.name}</b> {d.id === overview?.deviceId && <Badge tone="info">{t.devices.thisDevice}</Badge>}
                <div className="muted small">
                  {t.devices.lastSeen(relTime(d.lastSeen), dateTime(d.lastSeen))} · Ludonimbus {d.appVersion} · {d.os}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
