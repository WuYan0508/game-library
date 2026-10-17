import { useEffect, useRef, useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { checkForUpdate, downloadAndInstallUpdate } from "../services/updater";
import type { Update } from "@tauri-apps/plugin-updater";
import { toUserMessage } from "../services/errorMessages";
import styles from "../pages/SettingsPage.module.css";

type UpdateState =
  | "idle"
  | "checking"
  | "uptodate"
  | "available"
  | "downloading"
  | "installing"
  | "error";

export default function UpdateChecker() {
  const [state, setState] = useState<UpdateState>("idle");
  const [newVersion, setNewVersion] = useState<string | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Update 对象必须在回调间保持同一实例（下载依赖它），用 ref 持有
  const updateRef = useRef<Update | null>(null);

  useEffect(() => {
    return () => {
      updateRef.current = null;
    };
  }, []);

  async function handleCheck() {
    setState("checking");
    setError(null);
    try {
      const found = await checkForUpdate();
      updateRef.current = found;
      if (found) {
        setNewVersion(found.version);
        setState("available");
      } else {
        setState("uptodate");
      }
    } catch (err) {
      setState("error");
      setError(toUserMessage(err));
    }
  }

  async function handleInstall() {
    const found = updateRef.current;
    if (!found) {
      return;
    }
    setState("downloading");
    setProgress(null);
    setError(null);
    try {
      await downloadAndInstallUpdate(found, (percent, stage) => {
        setProgress(percent);
        if (stage === "installing") {
          setState("installing");
        }
      });
      // 安装完成后重启进入新版本
      await relaunch();
    } catch (err) {
      setState("error");
      setError(toUserMessage(err));
    }
  }

  return (
    <div className={styles.updateArea}>
      {state === "idle" && (
        <button className={styles.actionButton} type="button" onClick={() => void handleCheck()}>
          检查更新
        </button>
      )}

      {state === "checking" && <span className={styles.updateHint}>检查中…</span>}

      {state === "uptodate" && (
        <span className={styles.updateHint}>当前已是最新版本</span>
      )}

      {state === "available" && (
        <>
          <span className={styles.updateFound}>
            发现新版本 {newVersion}
          </span>
          <button
            className={styles.actionButton}
            type="button"
            onClick={() => void handleInstall()}
          >
            下载并安装
          </button>
        </>
      )}

      {state === "downloading" && (
        <span className={styles.updateHint}>
          下载中{progress !== null ? ` ${progress}%` : "…"}
        </span>
      )}

      {state === "installing" && (
        <span className={styles.updateHint}>安装中，完成后将自动重启…</span>
      )}

      {state === "error" && (
        <>
          <span className={styles.updateError}>{error ?? "检查失败"}</span>
          <button className={styles.actionButton} type="button" onClick={() => void handleCheck()}>
            重试
          </button>
        </>
      )}
    </div>
  );
}
