import { useState } from "react";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import styles from "./WarmupDialog.module.css";

interface WarmupGame {
  id: string;
  title: string;
  description: string;
  /** 独立窗口的 label（单实例复用）。 */
  windowLabel: string;
  /** 相对应用源的资源路径。 */
  url: string;
  width: number;
  height: number;
}

/** 赛前热身小游戏清单 —— 新游戏在这里追加即可。 */
const WARMUP_GAMES: WarmupGame[] = [
  {
    id: "reaction",
    title: "反应力测试挑战",
    description: "屏幕变绿后尽快点击，测测你的反应速度。支持三档难度与成绩统计。",
    windowLabel: "warmup-reaction",
    url: "/games/reaction/index.html",
    width: 760,
    height: 900,
  },
];

async function openGameWindow(game: WarmupGame) {
  // 单实例：窗口已存在就聚焦置前
  const existing = await WebviewWindow.getByLabel(game.windowLabel);
  if (existing) {
    await existing.show();
    await existing.setFocus();
    return;
  }
  const win = new WebviewWindow(game.windowLabel, {
    url: game.url,
    title: game.title,
    width: game.width,
    height: game.height,
    center: true,
    resizable: true,
    minWidth: 560,
    minHeight: 640,
  });
  win.once("tauri://error", (event) => {
    console.error("打开热身游戏窗口失败", event);
  });
}

export default function WarmupDialog({ onClose }: { onClose: () => void }) {
  const [openingId, setOpeningId] = useState<string | null>(null);

  async function handleOpen(game: WarmupGame) {
    setOpeningId(game.id);
    try {
      await openGameWindow(game);
    } finally {
      setOpeningId(null);
    }
  }

  return (
    <div className={styles.backdrop} role="presentation">
      <div
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-label="赛前热身"
      >
        <header className={styles.header}>
          <h2 className={styles.title}>赛前热身</h2>
          <button
            className={styles.close}
            type="button"
            aria-label="关闭"
            onClick={onClose}
          >
            ✕
          </button>
        </header>

        <div className={styles.body}>
          <p className={styles.hint}>开黑前活动一下手感 —— 热身游戏在独立小窗打开，不影响游戏库。</p>
          <ul className={styles.gameList}>
            {WARMUP_GAMES.map((game) => (
              <li key={game.id}>
                <button
                  type="button"
                  className={styles.gameItem}
                  disabled={openingId !== null}
                  onClick={() => void handleOpen(game)}
                >
                  <span className={styles.gameIcon} aria-hidden="true">
                    <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M6 12h4M8 10v4M15 11h.01M17.5 13.5h.01" />
                      <path d="M17.3 5H6.7a4.7 4.7 0 0 0-4.55 5.86l1.2 4.8A2.9 2.9 0 0 0 6.16 18c1.06 0 2.02-.57 2.54-1.49l.55-.98a2.9 2.9 0 0 1 5.06 0l.55.98A2.9 2.9 0 0 0 17.4 18a2.9 2.9 0 0 0 2.85-2.34l1.2-4.8A4.7 4.7 0 0 0 17.3 5Z" />
                    </svg>
                  </span>
                  <span className={styles.gameText}>
                    <span className={styles.gameTitle}>{game.title}</span>
                    <span className={styles.gameDesc}>{game.description}</span>
                  </span>
                  <span className={styles.gameAction}>
                    {openingId === game.id ? "打开中…" : "打开"}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      </div>
    </div>
  );
}
