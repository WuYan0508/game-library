/** 与 Rust 侧 `tracker::session` / `tracker::recover` 的 DTO 一一对应。 */

export interface SessionStarted {
  gameId: string;
  sessionId: string;
  startedAt: string;
  processId: number;
}

export type SessionStatus = "running" | "completed" | "interrupted";

export interface SessionEnded {
  gameId: string;
  sessionId: string;
  status: SessionStatus;
  startedAt: string;
  endedAt: string;
}

/** 会话进度（约每 2 秒推送一次）。 */
export interface SessionTick {
  gameId: string;
  sessionId: string;
  /** 从会话开始累计的秒数（不含系统休眠）。 */
  elapsedSeconds: number;
}

export interface ActiveSession {
  gameId: string;
  sessionId: string;
  startedAt: string;
  processId: number;
}

/** 启动时被保守恢复的残留会话。 */
export interface RecoveredSession {
  sessionId: string;
  gameId: string;
  /** 按最后一次心跳结算的秒数。没来得及心跳就是 0。 */
  durationSeconds: number;
}

export interface StopTrackingResult {
  /** false 表示该游戏本来就没在跟踪。 */
  stopped: boolean;
  session: SessionEnded | null;
}

/** 后端推送的事件名。 */
export const SESSION_EVENTS = {
  started: "session://started",
  tick: "session://tick",
  ended: "session://ended",
  recovered: "sessions://recovered",
} as const;
