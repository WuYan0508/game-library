//! 从 EXE 提取图标并落盘。

use std::path::Path;

use crate::error::AppResult;
use crate::platform::icon;

/// 相对数据根目录的图标目录名。
pub const ICONS_DIR: &str = "icons";

/// 图标 PNG 的体积上限。正常情况下 256px 图标只有几十 KB，
/// 超过说明来路可疑（或编码链路出了问题），不落盘。
const MAX_ICON_BYTES: usize = 2 * 1024 * 1024;

/// 提取图标并写入 `<data_dir>/icons/<game_id>.png`。
///
/// 返回相对数据根目录的路径（`icons/<game_id>.png`）。
///
/// **任何一步失败都返回 `Ok(None)`** —— 图标是「有更好、没有也能用」的东西，
/// 绝不能因为取不到图标就让游戏导入失败。
pub fn extract_and_store(
    launch_path: &str,
    data_dir: &Path,
    game_id: &str,
) -> AppResult<Option<String>> {
    let png = match icon::extract_icon_png(launch_path) {
        Ok(Some(png)) => png,
        Ok(None) => {
            log::info!("目标没有可提取的图标，将使用占位图: {launch_path}");
            return Ok(None);
        }
        Err(err) => {
            log::warn!(
                "图标提取失败，将使用占位图: {launch_path}（{}）",
                err.message()
            );
            return Ok(None);
        }
    };

    if png.len() > MAX_ICON_BYTES {
        log::warn!("图标体积异常（{} 字节），不落盘: {launch_path}", png.len());
        return Ok(None);
    }

    let file_name = format!("{game_id}.png");
    let directory = data_dir.join(ICONS_DIR);
    if let Err(err) = std::fs::create_dir_all(&directory) {
        log::warn!("创建图标目录失败（{}），将使用占位图", err);
        return Ok(None);
    }

    let absolute = directory.join(&file_name);
    if let Err(err) = std::fs::write(&absolute, &png) {
        log::warn!("写入图标失败（{}），将使用占位图", err);
        return Ok(None);
    }

    Ok(Some(format!("{ICONS_DIR}/{file_name}")))
}
