import { useCallback, useEffect, useState } from "react";
import AddGameDialog from "../components/AddGameDialog";
import ScanDialog from "../components/ScanDialog";
import WarmupDialog from "../components/WarmupDialog";
import GameGrid from "../components/GameGrid";
import GameDetailDialog from "../components/GameDetailDialog";
import { useGames } from "../hooks/useGames";
import { setGamepadLayer, type GamepadAction } from "../services/gamepad";
import { toUserMessage } from "../services/errorMessages";
import { pushToast } from "../services/toastStore";
import { gameApi } from "../services/gameApi";
import { useSessions } from "../hooks/useSessions";
import type { Game, GameSortField, SortOrder } from "../types/game";
import styles from "./LibraryPage.module.css";

interface LibraryPageProps {
  onOpenSettings: () => void;
}

const SORT_OPTIONS: ReadonlyArray<{ value: GameSortField; label: string }> = [
  { value: "createdAt", label: "添加时间" },
  { value: "name", label: "名称" },
  { value: "playTime", label: "游玩时长" },
  { value: "custom", label: "自定义" },
];

/** 输入防抖：避免每次按键都打一次数据库。 */
const SEARCH_DEBOUNCE_MS = 200;

export default function LibraryPage({ onOpenSettings }: LibraryPageProps) {
  const [keyword, setKeyword] = useState("");
  const [debouncedKeyword, setDebouncedKeyword] = useState("");
  // 排序偏好持久化：重启后恢复上次的选择（含「自定义」）
  const [sort, setSort] = useState<GameSortField>(() => {
    const value = localStorage.getItem("librarySort");
    return value === "name" || value === "playTime" || value === "createdAt" || value === "custom"
      ? value
      : "createdAt";
  });
  const [order, setOrder] = useState<SortOrder>(() => {
    const value = localStorage.getItem("libraryOrder");
    return value === "asc" || value === "desc" ? value : "desc";
  });

  useEffect(() => {
    localStorage.setItem("librarySort", sort);
  }, [sort]);

  useEffect(() => {
    localStorage.setItem("libraryOrder", order);
  }, [order]);
  const [addDialogOpen, setAddDialogOpen] = useState(false);
  const [scanDialogOpen, setScanDialogOpen] = useState(false);
  const [warmupOpen, setWarmupOpen] = useState(false);
  const [detailGame, setDetailGame] = useState<Game | null>(null);
  // 手柄导航：当前聚焦的卡面 id；null = 未使用手柄
  const [gamepadFocusId, setGamepadFocusId] = useState<string | null>(null);

  useEffect(() => {
    const timer = window.setTimeout(
      () => setDebouncedKeyword(keyword),
      SEARCH_DEBOUNCE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [keyword]);

  const { games, loading, error, reload } = useGames(sort, order, debouncedKeyword);

  // 会话结束后累计时长会变，需要刷新列表
  const {
    active,
    elapsed,
    launching,
    recovered,
    dismissRecovered,
    launch,
    stopTracking,
  } = useSessions(reload);

  const handleLaunch = useCallback(
    (game: Game) => {
      void launch(game.id);
    },
    [launch],
  );

  const handleStopTracking = useCallback(
    (game: Game) => {
      void stopTracking(game.id);
    },
    [stopTracking],
  );

  /** 从库中移除游戏。若正在跟踪计时，先停表再删记录（都只动库，不碰游戏文件）。 */
  // 拖拽排序：提交后切到「自定义」视图，让结果立即可见
  const handleReorder = useCallback(
    async (orderedIds: string[]) => {
      try {
        await gameApi.reorderGames(orderedIds);
        if (sort !== "custom") {
          setSort("custom");
          setOrder("asc");
        }
      } catch (err) {
        pushToast(toUserMessage(err));
      } finally {
        // 无论成败都刷新：FLIP 动画依赖新顺序渲染触发（失败时恢复 DB 现状）
        await reload();
      }
    },
    [sort, reload],
  );

  // 手柄导航（层 1）：方向在卡面网格间移动（基于矩形的空间导航，自动适配列数），
  // A 启动 / Y 详情 / B 返回 / 菜单键=设置。任何弹窗打开时挂起手柄导航。
  useEffect(() => {
    setGamepadLayer("library-grid", (action: GamepadAction) => {
      if (addDialogOpen || scanDialogOpen || warmupOpen || detailGame !== null) {
        return;
      }
      if (action === "menu") {
        onOpenSettings();
        return;
      }
      if (action === "back") {
        if (keyword) {
          setKeyword("");
        } else {
          setGamepadFocusId(null);
        }
        return;
      }
      if (action === "confirm" || action === "detail") {
        if (gamepadFocusId === null) {
          return;
        }
        const focused = games.find((game) => game.id === gamepadFocusId);
        if (!focused) {
          return;
        }
        if (action === "confirm") {
          handleLaunch(focused);
        } else {
          setDetailGame(focused);
        }
        return;
      }
      // 方向移动
      setGamepadFocusId((currentId) => {
        const cards = Array.from(
          document.querySelectorAll<HTMLElement>("[data-card-id]"),
        );
        if (cards.length === 0) {
          return null;
        }
        const ids = cards.map((card) => card.dataset.cardId ?? "");
        const currentIndex =
          currentId !== null ? ids.indexOf(currentId) : -1;
        if (currentIndex === -1) {
          return ids[0]; // 尚无焦点：聚焦第一张卡
        }
        const currentRect = cards[currentIndex].getBoundingClientRect();
        const cx = currentRect.left + currentRect.width / 2;
        const cy = currentRect.top + currentRect.height / 2;
        let best: number | null = null;
        let bestScore = Number.POSITIVE_INFINITY;
        cards.forEach((card, index) => {
          if (index === currentIndex) {
            return;
          }
          const rect = card.getBoundingClientRect();
          const dx = rect.left + rect.width / 2 - cx;
          const dy = rect.top + rect.height / 2 - cy;
          if (action === "right" && dx <= 4) return;
          if (action === "left" && dx >= -4) return;
          if (action === "down" && dy <= 4) return;
          if (action === "up" && dy >= -4) return;
          const score =
            action === "left" || action === "right"
              ? Math.abs(dy) * 3 + Math.abs(dx)
              : Math.abs(dx) * 3 + Math.abs(dy);
          if (score < bestScore) {
            bestScore = score;
            best = index;
          }
        });
        return best !== null ? ids[best] : currentId;
      });
    });
    // 订阅随弹窗开关与数据变化重建，保证闭包拿到最新状态
    // 弹窗打开时本层仍注册在栈里，但弹窗层压在其上接管 —— 此处的弹窗判断是双保险
  }, [
    addDialogOpen,
    scanDialogOpen,
    warmupOpen,
    detailGame,
    games,
    gamepadFocusId,
    keyword,
    handleLaunch,
    onOpenSettings,
  ]);

  // 手柄焦点变化时把卡面滚入视野
  useEffect(() => {
    if (gamepadFocusId === null) {
      return;
    }
    document
      .querySelector(`[data-card-id="${gamepadFocusId}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [gamepadFocusId]);

  const handleRemove = useCallback(
    async (game: Game) => {
      try {
        if (active.has(game.id)) {
          await stopTracking(game.id);
        }
        const result = await gameApi.removeGame(game.id);
        const parts = [`已从库中移除「${game.name}」`];
        if (result.removedSessions > 0) {
          parts.push(`清理 ${result.removedSessions} 条游玩记录`);
        }
        if (result.removedAssets > 0) {
          parts.push(`回收 ${result.removedAssets} 个封面/图标文件`);
        }
        pushToast(parts.join("，"), "info");
      } catch (err) {
        pushToast(toUserMessage(err));
      } finally {
        reload();
      }
    },
    [active, stopTracking, reload],
  );

  const searching = debouncedKeyword.trim().length > 0;
  const isEmpty = games.length === 0;
  const showLoading = loading && isEmpty && error === null;
  const runningCount = active.size;

  return (
    <div className={styles.page}>
      <header className={styles.topBar}>
        <div className={styles.brand}>
          <span className={styles.brandMark} aria-hidden="true" />
          <h1 className={styles.brandName}>游戏库</h1>
        </div>

        <input
          className={styles.search}
          type="search"
          value={keyword}
          onChange={(event) => setKeyword(event.currentTarget.value)}
          placeholder="搜索游戏"
          aria-label="搜索游戏"
        />

        <button
          className={styles.ghostButton}
          type="button"
          onClick={() => setWarmupOpen(true)}
        >
          赛前热身
        </button>

        <button
          className={styles.ghostButton}
          type="button"
          onClick={() => setScanDialogOpen(true)}
        >
          扫描游戏
        </button>

        <button
          className={styles.primaryButton}
          type="button"
          onClick={() => setAddDialogOpen(true)}
        >
          添加游戏
        </button>

        <button
          className={styles.ghostButton}
          type="button"
          onClick={onOpenSettings}
        >
          设置
        </button>
      </header>

      <div className={styles.controls}>
        <label className={styles.sortLabel}>
          排序
          <select
            className={styles.select}
            value={sort}
            onChange={(event) =>
              setSort(event.currentTarget.value as GameSortField)
            }
          >
            {SORT_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>

        <button
          className={styles.orderButton}
          type="button"
          onClick={() => setOrder(order === "asc" ? "desc" : "asc")}
          title={order === "asc" ? "当前升序，点击切为降序" : "当前降序，点击切为升序"}
        >
          {order === "asc" ? "升序 ↑" : "降序 ↓"}
        </button>

        <span className={styles.count}>
          {runningCount > 0 && `${runningCount} 个运行中 · `}
          {loading ? "载入中…" : `${games.length} 个游戏`}
        </span>
      </div>

      {error !== null && (
        <div className={styles.errorBar} role="alert">
          <span className={styles.errorText}>{error}</span>
          <button className={styles.retryButton} type="button" onClick={reload}>
            重试
          </button>
        </div>
      )}

      {recovered.length > 0 && (
        <div className={styles.recoveredBar} role="status">
          <span className={styles.errorText}>
            上次退出时有 {recovered.length} 段游玩未正常结束，已按最后一次记录的进度
            （共 {recovered.reduce((sum, item) => sum + item.durationSeconds, 0)} 秒）记为中断。
          </span>
          <button
            className={styles.retryButton}
            type="button"
            onClick={dismissRecovered}
          >
            知道了
          </button>
        </div>
      )}


      <main
        className={isEmpty ? `${styles.content} ${styles.centered}` : styles.content}
      >
        {showLoading ? (
          <p className={styles.loading}>正在读取游戏库…</p>
        ) : (
          <GameGrid
            games={games}
            emptyTitle={searching ? "没有匹配的游戏" : undefined}
            emptyDescription={
              searching
                ? `没有找到名称包含「${debouncedKeyword.trim()}」的游戏，试试换个关键词。`
                : undefined
            }
            onEmptyAction={searching ? undefined : () => setAddDialogOpen(true)}
            isRunning={(gameId) => active.has(gameId)}
            isLaunching={(gameId) => launching.has(gameId)}
            elapsedFor={(gameId) => elapsed.get(gameId) ?? null}
            onLaunch={handleLaunch}
            onStopTracking={handleStopTracking}
            onRemove={(game) => void handleRemove(game)}
            onOpen={setDetailGame}
            onReorder={(orderedIds) => void handleReorder(orderedIds)}
            dragEnabled={!searching}
            gamepadFocusId={gamepadFocusId}
          />
        )}

        {detailGame && (
          <GameDetailDialog
            game={detailGame}
            onClose={() => setDetailGame(null)}
            onUpdated={() => reload()}
          />
        )}
      </main>

      {addDialogOpen && (
        <AddGameDialog
          onClose={() => setAddDialogOpen(false)}
          onImported={reload}
        />
      )}

      {scanDialogOpen && (
        <ScanDialog
          onClose={() => setScanDialogOpen(false)}
          onImported={reload}
        />
      )}

      {warmupOpen && (
        <WarmupDialog onClose={() => setWarmupOpen(false)} />
      )}
    </div>
  );
}
