//! 导入相关的业务模块。
//!
//! 本层只做「解析与校验」，把结果交回 commands 层；**不直接定义数据库表结构**，
//! 落库统一走 `database::games`。

pub mod dedupe;
pub mod exe;
pub mod shortcut;
