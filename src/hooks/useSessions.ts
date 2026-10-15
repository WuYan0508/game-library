/** 会话状态：谁在运行、玩到第几秒、启动与停止跟踪。
 *
 * 运行状态与实时进度都以后端事件为准（`session://started` / `session://tick` /
 * `session://ended`），前端**不做任何计时判断** —— 计时权威只在 Rust 后端。
 */

import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { gameApi } from "../services/gameApi";
import { toUserMessage } from "../services/errorMessages";
import { pushToast } from "../services/toastStore";
import type {
  ActiveSession,
  RecoveredSession,
  SessionEnded,
  SessionStarted,
  SessionTick,
} from "../types/session";
import { SESSION_EVENTS } from "../types/session";

interface UseSessionsResult {
  /** gameId → 活动会话。 */
  active: Map<string, ActiveSession>;
  /** gameId → 本次会话已玩秒数（由 tick 事件驱动）。 */
  elapsed: Map<string, number>;
  /** 正在等待启动响应的 gameId 集合（按钮置灰用）。 */
  launching: Set<string>;
  /** 启动时恢复的残留会话；非空时前端应提示。 */
  recovered: RecoveredSession[];
  dismissRecovered: () => void;
  launch: (gameId: string) => Promise<void>;
  stopTracking: (gameId: string) => Promise<void>;
}

export function useSessions(onLibraryChanged: () => void): UseSessionsResult {
  const [active, setActive] = useState<Map<string, ActiveSession>>(new Map());
  const [elapsed, setElapsed] = useState<Map<string, number>>(new Map());
  const [launching, setLaunching] = useState<Set<string>>(new Set());
  const [recovered, setRecovered] = useState<RecoveredSession[]>([]);

  // 启动时同步一次：应用重启后可能仍有活动会话
  useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const sessions = await gameApi.getActiveSessions();
        if (cancelled) {
          return;
        }
        setActive(new Map(sessions.map((session) => [session.gameId, session])));
      } catch (err) {
        // 启动期的同步失败用 Toast 提示，不阻塞页面
        if (!cancelled) {
          pushToast(toUserMessage(err));
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  // 订阅后端事件
  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];

    void (async () => {
      const unlistenStarted = await listen<SessionStarted>(
        SESSION_EVENTS.started,
        (event) => {
          const payload = event.payload;
          setActive((previous) => {
            const next = new Map(previous);
            next.set(payload.gameId, {
              gameId: payload.gameId,
              sessionId: payload.sessionId,
              startedAt: payload.startedAt,
              processId: payload.processId,
            });
            return next;
          });
          // 新会话从 0 开始计
          setElapsed((previous) => {
            const next = new Map(previous);
            next.set(payload.gameId, 0);
            return next;
          });
        },
      );

      const unlistenTick = await listen<SessionTick>(
        SESSION_EVENTS.tick,
        (event) => {
          const payload = event.payload;
          setElapsed((previous) => {
            const next = new Map(previous);
            next.set(payload.gameId, payload.elapsedSeconds);
            return next;
          });
        },
      );

      const unlistenEnded = await listen<SessionEnded>(
        SESSION_EVENTS.ended,
        (event) => {
          const payload = event.payload;
          setActive((previous) => {
            const next = new Map(previous);
            // 只移除「同一个会话」，避免把新一轮会话误删
            if (next.get(payload.gameId)?.sessionId === payload.sessionId) {
              next.delete(payload.gameId);
            }
            return next;
          });
          setElapsed((previous) => {
            const next = new Map(previous);
            next.delete(payload.gameId);
            return next;
          });
          // 会话结束意味着累计时长变了，刷新列表
          onLibraryChanged();
        },
      );

      const unlistenRecovered = await listen<RecoveredSession[]>(
        SESSION_EVENTS.recovered,
        (event) => {
          if (event.payload.length > 0) {
            setRecovered(event.payload);
          }
        },
      );

      if (disposed) {
        for (const stop of [
          unlistenStarted,
          unlistenTick,
          unlistenEnded,
          unlistenRecovered,
        ]) {
          stop();
        }
      } else {
        unlisteners.push(
          unlistenStarted,
          unlistenTick,
          unlistenEnded,
          unlistenRecovered,
        );
      }
    })();

    return () => {
      disposed = true;
      for (const unlisten of unlisteners) {
        unlisten();
      }
    };
  }, [onLibraryChanged]);

  const launch = useCallback(async (gameId: string) => {
    setLaunching((previous) => new Set(previous).add(gameId));
    try {
      await gameApi.launchGame(gameId);
      // 活动状态由 `session://started` 事件更新，这里不抢先写入
    } catch (err) {
      pushToast(toUserMessage(err));
    } finally {
      setLaunching((previous) => {
        const next = new Set(previous);
        next.delete(gameId);
        return next;
      });
    }
  }, []);

  const stopTracking = useCallback(
    async (gameId: string) => {
        try {
        await gameApi.stopTracking(gameId);
        onLibraryChanged();
      } catch (err) {
        pushToast(toUserMessage(err));
      }
    },
    [onLibraryChanged],
  );

  const dismissRecovered = useCallback(() => setRecovered([]), []);

  return {
    active,
    elapsed,
    launching,
    recovered,
    dismissRecovered,
    launch,
    stopTracking,
  };
}
