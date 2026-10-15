//! 维护类命令：修复丢失的资源。

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::covers::icon_store;
use crate::database::Db;
use crate::database::games;
use crate::database::models::{GameSort, SortOrder};
use crate::error::AppResult;

#[derive(Debug, Serialize)]
pub struct ReextractReport {
    /// 成功重新提取并回写的数量。
    pub fixed: usize,
    /// 本来就完好的数量。
    pub already_ok: usize,
    /// 提取失败的条目（启动目标不存在、提取不到图标等）。
    pub failed: Vec<ReextractFailure>,
}

#[derive(Debug, Serialize)]
pub struct ReextractFailure {
    pub name: String,
    pub reason: String,
}

/// 重新提取缺失的图标：icon_path 为空或指向的文件不存在时，
/// 从启动目标重新提取图标并回写到记录。
#[tauri::command]
pub fn reextract_missing_icons(db: State<'_, Db>, app: AppHandle) -> AppResult<ReextractReport> {
    let data_dir = app.path().app_data_dir().map_err(|err| {
        crate::error::AppError::internal("应用数据目录不可用").with_detail(err.to_string())
    })?;

    let all = db.with_conn(|conn| games::list(conn, GameSort::CreatedAt, SortOrder::Asc))?;

    let mut fixed = 0usize;
    let mut already_ok = 0usize;
    let mut failed: Vec<ReextractFailure> = Vec::new();

    for game in all {
        let missing = match &game.icon_path {
            None => true,
            Some(relative) => !data_dir.join(relative).is_file(),
        };
        if !missing {
            already_ok += 1;
            continue;
        }

        match icon_store::extract_and_store(&game.launch_path, &data_dir, &game.id) {
            Ok(Some(relative)) => {
                db.with_conn(|conn| games::update_icon_path(conn, &game.id, &relative))?;
                fixed += 1;
            }
            Ok(None) => failed.push(ReextractFailure {
                name: game.name.clone(),
                reason: "无法从启动目标提取图标（文件可能已被移动或删除）".to_owned(),
            }),
            Err(err) => failed.push(ReextractFailure {
                name: game.name.clone(),
                reason: err.message().to_owned(),
            }),
        }
    }

    Ok(ReextractReport {
        fixed,
        already_ok,
        failed,
    })
}
