//! 启动与计时。
//!
//! - `launcher/`：启动进程（`CreateProcessW`）；
//! - `tracker/`：追踪进程树与会话生命周期。
//!
//! 计时基准与会话恢复在 `tracker` 内展开（docs/architecture.md ADR-04 / ADR-05 / ADR-06）。

pub mod process_tree;
pub mod recover;
pub mod session;
