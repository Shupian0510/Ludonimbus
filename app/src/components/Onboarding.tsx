import { useCallback, useEffect, useState } from "react";
import { api, errorText, Settings } from "../api";
import { LANGS, useI18n } from "../i18n";
import { DISCORD_URL, docsUrl, GITHUB_URL } from "../links";
import { SgdbKeyForm } from "./ArtPicker";
import { CloudOptions } from "./CloudForms";
import { Button, Icon, Spinner, useUi } from "./ui";

export type Usage = "emulators" | "recomps" | "steamThirdParty" | "multiDevice";
const USAGES: { id: Usage; icon: string }[] = [
  { id: "emulators", icon: "play" },
  { id: "recomps", icon: "library" },
  { id: "steamThirdParty", icon: "steam" },
  { id: "multiDevice", icon: "devices" },
];

const STEPS = ["welcome", "usage", "cloud", "art", "done"] as const;
type Step = (typeof STEPS)[number];

/**
 * First-run guide: what Ludonimbus does, what the user wants it for, then
 * a cloud and the SteamGridDB key. Every step can be skipped; closing it
 * anywhere counts as done (it can be reopened from Settings → About).
 */
export function Onboarding({
  onFinish,
  onChanged,
  initialStep,
}: {
  /** Browser preview (docs screenshots): start at this step. */
  initialStep?: string;
  /** `next`: what to open after the guide closes. */
  onFinish: (usage: Usage[], next: "library" | "scan" | "settings") => void;
  onChanged: () => Promise<void>;
}) {
  const ui = useUi();
  const { t, lang, setPref } = useI18n();
  const o = t.onboarding;
  const [step, setStep] = useState<Step>(STEPS.find((s) => s === initialStep) ?? "welcome");
  const [usage, setUsage] = useState<Set<Usage>>(
    new Set(initialStep === "usage" ? ["emulators", "multiDevice"] : initialStep === "done" ? ["emulators", "steamThirdParty"] : []),
  );
  const [settings, setSettings] = useState<Settings | null>(null);
  const [otherCloud, setOtherCloud] = useState(false);
  const [sgdb, setSgdb] = useState<{ hasKey: boolean; keyPage: string } | null>(null);

  const loadSettings = useCallback(() => api.settings().then(setSettings, (e) => ui.toast(errorText(e), "bad")), [ui]);
  const loadSgdb = useCallback(() => api.sgdbStatus().then(setSgdb, () => setSgdb(null)), []);
  useEffect(() => {
    loadSettings();
    loadSgdb();
  }, [loadSettings, loadSgdb]);

  const index = STEPS.indexOf(step);
  const go = (d: number) => setStep(STEPS[Math.min(STEPS.length - 1, Math.max(0, index + d))]);
  const finish = (next: "library" | "scan" | "settings") => onFinish([...usage], next);
  const has = (u: Usage) => usage.has(u);

  const toggle = (u: Usage) => {
    const n = new Set(usage);
    if (n.has(u)) n.delete(u);
    else n.add(u);
    setUsage(n);
  };

  const pickLang = async (l: (typeof LANGS)[number]["id"]) => {
    try {
      await api.setLanguage(l);
      setPref(l);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  const cloudDone = async (text: string) => {
    ui.toast(text);
    setOtherCloud(false);
    await loadSettings();
    await onChanged();
  };

  const connected = settings && settings.cloud.kind !== "none";

  // Esc closes the guide like the close button.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && finish("library");
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <div className="modal-backdrop">
      <div className="modal onboarding">
        <div className="onboarding-head">
          <div className="steps" aria-label={o.progress(index + 1, STEPS.length)}>
            {STEPS.map((s, i) => (
              <span key={s} className={`dot${i === index ? " on" : i < index ? " done" : ""}`} />
            ))}
          </div>
          {step !== "done" && (
            <button className="link small" onClick={() => finish("library")}>
              {o.skipAll}
            </button>
          )}
          <button className="icon-btn" onClick={() => finish("library")} aria-label={t.common.close}>
            <Icon name="x" />
          </button>
        </div>

        <div className="onboarding-body">
          {step === "welcome" && (
            <div className="ob-welcome">
              <img src="/app-icon.svg" alt="" className="ob-logo" />
              <h2>{o.welcome.title}</h2>
              <p className="ob-lead">{o.welcome.lead}</p>
              <ul className="ob-points">
                {o.welcome.points.map((p) => (
                  <li key={p.title}>
                    <Icon name="check" size={16} />
                    <div>
                      <b>{p.title}</b>
                      <span className="muted small">{p.text}</span>
                    </div>
                  </li>
                ))}
              </ul>
              <div className="ob-lang">
                <span className="muted small">Language / 语言</span>
                {LANGS.map((l) => (
                  <button key={l.id} className={`chip${lang === l.id ? " on" : ""}`} onClick={() => pickLang(l.id)}>
                    {l.label}
                  </button>
                ))}
              </div>
            </div>
          )}

          {step === "usage" && (
            <div>
              <h2>{o.usage.title}</h2>
              <p className="muted">{o.usage.lead}</p>
              <div className="ob-usages">
                {USAGES.map((u) => (
                  <button key={u.id} className={`ob-usage${has(u.id) ? " on" : ""}`} onClick={() => toggle(u.id)} aria-pressed={has(u.id)}>
                    <span className="ob-usage-icon">
                      <Icon name={u.icon} size={22} />
                    </span>
                    <span className="ob-usage-text">
                      <b>{o.usage.items[u.id].title}</b>
                      <span className="muted small">{o.usage.items[u.id].text}</span>
                    </span>
                    <span className="ob-tick">{has(u.id) && <Icon name="check" size={16} />}</span>
                  </button>
                ))}
              </div>
            </div>
          )}

          {step === "cloud" && (
            <div>
              <h2>{o.cloud.title}</h2>
              <p className="muted">{has("multiDevice") ? o.cloud.leadMulti : o.cloud.lead}</p>
              {!settings ? (
                <Spinner />
              ) : connected && !otherCloud ? (
                <div className="banner banner-ok">
                  <Icon name="check" />
                  <span>{o.cloud.connected(t.cloudNames[settings.cloud.kind])}</span>
                  <button className="link small" onClick={() => setOtherCloud(true)}>
                    {o.cloud.other}
                  </button>
                </div>
              ) : (
                <CloudOptions settings={settings} done={cloudDone} guide={{ multiDevice: has("multiDevice") }} />
              )}
              <p className="muted small">{o.cloud.privacy}</p>
            </div>
          )}

          {step === "art" && (
            <div>
              <h2>{o.art.title}</h2>
              <p className="muted">{o.art.lead}</p>
              {!sgdb ? (
                <Spinner />
              ) : sgdb.hasKey ? (
                <div className="banner banner-ok">
                  <Icon name="check" />
                  <span>{o.art.set}</span>
                </div>
              ) : (
                <>
                  <ol className="ob-steps">
                    {o.art.steps.map((s) => (
                      <li key={s}>{s}</li>
                    ))}
                  </ol>
                  <div className="row">
                    <Button small icon="search" onClick={() => api.openPath(sgdb.keyPage)}>
                      {o.art.openPage}
                    </Button>
                  </div>
                  <SgdbKeyForm keyPage={sgdb.keyPage} onSaved={loadSgdb} />
                </>
              )}
              <p className="muted small">{o.art.optional}</p>
            </div>
          )}

          {step === "done" && (
            <div>
              <h2>{o.done.title}</h2>
              <ul className="ob-summary">
                <li className={connected ? "ok" : ""}>
                  <Icon name={connected ? "check" : "x"} size={16} />
                  {connected && settings ? o.done.cloudOk(t.cloudNames[settings.cloud.kind]) : o.done.cloudSkipped}
                </li>
                <li className={sgdb?.hasKey ? "ok" : ""}>
                  <Icon name={sgdb?.hasKey ? "check" : "x"} size={16} />
                  {sgdb?.hasKey ? o.done.artOk : o.done.artSkipped}
                </li>
              </ul>
              <p className="muted">{o.done.next}</p>
              <ul className="ob-tips">
                {has("emulators") && <li>{o.done.tips.emulators}</li>}
                {has("recomps") && <li>{o.done.tips.recomps}</li>}
                {has("steamThirdParty") && (
                  <li>
                    {o.done.tips.steamThirdParty}{" "}
                    <button className="link small" onClick={() => finish("settings")}>
                      {o.done.openSettings}
                    </button>
                  </li>
                )}
                {has("multiDevice") && <li>{o.done.tips.multiDevice}</li>}
                <li>
                  {o.done.tips.docs}{" "}
                  <button className="link small" onClick={() => api.openPath(docsUrl(lang))}>
                    {o.done.openDocs}
                  </button>
                </li>
              </ul>
              <div className="ob-support">
                <div>
                  <b>{o.done.supportTitle}</b>
                  <span className="muted small">{o.done.supportText}</span>
                </div>
                <div className="row">
                  <Button small icon="star" onClick={() => api.openPath(GITHUB_URL)}>
                    {o.done.star}
                  </Button>
                  {DISCORD_URL && (
                    <Button small kind="ghost" onClick={() => api.openPath(DISCORD_URL)}>
                      {o.done.discord}
                    </Button>
                  )}
                </div>
              </div>
            </div>
          )}
        </div>

        <div className="modal-foot onboarding-foot">
          {index > 0 && step !== "done" && (
            <Button kind="ghost" onClick={() => go(-1)}>
              {o.back}
            </Button>
          )}
          <span className="grow" />
          {step === "welcome" && (
            <Button kind="primary" onClick={() => go(1)}>
              {o.start}
            </Button>
          )}
          {(step === "usage" || step === "cloud" || step === "art") && (
            <>
              {!(step === "cloud" && connected) && !(step === "art" && sgdb?.hasKey) && step !== "usage" && (
                <Button kind="ghost" onClick={() => go(1)}>
                  {o.skipStep}
                </Button>
              )}
              <Button kind="primary" onClick={() => go(1)}>
                {o.next}
              </Button>
            </>
          )}
          {step === "done" && (
            <>
              <Button onClick={() => finish("library")}>{o.done.enter}</Button>
              <Button kind="primary" icon="search" onClick={() => finish("scan")}>
                {o.done.scan}
              </Button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
