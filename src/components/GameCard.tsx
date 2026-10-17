import { useEffect, useRef, useState } from "react";
import type { Game } from "../types/game";
import { formatDuration, formatPlayTime } from "../utils/format";
import Cover from "./Cover";
import styles from "./GameCard.module.css";

interface GameCardProps {
  game: Game;
  /** 该游戏是否正在被跟踪。 */
  running: boolean;
  /** 启动请求进行中（按钮置灰防重复点击）。 */
  launching: boolean;
  /** 本次会话已玩秒数（由后端 tick 事件驱动）；未知时为 null。 */
  elapsedSeconds: number | null;
  onLaunch: (game: Game) => void;
  onStopTracking: (game: Game) => void;
  onRemove: (game: Game) => void;
  /** 点击卡面打开详情。 */
  onOpen: (game: Game) => void;
  /** 长按拖拽的视觉状态（由 GameGrid 协调）。 */
  isDragged: boolean;
  isDropTarget: boolean;
  /** 被拖卡片相对按下位置的偏移（跟随动画）；非拖拽时为 null。 */
  dragOffset: { x: number; y: number } | null;
  /** 指针按下时上报给 GameGrid（进入长按拖拽流程）。 */
  onCardPointerDown: (game: Game, event: React.PointerEvent) => void;
}

/** 二次确认的自动复位时间（毫秒）：超时未确认就退出确认态，避免误触。 */
const CONFIRM_RESET_MS = 5000;

export default function GameCard({
  game,
  running,
  launching,
  elapsedSeconds,
  onLaunch,
  onStopTracking,
  onRemove,
  onOpen,
  isDragged,
  isDropTarget,
  dragOffset,
  onCardPointerDown,
}: GameCardProps) {
  // 移除是破坏性操作（删除库内记录），做成两步确认且超时自动复位
  const [confirmingRemove, setConfirmingRemove] = useState(false);
  const resetTimer = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (resetTimer.current !== null) {
        window.clearTimeout(resetTimer.current);
      }
    };
  }, []);

  const enterConfirm = () => {
    setConfirmingRemove(true);
    resetTimer.current = window.setTimeout(() => {
      setConfirmingRemove(false);
      resetTimer.current = null;
    }, CONFIRM_RESET_MS);
  };

  const cancelConfirm = () => {
    if (resetTimer.current !== null) {
      window.clearTimeout(resetTimer.current);
      resetTimer.current = null;
    }
    setConfirmingRemove(false);
  };

  const confirmRemove = () => {
    cancelConfirm();
    onRemove(game);
  };

  return (
    <article
      data-card-id={game.id}
      className={[
        styles.card,
        running && styles.cardRunning,
        isDragged && styles.dragged,
        isDropTarget && styles.dropTarget,
      ]
        .filter(Boolean)
        .join(" ")}
      onPointerDown={(event) => onCardPointerDown(game, event)}
      style={
        isDragged && dragOffset
          ? {
              transform: `translate(${dragOffset.x}px, ${dragOffset.y}px) scale(1.04)`,
              zIndex: 10,
            }
          : undefined
      }
    >
      <button
        className={styles.moreButton}
        type="button"
        title="游戏详情"
        aria-label={`打开 ${game.name} 的详情`}
        onClick={() => onOpen(game)}
      >
        ···
      </button>

      <div className={styles.coverButton}>
        <Cover game={game} />
      </div>

      <div className={styles.body}>
        <h3 className={styles.name} title={game.name}>
          {game.name}
        </h3>

        <div className={styles.meta}>
          <span className={styles.time}>
            {formatPlayTime(game.totalPlaySeconds)}
          </span>
          {running && (
            <span
              className={styles.runningBadge}
              title="正在由本应用跟踪计时（不含系统休眠时间）"
            >
              <span className={styles.dot} aria-hidden="true" />
              运行中
              {elapsedSeconds !== null && ` · ${formatDuration(elapsedSeconds)}`}
            </span>
          )}
          {!running && game.isLauncher && (
            <span
              className={styles.badge}
              title="该条目通过平台启动器启动，计时可能不准确"
            >
              启动器
            </span>
          )}
        </div>
      </div>

      <div className={styles.actions}>
        {confirmingRemove ? (
          <div className={styles.confirmRow}>
            <button
              className={styles.confirmRemove}
              type="button"
              onClick={confirmRemove}
              title="从库中删除这条记录（不会删除游戏文件）"
            >
              确认移除
            </button>
            <button
              className={styles.cancelRemove}
              type="button"
              onClick={cancelConfirm}
            >
              取消
            </button>
          </div>
        ) : (
          <div className={styles.actionRow}>
            {running ? (
              <button
                className={styles.stopButton}
                type="button"
                onClick={() => onStopTracking(game)}
                title="只停止计时，不会关闭游戏"
              >
                停止计时
              </button>
            ) : (
              <button
                className={styles.launch}
                type="button"
                disabled={launching}
                onClick={() => onLaunch(game)}
              >
                {launching ? "启动中…" : "启动"}
              </button>
            )}
            <button
              className={styles.removeButton}
              type="button"
              disabled={launching}
              aria-label={`移除 ${game.name}`}
              title="从库中移除（不删除游戏文件）"
              onClick={enterConfirm}
            >
              移除
            </button>
          </div>
        )}
      </div>
    </article>
  );
}
