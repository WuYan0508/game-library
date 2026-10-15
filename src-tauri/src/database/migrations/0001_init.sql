-- 初始表结构（对应 docs/architecture.md §5）。
-- 所有时间列存 UTC ISO8601 文本；时长统一为整数秒。

CREATE TABLE games (
  id                 TEXT    PRIMARY KEY,              -- UUID v4
  name               TEXT    NOT NULL,                 -- 用户可编辑的显示名称
  original_name      TEXT    NOT NULL,                 -- 导入时解析出的原始名称
  source_type        TEXT    NOT NULL CHECK (source_type IN ('exe', 'shortcut')),
  source_path        TEXT    NOT NULL,                 -- 用户选择的原始 EXE / .lnk 路径
  launch_path        TEXT    NOT NULL,                 -- 实际启动目标
  launch_args        TEXT    NOT NULL DEFAULT '',      -- 原样参数串，不做分词
  working_directory  TEXT,                             -- 可为空
  cover_path         TEXT,                             -- 相对 app_data_dir，可为空
  icon_path          TEXT,                             -- 相对 app_data_dir，可为空
  launch_path_key    TEXT    NOT NULL,                 -- 规范化去重键
  is_launcher        INTEGER NOT NULL DEFAULT 0,       -- 目标疑似平台启动器
  total_play_seconds INTEGER NOT NULL DEFAULT 0 CHECK (total_play_seconds >= 0),
  last_played_at     TEXT,
  created_at         TEXT    NOT NULL,
  updated_at         TEXT    NOT NULL
);

-- 从数据库层面杜绝「无提示创建重复项」
CREATE UNIQUE INDEX idx_games_dedupe ON games (launch_path_key);
CREATE INDEX        idx_games_name   ON games (name);

CREATE TABLE play_sessions (
  id                     TEXT    PRIMARY KEY,
  game_id                TEXT    NOT NULL REFERENCES games (id) ON DELETE CASCADE,
  started_at             TEXT    NOT NULL,             -- UTC ISO8601（墙钟，仅展示）
  ended_at               TEXT,
  duration_seconds       INTEGER NOT NULL DEFAULT 0 CHECK (duration_seconds >= 0),
  tracked_seconds        INTEGER NOT NULL DEFAULT 0 CHECK (tracked_seconds >= 0),
  last_heartbeat_at      TEXT,
  status                 TEXT    NOT NULL CHECK (status IN ('running', 'completed', 'interrupted')),
  process_id             INTEGER NOT NULL,
  process_start_identity TEXT    NOT NULL,             -- 进程创建时间，用于识别 PID 复用
  created_at             TEXT    NOT NULL
);

CREATE INDEX idx_sessions_game   ON play_sessions (game_id, started_at DESC);
CREATE INDEX idx_sessions_status ON play_sessions (status);

-- 同一游戏同时只允许一个活动会话：数据库级保证不重复计时
CREATE UNIQUE INDEX idx_sessions_single_running
  ON play_sessions (game_id) WHERE status = 'running';

CREATE TABLE settings (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
