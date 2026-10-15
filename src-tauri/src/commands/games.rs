//! 游戏库管理命令（对应 docs/architecture.md §7.1「游戏管理」）。
//!
//! 本层只做参数校验与 DTO 组装，业务规则在 `database::games`。

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::covers::cover_store;
use crate::database::Db;
use crate::database::games::{self, GamePatch};
use crate::database::models::{Game, GameSort, SortOrder};
use crate::error::AppResult;

use super::app_data_dir;

#[tauri::command]
pub fn list_games(db: State<'_, Db>, sort: String, order: String) -> AppResult<Vec<Game>> {
    let sort = GameSort::parse(&sort)?;
    let order = SortOrder::parse(&order)?;
    db.with_conn(|conn| games::list(conn, sort, order))
}

#[tauri::command]
pub fn get_game(db: State<'_, Db>, id: String) -> AppResult<Game> {
    db.with_conn(|conn| games::get(conn, &id))
}

/// 关键词为空时退化为完整列表，避免前端分两条路径。
#[tauri::command]
pub fn search_games(
    db: State<'_, Db>,
    keyword: String,
    sort: String,
    order: String,
) -> AppResult<Vec<Game>> {
    let sort = GameSort::parse(&sort)?;
    let order = SortOrder::parse(&order)?;

    if keyword.trim().is_empty() {
        return db.with_conn(|conn| games::list(conn, sort, order));
    }

    db.with_conn(|conn| games::search(conn, &keyword, sort, order))
}

/// 局部更新：未提供的字段保持原值；`workingDirectory` 传空串表示清空。
#[tauri::command]
pub fn update_game(
    db: State<'_, Db>,
    id: String,
    name: Option<String>,
    launch_path: Option<String>,
    launch_args: Option<String>,
    working_directory: Option<String>,
) -> AppResult<Game> {
    let patch = GamePatch {
        name: name.as_deref(),
        launch_path: launch_path.as_deref(),
        launch_args: launch_args.as_deref(),
        working_directory: working_directory.as_deref(),
    };

    db.with_conn(|conn| games::update(conn, &id, &patch))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveGameResult {
    /// 被一并清理的会话记录数（外键级联）。
    pub removed_sessions: i64,
    /// 被回收的封面 / 图标文件数。原始游戏文件不受影响。
    pub removed_assets: usize,
}

/// 从游戏库移除记录。**只删库内数据与应用自管的封面/图标，绝不触碰原始文件。**
#[tauri::command]
pub fn remove_game(app: AppHandle, db: State<'_, Db>, id: String) -> AppResult<RemoveGameResult> {
    remove_game_impl(&app_data_dir(&app)?, &db, &id)
}

pub(crate) fn remove_game_impl(data_dir: &Path, db: &Db, id: &str) -> AppResult<RemoveGameResult> {
    let removed_sessions = db.with_tx(|tx| {
        let session_count = games::count_sessions(tx, id)?;
        games::remove(tx, id)?;
        Ok(session_count)
    })?;

    // 记录删掉之后再回收：此时这条记录的封面/图标已经不在「被引用」集合里。
    // 回收只针对**本应用生成的**文件（UUID 命名的封面、icons/ 下的 png）。
    let referenced: HashSet<String> = db.with_conn(games::all_asset_paths)?.into_iter().collect();
    let removed_assets = cover_store::gc(&referenced, data_dir)?;

    Ok(RemoveGameResult {
        removed_sessions,
        removed_assets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::import::add_game_from_exe_impl;
    use crate::database::models::GameSort;
    use crate::test_support::{TempDir, real_exe};

    fn list_all(db: &Db) -> Vec<Game> {
        db.with_conn(|conn| games::list(conn, GameSort::CreatedAt, SortOrder::Asc))
            .expect("列出")
    }

    #[test]
    fn remove_keeps_other_games_assets_and_reclaims_its_own() {
        let Some(exe_a) = real_exe() else {
            eprintln!("跳过：本机找不到可用的系统 exe");
            return;
        };
        // 需要一个「不同的启动目标」来绕过去重 —— 用同一个 exe 加不同参数是不行的
        // （EXE 导入不带参数），所以这里直接构造两条手动记录更直接。
        let temp = TempDir::new("remove");
        let db = Db::open_in_memory().expect("内存库");

        let first = add_game_from_exe_impl(&db, &temp.data(), exe_a, None, None).expect("导入 A");
        let first_icon = first.icon_path.clone().expect("A 应有图标");
        let first_icon_abs = temp
            .data()
            .join(first_icon.replace('/', std::path::MAIN_SEPARATOR_STR));
        assert!(first_icon_abs.exists());

        // 再插一条指向别处的记录（不经过导入流程，避免文件校验干扰）
        let second = db
            .with_conn(|conn| {
                games::insert(
                    conn,
                    &crate::database::games::InsertGame {
                        id: games::new_id(),
                        name: "第二条",
                        original_name: "第二条",
                        source_type: crate::database::models::SourceType::Exe,
                        source_path: r"C:\elsewhere\other.exe",
                        launch_path: r"C:\elsewhere\other.exe",
                        launch_args: "",
                        working_directory: None,
                        cover_path: None,
                        icon_path: None,
                        is_launcher: false,
                    },
                )
            })
            .expect("插入 B");

        assert_eq!(list_all(&db).len(), 2);

        let result = remove_game_impl(&temp.data(), &db, &first.id).expect("移除 A");

        assert_eq!(result.removed_assets, 1, "A 的图标应被回收");
        assert!(!first_icon_abs.exists(), "A 的图标文件应已删除");
        assert_eq!(list_all(&db).len(), 1);

        // B 还在，且它的记录未被波及
        let remaining = db
            .with_conn(|conn| games::get(conn, &second.id))
            .expect("B 应还在");
        assert_eq!(remaining.name, "第二条");
    }

    #[test]
    fn remove_missing_record_reports_not_found() {
        let temp = TempDir::new("remove-missing");
        let db = Db::open_in_memory().expect("内存库");
        let err = remove_game_impl(&temp.data(), &db, "不存在的id").expect_err("应报错");
        assert_eq!(err.code(), crate::error::AppErrorCode::NotFound);
    }
}

/// 自定义排序：按前端给出的完整 id 顺序持久化。
#[tauri::command]
pub fn reorder_games(db: State<'_, Db>, ordered_ids: Vec<String>) -> AppResult<()> {
    let ids: Vec<String> = ordered_ids;
    db.with_conn(|conn| games::reorder(conn, &ids.iter().map(String::as_str).collect::<Vec<_>>()))
}
