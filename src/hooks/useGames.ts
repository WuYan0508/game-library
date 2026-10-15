import { useCallback, useEffect, useState } from "react";
import { describeError, gameApi } from "../services/gameApi";
import type { Game, GameSortField, SortOrder } from "../types/game";

interface UseGamesResult {
  games: Game[];
  loading: boolean;
  /** 已转换为可展示文案的错误；null 表示无错误。 */
  error: string | null;
  reload: () => void;
}

/**
 * 读取游戏列表。关键词先经上层防抖后传入。
 * 请求竞态用 cleanup 标记丢弃：后发的请求生效，先发的过期结果直接忽略。
 */
export function useGames(
  sort: GameSortField,
  order: SortOrder,
  keyword: string,
): UseGamesResult {
  const [games, setGames] = useState<Game[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);

  const reload = useCallback(() => {
    setReloadToken((token) => token + 1);
  }, []);

  useEffect(() => {
    let cancelled = false;

    void (async () => {
      setLoading(true);
      try {
        const trimmed = keyword.trim();
        const result = trimmed
          ? await gameApi.searchGames({ keyword: trimmed, sort, order })
          : await gameApi.listGames({ sort, order });

        if (!cancelled) {
          setGames(result);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) {
          // 读取失败时清空列表，不保留可能已过期的旧数据冒充成功
          setGames([]);
          setError(describeError(err));
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [sort, order, keyword, reloadToken]);

  return { games, loading, error, reload };
}
