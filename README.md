# 游戏库（Windows 个人游戏库）

一个**只做手动导入**的本地游戏库：导入 `.exe` / `.lnk`、自定义名称与封面、应用内启动、并统计**仅由本应用启动**的游玩时长。

- 技术栈：Tauri 2 + React + TypeScript + Rust + SQLite（`rusqlite`，bundled）
- 目标平台：Windows 10 / 11 x64
- **完全不联网**：不抓取在线封面、无账号、无云同步、无社区功能

## 功能

- **导入**：选择 EXE 或快捷方式（`.lnk` 会解析出目标、启动参数与工作目录）；按规范化启动目标去重
- **封面**：自定义封面 → 提取 EXE 图标（256px）→ 内联 SVG 占位图，三级降级
- **启动**：`CreateProcessW` 直调（路径 / 参数 / 工作目录分离传递），不走 shell
- **计时**：只统计**由本应用启动**的会话；按系统「无偏中断时间」累计，**电脑睡眠的时间不会被算进去**
- **崩溃恢复**：应用异常退出后，下次启动按最后一次心跳把残留会话记为「中断」，时长只可能少记不会多记
- **进程树追踪**：启动器（如 Steam / Epic）先拉起客户端再拉起游戏的场景，以「整棵进程树是否还有存活成员」为结束依据
- **不碰你的文件**：移除游戏只删库内记录与应用自管的封面/图标，**绝不删除原始游戏文件**

## 环境安装

| 依赖 | 说明 |
|---|---|
| Node.js ≥ 20 | 前端构建（本仓库用 v22 开发） |
| Rust (stable-msvc) | `rustup` 安装，toolchain `stable-x86_64-pc-windows-msvc` |
| Visual Studio 2022 | 勾选「使用 C++ 的桌面开发」（MSVC + Windows SDK） |
| WebView2 Runtime | Win11 自带；Win10 如缺可从微软官网装 |

Rust 在国内拉 crates 索引慢时可配清华镜像（`~/.cargo/config.toml`）。

## 开发运行

```bash
npm install          # 安装前端依赖
npm run tauri dev    # 开发模式（热重载）
```

## 测试

```bash
cd src-tauri
cargo fmt --check              # 格式检查
cargo clippy --all-targets     # 静态检查（要求零警告）
cargo test                     # 单元 + 集成测试
cd ..
npx tsc --noEmit               # 前端类型检查
npm run build                  # 前端构建
```

说明：

- Rust 侧测试在 `src-tauri` 内，`cargo test --lib` 覆盖核心逻辑（110 条）。
- 会话生命周期的集成测试（真实启动进程 → 退出 → 结算）单独放在
  `src-tauri/tests/session_manager.rs`，**默认不参与 `cargo test`**，
  需要时显式运行：
  ```bash
  cargo test --features manual-tests --test session_manager
  ```
- 快捷方式与图标相关用例会真实调用 Windows API（生成 `.lnk`、提取图标），
  个别用例在无法取得系统样本时会跳过并打印原因，**不会伪装通过**。

## 打包

```bash
npm run tauri build
```

产物在 `src-tauri/target/release/bundle/`：

- `msi/game-library_<版本>_x64_en-US.msi`
- `nsis/game-library_<版本>_x64-setup.exe`

## 数据目录

| 内容 | 位置 | 说明 |
|---|---|---|
| 数据库、封面、图标 | `%APPDATA%\com.wuayn.gamelibrary\` | 随用户漫游 |
| 日志 | `%LOCALAPPDATA%\com.wuayn.gamelibrary\logs\` | 不漫游（Windows 惯例）；设置页有「打开日志目录」按钮 |

两者**不是父子关系**，路径一律由 Tauri 的 `app_data_dir()` / `app_log_dir()` 解析。

**卸载不会删除数据目录** —— 你的游戏库记录、封面与日志都会保留；想彻底清除请手动删除上述两个目录。

## 已知限制

完整设计见 [`docs/architecture.md`](docs/architecture.md)（含架构决策记录），
分阶段实施计划与每阶段实测结果见 [`docs/implementation-plan.md`](docs/implementation-plan.md)，
验收清单逐条核对见 [`docs/manual-testing.md`](docs/manual-testing.md)。

- **不联网**：不抓取在线封面、无账号、无云同步。封面只能手动导入或取自 EXE 图标。
- **启动器计时限制**：通过平台启动器（Steam / Epic 等）启动的游戏，以进程树判定结束时机。
  若启动器在游戏退出后仍长期驻留，会话会被判定为「仍在运行」；卡片上会标注「启动器」提示这一点。
- **休眠不计入时长**：时长按无偏中断时间累计。这意味着合盖睡眠期间不会计时——这是有意设计。
- **崩溃恢复只可能少记**：恢复按最后一次心跳（约 10 秒一次）结算，应用被强杀时最后不足 10 秒的时长不会计入。
- **不识别游戏内存/画面**：计时完全依赖进程树，不注入、不截图。
- **单实例数据**：数据存在本机 SQLite，不做多设备同步。
