//! 封面与图标命令（对应 docs/architecture.md §7.1「封面」）。

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::covers::{cover_store, icon_store};
use crate::database::Db;
use crate::database::games;
use crate::error::{AppError, AppResult};

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

/// 图标源文件大小上限（与封面同限）。
const MAX_ICON_SOURCE_BYTES: u64 = 20 * 1024 * 1024;

/// 导入自定义图标（EXE 提取其图标 / 图片转码为 PNG）。
#[tauri::command]
pub fn import_game_icon(
    app: AppHandle,
    db: State<'_, Db>,
    game_id: String,
    source_path: String,
) -> AppResult<IconResult> {
    import_game_icon_impl(&app_data_dir(&app)?, &db, &game_id, &source_path)
}

pub(crate) fn import_game_icon_impl(
    data_dir: &Path,
    db: &Db,
    game_id: &str,
    source_path: &str,
) -> AppResult<IconResult> {
    let trimmed = source_path.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_argument("没有选择图标文件"));
    }
    let path = Path::new(trimmed);
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_lowercase)
        .ok_or_else(|| {
            AppError::invalid_argument("图标只支持 EXE、ICO、PNG、JPG / JPEG、WebP 文件")
        })?;
    if !matches!(
        extension.as_str(),
        "exe" | "ico" | "png" | "jpg" | "jpeg" | "webp"
    ) {
        return Err(AppError::invalid_argument(
            "图标只支持 EXE、ICO、PNG、JPG / JPEG、WebP 文件",
        ));
    }

    let metadata = std::fs::metadata(path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => {
            AppError::not_found("图标文件不存在或已被移动").with_detail(format!("{trimmed}: {err}"))
        }
        std::io::ErrorKind::PermissionDenied => {
            AppError::permission_denied("无法读取该图标文件，可能没有访问权限")
                .with_detail(format!("{trimmed}: {err}"))
        }
        _ => AppError::internal("读取图标文件信息失败").with_detail(format!("{trimmed}: {err}")),
    })?;
    if !metadata.is_file() {
        return Err(AppError::invalid_argument("请选择文件，而不是文件夹"));
    }
    if metadata.len() > MAX_ICON_SOURCE_BYTES {
        return Err(AppError::invalid_argument(format!(
            "图标文件过大（{:.1} MB），上限为 20 MB",
            metadata.len() as f64 / (1024.0 * 1024.0)
        )));
    }

    let game = db.with_conn(|conn| games::get(conn, game_id))?;

    // EXE 走系统图标提取；图片统一解码后重编码为 PNG
    let png: Vec<u8> = if extension == "exe" {
        crate::platform::icon::extract_icon_png(trimmed)?.ok_or_else(|| {
            AppError::invalid_argument("这个 EXE 没有可提取的图标，请改用图片文件")
        })?
    } else {
        let decoded = image::open(path).map_err(|err| {
            AppError::invalid_argument("无法识别这个图标文件，可能已损坏或格式不受支持")
                .with_detail(format!("{trimmed}: {err}"))
        })?;
        let mut bytes = std::io::Cursor::new(Vec::new());
        decoded
            .write_to(&mut bytes, image::ImageFormat::Png)
            .map_err(|err| {
                AppError::internal("图标转码失败").with_detail(format!("{err}"))
            })?;
        bytes.into_inner()
    };

    if png.len() > icon_store::MAX_ICON_BYTES {
        return Err(AppError::invalid_argument(
            "图标转换后体积异常（超过 2 MB），请换一张小一些的图",
        ));
    }

    // 落盘为 uuid 新文件：路径变化可绕过前端缓存；旧图标文件随后清理
    let file_name = format!("{}.png", uuid::Uuid::new_v4());
    let directory = data_dir.join(icon_store::ICONS_DIR);
    std::fs::create_dir_all(&directory).map_err(|err| {
        AppError::internal("无法创建图标目录")
            .with_detail(format!("{}: {err}", directory.display()))
    })?;
    let destination = directory.join(&file_name);
    std::fs::write(&destination, &png).map_err(|err| {
        AppError::internal("写入图标失败")
            .with_detail(format!("{} -> {}: {err}", destination.display(), err))
    })?;
    let stored = format!("{}/{}", icon_store::ICONS_DIR, file_name);

    if let Some(previous) = &game.icon_path
        && previous != &stored
    {
        cover_store::remove_stored(data_dir, previous);
    }

    let updated = db.with_conn(|conn| games::set_icon_path(conn, game_id, Some(&stored)))?;
    Ok(IconResult {
        icon_path: updated.icon_path,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::import::add_game_from_exe_impl;
    use crate::database::Db;
    use crate::test_support::{TempDir, real_exe, write_test_png};

    #[test]
    fn import_png_icon_writes_file_and_updates_record() {
        let Some(exe_path) = real_exe() else {
            return;
        };
        let temp = TempDir::new("icon-import");
        let db = Db::open_in_memory().expect("内存库");
        let game = add_game_from_exe_impl(
            &db,
            &temp.data(),
            exe_path,
            Some("我的游戏".into()),
            None,
        )
        .expect("导入游戏");

        let source = write_test_png(&temp.data(), "icon-src.png", 64, 64);
        let result =
            import_game_icon_impl(&temp.data(), &db, &game.id, source.to_str().expect("路径"))
                .expect("导入图标应成功");

        let stored = result.icon_path.expect("应写入图标");
        assert!(stored.starts_with("icons/"));
        assert!(temp.data().join(&stored).is_file(), "图标文件应已落盘");
        assert_ne!(stored, game.icon_path.clone().unwrap_or_default());
    }
}
