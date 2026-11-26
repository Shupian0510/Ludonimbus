import { useEffect, useState } from "react";
import { api, errorText, SteamAccount } from "../api";
import { msg, useT } from "../i18n";
import { withSteamClosed } from "./steamAction";
import { Button, Hint, Icon, Spinner, Toggle, useUi } from "./ui";

export interface SteamCandidate {
  key: string;
  name: string;
}

/**
 * Create Steam shortcuts for games that are not in Steam yet. They start
 * through Ludonimbus, so saves sync automatically from the first launch.
 */
export function AddToSteam({ games, onDone, onCancel }: { games: SteamCandidate[]; onDone: (added: string[]) => void; onCancel?: () => void }) {
  const ui = useUi();
  const t = useT();
  const [accounts, setAccounts] = useState<SteamAccount[] | null>(null);
  const [account, setAccount] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hasKey, setHasKey] = useState(false);
  const [autoArt, setAutoArt] = useState(true);
  const [failed, setFailed] = useState<{ key: string; error: string }[]>([]);
  const [done, setDone] = useState<string[]>([]);

  useEffect(() => {
    api.steamAccounts().then(
      (r) => {
        setAccounts(r.accounts);
        setAccount(r.selected);
        if (!r.root) setError(msg().addToSteam.noSteam);
        else if (r.accounts.length === 0) setError(msg().addToSteam.noAccount);
      },
      (e) => setError(errorText(e)),
    );
    api.sgdbStatus().then((s) => setHasKey(s.hasKey), () => setHasKey(false));
  }, []);

  const run = async () => {
    if (!account) return;
    try {
      // A retry only sends the games that failed last time.
      const keys = failed.length ? failed.map((f) => f.key) : games.map((g) => g.key);
      const results = await withSteamClosed(ui, (close) => api.steamAddGames(keys, account, hasKey && autoArt, close));
      if (!results) return;
      const ok = results.filter((r) => !r.error).map((r) => r.key);
      const bad = results.filter((r) => r.error).map((r) => ({ key: r.key, error: r.error! }));
      const all = [...done, ...ok];
      setDone(all);
      setFailed(bad);
      if (ok.length) ui.toast(ok.length === 1 ? msg().addToSteam.addedOne(games.find((g) => g.key === ok[0])?.name ?? "") : msg().addToSteam.addedMany(ok.length));
      if (bad.length === 0) onDone(all);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  if (error) return <div className="banner banner-bad">{error}</div>;
  if (!accounts) return <Spinner />;

  return (
    <div className="steam-add">
      <p>
        {t.addToSteam.intro(games.length)} <Hint text={t.addToSteam.introHint} />
      </p>
      {games.length > 1 && (
        <ul className="steam-add-list">
          {games.map((g) => (
            <li key={g.key}>
              {g.name}
              {failed.find((f) => f.key === g.key) && <span className="bad small"> — {failed.find((f) => f.key === g.key)!.error}</span>}
            </li>
          ))}
        </ul>
      )}
      {games.length === 1 && failed[0] && <div className="health health-warn small">{failed[0].error}</div>}
      <div className="form compact">
        {accounts.length > 1 && (
          <label>
            <span>{t.addToSteam.account}</span>
            <select className="input select" value={account ?? ""} onChange={(e) => setAccount(e.target.value)}>
              {accounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.persona ?? a.accountName ?? a.id}
                </option>
              ))}
            </select>
          </label>
        )}
        <div className="row space">
          <span>
            {t.addToSteam.autoArt} <Hint text={hasKey ? t.addToSteam.autoArtOn : t.addToSteam.autoArtNoKey} />
          </span>
          <Toggle checked={hasKey && autoArt} onChange={setAutoArt} disabled={!hasKey} />
        </div>
      </div>
      <div className="row end">
        {onCancel && <Button onClick={done.length ? () => onDone(done) : onCancel}>{failed.length ? t.common.close : t.common.later}</Button>}
        <Button kind="primary" icon="steam" onClick={run} disabled={!account} busyText={t.addToSteam.adding}>
          {failed.length ? t.common.retry : t.addToSteam.add}
        </Button>
      </div>
      {failed.length > 0 && (
        <p className="muted small">
          <Icon name="warn" size={14} /> {t.addToSteam.someFailed(failed.length)}
        </p>
      )}
    </div>
  );
}
