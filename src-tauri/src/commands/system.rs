//! 系统与应用信息命令（对应 docs/architecture.md §7.1「系统与设置」）。
//!
//! 这两个命令**不依赖数据库**，因此可以在数据层落地之前就提供真实数据。

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};

/// 应用信息。字段名经 `camelCase` 序列化后与 `src/types/system.ts` 一一对应。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// 应用名，取自 tauri.conf.json 的 productName。
    pub name: String,
    /// 应用版本，取自 tauri.conf.json 的 version。
    pub version: String,
    /// Tauri 框架版本。
    pub tauri_version: String,
    /// 构建档位：debug 或 release。
    pub build_profile: &'static str,
    /// 应用标识符，同时也是应用数据目录名。
    pub identifier: String,
}

/// 应用数据目录布局。路径均为绝对路径；数据库与日志目录在首次使用时才真正创建。
///
/// 注意：**日志目录不在数据根目录下**。Tauri 的 `app_log_dir()` 在 Windows 上位于
/// LocalAppData，而 `app_data_dir()` 位于 Roaming —— 遵循 Windows 惯例
/// （数据可漫游、日志不漫游）。这里如实返回两者的真实位置，不做拼接。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataPath {
    /// 应用数据根目录（Windows：`%APPDATA%\<identifier>`）。
    pub data_dir: String,
    /// SQLite 数据库文件路径（P1 阶段才会真正创建）。
    pub db_path: String,
    /// 日志目录（Windows：`%LOCALAPPDATA%\<identifier>\logs`）。
    pub log_dir: String,
    /// 自定义封面目录。
    pub covers_dir: String,
    /// 从 EXE 提取的图标目录。
    pub icons_dir: String,
}

#[tauri::command]
pub fn get_app_info(app: AppHandle) -> AppResult<AppInfo> {
    let package = app.package_info();

    Ok(AppInfo {
        name: package.name.clone(),
        version: package.version.to_string(),
        tauri_version: tauri::VERSION.to_string(),
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        identifier: app.config().identifier.clone(),
    })
}

#[tauri::command]
pub fn get_app_data_path(app: AppHandle) -> AppResult<AppDataPath> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| AppError::internal("无法定位应用数据目录").with_detail(err.to_string()))?;

    // 日志目录来自 app_log_dir()，在 Windows 上位于 LocalAppData，与数据根目录不同源。
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|err| AppError::internal("无法定位日志目录").with_detail(err.to_string()))?;

    let db_path = data_dir.join("game_library.db");
    let covers_dir = data_dir.join("covers");
    let icons_dir = data_dir.join("icons");

    Ok(AppDataPath {
        data_dir: to_display_string(&data_dir)?,
        db_path: to_display_string(&db_path)?,
        log_dir: to_display_string(&log_dir)?,
        covers_dir: to_display_string(&covers_dir)?,
        icons_dir: to_display_string(&icons_dir)?,
    })
}

/// 路径转字符串。仅在路径含无法表示的字节时报错——Windows 上正常路径不会触发。
fn to_display_string(path: &Path) -> AppResult<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| AppError::internal("应用数据目录路径无法转换为文本"))
}

/// 用系统文件管理器打开日志目录。
///
/// 诊断入口：日志目录不在数据根目录下（遵循 Windows 惯例），用户很难自己找到，
/// 所以提供一个按钮直接打开。返回实际打开的路径，前端可据此展示。
#[tauri::command]
pub fn open_log_dir(app: AppHandle) -> AppResult<String> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|err| AppError::internal("无法定位日志目录").with_detail(err.to_string()))?;

    // 目录可能还没建过（一条日志都没写过时），先补上再打开
    std::fs::create_dir_all(&dir).map_err(|err| {
        AppError::internal("无法创建日志目录").with_detail(format!("{}: {err}", dir.display()))
    })?;

    // explorer 对「已打开的窗口」返回非零退出码，这里不把它当失败 —— 只要进程能拉起就算成功
    let spawned = std::process::Command::new("explorer")
        .arg(dir.as_os_str())
        .spawn();

    match spawned {
        Ok(_) => Ok(dir.to_string_lossy().into_owned()),
        Err(err) => {
            Err(AppError::internal("无法打开日志目录")
                .with_detail(format!("{}: {err}", dir.display())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::to_display_string;

    #[test]
    fn converts_plain_path() {
        let path =
            std::path::Path::new("C:\\Users\\example\\AppData\\Roaming\\com.wuayn.gamelibrary");
        assert_eq!(
            to_display_string(path).expect("正常路径应可转换"),
            "C:\\Users\\example\\AppData\\Roaming\\com.wuayn.gamelibrary"
        );
    }

    #[test]
    fn converts_non_ascii_path() {
        let path = std::path::Path::new("E:\\workbuddy数据\\游戏库");
        assert_eq!(
            to_display_string(path).expect("中文路径应可转换"),
            "E:\\workbuddy数据\\游戏库"
        );
    }
}
