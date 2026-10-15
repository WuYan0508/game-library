//! 去重判定。
//!
//! 规范化去重键的规则定义在 `database::games::normalize_launch_key` —— 它是唯一索引
//! `idx_games_dedupe` 的依据，必须与落库时完全一致，因此不在这里另写一份。
//! 本模块只负责「按启动目标查出已有记录」并提供给前端的精简引用。

use rusqlite::Connection;
use serde::Serialize;

use crate::database::games;
use crate::database::models::Game;
use crate::error::AppResult;

/// 供前端展示的已有记录引用。
///
/// 探测阶段就把重复项暴露出来，用户在确认导入前就能看到，
/// 而不是等到落库失败才被告知。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateRef {
    pub id: String,
    pub name: String,
}

impl From<&Game> for DuplicateRef {
    fn from(game: &Game) -> Self {
        Self {
            id: game.id.clone(),
            name: game.name.clone(),
        }
    }
}

/// 按「规范化启动目标 + 参数」查找已有记录。
pub fn find_existing(
    conn: &Connection,
    launch_path: &str,
    launch_args: &str,
) -> AppResult<Option<Game>> {
    games::find_by_launch_key(conn, launch_path, launch_args)
}
