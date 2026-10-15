//! 封面与图标命令（对应 docs/architecture.md §7.1「封面」）。

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::covers::{cover_store, icon_store};
use crate::database::Db;
use crate::database::games;
use crate::error::AppResult;

use super::app_data_dir;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverResult {
    /// 新的封面相对路径。理论上不会是 None（导入失败会直接报错）。
    pub cover_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IconResult {
    /// 提取并落盘后的图标相对路径；提不到则为 null（前端回退占位图）。
    pub icon_path: Option<String>,
}

/// 导入自定义封面。**只复制用户的原图，绝不移动或删除它。**
#[tauri::command]
pub fn import_cover(
    app: AppHandle,
    db: State<'_, Db>,
    game_id: String,
    source_path: String,
) -> AppResult<CoverResult> {
    import_cover_impl(&app_data_dir(&app)?, &db, &game_id, &source_path)
}

/// 重新提取图标。提不到会清空已有图标路径（而不是留一张过期的图）。
#[tauri::command]
pub fn extract_game_icon(
    app: AppHandle,
    db: State<'_, Db>,
    game_id: String,
) -> AppResult<IconResult> {
    extract_game_icon_impl(&app_data_dir(&app)?, &db, &game_id)
}

pub(crate) fn import_cover_impl(
    data_dir: &Path,
    db: &Db,
    game_id: &str,
    source_path: &str,
) -> AppResult<CoverResult> {
    // 顺序很重要：先把新封面落盘（校验不过就直接失败，不动现有记录），
    // 写库成功后再删旧文件 —— 反过来会出现「旧的删了、新的没写进去」的空窗。
    let relative = cover_store::import(source_path, data_dir)?;

    let previous = db.with_conn(|conn| games::get(conn, game_id))?.cover_path;
    let game = db.with_conn(|conn| games::set_cover_path(conn, game_id, Some(&relative)))?;

    if let Some(previous) = previous
        && previous != relative
    {
        cover_store::remove_stored(data_dir, &previous);
    }

    Ok(CoverResult {
        cover_path: game.cover_path,
    })
}

pub(crate) fn extract_game_icon_impl(
    data_dir: &Path,
    db: &Db,
    game_id: &str,
) -> AppResult<IconResult> {
    let game = db.with_conn(|conn| games::get(conn, game_id))?;

    // 提取失败返回 None，不报错
    let stored = icon_store::extract_and_store(&game.launch_path, data_dir, &game.id)?;

    let updated = db.with_conn(|conn| games::set_icon_path(conn, game_id, stored.as_deref()))?;

    // 这次没提到图标时，把上一张删掉 —— 否则会留一张与目标不再对应的旧图
    if stored.is_none()
        && let Some(previous) = game.icon_path
    {
        cover_store::remove_stored(data_dir, &previous);
    }

    Ok(IconResult {
        icon_path: updated.icon_path,
    })
}
