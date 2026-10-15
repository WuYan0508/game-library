//! 启动时的保守恢复（ADR-06）。
//!
//! 应用异常退出会留下 `status = 'running'` 的会话。启动时把它们结算为
//! `interrupted`，时长取**最后一次心跳**落库的 `tracked_seconds` ——
//! 这是保守值：只可能少记，绝不会多记。
//!
//! 这里的逻辑刻意不依赖 `SessionManager`：恢复发生在应用启动早期，
//! 且这样它才能被单元测试直接覆盖。

use serde::Serialize;

use crate::database::Db;
use crate::database::games::now_utc;
use crate::database::sessions::{self, STATUS_INTERRUPTED};
use crate::error::AppResult;

/// 一条被恢复的会话。返回给前端用于提示「上次有 N 段游玩被记为中断」。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveredSession {
    pub session_id: String,
    pub game_id: String,
    /// 按最后心跳结算的秒数。没来得及心跳就是 0。
    pub duration_seconds: i64,
}

/// 恢复所有残留的 running 会话。
///
/// 返回本次实际恢复的清单（空列表表示没有需要恢复的）。
/// 单条恢复失败只记日志，不让整批恢复中断。
pub fn recover_on_startup(db: &Db) -> AppResult<Vec<RecoveredSession>> {
    let running = db.with_conn(sessions::list_running)?;
    let mut recovered = Vec::new();

    for session in running {
        let ended_at = now_utc();
        let duration = session.tracked_seconds.max(0);

        let settled = match db.with_tx(|tx| {
            sessions::settle_and_accumulate(
                tx,
                &session.id,
                duration,
                STATUS_INTERRUPTED,
                &ended_at,
            )
        }) {
            Ok(settled) => settled,
            Err(err) => {
                log::error!("恢复会话 {} 失败: {}", session.id, err.message());
                continue;
            }
        };

        if settled {
            log::info!(
                "保守恢复残留会话: {}（游戏 {}，按最后心跳记 {} 秒，interrupted）",
                session.id,
                session.game_id,
                duration
            );
            recovered.push(RecoveredSession {
                session_id: session.id,
                game_id: session.game_id,
                duration_seconds: duration,
            });
        }
    }

    Ok(recovered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::games::{self, InsertGame, new_id};
    use crate::database::models::{Game, SourceType};
    use crate::database::sessions::{self, InsertSession};

    /// 每次生成不同的启动路径 —— games 表按规范化启动目标去重，
    /// 同一路径插第二条会被当作重复记录拒绝。
    fn unique_launch_path() -> String {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!(r"C:\Games\demo-{n}.exe")
    }

    fn seed_game(db: &Db) -> Game {
        let launch_path = unique_launch_path();
        let id = new_id();
        db.with_conn(|conn| {
            games::insert(
                conn,
                &InsertGame {
                    id: id.clone(),
                    name: "测试游戏",
                    original_name: "测试游戏",
                    source_type: SourceType::Exe,
                    source_path: &launch_path,
                    launch_path: &launch_path,
                    launch_args: "",
                    working_directory: None,
                    cover_path: None,
                    icon_path: None,
                    is_launcher: false,
                },
            )
        })
        .expect("插入游戏")
    }

    /// 造一条带心跳的残留 running 会话。
    fn seed_running_session(db: &Db, game_id: &str, tracked_seconds: i64) -> String {
        let id = sessions::new_id();
        db.with_conn(|conn| {
            sessions::insert(
                conn,
                &InsertSession {
                    id: id.clone(),
                    game_id,
                    started_at: &now_utc(),
                    process_id: 1,
                    process_start_identity: "133000000000000000",
                },
            )?;
            sessions::heartbeat(conn, &id, tracked_seconds, &now_utc())
        })
        .expect("造会话");
        id
    }

    #[test]
    fn recovers_stale_running_session_using_last_heartbeat() {
        let db = Db::open_in_memory().expect("内存库");
        let game = seed_game(&db);
        seed_running_session(&db, &game.id, 300);

        let recovered = recover_on_startup(&db).expect("恢复");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].duration_seconds, 300, "应按最后心跳结算");
        assert_eq!(recovered[0].game_id, game.id);

        // 会话状态与游戏累计都要落库
        let stored = db.with_conn(sessions::list_running).expect("查询");
        assert!(stored.is_empty(), "恢复后不应再有 running 会话");

        let refreshed = db
            .with_conn(|conn| games::get(conn, &game.id))
            .expect("读回");
        assert_eq!(refreshed.total_play_seconds, 300, "时长应被累加");
        assert!(refreshed.last_played_at.is_some());
    }

    #[test]
    fn session_without_heartbeat_recovers_as_zero() {
        let db = Db::open_in_memory().expect("内存库");
        let game = seed_game(&db);

        // 不写心跳直接造会话
        let id = sessions::new_id();
        db.with_conn(|conn| {
            sessions::insert(
                conn,
                &InsertSession {
                    id: id.clone(),
                    game_id: &game.id,
                    started_at: &now_utc(),
                    process_id: 1,
                    process_start_identity: "1",
                },
            )
        })
        .expect("造会话");

        let recovered = recover_on_startup(&db).expect("恢复");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].duration_seconds, 0, "没心跳过就是 0，绝不多记");

        let refreshed = db
            .with_conn(|conn| games::get(conn, &game.id))
            .expect("读回");
        assert_eq!(refreshed.total_play_seconds, 0);
    }

    #[test]
    fn already_settled_sessions_are_untouched() {
        let db = Db::open_in_memory().expect("内存库");
        let game = seed_game(&db);
        let id = seed_running_session(&db, &game.id, 60);

        db.with_tx(|tx| sessions::settle_and_accumulate(tx, &id, 60, "completed", &now_utc()))
            .expect("先正常结算");

        assert!(
            recover_on_startup(&db).expect("恢复").is_empty(),
            "没有残留会话时恢复应为空"
        );

        let refreshed = db
            .with_conn(|conn| games::get(conn, &game.id))
            .expect("读回");
        assert_eq!(refreshed.total_play_seconds, 60, "不应被恢复流程重复累加");
    }

    #[test]
    fn multiple_stale_sessions_are_all_recovered() {
        // 注意：同一游戏不可能有两条 running 会话（唯一部分索引挡着，这正是设计），
        // 所以这里用两个不同的游戏各留一条残留会话。
        let db = Db::open_in_memory().expect("内存库");
        let game_a = seed_game(&db);
        let game_b = seed_game(&db);
        seed_running_session(&db, &game_a.id, 100);
        seed_running_session(&db, &game_b.id, 50);

        let recovered = recover_on_startup(&db).expect("恢复");
        assert_eq!(recovered.len(), 2, "两条残留会话都要被恢复");

        let a = db
            .with_conn(|conn| games::get(conn, &game_a.id))
            .expect("读回");
        let b = db
            .with_conn(|conn| games::get(conn, &game_b.id))
            .expect("读回");
        assert_eq!(a.total_play_seconds, 100, "各自累加各自的");
        assert_eq!(b.total_play_seconds, 50);
    }
}
