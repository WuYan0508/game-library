//! 会话管理器：一次游玩从启动到结束的完整生命周期。
//!
//! 防重复做了三层（见 docs/architecture.md §9）：
//! 1. 前端按钮置灰；
//! 2. 本模块的 `active` 内存集合（`start` 全程持锁，消除「检查—写入」竞态）；
//! 3. 数据库的唯一部分索引 `idx_sessions_single_running`。
//!
//! 结束判定走 `process_tree::collect_tree`：以「进程树是否还有存活成员」为准，
//! 而不是「最初那个进程退没退出」（见 ADR-04）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::database::Db;
use crate::database::games::now_utc;
use crate::database::models::Game;
use crate::database::sessions::{self, InsertSession, STATUS_COMPLETED, STATUS_INTERRUPTED};
use crate::error::{AppError, AppResult};
use crate::launcher::launch::{LaunchRequest, LaunchedProcess, launch};
use crate::platform::win::{elapsed_seconds, unbiased_interrupt_time};
use crate::tracker::process_tree::{TreeStatus, collect_tree};
use crate::tracker::recover::{RecoveredSession, recover_on_startup};

/// 进程树轮询间隔。
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// 心跳落库间隔（按轮询次数计）：约 10 秒。
const HEARTBEAT_EVERY_TICKS: u32 = 5;

pub const EVENT_STARTED: &str = "session://started";
pub const EVENT_ENDED: &str = "session://ended";
pub const EVENT_TICK: &str = "session://tick";
pub const EVENT_RECOVERED: &str = "sessions://recovered";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStarted {
    pub game_id: String,
    pub session_id: String,
    pub started_at: String,
    pub process_id: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTick {
    pub game_id: String,
    pub session_id: String,
    /// 从会话开始累计的秒数（不含系统休眠）。
    pub elapsed_seconds: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEnded {
    pub game_id: String,
    pub session_id: String,
    pub status: String,
    pub started_at: String,
    pub ended_at: String,
}

/// 当前活动会话的对外视图。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSessionDto {
    pub game_id: String,
    pub session_id: String,
    pub started_at: String,
    pub process_id: u32,
}

struct ActiveSession {
    session_id: String,
    pid: u32,
    started_at: String,
    /// 计时基准（无偏中断时间，100ns），停止跟踪时据此算已玩时长。
    started_unbiased: u64,
    /// `stop_tracking` 用它通知追踪任务「已经处理过了，你直接退出」。
    stop_requested: Arc<AtomicBool>,
}

/// 会话管理器。可克隆，作为 Tauri managed state 注入。
#[derive(Clone)]
pub struct SessionManager {
    db: Db,
    /// 无 app 时不发事件 —— 测试就是这么构造的。
    app: Option<AppHandle>,
    active: Arc<Mutex<HashMap<String, ActiveSession>>>,
}

impl SessionManager {
    pub fn new(db: Db, app: Option<AppHandle>) -> Self {
        Self {
            db,
            app,
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 启动游戏并开始跟踪。
    ///
    /// **全程持有 `active` 锁**：检查、启动、写库、登记一气呵成，
    /// 避免两次快速点击都通过检查。数据库唯一索引是最后一道兜底。
    pub fn start(&self, game: &Game) -> AppResult<SessionStarted> {
        let mut active = self.lock_active();

        if active.contains_key(&game.id) {
            return Err(AppError::already_running("这个游戏已经在运行中"));
        }

        // 数据库兜底：可能是上次异常退出留下的残留会话
        if self
            .db
            .with_conn(|conn| sessions::running_for_game(conn, &game.id))?
            .is_some()
        {
            return Err(AppError::already_running(
                "该游戏已有一个正在跟踪的会话，请先停止跟踪",
            ));
        }

        let launched = launch(&LaunchRequest {
            program_path: &game.launch_path,
            raw_args: &game.launch_args,
            working_directory: game.working_directory.as_deref(),
        })?;

        // 计时基准：从这一刻起用无偏中断时间累计（不含系统休眠）
        let started_unbiased = unbiased_interrupt_time()?;
        let session_id = sessions::new_id();
        let started_at = now_utc();

        let created = match self.db.with_conn(|conn| {
            sessions::insert(
                conn,
                &InsertSession {
                    id: session_id.clone(),
                    game_id: &game.id,
                    started_at: &started_at,
                    process_id: launched.pid,
                    process_start_identity: &launched.creation_identity(),
                },
            )
        }) {
            Ok(session) => session,
            Err(err) => {
                // 进程已经起来了但会话没落库：这条必须显式记下来，
                // 否则就成了「游戏在跑、应用却完全不知情」的静默状态。
                log::error!(
                    "进程已启动但会话写入失败（pid {}）: {}",
                    launched.pid,
                    err.message()
                );
                return Err(err);
            }
        };

        let stop_requested = Arc::new(AtomicBool::new(false));
        active.insert(
            game.id.clone(),
            ActiveSession {
                session_id: created.id.clone(),
                pid: launched.pid,
                started_at: created.started_at.clone(),
                started_unbiased,
                stop_requested: Arc::clone(&stop_requested),
            },
        );
        drop(active);

        let payload = SessionStarted {
            game_id: game.id.clone(),
            session_id: created.id.clone(),
            started_at: created.started_at.clone(),
            process_id: launched.pid,
        };

        log::info!(
            "已启动并开始跟踪: {}（pid {}，会话 {}）",
            game.name,
            launched.pid,
            created.id
        );

        self.emit(EVENT_STARTED, &payload);

        spawn_tracker(TrackerContext {
            db: self.db.clone(),
            app: self.app.clone(),
            active: Arc::clone(&self.active),
            game_id: game.id.clone(),
            session_id: created.id,
            started_at: created.started_at,
            pid: launched.pid,
            creation_time: launched.creation_time,
            stop_requested,
            started_unbiased,
            _keep_handle: launched,
        });

        Ok(payload)
    }

    /// 停止跟踪：**只停表，绝不结束游戏进程**。
    ///
    /// 结算为 `interrupted`（这次时长不算「正常结束」）。
    /// 返回 `None` 表示该游戏没有活动会话。
    pub fn stop_tracking(&self, game_id: &str) -> AppResult<Option<SessionEnded>> {
        let (session_id, started_at, stop_requested, started_unbiased) = {
            let active = self.lock_active();
            match active.get(game_id) {
                Some(session) => (
                    session.session_id.clone(),
                    session.started_at.clone(),
                    Arc::clone(&session.stop_requested),
                    session.started_unbiased,
                ),
                None => return Ok(None),
            }
        };

        // 先通知追踪任务别再来抢
        stop_requested.store(true, Ordering::Relaxed);

        let ended_at = now_utc();
        // 手动停止也把已玩的时间算进去：按无偏基准算到此刻为止。
        // interrupted 同样累加 —— 用户玩过的就是玩过的，只是「没正常退出」。
        let now_unbiased = unbiased_interrupt_time()?;
        let duration = elapsed_seconds(started_unbiased, now_unbiased);
        let settled = self.db.with_tx(|tx| {
            sessions::settle_and_accumulate(
                tx,
                &session_id,
                duration,
                STATUS_INTERRUPTED,
                &ended_at,
            )
        })?;

        {
            let mut active = self.lock_active();
            // 只有当活动会话还是「同一个」时才移除，避免误删新一轮的会话
            if active
                .get(game_id)
                .is_some_and(|session| session.session_id == session_id)
            {
                active.remove(game_id);
            }
        }

        if !settled {
            // 说明已经被追踪任务正常结算过了，这里什么都不用做
            return Ok(None);
        }

        let payload = SessionEnded {
            game_id: game_id.to_owned(),
            session_id,
            status: STATUS_INTERRUPTED.to_owned(),
            started_at,
            ended_at,
        };
        self.emit(EVENT_ENDED, &payload);

        Ok(Some(payload))
    }

    /// 启动时保守恢复残留的 running 会话（ADR-06）。
    ///
    /// 时长按最后一次心跳结算为 `interrupted`，并累加进对应游戏。
    /// 有需要恢复的会话时向前端推送 `sessions://recovered` 供提示。
    pub fn recover_on_startup(&self) -> AppResult<Vec<RecoveredSession>> {
        let recovered = recover_on_startup(&self.db)?;

        if !recovered.is_empty() {
            log::warn!("启动时保守恢复了 {} 条残留会话", recovered.len());
            if let Some(app) = &self.app
                && let Err(err) = app.emit(EVENT_RECOVERED, recovered.clone())
            {
                log::warn!("推送会话恢复事件失败: {err}");
            }
        }

        Ok(recovered)
    }

    /// 当前所有活动会话。
    pub fn active_sessions(&self) -> Vec<ActiveSessionDto> {
        let active = self.lock_active();
        active
            .iter()
            .map(|(game_id, session)| ActiveSessionDto {
                game_id: game_id.clone(),
                session_id: session.session_id.clone(),
                started_at: session.started_at.clone(),
                process_id: session.pid,
            })
            .collect()
    }

    fn lock_active(&self) -> MutexGuard<'_, HashMap<String, ActiveSession>> {
        match self.active.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::error!("会话表互斥锁被污染，继续使用底层数据");
                poisoned.into_inner()
            }
        }
    }

    fn emit<T: Serialize + Clone>(&self, event: &str, payload: &T) {
        let Some(app) = &self.app else {
            return;
        };
        if let Err(err) = app.emit(event, payload.clone()) {
            log::warn!("推送事件 {event} 失败: {err}");
        }
    }
}

/// 追踪任务所需的一切。单独成结构体，便于整体移进异步任务。
struct TrackerContext {
    db: Db,
    app: Option<AppHandle>,
    active: Arc<Mutex<HashMap<String, ActiveSession>>>,
    game_id: String,
    session_id: String,
    started_at: String,
    pid: u32,
    creation_time: u64,
    stop_requested: Arc<AtomicBool>,
    /// 计时基准（无偏中断时间，100ns）。会话结束后随上下文丢弃。
    started_unbiased: u64,
    /// 持有进程句柄直到会话结束，避免句柄被提前释放。
    _keep_handle: LaunchedProcess,
}

impl TrackerContext {
    /// 结算并清理。返回 `true` 表示「这次是我结算的」。
    ///
    /// 结算与时长累加在**同一事务**里完成，且只有第一个到达者会生效。
    fn finish(&self, status: &str, duration_seconds: i64) -> bool {
        let ended_at = now_utc();

        let settled = match self.db.with_tx(|tx| {
            sessions::settle_and_accumulate(
                tx,
                &self.session_id,
                duration_seconds,
                status,
                &ended_at,
            )
        }) {
            Ok(settled) => settled,
            Err(err) => {
                log::error!("结算会话失败: {}", err.message());
                false
            }
        };

        {
            let mut active = match self.active.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if active
                .get(&self.game_id)
                .is_some_and(|session| session.session_id == self.session_id)
            {
                active.remove(&self.game_id);
            }
        }

        if settled {
            log::info!("会话结束: {}（{}）", self.session_id, status);
            if let Some(app) = &self.app {
                let payload = SessionEnded {
                    game_id: self.game_id.clone(),
                    session_id: self.session_id.clone(),
                    status: status.to_owned(),
                    started_at: self.started_at.clone(),
                    ended_at,
                };
                if let Err(err) = app.emit(EVENT_ENDED, payload) {
                    log::warn!("推送会话结束事件失败: {err}");
                }
            }
        }

        settled
    }

    /// 把已跟踪秒数写进心跳字段。失败由调用方记日志后继续，不影响计时。
    fn heartbeat(&self, tracked_seconds: i64) -> AppResult<()> {
        self.db.with_conn(|conn| {
            sessions::heartbeat(conn, &self.session_id, tracked_seconds, &now_utc())
        })?;
        Ok(())
    }

    /// 给前端推送实时进度。没有 AppHandle 时静默跳过。
    fn emit_tick(&self, elapsed_seconds: i64) {
        let Some(app) = &self.app else {
            return;
        };
        let payload = SessionTick {
            game_id: self.game_id.clone(),
            session_id: self.session_id.clone(),
            elapsed_seconds,
        };
        if let Err(err) = app.emit(EVENT_TICK, payload) {
            log::warn!("推送会话进度事件失败: {err}");
        }
    }
}

/// 启动追踪轮询任务。
fn spawn_tracker(context: TrackerContext) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(POLL_INTERVAL);
        // 第一次 tick 会立刻返回，跳过它，避免刚启动就采样
        ticker.tick().await;
        let mut tick_count: u32 = 0;

        loop {
            ticker.tick().await;
            tick_count += 1;

            if context.stop_requested.load(Ordering::Relaxed) {
                // stop_tracking 已经结算并清理过了
                return;
            }

            // 已玩秒数：按无偏中断时间算，系统睡眠不计入。
            // 读不到基准时跳过这一轮，下一轮再算 —— 不能因此把时长清零。
            let elapsed = match unbiased_interrupt_time() {
                Ok(now) => elapsed_seconds(context.started_unbiased, now),
                Err(err) => {
                    log::warn!("读取计时基准失败，本轮跳过: {}", err.message());
                    continue;
                }
            };

            context.emit_tick(elapsed);

            // 心跳约每 10 秒落一次库，崩溃恢复靠它
            if tick_count.is_multiple_of(HEARTBEAT_EVERY_TICKS)
                && let Err(err) = context.heartbeat(elapsed)
            {
                log::warn!("写入会话心跳失败: {}", err.message());
            }

            let pid = context.pid;
            let creation_time = context.creation_time;

            // 枚举进程是同步且可能耗时的系统调用，放到阻塞线程池里，别占着 runtime
            let outcome =
                tauri::async_runtime::spawn_blocking(move || collect_tree(pid, creation_time))
                    .await;

            match outcome {
                Ok(Ok(TreeStatus::Alive { .. })) => {}
                Ok(Ok(_)) => {
                    context.finish(STATUS_COMPLETED, elapsed);
                    return;
                }
                Ok(Err(err)) => {
                    // 查询失败按暂态处理，下一轮再试 —— 不能因为一次失败就误判游戏结束
                    log::warn!("进程树查询失败，稍后重试: {}", err.message());
                }
                Err(err) => {
                    log::warn!("进程树查询任务异常: {err}");
                }
            }
        }
    });
}
