import { useRef, useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { type Update } from "@tauri-apps/plugin-updater";
import { downloadAndInstallUpdate } from "../services/updater";
import { toUserMessage } from "../services/errorMessages";
import { useGamepadDialog } from "../hooks/useGamepadDialog";
import styles from "./UpdatePromptDialog.module.css";

type Phase = "prompt" | "downloading" | "installing" | "error";

/**
 * 启动时检测到新版本的升级弹窗：
 * 弹窗内直接下载安装并重启，无需前往设置页。
 */
export default function UpdatePromptDialog({
  update,
  onClose,
}: {
  update: Update;
  onClose: () => void;
}) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const [phase, setPhase] = useState<Phase>("prompt");
  const [progress, setProgress] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleInstall() {
    setPhase("downloading");
    setProgress(null);
    setError(null);
    try {
      await downloadAndInstallUpdate(update, (percent, stage) => {
        setProgress(percent);
        if (stage === "installing") {
          setPhase("installing");
        }
      });
      await relaunch();
    } catch (err) {
      setPhase("error");
      setError(toUserMessage(err));
    }
  }

  const busy = phase === "downloading" || phase === "installing";
  useGamepadDialog(dialogRef, {
    onBack: onClose,
    // 下载/安装进行中禁用手柄操作，防止误关弹窗
    enabled: !busy,
    hints: [
      { key: "A", label: "立即更新" },
      { key: "B", label: "稍后再说" },
    ],
  });

  return (
    <div className={styles.backdrop} role="presentation">
      <div
        ref={dialogRef}
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-label="发现新版本"
      >
        <div className={styles.header}>
          <h2 className={styles.title}>
            发现新版本 <span className={styles.versionTag}>v{update.version}</span>
          </h2>
          {!busy && (
            <button
              className={styles.closeButton}
              type="button"
              onClick={onClose}
              aria-label="稍后再说"
            >
              ✕
            </button>
          )}
        </div>

        <div className={styles.body}>
          {update.body ? (
            <p className={styles.notes}>{update.body}</p>
          ) : (
            <p className={styles.notes}>本次更新包含功能改进与体验优化。</p>
          )}

          {phase === "downloading" && (
            <p className={styles.statusText}>
              下载中{progress !== null ? ` ${progress}%` : "…"}
            </p>
          )}
          {phase === "installing" && (
            <p className={styles.statusText}>安装中，完成后将自动重启…</p>
          )}
          {phase === "error" && (
            <p className={styles.errorText}>{error ?? "更新失败"}</p>
          )}
        </div>

        <div className={styles.footer}>
          <button
            className={styles.secondaryButton}
            type="button"
            onClick={onClose}
            disabled={busy}
          >
            稍后再说
          </button>
          <button
            className={styles.primaryButton}
            type="button"
            onClick={() => void handleInstall()}
            disabled={busy}
          >
            {phase === "downloading"
              ? "下载中…"
              : phase === "installing"
                ? "安装中…"
                : phase === "error"
                  ? "重试"
                  : "立即更新"}
          </button>
        </div>
      </div>
    </div>
  );
}
