import { useRef, useState } from "react";
import type { Game } from "../types/game";
import GameCard from "./GameCard";
import styles from "./GameGrid.module.css";

interface GameGridProps {
  games: Game[];
  /** 列表为空时的文案。搜索无结果与空库使用不同措辞。 */
  emptyTitle?: string;
  emptyDescription?: string;
  /** 空状态主按钮的动作；仅空库场景提供。 */
  onEmptyAction?: () => void;
  isRunning: (gameId: string) => boolean;
  isLaunching: (gameId: string) => boolean;
  /** 本次会话已玩秒数；未知时为 null。 */
  elapsedFor: (gameId: string) => number | null;
  onLaunch: (game: Game) => void;
  onStopTracking: (game: Game) => void;
  onRemove: (game: Game) => void;
  onOpen: (game: Game) => void;
  /** 拖拽排序完成：给出完整 id 顺序。 */
  onReorder: (orderedIds: string[]) => void;
  /** 搜索过滤时禁用拖拽（过滤视图无法给出完整顺序）。 */
  dragEnabled: boolean;
}

/** 长按进入拖拽所需的时间（毫秒）。 */
const LONG_PRESS_MS = 350;

/**
 * 卡片网格 + 长按拖拽排序。
 *
 * 不用 HTML5 拖拽（draggable 在长按场景下受浏览器「按下时快照拖拽候选」限制，
 * 两次实测都不可靠），改为纯指针事件实现：
 *   按下卡面 → 350ms 后进入拖拽 → 移动指针实时高亮目标卡 → 松手按落点提交。
 */
export default function GameGrid({
  games,
  emptyTitle,
  emptyDescription,
  onEmptyAction,
  isRunning,
  isLaunching,
  elapsedFor,
  onLaunch,
  onStopTracking,
  onRemove,
  onOpen,
  onReorder,
  dragEnabled,
}: GameGridProps) {
  // 视觉状态：正在拖的卡 / 当前悬停的目标卡
  const [dragId, setDragId] = useState<string | null>(null);
  const [overId, setOverId] = useState<string | null>(null);
  // 指针实时位置（驱动被拖卡片跟随动画）
  const [pointer, setPointer] = useState<{ x: number; y: number } | null>(null);

  // 全部用 ref 存拖拽过程数据，window 级监听器里读到的一定是最新值
  const dragIdRef = useRef<string | null>(null);
  const overIdRef = useRef<string | null>(null);
  const cleanupRef = useRef<(() => void) | null>(null);
  const pressStartRef = useRef<{ x: number; y: number } | null>(null);

  function onCardPointerDown(game: Game, event: React.PointerEvent) {
    if (!dragEnabled || dragIdRef.current !== null) {
      return;
    }
    // 按在按钮上（启动/移除/封面按钮）不进入拖拽，避免抢按钮的点击
    if ((event.target as HTMLElement).closest("button")) {
      return;
    }

    const startX = event.clientX;
    const startY = event.clientY;
    pressStartRef.current = { x: startX, y: startY };
    const timer = window.setTimeout(() => {
      // 长按达成：进入拖拽，挂全局指针监听
      dragIdRef.current = game.id;
      setDragId(game.id);
      setPointer({ x: startX, y: startY });
      // 进入拖拽后不再需要「移动取消长按」监听（它会把清理函数置空，留下隐患）
      window.removeEventListener("pointermove", onCancelMove);
      // 禁止文字/图片被拖拽选择，并清掉按下前已存在的选区
      document.body.classList.add("dragging-cards");
      window.getSelection()?.removeAllRanges();

      const onMove = (moveEvent: PointerEvent) => {
        setPointer({ x: moveEvent.clientX, y: moveEvent.clientY });
        const next = hitTestCard(moveEvent.clientX, moveEvent.clientY, game.id);
        if (next !== overIdRef.current) {
          overIdRef.current = next;
          setOverId(next);
        }
      };

      const onUp = (upEvent: PointerEvent) => {
        cleanup();
        setPointer(null);
        const dragged = dragIdRef.current;
        const targetId = overIdRef.current;
        dragIdRef.current = null;
        overIdRef.current = null;
        setDragId(null);
        setOverId(null);
        if (!dragged || !targetId) {
          return;
        }
        const targetEl = document.querySelector(`[data-card-id="${targetId}"]`);
        const before = targetEl
          ? upEvent.clientX <
            targetEl.getBoundingClientRect().left +
              targetEl.getBoundingClientRect().width / 2
          : true;
        commitReorder(dragged, targetId, before);
      };

      function cleanup() {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        window.removeEventListener("pointermove", onCancelMove);
        document.body.classList.remove("dragging-cards");
        cleanupRef.current = null;
      }
      cleanupRef.current = cleanup;
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
    }, LONG_PRESS_MS);

    // 长按期间移动超过 8px 视为普通滑动（比如滚轮前的拖动），取消长按
    const onCancelMove = (moveEvent: PointerEvent) => {
      if (
        Math.abs(moveEvent.clientX - startX) > 8 ||
        Math.abs(moveEvent.clientY - startY) > 8
      ) {
        cancelPress();
      }
    };
    function cancelPress() {
      window.clearTimeout(timer);
      window.removeEventListener("pointermove", onCancelMove);
      cleanupRef.current = null;
    }
    cleanupRef.current = cancelPress;
    window.addEventListener("pointermove", onCancelMove);
  }

  // 被拖卡片相对按下位置的偏移（驱动跟随动画）
  const dragOffset =
    dragId !== null && pointer !== null && pressStartRef.current !== null
      ? {
          x: pointer.x - pressStartRef.current.x,
          y: pointer.y - pressStartRef.current.y,
        }
      : null;

  /**
   * 命中检测：排除被拖卡片，返回离指针最近的卡片 id。
   * 用「指针到卡片矩形的距离」而非 elementFromPoint —— 被拖卡片（置顶跟随指针）
   * 会挡住下方的目标卡，且卡片间有空隙，几何最近距离的判定范围最大。
   */
  function hitTestCard(x: number, y: number, draggedId: string): string | null {
    const cards = document.querySelectorAll<HTMLElement>("[data-card-id]");
    let best: string | null = null;
    let bestDist = Number.POSITIVE_INFINITY;
    cards.forEach((el) => {
      const id = el.dataset.cardId;
      if (!id || id === draggedId) {
        return;
      }
      const rect = el.getBoundingClientRect();
      const dx = Math.max(rect.left - x, 0, x - rect.right);
      const dy = Math.max(rect.top - y, 0, y - rect.bottom);
      const dist = dx * dx + dy * dy;
      if (dist < bestDist) {
        bestDist = dist;
        best = id;
      }
    });
    return best;
  }

  function commitReorder(draggedId: string, targetId: string, before: boolean) {
    const ids = games.map((game) => game.id);
    const from = ids.indexOf(draggedId);
    if (from < 0) {
      return;
    }
    ids.splice(from, 1);
    const to = ids.indexOf(targetId);
    if (to < 0) {
      return;
    }
    ids.splice(before ? to : to + 1, 0, draggedId);
    onReorder(ids);
  }

  if (games.length === 0) {
    return (
      <div className={styles.empty}>
        <p className={styles.emptyTitle}>{emptyTitle ?? "游戏库还是空的"}</p>
        {emptyDescription && (
          <p className={styles.emptyDescription}>{emptyDescription}</p>
        )}
        {onEmptyAction && (
          <button className={styles.emptyAction} type="button" onClick={onEmptyAction}>
            添加第一个游戏
          </button>
        )}
      </div>
    );
  }

  return (
    <div className={styles.grid}>
      {games.map((game) => (
        <GameCard
          key={game.id}
          game={game}
          running={isRunning(game.id)}
          launching={isLaunching(game.id)}
          elapsedSeconds={elapsedFor(game.id) ?? null}
          onLaunch={onLaunch}
          onStopTracking={onStopTracking}
          onRemove={onRemove}
          onOpen={onOpen}
          isDragged={dragId === game.id}
          isDropTarget={dragId !== null && overId === game.id}
          dragOffset={dragId === game.id ? dragOffset : null}
          onCardPointerDown={onCardPointerDown}
        />
      ))}
    </div>
  );
}
