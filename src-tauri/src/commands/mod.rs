//! Tauri 命令层。
//!
//! 本层只做三件事：反序列化参数 → 调用下层模块 → 把结果/错误映射成前端契约。
//! 业务规则不写在这里。
//!
//! 约定：命令体很薄，真正的编排放在 `*_impl` 里且**只接收纯数据参数**
//! （`&Db`、`&Path` 等，而不是 `AppHandle`），这样整条链路可以被单元测试直接覆盖。

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};

pub mod covers;
pub mod discovery;
pub mod games;
pub mod import;
pub mod maintenance;
pub mod sessions;
pub mod system;
pub mod tray_settings;

/// 解析应用数据目录。命令层负责把 `AppHandle` 转成纯路径再传给 `*_impl`。
pub(crate) fn app_data_dir(app: &AppHandle) -> AppResult<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|err| AppError::internal("无法定位应用数据目录").with_detail(err.to_string()))
}
