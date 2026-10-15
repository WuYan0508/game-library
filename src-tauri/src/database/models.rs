//! 行模型与排序参数。序列化后与 `src/types/game.ts` 一一对应。

use rusqlite::Row;
use serde::Serialize;

use crate::error::{AppError, AppResult};

/// 数据来源类型。与 `games.source_type` 的 CHECK 约束一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    Exe,
    Shortcut,
}

impl SourceType {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceType::Exe => "exe",
            SourceType::Shortcut => "shortcut",
        }
    }
}

/// 一条游戏记录。`launch_path_key` 属内部去重键，不对外暴露。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: String,
    pub name: String,
    pub original_name: String,
    pub source_type: String,
    pub source_path: String,
    pub launch_path: String,
    pub launch_args: String,
    pub working_directory: Option<String>,
    pub cover_path: Option<String>,
    pub icon_path: Option<String>,
    pub is_launcher: bool,
    pub total_play_seconds: i64,
    pub last_played_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Game {
    /// 查询列清单。集中一处，避免各处 SELECT 字段不一致。
    pub(crate) const COLUMNS: &'static str = "id, name, original_name, source_type, source_path, \
         launch_path, launch_args, working_directory, cover_path, icon_path, is_launcher, \
         total_play_seconds, last_played_at, created_at, updated_at";

    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            name: row.get("name")?,
            original_name: row.get("original_name")?,
            source_type: row.get("source_type")?,
            source_path: row.get("source_path")?,
            launch_path: row.get("launch_path")?,
            launch_args: row.get("launch_args")?,
            working_directory: row.get("working_directory")?,
            cover_path: row.get("cover_path")?,
            icon_path: row.get("icon_path")?,
            is_launcher: row.get::<_, i64>("is_launcher")? != 0,
            total_play_seconds: row.get("total_play_seconds")?,
            last_played_at: row.get("last_played_at")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

/// 一条游玩会话记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaySession {
    pub id: String,
    pub game_id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_seconds: i64,
    pub tracked_seconds: i64,
    pub last_heartbeat_at: Option<String>,
    pub status: String,
    pub process_id: i64,
    pub process_start_identity: String,
    pub created_at: String,
}

impl PlaySession {
    pub(crate) const COLUMNS: &'static str = "id, game_id, started_at, ended_at, duration_seconds, \
         tracked_seconds, last_heartbeat_at, status, process_id, process_start_identity, created_at";

    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            game_id: row.get("game_id")?,
            started_at: row.get("started_at")?,
            ended_at: row.get("ended_at")?,
            duration_seconds: row.get("duration_seconds")?,
            tracked_seconds: row.get("tracked_seconds")?,
            last_heartbeat_at: row.get("last_heartbeat_at")?,
            status: row.get("status")?,
            process_id: row.get("process_id")?,
            process_start_identity: row.get("process_start_identity")?,
            created_at: row.get("created_at")?,
        })
    }
}

/// 排序字段。只允许白名单值，列名不来自用户输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameSort {
    Name,
    PlayTime,
    CreatedAt,
    /// 用户在卡片视图里手动拖出的顺序（sort_order 列）。
    Custom,
}

impl GameSort {
    pub fn parse(value: &str) -> AppResult<Self> {
        match value {
            "name" => Ok(GameSort::Name),
            "playTime" => Ok(GameSort::PlayTime),
            "createdAt" => Ok(GameSort::CreatedAt),
            "custom" => Ok(GameSort::Custom),
            other => Err(AppError::invalid_argument(format!(
                "不支持的排序字段：{other}"
            ))),
        }
    }

    pub fn column(self) -> &'static str {
        match self {
            GameSort::Name => "name COLLATE NOCASE",
            GameSort::PlayTime => "total_play_seconds",
            GameSort::CreatedAt => "created_at",
            GameSort::Custom => "sort_order",
        }
    }
}

/// 排序方向。同样只允许白名单值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    pub fn parse(value: &str) -> AppResult<Self> {
        match value {
            "asc" => Ok(SortOrder::Asc),
            "desc" => Ok(SortOrder::Desc),
            other => Err(AppError::invalid_argument(format!(
                "不支持的排序方向：{other}"
            ))),
        }
    }

    pub fn keyword(self) -> &'static str {
        match self {
            SortOrder::Asc => "ASC",
            SortOrder::Desc => "DESC",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_sort_field() {
        let err =
            GameSort::parse("total_play_seconds; DROP TABLE games").expect_err("应拒绝非白名单值");
        assert_eq!(err.code(), crate::error::AppErrorCode::InvalidArgument);
    }

    #[test]
    fn rejects_unknown_sort_order() {
        assert!(SortOrder::parse("ascending").is_err());
    }

    #[test]
    fn parses_known_values() {
        assert_eq!(GameSort::parse("name").expect("name"), GameSort::Name);
        assert_eq!(
            GameSort::parse("playTime").expect("playTime"),
            GameSort::PlayTime
        );
        assert_eq!(
            GameSort::parse("createdAt").expect("createdAt"),
            GameSort::CreatedAt
        );
        assert_eq!(SortOrder::parse("asc").expect("asc"), SortOrder::Asc);
        assert_eq!(SortOrder::parse("desc").expect("desc"), SortOrder::Desc);
    }
}
