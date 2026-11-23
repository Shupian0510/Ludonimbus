import { useEffect, useState } from "react";
import { api, errorText, SteamAccount } from "../api";
import { withSteamClosed } from "./steamAction";
import { Button, Icon, Spinner, Toggle, useUi } from "./ui";

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
        if (!r.root) setError("没有找到 Steam");
        else if (r.accounts.length === 0) setError("Steam 里还没有登录过的账号");
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
      if (ok.length) ui.toast(ok.length === 1 ? `已加入 Steam：${games.find((g) => g.key === ok[0])?.name}` : `已把 ${ok.length} 个游戏加入 Steam`);
      if (bad.length === 0) onDone(all);
    } catch (e) {
      ui.toast(errorText(e), "bad");
    }
  };

  if (error) return <div className="banner banner-bad">{error}</div>;
  if (!accounts) return <Spinner />;

  return (
    <div className="steam-add">
      <p className="muted small">
        在 Steam 里创建{games.length > 1 ? "这些游戏的" : "这个游戏的"}「非 Steam 游戏」快捷方式。从 Steam 启动时会先经过 Ludonimbus：开始前拉取云端存档，退出后上传，Steam 里的游戏时长、截图和手柄设置照常可用。
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
            <span>Steam 账号</span>
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
          <div>
            <b>自动配封面</b>
            <p className="muted small">
              {hasKey ? "从 SteamGridDB 选评分最高的封面、背景、Logo 和图标，已经选过的不会被替换。" : "需要先在设置里填写 SteamGridDB API Key。之后也可以在游戏详情页里自己挑。"}
            </p>
          </div>
          <Toggle checked={hasKey && autoArt} onChange={setAutoArt} disabled={!hasKey} />
        </div>
      </div>
      <div className="row end">
        {onCancel && <Button onClick={done.length ? () => onDone(done) : onCancel}>{failed.length ? "关闭" : "以后再说"}</Button>}
        <Button kind="primary" icon="steam" onClick={run} disabled={!account} busyText="正在加入…">
          {failed.length ? "重试" : "加入 Steam"}
        </Button>
      </div>
      {failed.length > 0 && (
        <p className="muted small">
          <Icon name="warn" size={14} /> 有 {failed.length} 个没能加入，其他的已经加好了。
        </p>
      )}
    </div>
  );
}
