import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { api, errorText, inTauri, UpdateInfo } from "../api";
import { bytes } from "../format";
import { Button, Modal, useUi } from "./ui";

export function UpdateDialog({ info, onClose }: { info: UpdateInfo; onClose: () => void }) {
  const ui = useUi();
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<{ downloaded: number; total: number | null } | null>(null);

  useEffect(() => {
    if (!inTauri) return;
    let un: (() => void) | undefined;
    listen<{ downloaded: number; total: number | null }>("update-progress", (e) => setProgress(e.payload)).then((u) => (un = u));
    return () => un?.();
  }, []);

  const install = async () => {
    setInstalling(true);
    try {
      await api.installUpdate();
    } catch (e) {
      ui.toast(errorText(e), "bad");
      setInstalling(false);
    }
  };

  const pct = progress?.total ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100)) : null;

  return (
    <Modal
      title={`新版本 ${info.version} 可用`}
      onClose={() => !installing && onClose()}
      footer={
        <>
          <Button kind="ghost" onClick={onClose} disabled={installing}>
            以后再说
          </Button>
          <Button kind="primary" icon="download" onClick={install} disabled={installing}>
            {installing ? "正在更新…" : "更新并重启"}
          </Button>
        </>
      }
    >
      <p className="muted small">当前版本 {info.current}</p>
      {info.notes ? <div className="release-notes">{info.notes}</div> : <p className="muted">这个版本没有附更新说明。</p>}
      {installing && (
        <div className="update-progress">
          <div className="bar">
            <span style={{ width: `${pct ?? 5}%` }} />
          </div>
          <span className="muted small">
            {progress
              ? `已下载 ${bytes(progress.downloaded)}${progress.total ? ` / ${bytes(progress.total)}` : ""}`
              : "正在连接…"}
            {pct === 100 ? " · 正在安装，完成后会自动重启" : ""}
          </span>
        </div>
      )}
      <p className="muted small">更新会关闭 Ludonimbus，安装完成后自动重新打开。安装包经过签名校验，存档和设置不受影响。</p>
    </Modal>
  );
}
