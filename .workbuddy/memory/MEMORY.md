# 项目长期备忘 · Windows 个人游戏库（手动导入版）

## 项目定位

Windows 桌面个人游戏库，只做**手动导入**：导入 `.exe` / `.lnk`、自定义名称与封面、应用内启动、**仅统计本应用启动的**游玩时长。
技术栈：Tauri 2 + React + TypeScript + Rust + SQLite。目标平台 Windows 10/11 x64。

- 工程根目录：`E:/workbuddy数据/游戏库`
- 需求来源：`C:\Users\wuayn\Desktop\windows_game_library_agent_implementation.md`
- 设计文档：`docs/architecture.md`（架构与 ADR）、`docs/implementation-plan.md`（P0–P8 阶段计划）

## 文档存放与同步规则（重要）

**用户在 2026-10-14 明确要求：之后的改动直接更新资料库云端版本，不再只写本地。**

资料库空间 `GryY6izTzgUdfqO17iZ6nx`（个人空间）下的两份云端权威文档：

| 文档 | nodeId | 链接 |
|---|---|---|
| `architecture` | `h7MaZGt1KjzsQEPkQ6brhy` | https://www.workbuddy.cn/space/d/h7MaZGt1KjzsQEPkQ6brhy |
| `implementation-plan` | `nb5nUGdWypwX6vTX6pWqNT` | https://www.workbuddy.cn/space/d/nb5nUGdWypwX6vTX6pWqNT |

- 本地 `docs/architecture.md`、`docs/implementation-plan.md` 作为离线镜像/工作副本保留，改动后两边需保持一致。
- 云端文档走 `library` skill 的 `doc` 模块；**含 GFM 表格的正文直接 create 会 524，必须走「建无表格骨架 + 分次 insert_after 插表」**。
- 单元格文字里反引号代码与 `**粗体**` 都写成 `<Mark bold>…</Mark>`；action 字段名是 `type`；单次 insert ≤ 约 3.6 KB / 12 行。

## 项目铁律（来自需求文档，不可违反）

1. 不做 NVIDIA / Steam / Epic 自动扫描；不做账号、云同步、社区。
2. **绝不删除或修改用户的原始游戏文件**（EXE / `.lnk` / 游戏目录）。移除游戏只删库内记录。
3. 不做未经用户确认的联网封面抓取 —— 本应用**完全不联网**。
4. **计时会话必须由 Rust 后端持久化和管理**，前端不得作为计时权威来源，前端不得直连 SQLite。
5. 不因此启动失败 / 封面提取失败 / 单条记录损坏而让应用崩溃。
6. 不需要管理员权限。
7. 不用假数据或硬编码示例冒充真实游戏；未实现的功能不标记为完成。
8. 每阶段结束跑格式化 / 类型检查 / 测试 / 构建，并**如实**报告结果。

## 工作方式约定

- 阶段化推进（P0–P8），一次只做一个阶段，做完汇报六项：改了哪些文件 / 实现了什么 / 跑了哪些命令 / 测试真实结果 / 仍存在的限制 / 下一阶段计划。
- 设计与实现出现偏差时，**先改文档再改代码**。
- 每次汇报的效果以真实构建、真实运行结果为准。

## 关键技术决策摘要

| 主题 | 决策 |
|---|---|
| 数据访问 | Rust 侧 `rusqlite`（bundled）+ `rusqlite_migration`，`Arc<Mutex<Connection>>`；**不用** `tauri-plugin-sql` / `sqlx` |
| 启动进程 | `CreateProcessW` 直调，路径与参数分离传递，不走 shell，不用 `Command` 拼串 |
| 参数存储 | `launch_args` 存**原样参数串**，不分词、不重新转义 |
| 进程识别 | 进程树 BFS（root + 后代）+ 进程创建时间校验 PID 复用；不猜游戏进程名 |
| 计时基准 | `QueryUnbiasedInterruptTime`（单调、不含休眠）；墙钟仅展示 |
| 崩溃恢复 | 心跳 10s 落库；残留 `running` 会话一律标 `interrupted`，时长取最后心跳值 |
| COM | `IShellLinkW` / `IShellItemImageFactory` 在独立 STA 线程执行，`oneshot` 回传 |
| 封面 | 复制到 `app_data_dir/covers|icons`，DB 存相对路径；asset protocol 只放行这两个目录 |
| 数据目录 | 数据/封面/图标在 `%APPDATA%\com.wuayn.gamelibrary\`；**日志在 `%LOCALAPPDATA%\com.wuayn.gamelibrary\logs\`** —— 两者不是父子关系，查路径一律走 Tauri 的 `app_data_dir()` / `app_log_dir()`，不要自行拼接或复刻规则 |
| 编译并发 | `CARGO_HOME/config.toml` 固定 `[build] jobs = 6`（24 核但内存仅 15.8 GB，默认并发会让 rustc 以 `STATUS_ACCESS_VIOLATION` 崩溃） |

## 进度

- **P0 已完成并验证**（fmt / clippy 零警告 / 4 个测试通过 / MSI+NSIS 打包 / release 程序实测启动）。
- **P1 已完成并验证**（迁移 + 游戏 CRUD；真实库核验 WAL/CASCADE/部分唯一索引；fmt / clippy 零警告 / 27 tests / 打包通过）。
- **P2 已完成并验证**（EXE 导入与去重；`probe_exe` / `add_game_from_exe`；`AddGameDialog` 原生选择器 + 预览 + 重复提示；fmt / clippy 零警告 / 39 tests / 打包通过）。
- **P3 已完成并验证**（`.lnk` 快捷方式解析与导入；`platform::win::run_sta` 隔离 STA 线程；`IShellLinkW` 解析；目标缺失回传标记 + 手动修正；真实 `.lnk` 测试；fmt / clippy 零警告 / 56 tests / 打包通过）。
- **P4 已完成并验证**（图标提取 + 自定义封面 + 资源回收；`IShellItemImageFactory` → `image` 编码 PNG；Cover 三级降级；fmt / clippy 零警告 / **76 tests** / 打包通过；13 个命令契约核对无差异）。
  - ⚠️ 关键坑：`canonicalize` 的 `\\?\` verbatim 前缀 Shell API 不认，必须用 `platform::win::strip_verbatim_prefix` 剥掉，否则**所有游戏都提不到图标且不报错**。
- **P5 代码完成**（启动 + 进程树追踪 + 会话管理；`CreateProcessW` 直调、BFS 进程树、会话幂等结算、`stop_tracking` 只停表；fmt / clippy 零警告 / **lib 97 tests** / 打包通过；13 个命令契约核对无差异）。
  - ⚠️ **环境阻塞**：`#[cfg(test)]` 代码引用 `SessionManager` 会让测试二进制加载失败（`STATUS_ENTRYPOINT_NOT_FOUND`），已排查磁盘/增量/PATH/导入表/API Set/安全中心，均排除；最小 crate 测试正常 → 判定为本机工具链的代码生成问题。5 条会话编排测试移到 `tests/session_manager.rs` 并用 `[features] manual-tests` 门控，默认套件保持全绿。
  - P5 的 `duration_seconds` 恒为 0（时长计算属 P6），累计时长仍为 0 是**预期状态**。
- **P6 已完成并验证**（`QueryUnbiasedInterruptTime` 计时基准（不含休眠）、10s 心跳、`settle_and_accumulate` 事务结算+累加、启动时按最后心跳保守恢复；前端 tick 实时显示与恢复提示条；fmt / clippy 零警告 / **lib 108 tests** / 打包通过；**端到端恢复验证通过**——真实库种残留会话→启动程序→正确结算为 interrupted 并累加 120 秒）。
  - P5 遗留的「会话编排无运行证据」已在 P6 用真实库+真实程序补上。
- **P7 已完成并验证**（Toast 容器 + 按 AppErrorCode 的文案兜底表、设置页「诊断」区块新增 open_log_dir 命令、导入弹窗 Enter 确认；fmt / clippy 零警告 / **lib 108 tests** / 打包通过；契约 **17↔17** 无差异）。
  - 补齐了 TS 侧漏掉的 `LaunchFailed` / `AlreadyRunning` 错误码。
- **P8 已完成并验证** —— 🎉 **项目 P0–P8 全部完成**。全量校验七项退出码全 0（fmt / clippy 零警告 / lib 108 tests / 含集成目标 / tsc / vite / tauri build）；§11 验收清单逐条核对产出 `docs/manual-testing.md`；`README.md` 全新重写（含休眠策略、卸载不删数据等已知限制）。
  - **待人工收尾**（步骤见 manual-testing.md）：GUI 交互核对、安装包安装/卸载实测。
- **自动发现功能（新需求，用户推翻原铁律 #1）**：需求文档在 `C:\Users\wuayn\Downloads\windows_game_library_auto_discovery_implementation.md`。阶段一（调查）已完成，结论与设计在 `docs/auto-discovery-design.md`。关键结论：Steam `appmanifest` 与 **NVIDIA App 的 `ApplicationStorage.json`（明文 JSON，`DetectedFiles[0]` 即正确启动 exe）** 都真实可用；Epic 本机无游戏（清单为空，字段待实测）；Steam 不承诺自动找到 exe。铁律 #1 已修订为「不做未经用户确认的自动导入」。下一步：阶段二实现 `discovery/` 模块。

## 环境

- Node v22.22.2 / npm 10.9.7；VS2022 Community（MSVC 14.42 + 14.44，`E:\vs2022`）；Windows SDK 10；WebView2 Runtime 154/155。
- **Rust 已装好（2026-10-14）**：`rustc 1.99.0` / `cargo 1.99.0` / `clippy 0.1.99` / `rustfmt 1.10.0`，toolchain `stable-x86_64-pc-windows-msvc`。
  - 安装位置（都在 E 盘）：`CARGO_HOME=E:\rust\cargo`、`RUSTUP_HOME=E:\rust\rustup`，用户 PATH 已含 `E:\rust\cargo\bin`。
  - 上述两个变量已写入**用户级环境变量**（因为 C 盘只剩 16 GB）。**注意：正在运行的进程（含本工具的 shell）不会自动继承，跨会话调 cargo 时需手动 export。**
  - `RUSTUP_DIST_SERVER` / `RUSTUP_UPDATE_ROOT` 已指向清华镜像 —— 因为 `static.rust-lang.org` 在本机返回 **502 不可用**，而 `mirrors.tuna.tsinghua.edu.cn/rustup` 可用。
  - `index.crates.io` 与 `static.crates.io` **直连可用**（200，约 1s），暂未配 crates 镜像；若首次构建太慢，可在 `E:\rust\cargo\config.toml` 换清华源。
  - ⚠️ **坑：rustup 安装时创建 shim 硬链接失败，`E:\rust\cargo\bin` 下 13 个 exe 全是 0 字节**（只有 rustup.exe 正常）。已用 `cp rustup.exe <name>.exe` 逐个补齐解决。以后若 `rustup component add` 后再现「命令无输出」，同法修复。
  - ⚠️ **坑：并行编译会打爆内存。** 本机 24 逻辑核心但物理内存仅 15.8 GB（可用约 5 GB），cargo 默认按核心数并发调 rustc，rustc 会以 `STATUS_ACCESS_VIOLATION (0xc0000005)` 崩溃 —— **这不是代码错误，别去查代码**。已写 `E:\rust\cargo\config.toml` 设 `[build] jobs = 6` 限制并发。若再遇到同样的崩溃，先降 jobs。
- 已验证：`cargo build` 编译并运行 hello world 成功，MSVC `link.exe` 被自动定位，无需配置。
- Tauri 侧：`tauri.conf.json` 启用 `app.security.assetProtocol` 时，`Cargo.toml` 的 `tauri` 依赖**必须**加 `features = ["protocol-asset"]`，否则 build script 直接报错。
- 注意：工程路径含中文，若构建异常优先怀疑路径问题，可改 ASCII 路径。

## 版本号规则（用户 2026-10-15 指定）

- **每次对软件做出修改后，必须先询问用户是否修改版本号**，得到确认后才改版本。
- 当前版本：**1.0.0**（tauri.conf.json 与 src-tauri/Cargo.toml 两处同步改；设置页底部会显示版本号，读取自 Tauri 的 getVersion()）。
