import { useEffect, useState } from "react";
import ToastHost from "./components/Toast";
import { pushToast } from "./services/toastStore";
import TitleBar from "./components/TitleBar";
import LibraryPage from "./pages/LibraryPage";
import SettingsPage from "./pages/SettingsPage";
import styles from "./App.module.css";
import { checkForUpdate } from "./services/updater";

type Page = "library" | "settings";

export default function App() {
  const [page, setPage] = useState<Page>("library");

  // 启动 3 秒后静默检查一次更新（发现新版才提示，失败静默）
  useEffect(() => {
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          const update = await checkForUpdate();
          if (update) {
            pushToast(
              `发现新版本 ${update.version}：设置 → 检查更新 可直接升级`,
            );
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
        <ToastHost />
      </main>
    </div>
  );
}
