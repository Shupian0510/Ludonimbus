import { useEffect, useState } from "react";
import { api, GameFolder } from "../api";

let cache: GameFolder[] | null = null;

/**
 * Shortcuts to the folders game stores install into (Steam libraries, Xbox,
 * Epic, GOG, EA, Ubisoft): each opens the exe picker there.
 */
export function PlatformFolders({ onPick }: { onPick: (exe: string) => void }) {
  const [folders, setFolders] = useState<GameFolder[]>(cache ?? []);
  useEffect(() => {
    if (cache) return;
    api.gameFolders().then(
      (list) => {
        cache = list;
        setFolders(list);
      },
      () => setFolders([]),
    );
  }, []);
  if (folders.length === 0) return null;
  return (
    <div className="preset-row">
      <span className="muted small">从游戏平台的安装目录选：</span>
      {folders.map((f) => (
        <button
          key={f.path}
          className="chip"
          title={f.path}
          onClick={async () => {
            const exe = await api.pickExe(f.path);
            if (exe) onPick(exe);
          }}
        >
          {f.label}
        </button>
      ))}
      {folders.some((f) => f.label.startsWith("Xbox")) && (
        <span className="muted small">Xbox 游戏请选游戏文件夹 Content 里的 gamelaunchhelper.exe（游戏自己的程序受保护，选不了）。</span>
      )}
    </div>
  );
}
