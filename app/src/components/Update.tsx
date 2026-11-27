import { useState } from "react";
import { api, errorText, UpdateInfo } from "../api";
import { bytes } from "../format";
import { currentLang, useT } from "../i18n";
import { useTauriEvent } from "../useTauriEvent";
import { Button, Modal, useUi } from "./ui";

/**
 * Release notes hold both halves of the changelog ("### 中文" and
 * "### English"); show the one matching the interface language.
 */
export function localNotes(notes: string): string {
  const zh = notes.indexOf("### 中文");
  const en = notes.indexOf("### English");
  if (zh < 0 || en < 0) return notes;
  const [start, end] = currentLang() === "zh-CN" ? [zh, en > zh ? en : notes.length] : [en, zh > en ? zh : notes.length];
  return notes.slice(start, end).replace(/^### .*\n+/, "").trim();
}

export function UpdateDialog({ info, onClose }: { info: UpdateInfo; onClose: () => void }) {
  const ui = useUi();
  const t = useT();
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<{ downloaded: number; total: number | null } | null>(null);

  useTauriEvent<{ downloaded: number; total: number | null }>("update-progress", setProgress);

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
      title={t.update.title(info.version)}
      onClose={() => !installing && onClose()}
      footer={
        <>
          <Button kind="ghost" onClick={onClose} disabled={installing}>
            {t.common.later}
          </Button>
          <Button kind="primary" icon="download" onClick={install} disabled={installing}>
            {installing ? t.update.installing : t.update.install}
          </Button>
        </>
      }
    >
      <p className="muted small">{t.update.current(info.current)}</p>
      {info.notes ? <div className="release-notes">{localNotes(info.notes)}</div> : <p className="muted">{t.update.noNotes}</p>}
      {installing && (
        <div className="update-progress">
          <div className="bar">
            <span style={{ width: `${pct ?? 5}%` }} />
          </div>
          <span className="muted small">
            {progress
              ? t.update.downloaded(bytes(progress.downloaded), progress.total ? bytes(progress.total) : null)
              : t.update.connecting}
            {pct === 100 ? t.update.installingNow : ""}
          </span>
        </div>
      )}
      <p className="muted small">{t.update.note}</p>
    </Modal>
  );
}
