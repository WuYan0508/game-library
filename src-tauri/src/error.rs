//! 统一错误类型。
//!
//! 约定（见 docs/architecture.md §7.3）：
//! - 前端只看到 `{ code, message }`，**不暴露任何内部堆栈或路径细节**；
//! - 细节信息通过 `log::error!` 落到日志，供诊断使用。

use std::fmt;

use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};

/// 面向用户的错误分类。前端按 `code` 决定提示文案与可执行动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppErrorCode {
    /// 资源不存在（文件、记录等）。
    NotFound,
    /// 权限不足或被系统拒绝。
    PermissionDenied,
    /// 参数非法。
    InvalidArgument,
    /// 快捷方式指向的启动目标不存在。前端据此提供「取消 / 手动选择启动目标」。
    TargetMissing,
    /// 启动进程失败（路径被阻止、工作目录不可用等）。
    LaunchFailed,
    /// 该游戏已经有正在跟踪的会话。重复点击启动时返回。
    AlreadyRunning,
    /// 已存在等价记录（按规范化启动目标去重）。
    Duplicate,
    /// 数据库读写失败。
    DatabaseError,
    /// 内部错误。文案应保持通用，细节只进日志。
    Internal,
}

impl AppErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            AppErrorCode::NotFound => "NotFound",
            AppErrorCode::PermissionDenied => "PermissionDenied",
            AppErrorCode::InvalidArgument => "InvalidArgument",
            AppErrorCode::TargetMissing => "TargetMissing",
            AppErrorCode::LaunchFailed => "LaunchFailed",
            AppErrorCode::AlreadyRunning => "AlreadyRunning",
            AppErrorCode::Duplicate => "Duplicate",
            AppErrorCode::DatabaseError => "DatabaseError",
            AppErrorCode::Internal => "Internal",
        }
    }
}

/// 应用统一错误。
///
/// `message` 是**面向用户**的说明（中文、可读、不含堆栈）；
/// `detail` 是给日志用的补充信息，不参与序列化。
#[derive(Debug)]
pub struct AppError {
    code: AppErrorCode,
    message: String,
    detail: Option<String>,
}

impl AppError {
    pub fn new(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
        }
    }

    /// 以下构造函数构成 docs/architecture.md §7.3 约定的错误契约。
    /// P0 阶段只有系统信息类命令，因此暂时只用到 `internal`；
    /// 其余在导入、数据库、启动等阶段接入。这里显式标注而不是删掉，
    /// 是为了保持契约完整，同时避免 dead_code 警告掩盖将来真正的问题。
    #[allow(dead_code)]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::NotFound, message)
    }

    #[allow(dead_code)]
    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::PermissionDenied, message)
    }

    #[allow(dead_code)]
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::InvalidArgument, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Internal, message)
    }

    pub fn duplicate(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Duplicate, message)
    }

    pub fn database(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::DatabaseError, message)
    }

    /// 快捷方式指向的目标不存在。
    pub fn target_missing(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::TargetMissing, message)
    }

    /// 启动进程失败。
    pub fn launch_failed(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::LaunchFailed, message)
    }

    /// 该游戏已经有正在跟踪的会话。
    pub fn already_running(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::AlreadyRunning, message)
    }

    /// 附加仅用于日志的细节。
    #[allow(dead_code)]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    #[allow(dead_code)]
    pub fn code(&self) -> AppErrorCode {
        self.code
    }

    #[allow(dead_code)]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[allow(dead_code)]
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for AppError {}

/// 序列化为前端契约 `{ code, message }`。`detail` 刻意不序列化。
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("code", self.code.as_str())?;
        state.serialize_field("message", self.message.as_str())?;
        state.end()
    }
}

/// Tauri 命令统一返回类型。
pub type AppResult<T> = Result<T, AppError>;

/// 把可能带内部细节的底层错误转成通用内部错误：
/// 对用户只说「内部错误」，细节写日志。
macro_rules! internal_from {
    ($src:ty) => {
        impl From<$src> for AppError {
            fn from(err: $src) -> Self {
                AppError::internal("操作未完成，请稍后重试").with_detail(err.to_string())
            }
        }
    };
}

internal_from!(std::io::Error);
internal_from!(tauri::Error);
internal_from!(serde_json::Error);

/// 数据库错误单独归类：前端可用它区分「操作失败」与「内部异常」。
impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::database("数据库操作失败").with_detail(err.to_string())
    }
}

impl From<rusqlite_migration::Error> for AppError {
    fn from(err: rusqlite_migration::Error) -> Self {
        AppError::database("数据库结构升级失败").with_detail(err.to_string())
    }
}

/// Windows API / COM 调用失败。本应用仅面向 Windows，这里直接依赖 `windows` crate。
impl From<windows::core::Error> for AppError {
    fn from(err: windows::core::Error) -> Self {
        AppError::internal("Windows 接口调用失败").with_detail(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_only_code_and_message() {
        let err = AppError::not_found("找不到该游戏记录").with_detail("/tmp/secret/path.db");
        let json = serde_json::to_string(&err).expect("序列化应成功");

        assert!(json.contains("NotFound"), "应包含 code: {json}");
        assert!(json.contains("找不到该游戏记录"), "应包含 message: {json}");
        assert!(
            !json.contains("secret"),
            "detail 绝不能被序列化给前端: {json}"
        );
    }

    #[test]
    fn io_error_maps_to_internal_without_leaking_path() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "C:\\very\\secret\\path");
        let err = AppError::from(io);

        assert_eq!(err.code(), AppErrorCode::Internal);
        assert!(!err.message().contains("secret"), "message 不应泄漏路径");
        assert!(err.detail().is_some_and(|d| d.contains("secret")));
    }
}
