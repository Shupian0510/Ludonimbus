import { useEffect, useState } from "react";
import { api, DeviceInfo, errorText, Overview } from "../api";
import { useT } from "../i18n";
import { kindOf, learnDeviceKinds, Pill, Stat, When } from "./kit";
import { Empty, Hint, Icon, Spinner } from "./ui";

/** The system a device record names ("windows x86_64", "linux x86_64", "switch"). */
function osName(os: string): string {
  const o = os.toLowerCase();
  if (o.startsWith("windows")) return "Windows";
  if (o.startsWith("linux")) return "Linux";
  if (o.startsWith("macos")) return "macOS";
  if (o.startsWith("switch")) return "Switch";
  return os.split(" ")[0] ?? os;
}

export function Devices({ overview }: { overview: Overview | null }) {
  const t = useT();
  const [devices, setDevices] = useState<DeviceInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!overview || overview.cloud.kind === "none") return;
    api.devices().then(
      (list) => {
        setDevices(list);
        learnDeviceKinds(list);
      },
      (e) => setError(errorText(e)),
    );
  }, [overview]);

  // A device whose Ludonimbus knows an older cloud layout than this one's.
  const here = devices?.find((d) => d.id === overview?.deviceId)?.layout ?? 0;
  const outdated = (d: DeviceInfo) => (d.layout ?? 0) < here;

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
          {devices.map((d) => {
            const mine = d.id === overview?.deviceId;
            const games = new Set((d.installs ?? []).map((i) => i.game)).size;
            return (
              <div key={d.id} className={`panel device-card${mine ? " mine" : ""}`}>
                <span className="device-tile">
                  <Icon name={`dev-${kindOf(d.name, d.os, d.kind)}`} size={30} />
                </span>
                <div className="device-body">
                  <div className="device-title">
                    <b>{d.name}</b>
                    {mine && <Pill tone="info">{t.devices.thisDevice}</Pill>}
                    {outdated(d) && (
                      <>
                        <Pill tone="warn">{t.devices.outdated}</Pill>
                        <Hint text={t.devices.outdatedHint} />
                      </>
                    )}
                  </div>
                  <div className="device-facts">
                    <Stat icon="clock" title={t.devices.lastSeen}>
                      <When at={d.lastSeen} />
                    </Stat>
                    {games > 0 && (
                      <Stat icon="library" title={t.devices.gamesTitle}>
                        {games}
                      </Stat>
                    )}
                    <span className="version-tag" title={t.devices.version}>
                      v{d.appVersion}
                    </span>
                    <span className="device-os">{osName(d.os)}</span>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
