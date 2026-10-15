//! `games` 表的读写。
//!
//! 约定：
//! - 排序字段与方向走白名单枚举，**列名永远不来自用户输入**；
//! - 模糊搜索对 `%` `_` `\` 做转义，避免用户输入的 `%` 变成通配符；
//! - 去重键先在应用侧查重给出可读错误，唯一索引作为并发兜底。

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{AppError, AppResult};

use super::models::{Game, GameSort, SortOrder, SourceType};

/// SQLITE_CONSTRAINT_UNIQUE
const SQLITE_CONSTRAINT_UNIQUE: i32 = 2067;
/// SQLITE_CONSTRAINT_PRIMARYKEY
const SQLITE_CONSTRAINT_PRIMARYKEY: i32 = 1555;

/// 当前 UTC 时间，ISO8601 毫秒精度（例：`2026-10-14T03:30:00.000Z`）。
pub fn now_utc() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 规范化去重键：解析真实路径 → 统一分隔符 → 去尾随分隔符 → 转小写 → 拼接参数。
///
/// 只按启动路径与参数去重，**不按显示名称**（不同游戏可能同名）。
pub fn normalize_launch_key(launch_path: &str, launch_args: &str) -> String {
    let resolved = std::fs::canonicalize(launch_path)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| launch_path.to_owned());

    let unified = resolved.replace('/', "\\");
    let mut key = unified.trim_end_matches('\\').to_lowercase();
    if key.is_empty() {
        key = unified.to_lowercase();
    }

    let args = launch_args.trim();
    if !args.is_empty() {
        key.push('\u{1f}');
        key.push_str(&args.to_lowercase());
    }

    key
}

/// 转义 LIKE 通配符，配合 `ESCAPE '\'` 使用。
pub fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 4);
    for ch in input.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// 生成一个新的记录 id。
///
/// 由调用方先生成 id，是为了在写库**之前**就能确定图标文件名 ——
/// 这样「落图标 → 写记录」可以合并成一次写入，不留半成品。
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 待写入的新记录。
pub struct InsertGame<'a> {
    pub id: String,
    pub name: &'a str,
    pub original_name: &'a str,
    pub source_type: SourceType,
    pub source_path: &'a str,
    pub launch_path: &'a str,
    pub launch_args: &'a str,
    pub working_directory: Option<&'a str>,
    pub cover_path: Option<&'a str>,
    pub icon_path: Option<&'a str>,
    pub is_launcher: bool,
}

/// 局部更新。`None` 表示不修改；`working_directory` 传 `Some("")` 表示清空。
#[derive(Default)]
pub struct GamePatch<'a> {
    pub name: Option<&'a str>,
    pub launch_path: Option<&'a str>,
    pub launch_args: Option<&'a str>,
    pub working_directory: Option<&'a str>,
}

/// 写入一条新游戏记录。去重键冲突时返回 `Duplicate`。
pub fn insert(conn: &Connection, input: &InsertGame<'_>) -> AppResult<Game> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::invalid_argument("游戏名称不能为空"));
    }
    let launch_path = input.launch_path.trim();
    if launch_path.is_empty() {
        return Err(AppError::invalid_argument("启动目标不能为空"));
    }

    let key = normalize_launch_key(launch_path, input.launch_args);
    if find_by_key(conn, &key)?.is_some() {
        return Err(AppError::duplicate("该游戏已在库中"));
    }

    let id = input.id.clone();
    let now = now_utc();

    conn.execute(
        "INSERT INTO games (id, name, original_name, source_type, source_path, launch_path, \
         launch_args, working_directory, cover_path, icon_path, launch_path_key, is_launcher, \
         total_play_seconds, last_played_at, created_at, updated_at, sort_order) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, NULL, ?13, ?13, \
         (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM games))",
        params![
            id,
            name,
            input.original_name.trim(),
            input.source_type.as_str(),
            input.source_path,
            launch_path,
            input.launch_args,
            input.working_directory,
            input.cover_path,
            input.icon_path,
            key,
            i64::from(input.is_launcher),
            now,
        ],
    )
    .map_err(|err| {
        if is_unique_violation(&err) {
            AppError::duplicate("该游戏已在库中").with_detail(err.to_string())
        } else {
            AppError::database("保存游戏记录失败").with_detail(err.to_string())
        }
    })?;

    get(conn, &id)
}

/// 按前端给出的完整 id 顺序重排（自定义排序）。
///
/// `ordered_ids` 必须覆盖当前全部游戏；未覆盖的（并发导入等边缘情况）
/// 按原相对顺序接在末尾。
pub fn reorder(conn: &Connection, ordered_ids: &[&str]) -> AppResult<()> {
    let tx = conn
        .unchecked_transaction()
        .map_err(db_error("开启重排事务失败"))?;

    // 先全部标记为未分配，再按给定顺序落位
    tx.execute("UPDATE games SET sort_order = -1", [])
        .map_err(db_error("重置排序失败"))?;
    for (index, id) in ordered_ids.iter().enumerate() {
        tx.execute(
            "UPDATE games SET sort_order = ?1 WHERE id = ?2",
            params![index as i64, id],
        )
        .map_err(db_error("更新排序失败"))?;
    }

    // 未列出的游戏接在末尾，保持相对顺序
    let base = ordered_ids.len() as i64;
    tx.execute(
        "UPDATE games \
         SET sort_order = ?1 - 1 + (SELECT COUNT(*) FROM games g2 \
             WHERE g2.sort_order < 0 AND g2.rowid <= games.rowid) \
         WHERE sort_order < 0",
        params![base],
    )
    .map_err(db_error("追加未排序游戏失败"))?;

    tx.commit().map_err(db_error("提交重排失败"))
}

/// 更新某个游戏的图标相对路径（重新提取图标后回写）。
pub fn update_icon_path(conn: &Connection, id: &str, icon_path: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE games SET icon_path = ?1, updated_at = ?2 WHERE id = ?3",
        params![icon_path, now_utc(), id],
    )
    .map_err(db_error("更新图标路径失败"))?;
    Ok(())
}

pub fn list(conn: &Connection, sort: GameSort, order: SortOrder) -> AppResult<Vec<Game>> {
    let sql = format!(
        "SELECT {} FROM games ORDER BY {} {}",
        Game::COLUMNS,
        sort.column(),
        order.keyword()
    );
    let mut stmt = conn.prepare(&sql).map_err(db_error("准备列表查询失败"))?;
    let games = stmt
        .query_map([], Game::from_row)
        .map_err(db_error("查询游戏列表失败"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error("读取游戏列表失败"))?;
    Ok(games)
}

pub fn search(
    conn: &Connection,
    keyword: &str,
    sort: GameSort,
    order: SortOrder,
) -> AppResult<Vec<Game>> {
    let pattern = format!("%{}%", escape_like(keyword.trim()));
    let sql = format!(
        "SELECT {} FROM games \
         WHERE name LIKE ?1 ESCAPE '\\' OR original_name LIKE ?1 ESCAPE '\\' \
         ORDER BY {} {}",
        Game::COLUMNS,
        sort.column(),
        order.keyword()
    );
    let mut stmt = conn.prepare(&sql).map_err(db_error("准备搜索查询失败"))?;
    let games = stmt
        .query_map(params![pattern], Game::from_row)
        .map_err(db_error("搜索游戏失败"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error("读取搜索结果失败"))?;
    Ok(games)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Game> {
    let sql = format!("SELECT {} FROM games WHERE id = ?1", Game::COLUMNS);
    conn.query_row(&sql, params![id], Game::from_row)
        .optional()
        .map_err(db_error("查询游戏失败"))?
        .ok_or_else(|| AppError::not_found("找不到该游戏记录"))
}

/// 局部更新。**改显示名称不会改动启动路径**（两者互不影响）。
pub fn update(conn: &Connection, id: &str, patch: &GamePatch<'_>) -> AppResult<Game> {
    let current = get(conn, id)?;

    let name = match patch.name {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(AppError::invalid_argument("游戏名称不能为空"));
            }
            trimmed.to_owned()
        }
        None => current.name.clone(),
    };

    let launch_path = match patch.launch_path {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(AppError::invalid_argument("启动目标不能为空"));
            }
            trimmed.to_owned()
        }
        None => current.launch_path.clone(),
    };

    let launch_args = patch
        .launch_args
        .map(str::to_owned)
        .unwrap_or_else(|| current.launch_args.clone());

    let working_directory = match patch.working_directory {
        Some(value) if value.trim().is_empty() => None,
        Some(value) => Some(value.trim().to_owned()),
        None => current.working_directory.clone(),
    };

    let key = normalize_launch_key(&launch_path, &launch_args);
    if let Some(existing) = find_by_key(conn, &key)?
        && existing.id != current.id
    {
        return Err(AppError::duplicate("另一个游戏已使用相同的启动目标与参数"));
    }

    let affected = conn
        .execute(
            "UPDATE games SET name = ?2, launch_path = ?3, launch_args = ?4, \
             working_directory = ?5, launch_path_key = ?6, updated_at = ?7 WHERE id = ?1",
            params![
                current.id,
                name,
                launch_path,
                launch_args,
                working_directory,
                key,
                now_utc(),
            ],
        )
        .map_err(|err| {
            if is_unique_violation(&err) {
                AppError::duplicate("另一个游戏已使用相同的启动目标与参数")
                    .with_detail(err.to_string())
            } else {
                AppError::database("更新游戏记录失败").with_detail(err.to_string())
            }
        })?;

    if affected == 0 {
        return Err(AppError::not_found("找不到该游戏记录"));
    }

    get(conn, &current.id)
}

/// 从库中移除记录。会话记录由外键 `ON DELETE CASCADE` 一并清理。
///
/// **绝不触碰任何原始文件**（EXE / `.lnk` / 游戏目录）。
pub fn remove(conn: &Connection, id: &str) -> AppResult<()> {
    let affected = conn
        .execute("DELETE FROM games WHERE id = ?1", params![id])
        .map_err(db_error("删除游戏记录失败"))?;

    if affected == 0 {
        return Err(AppError::not_found("找不到该游戏记录"));
    }
    Ok(())
}

pub fn count_sessions(conn: &Connection, game_id: &str) -> AppResult<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM play_sessions WHERE game_id = ?1",
        params![game_id],
        |row| row.get(0),
    )
    .map_err(db_error("统计会话数失败"))
}

/// 设置封面的相对路径。传 `None` 表示清除。
pub fn set_cover_path(conn: &Connection, id: &str, cover_path: Option<&str>) -> AppResult<Game> {
    set_asset_path(conn, id, "cover_path", cover_path)
}

/// 设置图标的相对路径。传 `None` 表示清除。
pub fn set_icon_path(conn: &Connection, id: &str, icon_path: Option<&str>) -> AppResult<Game> {
    set_asset_path(conn, id, "icon_path", icon_path)
}

/// 列名只允许在本函数内出现，因此不存在 SQL 注入面。
fn set_asset_path(
    conn: &Connection,
    id: &str,
    column: &'static str,
    value: Option<&str>,
) -> AppResult<Game> {
    debug_assert!(matches!(column, "cover_path" | "icon_path"));

    let sql = format!("UPDATE games SET {column} = ?2, updated_at = ?3 WHERE id = ?1");
    let affected = conn
        .execute(&sql, params![id, value, now_utc()])
        .map_err(db_error("更新资源路径失败"))?;

    if affected == 0 {
        return Err(AppError::not_found("找不到该游戏记录"));
    }

    get(conn, id)
}

/// 收集所有被引用的封面 / 图标相对路径，供资源回收使用。
pub fn all_asset_paths(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn
        .prepare(
            "SELECT cover_path, icon_path FROM games \
             WHERE cover_path IS NOT NULL OR icon_path IS NOT NULL",
        )
        .map_err(db_error("准备资源清单查询失败"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>("cover_path")?,
                row.get::<_, Option<String>>("icon_path")?,
            ))
        })
        .map_err(db_error("查询资源清单失败"))?;

    let mut paths = Vec::new();
    for row in rows {
        let (cover, icon) = row.map_err(db_error("读取资源清单失败"))?;
        paths.extend(cover);
        paths.extend(icon);
    }
    Ok(paths)
}

/// 按「启动目标 + 参数」查找已有记录。
///
/// 去重键的计算规则与唯一索引 `idx_games_dedupe` 完全一致，
/// 导入流程在探测阶段用它提前发现重复项。
pub fn find_by_launch_key(
    conn: &Connection,
    launch_path: &str,
    launch_args: &str,
) -> AppResult<Option<Game>> {
    find_by_key(conn, &normalize_launch_key(launch_path, launch_args))
}

fn find_by_key(conn: &Connection, key: &str) -> AppResult<Option<Game>> {
    let sql = format!(
        "SELECT {} FROM games WHERE launch_path_key = ?1",
        Game::COLUMNS
    );
    conn.query_row(&sql, params![key], Game::from_row)
        .optional()
        .map_err(db_error("按去重键查询失败"))
}

fn is_unique_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.extended_code == SQLITE_CONSTRAINT_UNIQUE
                || inner.extended_code == SQLITE_CONSTRAINT_PRIMARYKEY
    )
}

fn db_error(message: &'static str) -> impl Fn(rusqlite::Error) -> AppError {
    move |err| AppError::database(message).with_detail(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Db;

    fn sample<'a>(name: &'a str, launch_path: &'a str) -> InsertGame<'a> {
        InsertGame {
            id: new_id(),
            name,
            original_name: name,
            source_type: SourceType::Exe,
            source_path: launch_path,
            launch_path,
            launch_args: "",
            working_directory: None,
            cover_path: None,
            icon_path: None,
            is_launcher: false,
        }
    }

    #[test]
    fn insert_then_get_round_trip() {
        let db = Db::open_in_memory().expect("内存库");
        let game = db
            .with_conn(|conn| insert(conn, &sample("测试游戏", "C:\\Games\\demo.exe")))
            .expect("插入应成功");

        assert_eq!(game.name, "测试游戏");
        assert_eq!(game.source_type, "exe");
        assert_eq!(game.total_play_seconds, 0);
        assert!(!game.is_launcher);
        assert!(game.last_played_at.is_none());
        assert!(!game.created_at.is_empty());

        let fetched = db
            .with_conn(|conn| get(conn, &game.id))
            .expect("应能按 id 查回");
        assert_eq!(fetched, game);
    }

    #[test]
    fn reject_duplicate_launch_target_ignoring_case_and_separators() {
        let db = Db::open_in_memory().expect("内存库");
        db.with_conn(|conn| insert(conn, &sample("游戏甲", "C:\\Games\\demo.exe")))
            .expect("首次插入");

        let err = db
            .with_conn(|conn| insert(conn, &sample("游戏乙", "c:/games/DEMO.exe")))
            .expect_err("同一路径不同写法应被判重");
        assert_eq!(err.code(), crate::error::AppErrorCode::Duplicate);
    }

    #[test]
    fn same_name_different_path_is_allowed() {
        let db = Db::open_in_memory().expect("内存库");
        db.with_conn(|conn| insert(conn, &sample("同名游戏", "C:\\A\\game.exe")))
            .expect("第一条");
        db.with_conn(|conn| insert(conn, &sample("同名游戏", "C:\\B\\game.exe")))
            .expect("同名不同路径应允许");
    }

    #[test]
    fn different_args_are_distinct_records() {
        let db = Db::open_in_memory().expect("内存库");
        db.with_conn(|conn| insert(conn, &sample("带参游戏", "C:\\Games\\demo.exe")))
            .expect("无参");

        let mut with_args = sample("带参游戏", "C:\\Games\\demo.exe");
        with_args.launch_args = "-windowed";
        db.with_conn(|conn| insert(conn, &with_args))
            .expect("参数不同应视为不同记录");
    }

    #[test]
    fn records_survive_reopen() {
        let dir = std::env::temp_dir().join(format!("game-lib-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let db_path = dir.join("game_library.db");

        let id = {
            let db = Db::open(&db_path).expect("打开文件库");
            db.with_conn(|conn| insert(conn, &sample("持久化游戏", "C:\\Games\\persist.exe")))
                .expect("插入")
                .id
        };

        // 重新打开：模拟应用重启
        let db = Db::open(&db_path).expect("重新打开文件库");
        let game = db
            .with_conn(|conn| get(conn, &id))
            .expect("重启后应仍能查到记录");
        assert_eq!(game.name, "持久化游戏");

        drop(db);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn search_treats_percent_and_underscore_as_literals() {
        let db = Db::open_in_memory().expect("内存库");
        db.with_conn(|conn| {
            insert(conn, &sample("进度100%完成", "C:\\A\\a.exe"))?;
            insert(conn, &sample("ABC_DEF", "C:\\B\\b.exe"))?;
            insert(conn, &sample("普通游戏", "C:\\C\\c.exe"))?;
            Ok(())
        })
        .expect("准备数据");

        let only_percent = db
            .with_conn(|conn| search(conn, "%", GameSort::Name, SortOrder::Asc))
            .expect("搜索 %");
        assert_eq!(
            only_percent.len(),
            1,
            "`%` 应被当作字面量，只匹配含 % 的记录，实际 {:?}",
            only_percent.iter().map(|g| &g.name).collect::<Vec<_>>()
        );
        assert_eq!(only_percent[0].name, "进度100%完成");

        let only_underscore = db
            .with_conn(|conn| search(conn, "_", GameSort::Name, SortOrder::Asc))
            .expect("搜索 _");
        assert_eq!(only_underscore.len(), 1);
        assert_eq!(only_underscore[0].name, "ABC_DEF");

        let plain = db
            .with_conn(|conn| search(conn, "普通", GameSort::Name, SortOrder::Asc))
            .expect("中文搜索");
        assert_eq!(plain.len(), 1);
    }

    #[test]
    fn search_matches_original_name_too() {
        let db = Db::open_in_memory().expect("内存库");
        db.with_conn(|conn| {
            let mut input = sample("显示名", "C:\\A\\x.exe");
            input.original_name = "原始名-Alpha";
            insert(conn, &input)?;
            Ok(())
        })
        .expect("准备数据");

        let hits = db
            .with_conn(|conn| search(conn, "Alpha", GameSort::Name, SortOrder::Asc))
            .expect("按原始名搜索");
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn list_supports_declared_sorts() {
        let db = Db::open_in_memory().expect("内存库");
        db.with_conn(|conn| {
            insert(conn, &sample("Bravo", "C:\\A\\b.exe"))?;
            insert(conn, &sample("alpha", "C:\\A\\a.exe"))?;
            Ok(())
        })
        .expect("准备数据");

        let by_name = db
            .with_conn(|conn| list(conn, GameSort::Name, SortOrder::Asc))
            .expect("按名称升序");
        assert_eq!(
            by_name.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "Bravo"],
            "名称排序应忽略大小写"
        );

        let by_created_desc = db
            .with_conn(|conn| list(conn, GameSort::CreatedAt, SortOrder::Desc))
            .expect("按创建时间降序");
        assert_eq!(by_created_desc.len(), 2);
    }

    #[test]
    fn update_name_does_not_touch_launch_path() {
        let db = Db::open_in_memory().expect("内存库");
        let game = db
            .with_conn(|conn| insert(conn, &sample("旧名", "C:\\Games\\keep.exe")))
            .expect("插入");

        let updated = db
            .with_conn(|conn| {
                update(
                    conn,
                    &game.id,
                    &GamePatch {
                        name: Some("新名"),
                        ..Default::default()
                    },
                )
            })
            .expect("改名");

        assert_eq!(updated.name, "新名");
        assert_eq!(
            updated.launch_path, game.launch_path,
            "改名不得影响启动路径"
        );
        assert_eq!(updated.created_at, game.created_at);
        assert_ne!(updated.updated_at, "", "更新时间应被写入");
    }

    #[test]
    fn update_rejects_empty_name() {
        let db = Db::open_in_memory().expect("内存库");
        let game = db
            .with_conn(|conn| insert(conn, &sample("游戏", "C:\\A\\a.exe")))
            .expect("插入");

        let err = db
            .with_conn(|conn| {
                update(
                    conn,
                    &game.id,
                    &GamePatch {
                        name: Some("   "),
                        ..Default::default()
                    },
                )
            })
            .expect_err("空名称应被拒绝");
        assert_eq!(err.code(), crate::error::AppErrorCode::InvalidArgument);
    }

    #[test]
    fn update_can_clear_working_directory() {
        let db = Db::open_in_memory().expect("内存库");
        let game = db
            .with_conn(|conn| {
                let mut input = sample("游戏", "C:\\A\\a.exe");
                input.working_directory = Some("C:\\A");
                insert(conn, &input)
            })
            .expect("插入");
        assert_eq!(game.working_directory.as_deref(), Some("C:\\A"));

        let cleared = db
            .with_conn(|conn| {
                update(
                    conn,
                    &game.id,
                    &GamePatch {
                        working_directory: Some("   "),
                        ..Default::default()
                    },
                )
            })
            .expect("清空工作目录");
        assert!(cleared.working_directory.is_none());
    }

    #[test]
    fn remove_deletes_record_and_cascades_sessions() {
        let db = Db::open_in_memory().expect("内存库");
        let game = db
            .with_conn(|conn| insert(conn, &sample("待删除", "C:\\A\\a.exe")))
            .expect("插入");

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO play_sessions (id, game_id, started_at, duration_seconds, tracked_seconds, \
                 status, process_id, process_start_identity, created_at) \
                 VALUES ('s1', ?1, '2026-10-14T00:00:00.000Z', 60, 60, 'completed', 1234, '133000000000000000', \
                 '2026-10-14T00:00:00.000Z')",
                params![game.id],
            )
            .map_err(db_error("插入会话"))?;
            assert_eq!(count_sessions(conn, &game.id)?, 1);
            Ok(())
        })
        .expect("准备会话");

        db.with_conn(|conn| remove(conn, &game.id)).expect("删除");

        db.with_conn(|conn| {
            assert!(get(conn, &game.id).is_err(), "记录应已删除");
            assert_eq!(count_sessions(conn, &game.id)?, 0, "会话应被级联删除");
            Ok(())
        })
        .expect("校验级联");
    }

    #[test]
    fn remove_missing_record_reports_not_found() {
        let db = Db::open_in_memory().expect("内存库");
        let err = db
            .with_conn(|conn| remove(conn, "不存在的id"))
            .expect_err("删除不存在的记录应报错");
        assert_eq!(err.code(), crate::error::AppErrorCode::NotFound);
    }

    #[test]
    fn negative_total_play_seconds_is_rejected_by_schema() {
        let db = Db::open_in_memory().expect("内存库");
        let game = db
            .with_conn(|conn| insert(conn, &sample("游戏", "C:\\A\\a.exe")))
            .expect("插入");

        let result = db.with_conn(|conn| {
            conn.execute(
                "UPDATE games SET total_play_seconds = -1 WHERE id = ?1",
                params![game.id],
            )
            .map_err(db_error("写入负数时长"))?;
            Ok(())
        });
        assert!(result.is_err(), "CHECK 约束应拒绝负数时长");
    }

    #[test]
    fn single_running_session_index_is_enforced() {
        let db = Db::open_in_memory().expect("内存库");
        let game = db
            .with_conn(|conn| insert(conn, &sample("游戏", "C:\\A\\a.exe")))
            .expect("插入");

        db.with_conn(|conn| {
            for (id, status) in [("s1", "running"), ("s2", "running")] {
                let sql = format!(
                    "INSERT INTO play_sessions (id, game_id, started_at, duration_seconds, tracked_seconds, \
                     status, process_id, process_start_identity, created_at) \
                     VALUES ('{id}', ?1, '2026-10-14T00:00:00.000Z', 0, 0, '{status}', 1, '1', \
                     '2026-10-14T00:00:00.000Z')"
                );
                let result = conn.execute(&sql, params![game.id]);
                if id == "s2" {
                    assert!(result.is_err(), "同一游戏不应存在两个 running 会话");
                } else {
                    assert!(result.is_ok(), "第一个 running 会话应写入成功");
                }
            }
            Ok(())
        })
        .expect("准备会话");
    }

    #[test]
    fn escape_like_escapes_wildcards() {
        assert_eq!(escape_like("100%"), "100\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        assert_eq!(escape_like("c:\\d"), "c:\\\\d");
        assert_eq!(escape_like("普通"), "普通");
    }

    #[test]
    fn normalize_launch_key_is_case_and_separator_insensitive() {
        let a = normalize_launch_key("C:\\Games\\Demo.exe", "");
        let b = normalize_launch_key("c:/games/demo.exe", "");
        assert_eq!(a, b);
        assert_ne!(a, normalize_launch_key("C:\\Games\\Demo.exe", "-foo"));
    }
}
