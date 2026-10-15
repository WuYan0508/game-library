//! 启动游戏进程。
//!
//! 分两个文件：`quote.rs` 负责命令行引号，本文件负责 `CreateProcessW`。
//!
//! 铁律（见 docs/architecture.md ADR-02 / ADR-03）：
//! - **不走 shell**，程序路径与参数分离传递；
//! - 参数串**原样透传**，不重写、不重新转义。

pub mod launch;
pub mod quote;
