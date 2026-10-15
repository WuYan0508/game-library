//! 启动与会话命令（对应 docs/architecture.md §7.1「启动与会话」）。

use serde::Serialize;
use tauri::State;

use crate::database::Db;
use crate::database::games;
use crate::error::AppResult;
use crate::tracker::session::{ActiveSessionDto, SessionEnded, SessionManager, SessionStarted};

/// 从应用内启动游戏，并开始跟踪这次会话。
///
/// 失败返回结构化错误；同一游戏已有活动会话时返回 `AlreadyRunning`。
#[tauri::command]
pub fn launch_game(
    db: State<'_, Db>,
    sessions: State<'_, SessionManager>,
    game_id: String,
) -> AppResult<SessionStarted> {
    let game = db.with_conn(|conn| games::get(conn, &game_id))?;
    sessions.start(&game)
}

/// 当前正在被跟踪的会话。
#[tauri::command]
pub fn get_active_sessions(sessions: State<'_, SessionManager>) -> Vec<ActiveSessionDto> {
    sessions.active_sessions()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopTrackingResult {
    /// 是否真的停掉了一个会话（false 表示该游戏本来就没在跟踪）。
    pub stopped: bool,
    pub session: Option<SessionEnded>,
}

/// 停止跟踪。
///
/// **只停表，绝不结束游戏进程** —— 用户可能只是不想继续计时了。
#[tauri::command]
pub fn stop_tracking(
    sessions: State<'_, SessionManager>,
    game_id: String,
) -> AppResult<StopTrackingResult> {
    let session = sessions.stop_tracking(&game_id)?;
    Ok(StopTrackingResult {
        stopped: session.is_some(),
        session,
    })
}
