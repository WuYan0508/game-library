import { useEffect, useState } from "react";
import ToastHost from "./components/Toast";
import TitleBar from "./components/TitleBar";
import UpdatePromptDialog from "./components/UpdatePromptDialog";
import GamepadHintBar from "./components/GamepadHintBar";
import LibraryPage from "./pages/LibraryPage";
import SettingsPage from "./pages/SettingsPage";
import styles from "./App.module.css";
import { checkForUpdate } from "./services/updater";
import type { Update } from "@tauri-apps/plugin-updater";

type Page = "library" | "settings";

export default function App() {
  const [page, setPage] = useState<Page>("library");
  // 启动静默检查发现的新版本（弹窗展示，弹窗内直接升级）
  const [pendingUpdate, setPendingUpdate] = useState<Update | null>(null);

  // 启动 3 秒后静默检查一次更新（发现新版弹窗提示，失败静默）
  useEffect(() => {
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          const update = await checkForUpdate();
          if (update) {
            setPendingUpdate(update);
          }
        } catch {
          // 静默失败：未发布新版本 / 无网络 / 端点不可达都不打扰用户
        }
      })();
    }, 3000);
    return () => window.clearTimeout(timer);
  }, []);

  return (
    <div className={styles.shell}>
      <TitleBar />
      <main className={styles.content}>
        {page === "settings" ? (
          <SettingsPage onBack={() => setPage("library")} />
        ) : (
          <LibraryPage onOpenSettings={() => setPage("settings")} />
        )}
        {pendingUpdate && (
          <UpdatePromptDialog
            update={pendingUpdate}
            onClose={() => setPendingUpdate(null)}
          />
        )}
        <GamepadHintBar />
        <ToastHost />
      </main>
    </div>
  );
}
