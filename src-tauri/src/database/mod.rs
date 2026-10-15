//! 数据访问层：连接管理、迁移、行模型与查询。
//!
//! 唯一持有 SQLite 的位置（见 docs/architecture.md ADR-01）。
//! 前端不得直接接触数据库，一切经 commands 层转达。

pub mod games;
pub mod models;
pub mod sessions;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, Transaction};
use rusqlite_migration::{M, Migrations};

use crate::error::{AppError, AppResult};

/// 迁移脚本在编译期内联，避免运行时依赖外部文件。
const MIGRATION_0001: &str = include_str!("migrations/0001_init.sql");

/// 数据库句柄。克隆成本低（内部为 `Arc`），可作为 Tauri managed state 注入。
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// 打开（或创建）数据库文件，配置连接参数并执行迁移。
    pub fn open(path: &Path) -> AppResult<Self> {
        let mut conn = Connection::open(path).map_err(db_error("打开数据库失败"))?;
        configure(&conn)?;
        run_migrations(&mut conn)?;
        Ok(Self::wrap(conn))
    }

    /// 内存库，仅供测试使用（生产路径一律走 `open`）。
    #[allow(dead_code)]
    pub fn open_in_memory() -> AppResult<Self> {
        let mut conn = Connection::open_in_memory().map_err(db_error("打开内存数据库失败"))?;
        configure(&conn)?;
        run_migrations(&mut conn)?;
        Ok(Self::wrap(conn))
    }

    fn wrap(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    /// 取锁。锁被污染（此前有线程 panic）时不让整个应用跟着崩，
    /// 记录日志后继续使用底层连接。
    fn lock(&self) -> MutexGuard<'_, Connection> {
        match self.conn.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::error!("数据库互斥锁已被污染（此前有线程 panic），继续使用底层连接");
                poisoned.into_inner()
            }
        }
    }

    /// 只读查询。
    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T> {
        let conn = self.lock();
        f(&conn)
    }

    /// 事务。结算类操作必须走这里，保证「写会话 + 累加时长」原子完成。
    pub fn with_tx<T>(&self, f: impl FnOnce(&Transaction<'_>) -> AppResult<T>) -> AppResult<T> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(db_error("开启事务失败"))?;
        let value = f(&tx)?;
        tx.commit().map_err(db_error("提交事务失败"))?;
        Ok(value)
    }
}

fn configure(conn: &Connection) -> AppResult<()> {
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;")
        .map_err(db_error("配置连接参数失败"))?;
    conn.busy_timeout(Duration::from_millis(5_000))
        .map_err(db_error("设置 busy_timeout 失败"))?;

    // WAL 只对文件库生效；内存库会返回 "memory"，不是错误。
    let mode: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(db_error("启用 WAL 失败"))?;
    log::debug!("SQLite journal_mode = {mode}");

    Ok(())
}

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(MIGRATION_0001),
        M::up(include_str!("migrations/0002_custom_order.sql")),
    ])
}

fn run_migrations(conn: &mut Connection) -> AppResult<()> {
    migrations()
        .to_latest(conn)
        .map_err(|err| AppError::database("数据库结构升级失败").with_detail(err.to_string()))
}

/// 把底层错误包装成面向用户的 `DatabaseError`，细节只进日志。
fn db_error(message: &'static str) -> impl Fn(rusqlite::Error) -> AppError {
    move |err| AppError::database(message).with_detail(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_idempotent() {
        let mut conn = Connection::open_in_memory().expect("打开内存库");
        let migrations = migrations();

        migrations.to_latest(&mut conn).expect("首次迁移应成功");
        migrations
            .to_latest(&mut conn)
            .expect("重复执行迁移应成功且无副作用");
    }

    #[test]
    fn expected_indexes_exist() {
        let db = Db::open_in_memory().expect("打开内存库");
        let indexes: Vec<String> = db
            .with_conn(|conn| {
                let mut stmt = conn
                    .prepare("SELECT name FROM sqlite_master WHERE type = 'index' ORDER BY name")
                    .map_err(db_error("读取索引清单失败"))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(db_error("读取索引清单失败"))?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.map_err(db_error("读取索引清单失败"))?);
                }
                Ok(out)
            })
            .expect("查询索引");

        for expected in [
            "idx_games_dedupe",
            "idx_games_name",
            "idx_sessions_game",
            "idx_sessions_status",
            "idx_sessions_single_running",
        ] {
            assert!(
                indexes.iter().any(|name| name == expected),
                "缺少索引 {expected}，实际为 {indexes:?}"
            );
        }
    }

    #[test]
    fn foreign_keys_are_enabled() {
        let db = Db::open_in_memory().expect("打开内存库");
        let enabled: i64 = db
            .with_conn(|conn| {
                conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))
                    .map_err(db_error("读取 foreign_keys 失败"))
            })
            .expect("读取 foreign_keys");
        assert_eq!(enabled, 1, "foreign_keys 必须开启，否则级联删除不生效");
    }
}
