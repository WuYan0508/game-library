//! EXE 探测。
//!
//! 只做校验与信息提取，**不落库、不修改任何文件**。
//! 探测结果供添加弹窗预览；用户确认后再由命令重新探测一次并写入。

use std::path::Path;

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::platform::win::strip_verbatim_prefix;

/// 探测结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExeProbe {
    /// 用户选择的原始路径，原样保留供展示。
    pub source_path: String,
    /// 规范化后的绝对路径，同时也是实际启动目标。
    pub launch_path: String,
    /// 默认显示名：文件名去掉 `.exe`。
    pub default_name: String,
    /// 默认工作目录：EXE 所在目录。
    pub working_directory: String,
    pub size_bytes: u64,
}

/// 校验并提取 EXE 信息。
pub fn inspect_exe(raw_path: &str) -> AppResult<ExeProbe> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_argument("没有选择文件"));
    }

    let path = Path::new(trimmed);

    let is_exe = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
    if !is_exe {
        return Err(AppError::invalid_argument("请选择 .exe 可执行文件"));
    }

    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) => return Err(map_metadata_error(&err, trimmed)),
    };

    if !metadata.is_file() {
        return Err(AppError::invalid_argument(
            "这里是一个文件夹，请选择具体的 .exe 文件",
        ));
    }

    // 规范化路径：解析相对路径、去掉 `.` / `..` 与尾随分隔符。
    // 失败时不静默降级为原路径 —— 否则去重键会不稳定。
    let canonical = std::fs::canonicalize(path).map_err(|err| {
        AppError::permission_denied("无法解析该文件的真实路径，可能没有访问权限")
            .with_detail(format!("{trimmed}: {err}"))
    })?;

    // canonicalize 返回的是 verbatim 路径（`\\?\C:\...`）。
    // Shell API（图标提取、快捷方式解析）不认这个前缀，用户看着也别扭，
    // 统一在**产出路径**的地方剥掉。
    let launch_path = strip_verbatim_prefix(&canonical.to_string_lossy());

    let default_name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("未命名游戏")
        .to_owned();

    let working_directory = canonical
        .parent()
        .map(|parent| strip_verbatim_prefix(&parent.to_string_lossy()))
        .ok_or_else(|| AppError::invalid_argument("无法确定该文件所在目录"))?;

    Ok(ExeProbe {
        source_path: trimmed.to_owned(),
        launch_path,
        default_name,
        working_directory,
        size_bytes: metadata.len(),
    })
}

/// 把文件系统错误翻译成用户能看懂、且带正确分类的错误。
fn map_metadata_error(err: &std::io::Error, shown_path: &str) -> AppError {
    use std::io::ErrorKind;

    let detail = format!("{shown_path}: {err}");
    match err.kind() {
        ErrorKind::NotFound => {
            AppError::not_found("文件不存在或已被移动，请重新选择").with_detail(detail)
        }
        ErrorKind::PermissionDenied => {
            AppError::permission_denied("无法读取该文件，可能没有访问权限").with_detail(detail)
        }
        _ => AppError::internal("读取文件信息失败").with_detail(detail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppErrorCode;

    #[test]
    fn rejects_empty_path() {
        let err = inspect_exe("   ").expect_err("空路径应被拒绝");
        assert_eq!(err.code(), AppErrorCode::InvalidArgument);
    }

    #[test]
    fn rejects_non_exe_extension() {
        let err = inspect_exe("C:\\Windows\\System32\\notepad.txt").expect_err("非 exe 应被拒绝");
        assert_eq!(err.code(), AppErrorCode::InvalidArgument);
        assert!(err.message().contains("exe"), "错误说明应提到 exe");
    }

    #[test]
    fn rejects_directory_masquerading_as_exe() {
        // 目录名以 .exe 结尾：扩展名检查会通过，必须靠 is_file 兜住
        let dir = std::env::temp_dir().join(format!("fake-{}.exe", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("建临时目录");

        let err = inspect_exe(&dir.to_string_lossy()).expect_err("目录应被拒绝");
        assert_eq!(err.code(), AppErrorCode::InvalidArgument);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reports_missing_file_as_not_found() {
        let missing = std::env::temp_dir().join("definitely-missing-9f3a.exe");
        let err = inspect_exe(&missing.to_string_lossy()).expect_err("不存在应报错");
        assert_eq!(err.code(), AppErrorCode::NotFound);
    }

    #[test]
    fn accepts_a_real_exe_and_derives_defaults() {
        // 用系统自带的真实可执行文件，不伪造
        let target = r"C:\Windows\System32\notepad.exe";
        if !Path::new(target).exists() {
            eprintln!("跳过：本机没有 {target}");
            return;
        }

        let probe = inspect_exe(target).expect("真实 EXE 应探测成功");
        assert_eq!(probe.default_name, "notepad");
        assert!(probe.launch_path.to_lowercase().ends_with("notepad.exe"));
        assert!(probe.working_directory.to_lowercase().ends_with("system32"));
        assert!(probe.size_bytes > 0);
        assert_eq!(probe.source_path, target);
    }

    #[test]
    fn extension_matching_is_case_insensitive() {
        let err = inspect_exe("C:\\Windows\\System32\\NOTEPAD.EXE");
        // 大写扩展名应通过扩展名检查（后续可能因文件不存在而报 NotFound，
        // 但绝不能报「请选择 .exe」）
        if let Err(err) = err {
            assert_ne!(err.code(), AppErrorCode::InvalidArgument);
        }
    }
}
