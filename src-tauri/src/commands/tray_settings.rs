//! 「关闭按钮行为」的持久化设置 + 读写命令。
//!
//! 存在应用数据目录的 `settings.json` 里（重启后仍生效）。
//! 默认开启「关闭时最小化到系统托盘」。

use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::error::AppResult;

/// 共享状态：关闭按钮是否最小化到托盘。
#[derive(Debug)]
pub struct CloseBehavior {
    pub close_to_tray: RwLock<bool>,
}

impl CloseBehavior {
    /// 从 settings.json 读取（文件缺失 / 损坏时用默认值 true）。
    pub fn load(app: &AppHandle) -> Self {
        let close_to_tray = std::fs::read_to_string(settings_path(app))
            .ok()
            .and_then(|raw| serde_json::from_str::<SettingsFile>(&raw).ok())
            .map(|file| file.close_to_tray)
            .unwrap_or(true);

        Self {
            close_to_tray: RwLock::new(close_to_tray),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SettingsFile {
    close_to_tray: bool,
}

fn settings_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("应用数据目录应可用")
        .join("settings.json")
}

fn persist(app: &AppHandle, close_to_tray: bool) -> AppResult<()> {
    let file = SettingsFile { close_to_tray };
    let json = serde_json::to_string_pretty(&file)?;
    std::fs::write(settings_path(app), json)?;
    Ok(())
}

/// 读取当前行为。
#[tauri::command]
pub fn get_close_to_tray(state: State<'_, CloseBehavior>) -> bool {
    *state
        .close_to_tray
        .read()
        .expect("close_to_tray 锁未被污染")
}

/// 设置关闭按钮行为并持久化。
#[tauri::command]
pub fn set_close_to_tray(
    app: AppHandle,
    state: State<'_, CloseBehavior>,
    enabled: bool,
) -> AppResult<()> {
    *state
        .close_to_tray
        .write()
        .expect("close_to_tray 锁未被污染") = enabled;
    persist(&app, enabled)?;
    Ok(())
}
