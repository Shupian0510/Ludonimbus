import { useEffect, useState } from "react";
import { api, DeviceInfo, errorText, Overview } from "../api";
import { dateTime, relTime } from "../format";
import { Badge, Empty, Icon, Spinner } from "./ui";

export function Devices({ overview }: { overview: Overview | null }) {
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
          <h1>设备</h1>
          <p className="muted">连接到同一个云盘的电脑。每个版本都会记录是哪台设备上传的。</p>
        </div>
      </header>
      {overview?.cloud.kind === "none" ? (
        <Empty icon="cloud" title="还没有连接云盘" />
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
                <b>{d.name}</b> {d.id === overview?.deviceId && <Badge tone="info">本机</Badge>}
                <div className="muted small">
                  最后在线 {relTime(d.lastSeen)}（{dateTime(d.lastSeen)}） · Ludonimbus {d.appVersion} · {d.os}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
