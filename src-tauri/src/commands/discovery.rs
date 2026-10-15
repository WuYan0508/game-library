//! 自动发现命令（对应 docs/auto-discovery-design.md §3 阶段三）。
//!
//! **边界**：`scan_games` 只读本地清单；`import_discovered_games` 对每个勾选项调用
//! **现有的** `add_game_from_exe_impl`（含去重 / 名称 / 封面提取），绝不绕过业务逻辑，
//! 且单条失败不中断整个批次。

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, State};

use super::app_data_dir;
use crate::commands::import::add_game_from_exe_impl;
use crate::database::Db;
use crate::database::games;
use crate::discovery::{self, ScanOutcome};
use crate::error::{AppError, AppResult};

/// 扫描候选游戏。
///
/// `directories` 是「指定目录」来源要扫的目录（用户在 UI 里选的），可为空。
#[tauri::command]
pub fn scan_games(db: State<'_, Db>, directories: Option<Vec<String>>) -> AppResult<ScanOutcome> {
    let dirs: Vec<PathBuf> = directories
        .unwrap_or_default()
        .into_iter()
        .map(|value| PathBuf::from(value.trim()))
        .filter(|path| !path.as_os_str().is_empty())
        .collect();

    discovery::scan_all(&db, &dirs)
}

/// 勾选项。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredImportItem {
    /// 显示名称（发现来源给的，用户可在预览里改）。
    pub name: Option<String>,
    /// 启动目标。
    pub launch_path: String,
    /// 启动参数（Epic 等来源可能提供）。
    pub launch_args: Option<String>,
    /// 工作目录。
    pub working_directory: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailure {
    pub name: String,
    pub launch_path: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDiscoveredResult {
    pub imported: usize,
    /// 导入时发现已在库中而跳过的数量（后端兜底，前端默认就不会勾选）。
    pub skipped_existing: usize,
    pub failures: Vec<ImportFailure>,
}

/// 批量导入勾选的候选。
///
/// 单条失败（路径失效、数据库错误等）记录原因并继续，**不得无提示地中断整批**。
pub(crate) fn import_discovered_impl(
    db: &Db,
    data_dir: &Path,
    items: &[DiscoveredImportItem],
) -> AppResult<ImportDiscoveredResult> {
    let mut imported = 0usize;
    let mut skipped_existing = 0usize;
    let mut failures = Vec::new();

    for item in items {
        let name = item.name.clone().unwrap_or_default();
        let launch_path = item.launch_path.trim().to_owned();

        if launch_path.is_empty() {
            failures.push(ImportFailure {
                name,
                launch_path,
                reason: "启动目标为空".to_owned(),
            });
            continue;
        }

        // 后端兜底：已在库中的直接跳过（前端默认不勾选，但防手改）
        if db
            .with_conn(|conn| {
                games::find_by_launch_key(
                    conn,
                    &launch_path,
                    item.launch_args.as_deref().unwrap_or(""),
                )
            })?
            .is_some()
        {
            skipped_existing += 1;
            continue;
        }

        match add_game_from_exe_impl(db, data_dir, &launch_path, item.name.clone(), None) {
            Ok(game) => {
                imported += 1;

                // 发现来源可能带启动参数 / 工作目录（Epic 等），补写到刚导入的记录上。
                // 用的是现有的 update 接口，不算绕过业务逻辑。
                if item
                    .launch_args
                    .as_deref()
                    .is_some_and(|v| !v.trim().is_empty())
                    || item
                        .working_directory
                        .as_deref()
                        .is_some_and(|v| !v.trim().is_empty())
                {
                    let patch = games::GamePatch {
                        name: None,
                        launch_path: None,
                        launch_args: item.launch_args.as_deref(),
                        working_directory: item.working_directory.as_deref(),
                    };
                    if let Err(err) = db.with_conn(|conn| games::update(conn, &game.id, &patch)) {
                        // 补写失败只记日志：游戏本体已导入成功，不该因此报失败
                        log::warn!("补写启动参数失败（{}）: {}", game.name, err.message());
                    }
                }
            }
            Err(err) => {
                failures.push(ImportFailure {
                    name,
                    launch_path,
                    reason: err.message().to_owned(),
                });
            }
        }
    }

    Ok(ImportDiscoveredResult {
        imported,
        skipped_existing,
        failures,
    })
}

/// 批量导入勾选的候选（Tauri 命令入口）。
#[tauri::command]
pub fn import_discovered_games(
    app: AppHandle,
    db: State<'_, Db>,
    items: Vec<DiscoveredImportItem>,
) -> AppResult<ImportDiscoveredResult> {
    if items.is_empty() {
        return Err(AppError::invalid_argument("没有勾选要导入的游戏"));
    }

    let data_dir = app_data_dir(&app)?;
    import_discovered_impl(&db, &data_dir, &items)
}
