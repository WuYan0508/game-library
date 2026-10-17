import { useLayoutEffect, useRef, useState } from "react";
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
  // FLIP 动画：长按达成时记录全部卡片的「旧位置」，落位后据此做平滑过渡
  const flipRef = useRef<Map<string, DOMRect> | null>(null);
  // 拖拽时的边缘自动滚动
  const scrollContainerRef = useRef<HTMLElement | null>(null);
  const autoScrollRafRef = useRef<number | null>(null);
  const lastPointerRef = useRef({ x: 0, y: 0 });
  // 拖拽开始至今的滚动差值：被拖卡片的跟随偏移必须把它算进去，
  // 否则自动滚动时卡片会跟着内容走而不是跟着鼠标走
  const [scrollOffset, setScrollOffset] = useState(0);
  const scrollDeltaRef = useRef(0);
  const scrollAtDragStartRef = useRef(0);
  const onContainerScrollRef = useRef<(() => void) | null>(null);
  const gridRef = useRef<HTMLDivElement | null>(null);

  function findScrollable(element: HTMLElement | null): HTMLElement | null {
    let node = element?.parentElement ?? null;
    while (node) {
      const overflowY = window.getComputedStyle(node).overflowY;
      if ((overflowY === "auto" || overflowY === "scroll") && node.scrollHeight > node.clientHeight) {
        return node;
      }
      node = node.parentElement;
    }
    return null;
  }

  function stopAutoScroll() {
    if (autoScrollRafRef.current !== null) {
      cancelAnimationFrame(autoScrollRafRef.current);
      autoScrollRafRef.current = null;
    }
  }
  // 提交后置 true；games 更新（或超时兜底）时消费并释放拖拽视觉
  const pendingSettleRef = useRef(false);

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

      // 记录拖拽前全部卡片的视口位置（FLIP 动画的 First；丢失则动画不会播放）
      const firstRects = new Map<string, DOMRect>();
      document.querySelectorAll<HTMLElement>("[data-card-id]").forEach((el) => {
        const id = el.dataset.cardId;
        if (id) {
          firstRects.set(id, el.getBoundingClientRect());
        }
      });
      flipRef.current = firstRects;

      // 边缘自动滚动：拖到列表上/下边缘时自动滚动，越靠边越快
      const cardEl = document.querySelector<HTMLElement>(`[data-card-id="${game.id}"]`);
      scrollContainerRef.current = findScrollable(cardEl);
      scrollAtDragStartRef.current = scrollContainerRef.current?.scrollTop ?? 0;
      scrollDeltaRef.current = 0;
      setScrollOffset(0);
      const containerForListen = scrollContainerRef.current;
      if (containerForListen) {
        const onContainerScroll = () => {
          const offset = containerForListen.scrollTop - scrollAtDragStartRef.current;
          scrollDeltaRef.current = offset;
          setScrollOffset(offset);
        };
        containerForListen.addEventListener("scroll", onContainerScroll);
        onContainerScrollRef.current = () => {
          containerForListen.removeEventListener("scroll", onContainerScroll);
          onContainerScrollRef.current = null;
        };
      }
      lastPointerRef.current = { x: startX, y: startY };

      const updateHover = () => {
        const next = hitTestCard(game.id);
        if (next !== overIdRef.current) {
          overIdRef.current = next;
          setOverId(next);
        }
      };

      const onMove = (moveEvent: PointerEvent) => {
        lastPointerRef.current = { x: moveEvent.clientX, y: moveEvent.clientY };
        setPointer({ x: moveEvent.clientX, y: moveEvent.clientY });
        updateHover();
        // 直接写 transform：绕过 React 渲染排队，鼠标再快也零延迟跟随
        // （滚动差值用 ref 取实时值；React 渲染写入的是同一值，不会打架）
        const press = pressStartRef.current;
        const draggedEl = document.querySelector<HTMLElement>(`[data-card-id="${game.id}"]`);
        if (press && draggedEl) {
          const dx = moveEvent.clientX - press.x;
          const dy = moveEvent.clientY - press.y + scrollDeltaRef.current;
          draggedEl.style.transform = `translate(${dx}px, ${dy}px) scale(1.04)`;
        }
      };

      const autoScrollStep = () => {
        const container = scrollContainerRef.current;
        const { y } = lastPointerRef.current;
        if (!container) {
          autoScrollRafRef.current = null;
          return;
        }
        const rect = container.getBoundingClientRect();
        const edge = 72; // 距上/下边缘 72px 内触发
        let delta = 0;
        // 以卡面网格自身的边界为滚动上限：网格底已进入视口就不再下滚，顶同理
        // （否则滚动容器的多余空间会把所有卡片滚出视野）
        const gridRect = gridRef.current?.getBoundingClientRect();
        if (y < rect.top + edge) {
          if (!gridRect || gridRect.top < rect.top - 1) {
            delta = -Math.ceil((1 - (y - rect.top) / edge) * 16); // 最快 16px/帧
          }
        } else if (y > rect.bottom - edge) {
          if (!gridRect || gridRect.bottom > rect.bottom + 1) {
            delta = Math.ceil((1 - (rect.bottom - y) / edge) * 16);
          }
        }
        if (delta !== 0) {
          container.scrollTop += delta;
          updateHover(); // 滚动后重新判定悬停目标
        }
        autoScrollRafRef.current = requestAnimationFrame(autoScrollStep);
      };

      autoScrollRafRef.current = requestAnimationFrame(autoScrollStep);

      const onUp = () => {
        cleanup();
        const dragged = dragIdRef.current;
        const targetId = overIdRef.current;
        dragIdRef.current = null;
        overIdRef.current = null;
        setOverId(null);
        if (!dragged || !targetId) {
          // 未成交换：卡片滑回原位
          setDragId(null);
          setPointer(null);
          return;
        }
        // 插入方向跟随拖拽方向（按交换前的原始顺序判断）：
        // 从左往右拖 → 放到目标之后；从右往左 → 放到目标之前。
        // 与重叠高亮配套：边缘刚接触松手，两卡即真正的位置互换。
        const fromIdx = games.findIndex((g) => g.id === dragged);
        const toIdx = games.findIndex((g) => g.id === targetId);
        const insertAfter = fromIdx < toIdx;
        // 提交后保持拖拽视觉（卡片停在指针处），等新顺序渲染后由 FLIP 接管滑入新位
        pendingSettleRef.current = true;
        // 兜底：1 秒内 games 未更新（IPC 异常）也强制释放，卡片绝不悬停
        window.setTimeout(() => {
          if (pendingSettleRef.current) {
            pendingSettleRef.current = false;
            setDragId(null);
            setPointer(null);
          }
        }, 1000);
        commitReorder(dragged, targetId, insertAfter);
      };

      function cleanup() {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        window.removeEventListener("pointermove", onCancelMove);
        document.body.classList.remove("dragging-cards");
        stopAutoScroll();
        onContainerScrollRef.current?.();
        scrollContainerRef.current = null;
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

  // FLIP：交换提交后的首次 games 更新，让所有位移过的卡片（含被拖卡）平滑滑入新位
  useLayoutEffect(() => {
    if (!pendingSettleRef.current) {
      // 与交换无关的 games 更新（搜索 / 会话计时等）不触发动画、不消费记录
      return;
    }
    pendingSettleRef.current = false;
    const first = flipRef.current;
    flipRef.current = null;
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    if (first && !reduceMotion) {
      document.querySelectorAll<HTMLElement>("[data-card-id]").forEach((el) => {
        const id = el.dataset.cardId;
        const firstRect = id ? first.get(id) : undefined;
        if (!id || !firstRect) {
          return;
        }
        const newRect = el.getBoundingClientRect();
        const dragged = el.style.transform !== "";
        // First 位置按拖拽期间的滚动量校正（两者坐标系才一致）
        const firstTop = firstRect.top - scrollDeltaRef.current;
        const start = dragged
          ? el.style.transform // 被拖卡：从当前指针位置出发
          : `translate(${firstRect.left - newRect.left}px, ${firstTop - newRect.top}px)`;
        const moved = dragged || firstRect.left !== newRect.left || firstTop !== newRect.top;
        if (!moved) {
          return;
        }
        el.animate(
          [{ transform: start }, { transform: "none" }],
          { duration: 500, easing: "cubic-bezier(0.45, 0.05, 0.55, 0.95)" }
        );
      });
    }

    // 无条件释放拖拽视觉状态：flipRef 缺失（被中途更新消费）时也要落位，绝不悬停
    setDragId(null);
    setPointer(null);
  }, [games]);

  // 被拖卡片相对按下位置的偏移（驱动跟随动画）
  const dragOffset =
    dragId !== null && pointer !== null && pressStartRef.current !== null
      ? {
          x: pointer.x - pressStartRef.current.x,
          // + 滚动差值：自动滚动时卡片跟随鼠标而非跟随内容
          y: pointer.y - pressStartRef.current.y + scrollOffset,
        }
      : null;

  /**
   * 命中检测：排除被拖卡片，返回离指针最近的卡片 id。
   * 用「指针到卡片矩形的距离」而非 elementFromPoint —— 被拖卡片（置顶跟随指针）
   * 会挡住下方的目标卡，且卡片间有空隙，几何最近距离的判定范围最大。
   */
  function hitTestCard(draggedId: string): string | null {
    // 交换判定：被拖卡片与目标卡「出现重叠部分」即可交换，取重叠面积最大者
    const draggedEl = document.querySelector<HTMLElement>(
      `[data-card-id="${draggedId}"]`,
    );
    if (!draggedEl) {
      return null;
    }
    const draggedRect = draggedEl.getBoundingClientRect();
    const cards = document.querySelectorAll<HTMLElement>("[data-card-id]");
    let best: string | null = null;
    let bestArea = 0;
    cards.forEach((el) => {
      const id = el.dataset.cardId;
      if (!id || id === draggedId) {
        return;
      }
      const rect = el.getBoundingClientRect();
      const overlapX =
        Math.min(draggedRect.right, rect.right) -
        Math.max(draggedRect.left, rect.left);
      const overlapY =
        Math.min(draggedRect.bottom, rect.bottom) -
        Math.max(draggedRect.top, rect.top);
      if (overlapX <= 0 || overlapY <= 0) {
        return;
      }
      const area = overlapX * overlapY;
      if (area > bestArea) {
        bestArea = area;
        best = id;
      }
    });
    return best;
  }

  function commitReorder(draggedId: string, targetId: string, insertAfter: boolean) {
    const ids = games.map((game) => game.id);
    const from = ids.indexOf(draggedId);
    const target = ids.indexOf(targetId);
    if (from < 0 || target < 0) {
      return;
    }
    // 插入方向跟随拖拽方向：从左往右拖 → 放到目标之后；从右往左 → 放到目标之前。
    // 与「边缘接触即高亮」的重叠判定配套：刚碰到邻卡松手，就是真正的位置互换。
    ids.splice(from, 1);
    const insertAt = insertAfter ? ids.indexOf(targetId) + 1 : ids.indexOf(targetId);
    ids.splice(insertAt, 0, draggedId);
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
    <div ref={gridRef} className={styles.grid}>
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
