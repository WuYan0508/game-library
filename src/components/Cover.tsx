import { useEffect, useState } from "react";
import { resolveAssetUrl } from "../services/libraryPaths";
import type { Game } from "../types/game";
import styles from "./Cover.module.css";

interface CoverProps {
  game: Game;
}

/**
 * 封面按三级策略取用：**自定义封面 → 提取的图标 → 占位图**。
 *
 * 每一级失败都直接回退到下一级：后端拿不到图标就给 null，前端加载失败则由
 * `onError` 兜住 —— 封面文件被手工删掉也不会白屏。
 */
export default function Cover({ game }: CoverProps) {
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  const hasCustomCover = game.coverPath !== null;
  const relativePath = game.coverPath ?? game.iconPath;

  useEffect(() => {
    let cancelled = false;
    setFailed(false);
    setUrl(null);

    if (relativePath === null) {
      return;
    }

    void (async () => {
      try {
        const resolved = await resolveAssetUrl(relativePath);
        if (!cancelled) {
          setUrl(resolved);
        }
      } catch {
        // 拿不到 URL 等同于「没有封面」，走占位图
        if (!cancelled) {
          setFailed(true);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [relativePath]);

  const showImage = url !== null && !failed;

  return (
    <div className={styles.cover}>
      {showImage ? (
        <img
      draggable={false}
          className={hasCustomCover ? styles.custom : styles.icon}
          src={url}
          alt=""
          /* 文件缺失 / 解码失败都走这里 —— 这是「回退占位图」的兜底 */
          onError={() => setFailed(true)}
        />
      ) : (
        <svg
          className={styles.placeholder}
          viewBox="0 0 48 48"
          width="40"
          height="40"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <rect x="6" y="13" width="36" height="24" rx="4" />
          <path d="M17 25h6M20 22v6" />
          <circle cx="31" cy="23" r="1.7" fill="currentColor" stroke="none" />
          <circle cx="34.5" cy="27" r="1.7" fill="currentColor" stroke="none" />
        </svg>
      )}
    </div>
  );
}
