import { useState } from "react";
import ToastHost from "./components/Toast";
import TitleBar from "./components/TitleBar";
import LibraryPage from "./pages/LibraryPage";
import SettingsPage from "./pages/SettingsPage";
import styles from "./App.module.css";

type Page = "library" | "settings";

export default function App() {
  const [page, setPage] = useState<Page>("library");

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
