//! `play_sessions` 表的读写。
//!
//! 两条关键约束（见 docs/architecture.md §5）：
//! - 唯一部分索引 `idx_sessions_single_running` 保证**同一游戏同时只有一个 running 会话**；
//! - 结算语句带 `status = 'running'` 条件，因此**天然幂等** —— 重复调用只会成功一次。

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{AppError, AppResult};

use super::games::now_utc;
use super::models::PlaySession;

/// SQLITE_CONSTRAINT_UNIQUE
const SQLITE_CONSTRAINT_UNIQUE: i32 = 2067;

pub const STATUS_RUNNING: &str = "running";
pub const STATUS_COMPLETED: &str = "completed";
pub const STATUS_INTERRUPTED: &str = "interrupted";

/// 新建一个会话 id。
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 待写入的新会话。
pub struct InsertSession<'a> {
    pub id: String,
    pub game_id: &'a str,
    pub started_at: &'a str,
    pub process_id: u32,
    pub process_start_identity: &'a str,
}

/// 写入一条 `running` 会话。
///
/// 命中 `idx_sessions_single_running` 时返回 `AlreadyRunning` —— 说明该游戏已经有活动会话。
pub fn insert(conn: &Connection, input: &InsertSession<'_>) -> AppResult<PlaySession> {
    let now = now_utc();

    conn.execute(
        "INSERT INTO play_sessions (id, game_id, started_at, ended_at, duration_seconds, \
         tracked_seconds, last_heartbeat_at, status, process_id, process_start_identity, created_at) \
         VALUES (?1, ?2, ?3, NULL, 0, 0, NULL, ?4, ?5, ?6, ?7)",
        params![
            input.id,
            input.game_id,
            input.started_at,
            STATUS_RUNNING,
            input.process_id,
            input.process_start_identity,
            now,
        ],
    )
    .map_err(|err| {
        if is_unique_violation(&err) {
            AppError::already_running("该游戏已经有一个正在跟踪的会话")
                .with_detail(err.to_string())
        } else {
            AppError::database("创建会话失败").with_detail(err.to_string())
        }
    })?;

    get(conn, &input.id)
}

/// 结算一个会话。
///
/// **只有当前仍是 `running` 的会话会被更新**，因此重复调用是安全的：
/// 第二次及以后返回 `Ok(false)`，调用方据此跳过后续动作（比如累加时长）。
///
/// 返回 `true` 表示「这次是我结算的」。
pub fn settle(
    conn: &Connection,
    session_id: &str,
    duration_seconds: i64,
    status: &str,
    ended_at: &str,
) -> AppResult<bool> {
    debug_assert!(status != STATUS_RUNNING, "结算不能把状态写成 running");

    let affected = conn
        .execute(
            "UPDATE play_sessions SET ended_at = ?3, duration_seconds = ?4, status = ?5 \
             WHERE id = ?1 AND status = ?2",
            params![
                session_id,
                STATUS_RUNNING,
                ended_at,
                duration_seconds.max(0),
                status
            ],
        )
        .map_err(|err| AppError::database("结算会话失败").with_detail(err.to_string()))?;

    Ok(affected == 1)
}

/// 更新心跳：把已跟踪秒数与心跳时间落库。
///
/// 这是崩溃恢复的依据 —— 应用异常退出后，残留会话按时 last 心跳的秒数结算。
/// 只有仍是 running 的会话会被更新（已结算的不回写）。
pub fn heartbeat(
    conn: &Connection,
    session_id: &str,
    tracked_seconds: i64,
    at: &str,
) -> AppResult<bool> {
    let affected = conn
        .execute(
            "UPDATE play_sessions SET tracked_seconds = ?2, last_heartbeat_at = ?3 \
             WHERE id = ?1 AND status = ?4",
            params![session_id, tracked_seconds.max(0), at, STATUS_RUNNING],
        )
        .map_err(|err| AppError::database("写入会话心跳失败").with_detail(err.to_string()))?;
    Ok(affected == 1)
}

/// 结算一个会话，并在**同一事务**里把时长累加进游戏的累计值。
///
/// 结算语句带 `status = 'running'` 条件，因此天然幂等：只有真正完成结算的那一次
/// 才会累加时长，重复调用返回 `Ok(false)`。
///
/// **调用方必须用 `Db::with_tx` 包住**，保证「结算」与「累加」要么都成功要么都不成功。
pub fn settle_and_accumulate(
    conn: &Connection,
    session_id: &str,
    duration_seconds: i64,
    status: &str,
    ended_at: &str,
) -> AppResult<bool> {
    debug_assert!(status != STATUS_RUNNING, "结算不能把状态写成 running");

    let duration = duration_seconds.max(0);

    let affected = conn
        .execute(
            "UPDATE play_sessions SET ended_at = ?3, duration_seconds = ?4, status = ?5 \
             WHERE id = ?1 AND status = ?2",
            params![session_id, STATUS_RUNNING, ended_at, duration, status],
        )
        .map_err(|err| AppError::database("结算会话失败").with_detail(err.to_string()))?;

    if affected == 0 {
        // 已经被结算过（可能是追踪循环与手动停止竞争，输了的一方）
        return Ok(false);
    }

    // 子查询定位 game_id，避免先读后写
    conn.execute(
        "UPDATE games SET total_play_seconds = total_play_seconds + ?2, last_played_at = ?3 \
         WHERE id = (SELECT game_id FROM play_sessions WHERE id = ?1)",
        params![session_id, duration, ended_at],
    )
    .map_err(|err| AppError::database("累加游戏时长失败").with_detail(err.to_string()))?;

    Ok(true)
}

/// 查询某游戏当前的活动会话。
pub fn running_for_game(conn: &Connection, game_id: &str) -> AppResult<Option<PlaySession>> {
    let sql = format!(
        "SELECT {} FROM play_sessions WHERE game_id = ?1 AND status = ?2",
        PlaySession::COLUMNS
    );
    conn.query_row(
        &sql,
        params![game_id, STATUS_RUNNING],
        PlaySession::from_row,
    )
    .optional()
    .map_err(|err| AppError::database("查询活动会话失败").with_detail(err.to_string()))
}

/// 查询所有活动会话（应用启动时用于排查残留）。
pub fn list_running(conn: &Connection) -> AppResult<Vec<PlaySession>> {
    let sql = format!(
        "SELECT {} FROM play_sessions WHERE status = ?1 ORDER BY started_at ASC",
        PlaySession::COLUMNS
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|err| AppError::database("准备活动会话查询失败").with_detail(err.to_string()))?;

    let rows = stmt
        .query_map(params![STATUS_RUNNING], PlaySession::from_row)
        .map_err(|err| AppError::database("查询活动会话失败").with_detail(err.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| AppError::database("读取活动会话失败").with_detail(err.to_string()))?;

    Ok(rows)
}

/// 按 id 查询会话。
pub fn get(conn: &Connection, id: &str) -> AppResult<PlaySession> {
    let sql = format!(
        "SELECT {} FROM play_sessions WHERE id = ?1",
        PlaySession::COLUMNS
    );
    conn.query_row(&sql, params![id], PlaySession::from_row)
        .optional()
        .map_err(|err| AppError::database("查询会话失败").with_detail(err.to_string()))?
        .ok_or_else(|| AppError::not_found("找不到该会话记录"))
}

fn is_unique_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.extended_code == SQLITE_CONSTRAINT_UNIQUE
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Db;
    use crate::database::games::{self, InsertGame, new_id};
    use crate::database::models::SourceType;

    fn seeded_game(db: &Db) -> String {
        let id = new_id();
        db.with_conn(|conn| {
            games::insert(
                conn,
                &InsertGame {
                    id: id.clone(),
                    name: "测试游戏",
                    original_name: "测试游戏",
                    source_type: SourceType::Exe,
                    source_path: r"C:\Games\demo.exe",
                    launch_path: r"C:\Games\demo.exe",
                    launch_args: "",
                    working_directory: None,
                    cover_path: None,
                    icon_path: None,
                    is_launcher: false,
                },
            )
        })
        .expect("插入游戏");
        id
    }

    fn start_session(db: &Db, game_id: &str, pid: u32) -> PlaySession {
        db.with_conn(|conn| {
            insert(
                conn,
                &InsertSession {
                    id: new_id(),
                    game_id,
                    started_at: &now_utc(),
                    process_id: pid,
                    process_start_identity: "133000000000000000",
                },
            )
        })
        .expect("创建会话")
    }

    #[test]
    fn creates_running_session() {
        let db = Db::open_in_memory().expect("内存库");
        let game_id = seeded_game(&db);

        let session = start_session(&db, &game_id, 4242);
        assert_eq!(session.status, STATUS_RUNNING);
        assert_eq!(session.process_id, 4242);
        assert_eq!(session.duration_seconds, 0);
        assert!(session.ended_at.is_none());
    }

    #[test]
    fn second_running_session_for_same_game_is_rejected() {
        let db = Db::open_in_memory().expect("内存库");
        let game_id = seeded_game(&db);
        start_session(&db, &game_id, 1);

        let err = db
            .with_conn(|conn| {
                insert(
                    conn,
                    &InsertSession {
                        id: new_id(),
                        game_id: &game_id,
                        started_at: &now_utc(),
                        process_id: 2,
                        process_start_identity: "1",
                    },
                )
            })
            .expect_err("同一游戏不应有两个活动会话");

        assert_eq!(err.code(), crate::error::AppErrorCode::AlreadyRunning);
    }

    #[test]
    fn settle_is_idempotent() {
        let db = Db::open_in_memory().expect("内存库");
        let game_id = seeded_game(&db);
        let session = start_session(&db, &game_id, 1);

        let first = db
            .with_conn(|conn| settle(conn, &session.id, 0, STATUS_COMPLETED, &now_utc()))
            .expect("首次结算");
        assert!(first, "第一次结算应当成功");

        let second = db
            .with_conn(|conn| settle(conn, &session.id, 999, STATUS_COMPLETED, &now_utc()))
            .expect("重复结算不应报错");
        assert!(!second, "第二次结算必须被拒（否则会重复累加）");

        let stored = db.with_conn(|conn| get(conn, &session.id)).expect("读回");
        assert_eq!(stored.status, STATUS_COMPLETED);
        assert_eq!(stored.duration_seconds, 0, "重复结算不得改写已结算的值");
    }

    #[test]
    fn settling_frees_the_game_for_a_new_session() {
        let db = Db::open_in_memory().expect("内存库");
        let game_id = seeded_game(&db);
        let first = start_session(&db, &game_id, 1);

        db.with_conn(|conn| settle(conn, &first.id, 0, STATUS_COMPLETED, &now_utc()))
            .expect("结算");

        // 结算之后应能再开一个
        let second = start_session(&db, &game_id, 2);
        assert_eq!(second.status, STATUS_RUNNING);

        let running = db
            .with_conn(|conn| running_for_game(conn, &game_id))
            .expect("查询")
            .expect("应有新的活动会话");
        assert_eq!(running.id, second.id, "当前活动会话应换成新的那个");
    }

    #[test]
    fn running_lookup_and_listing_work() {
        let db = Db::open_in_memory().expect("内存库");
        let game_id = seeded_game(&db);

        assert!(
            db.with_conn(|conn| running_for_game(conn, &game_id))
                .expect("查询")
                .is_none()
        );

        let session = start_session(&db, &game_id, 7);
        let found = db
            .with_conn(|conn| running_for_game(conn, &game_id))
            .expect("查询")
            .expect("应有活动会话");
        assert_eq!(found.id, session.id);

        assert_eq!(db.with_conn(list_running).expect("列出").len(), 1);
    }

    #[test]
    fn negative_duration_is_clamped_to_zero() {
        let db = Db::open_in_memory().expect("内存库");
        let game_id = seeded_game(&db);
        let session = start_session(&db, &game_id, 1);

        db.with_conn(|conn| settle(conn, &session.id, -5, STATUS_COMPLETED, &now_utc()))
            .expect("结算");

        let stored = db.with_conn(|conn| get(conn, &session.id)).expect("读回");
        assert_eq!(stored.duration_seconds, 0, "负数时长必须被夹到 0");
    }

    #[test]
    fn settle_and_accumulate_adds_duration_to_game() {
        let db = Db::open_in_memory().expect("内存库");
        let game_id = seeded_game(&db);
        let session = start_session(&db, &game_id, 1);

        db.with_conn(|conn| heartbeat(conn, &session.id, 120, &now_utc()))
            .expect("写心跳");

        let settled = db
            .with_tx(|tx| settle_and_accumulate(tx, &session.id, 120, STATUS_COMPLETED, &now_utc()))
            .expect("结算");
        assert!(settled);

        let game = db
            .with_conn(|conn| games::get(conn, &game_id))
            .expect("读回游戏");
        assert_eq!(game.total_play_seconds, 120, "累计时长应被累加");
        assert!(game.last_played_at.is_some(), "最后游玩时间应被写入");

        let stored = db
            .with_conn(|conn| get(conn, &session.id))
            .expect("读回会话");
        assert_eq!(stored.duration_seconds, 120);
        assert_eq!(stored.status, STATUS_COMPLETED);
    }

    #[test]
    fn accumulate_happens_only_once() {
        let db = Db::open_in_memory().expect("内存库");
        let game_id = seeded_game(&db);
        let session = start_session(&db, &game_id, 1);

        let first = db
            .with_tx(|tx| settle_and_accumulate(tx, &session.id, 120, STATUS_COMPLETED, &now_utc()))
            .expect("首次结算");
        assert!(first);

        let second = db
            .with_tx(|tx| settle_and_accumulate(tx, &session.id, 999, STATUS_COMPLETED, &now_utc()))
            .expect("重复结算不应报错");
        assert!(!second, "第二次结算必须被拒");

        let game = db
            .with_conn(|conn| games::get(conn, &game_id))
            .expect("读回游戏");
        assert_eq!(game.total_play_seconds, 120, "重复结算不得再累加");
    }

    #[test]
    fn heartbeat_only_touches_running_sessions() {
        let db = Db::open_in_memory().expect("内存库");
        let game_id = seeded_game(&db);
        let session = start_session(&db, &game_id, 1);

        let updated = db
            .with_conn(|conn| heartbeat(conn, &session.id, 30, &now_utc()))
            .expect("心跳");
        assert!(updated);

        // 结算后再心跳：不该改写任何字段
        db.with_tx(|tx| settle_and_accumulate(tx, &session.id, 30, STATUS_COMPLETED, &now_utc()))
            .expect("结算");
        let after_settle = db
            .with_conn(|conn| heartbeat(conn, &session.id, 999, &now_utc()))
            .expect("结算后再心跳");
        assert!(!after_settle, "已结算的会话不应再接受心跳");

        let stored = db.with_conn(|conn| get(conn, &session.id)).expect("读回");
        assert_eq!(stored.tracked_seconds, 30, "已结算的心跳不应被改写");
    }

    #[test]
    fn interrupted_settlement_also_accumulates() {
        // 手动「停止计时」也该把已玩的时间算进去
        let db = Db::open_in_memory().expect("内存库");
        let game_id = seeded_game(&db);
        let session = start_session(&db, &game_id, 1);

        db.with_tx(|tx| settle_and_accumulate(tx, &session.id, 45, STATUS_INTERRUPTED, &now_utc()))
            .expect("结算");

        let game = db
            .with_conn(|conn| games::get(conn, &game_id))
            .expect("读回游戏");
        assert_eq!(game.total_play_seconds, 45, "interrupted 也要累加");
    }
}
