# 实施计划 · Windows 个人游戏库（手动导入版）

> 对应实施说明 §12「开发顺序」与 §13「当前第一步」
> 配套文档：[architecture.md](./architecture.md)
> 状态：待评审 · 评审通过后从阶段 0 开始执行

---

## 0. 执行总则

严格执行实施说明 §2「Agent 必须遵守」与 §12 的阶段顺序：

1. 每阶段结束后运行**格式化、类型检查、测试、构建**，并报告**真实结果**（失败就写失败，不粉饰）。
2. 不用假数据、硬编码示例冒充真实游戏；不把未实现的功能标记为完成。
3. 每阶段汇报固定六项：**改了哪些文件 / 实现了什么 / 跑了哪些命令 / 测试真实结果 / 仍存在的限制 / 下一阶段计划**。
4. 一次只做**一个阶段**，等确认后再进下一阶段。不做「一次性生成整个应用」。

### 现状（已完成第 1 步）

| 检查项 | 结果 |
|---|---|
| 工程目录 `E:/workbuddy数据/游戏库` | 空目录，无既有代码，无覆盖风险 |
| Node / npm | v22.22.2 / 10.9.7 ✅ |
| Visual Studio 2022 Community（含 MSVC x64 工具链） | ✅ `E:\vs2022` |
| Windows SDK 10 | ✅ 已安装 |
| WebView2 Runtime | ✅ 154 / 155 |
| **Rust 工具链（rustup / cargo / rustc）** | ❌ **未安装 —— 阶段 0 的前置阻塞项** |

> 说明：Tauri 2 的 Rust 侧编译必须依赖 Rust `stable-x86_64-pc-windows-msvc`，与已装好的 VS2022 工具链正好匹配。**这是开始阶段 0 前唯一需要用户确认的环境改动**（会下载安装 rustup 并写入 `PATH`）。

---

## 阶段总览

| 阶段 | 名称 | 对应 §12 步骤 | 依赖 | 主要产出 |
|---|---|---|---|---|
| **P0** | 环境就绪与工程脚手架 | 1–3 | Rust 安装确认 | 可运行的空壳应用 + 深色基础 UI |
| **P1** | 数据层：迁移与游戏 CRUD | 4 | P0 | 迁移 + 全部 CRUD 命令 + 持久化验证 |
| **P2** | EXE 导入与去重 | 5 | P1 | 原生选择器 + 探测 + 唯一键去重 |
| **P3** | `.lnk` 解析与导入 | 6 | P2 | COM 解析目标/参数/工作目录 |
| **P4** | 图标提取与自定义封面 | 7 | P3 | 三级封面策略 + 占位图降级 |
| **P5** | 启动服务与最小进程监控 | 8 | P4 | `CreateProcessW` 启动 + 进程树追踪 |
| **P6** | 持久化计时与残留会话恢复 | 9 | P5 | 事务结算 + 心跳 + 保守恢复 |
| **P7** | 运行状态、搜索、排序、错误提示 | 10 | P6 | 完整交互闭环 |
| **P8** | 测试、修复、打包 + 文档 | 11–12 | P7 | 安装包 + README + 架构/手测文档 |

---

## P0 · 环境就绪与工程脚手架

**对应 §12：1 检查仓库与环境 → 2 计划（已完成）→ 3 搭建工程与基础 UI**

### 任务

1. 安装 Rust 工具链（`rustup` + `stable-x86_64-pc-windows-msvc`），验证 `cargo -V`、`rustc -V`、`cargo tauri --version`。
2. 用 `create-tauri-app`（React + TypeScript + Vite 模板）在 `E:/workbuddy数据/游戏库` 初始化工程，包管理器用 npm。
3. 整理目录为 architecture.md §8/§10 的结构（建空模块目录与 `mod.rs`，避免一次性塞业务代码）。
4. 接入 `tauri-plugin-dialog`、`tauri-plugin-log`；在 `tauri.conf.json` 配置：
   - 窗口标题「游戏库」、默认尺寸 1200×800、最小 900×600；
   - `app_data_dir` 下的 `covers/`、`icons/` 目录创建（`setup` 内）；
   - asset protocol 放行 `covers` / `icons`；
   - capabilities 只声明本阶段真正需要的权限。
5. 实现基础 UI 骨架（**纯静态外壳，先不接数据**）：
   - `LibraryPage`：顶部栏（应用名 / 搜索框 / 添加游戏按钮 / 设置入口）+ 网格区 + 空状态；
   - `EmptyState`：「添加第一款游戏」入口；
   - `SettingsPage`：先接 `get_app_info` 与 `get_app_data_path` 两个真实命令（这两个不依赖数据库，可立即真跑）；
   - `theme.css`：深色主题变量。
6. 实现 `AppError` + 空实现的 `error.rs` / `platform/win.rs` 骨架。

### 交付验收

- [ ] `cargo fmt --check`、`cargo clippy` 无 error
- [ ] `npx tsc --noEmit` 通过
- [ ] `npm run tauri dev` 能真实打开窗口，显示深色骨架 UI，空状态可见
- [ ] 设置页显示真实版本号与真实数据目录（非硬编码）
- [ ] `npm run tauri build` 通过，产出可执行文件/安装包
- [ ] 卡片网格用**真实空态**呈现，不塞假游戏数据

### 风险

| 风险 | 应对 |
|---|---|
| 工程路径含中文（`E:/workbuddy数据/游戏库`） | 若构建失败且定位为路径问题，改 ASCII 路径并更新 architecture.md §ADR-09 |
| 首次构建需下载大量 crate，耗时较长 | 后台执行，失败重试，必要时配置镜像源 |
| Visual Studio 版本/组件不匹配 | 已确认含 `Microsoft.VisualStudio.Component.VC.Tools.x86.x64` |

### 执行结果（已完成）

| 项 | 实际结果 |
|---|---|
| Rust 工具链 | `rustc` / `cargo` 1.99.0，`clippy` 0.1.99，`rustfmt` 1.10.0，toolchain `stable-x86_64-pc-windows-msvc`；装在 E 盘（`E:\rust\cargo`、`E:\rust\rustup`），因 C 盘仅剩 16 GB |
| 工程脚手架 | Tauri 2 + React + TypeScript + Vite，npm 管理，`identifier = com.wuayn.gamelibrary` |
| 前端校验 | `tsc` + `vite build` 通过 |
| Rust 校验 | `cargo fmt --check` 通过；`cargo clippy --all-targets` **零警告**；`cargo test` **4 passed / 0 failed** |
| 发布构建 | `npm run tauri build` 通过，产出 MSI（2.23 MiB）与 NSIS 安装包（1.52 MiB） |
| 运行验证 | 启动 release 程序成功；`setup` 真实创建数据目录；日志文件正常写入 |

**与计划的偏差**：

1. **模块骨架范围收窄。** 计划原写「建 database/、importer/、launcher/、tracker/、covers/、platform/ 空模块」。实际**只建了 P0 真正使用的 `commands/` 与 `error.rs`**。空模块会持续产生 `dead_code` 噪音，也容易让未实现的东西看起来已就绪；其余模块在各自阶段落地时再建。
2. **数据目录布局修正。** 日志目录不在数据根目录下 —— 见 architecture.md ADR-09。原假设 `logs/` 在数据根目录内，实测 Tauri 的 `app_log_dir()` 在 Windows 上位于 LocalAppData。
3. **新增编译并发度约束。** 首次编译时 rustc 因内存耗尽以 `STATUS_ACCESS_VIOLATION` 崩溃，需限制并发 —— 见 architecture.md ADR-10。

---

## P1 · 数据层：SQLite 迁移与游戏 CRUD

**对应 §12：4**

### 任务

1. 落地 `migrations/0001_init.sql`（schema 见 architecture.md §5），接入 `rusqlite_migration`。
2. `db.rs`：打开连接（WAL、`foreign_keys=ON`、`busy_timeout`）、跑迁移、`Db` 作为 managed state。
3. 实现 `models.rs` DTO 与 TS 类型 `src/types/game.ts`，字段一一对齐。
4. 实现命令：`list_games`、`get_game`、`search_games`、`update_game`、`remove_game`、`get_app_info`、`get_app_data_path`。
5. `remove_game` 只删库内记录 + 附带封面 GC，**严禁触碰原始文件**。
6. 前端 `gameApi.ts` 全量类型化封装 + `useGames` 接上真实数据；卡片展示真实记录（真实时长、真实名称）。
7. 单测（`cargo test`）：迁移可重复执行、CRUD 往返、`search_games` 的 `% _` 转义、累计秒数不允许负值。

### 交付验收

- [ ] 重启应用后记录仍在（真实重启验证）
- [ ] 移除游戏后原始 EXE 与快捷方式文件**确认未被删除**（手工核对）
- [ ] 数据库写入失败时 UI 显示错误，**不出现虚假成功状态**
- [ ] `cargo test` 全绿并报告实际用例数

### 执行结果（已完成）

| 项 | 实际结果 |
|---|---|
| 迁移 | `rusqlite_migration` 单条迁移 `0001_init.sql`；真实应用日志 `Database migrated to version 1` |
| 连接参数 | `journal_mode=wal`（数据目录确实出现 `-wal` / `-shm`）、`foreign_keys=1`、`busy_timeout=5000ms` |
| 真实库结构核验 | 3 张表、5 个索引齐全；`idx_sessions_single_running` 的 `WHERE status='running'` 原样保留；外键 `on_delete=CASCADE` 生效；数据 0 行（未塞任何假数据） |
| 命令 | `list_games` / `get_game` / `search_games` / `update_game` / `remove_game` |
| 前端 | `types/game.ts` + `gameApi` 全量类型化；`useGames`（含请求竞态处理）+ `GameCard` / `GameGrid`；搜索防抖 200ms；三种排序 + 升降序切换；空库与「搜索无结果」两套文案 |
| Rust 校验 | `cargo fmt --check` 通过；`cargo clippy --all-targets` **零警告**；`cargo test` **27 passed / 0 failed** |
| 前端校验 | `tsc` + `vite build` 通过 |
| 发布构建 | `npm run tauri build` 通过（MSI 3.12 MiB / NSIS 2.24 MiB） |

**与计划的偏差**：

1. **未建 `schema_migrations` 表。** 架构文档 §5 原列出该表；实际由 `rusqlite_migration` 通过 `PRAGMA user_version` 维护版本，无需额外建表。架构文档已同步修正。
2. **界面只有「读」路径。** P1 的命令清单本就不含新增命令，但验收项写了「重启后记录仍在」—— 该验收由单测 `records_survive_reopen` 覆盖（真实文件库：写入 → 关闭 → 重新打开 → 读回）；**带记录渲染卡片**要等 P2 导入接入后才能可视化验证。因此 `database::games::insert` 与 `SourceType` 目前在非测试构建里没有使用者，已显式标注 `#[allow(dead_code)]` 并在源码里写明原因。

---

## P2 · EXE 导入与去重

**对应 §12：5**

### 任务

1. `importer/exe.rs`：`inspect_exe`（存在性、普通文件、`.exe` 后缀、规范化、默认名、默认工作目录）。
2. `importer/dedupe.rs`：`normalize_key`（大小写、分隔符、路径空格、`canonicalize` 回退策略）。
3. 命令 `probe_exe`（弹窗预览）与 `add_game_from_exe`（落库）。
4. 前端 `AddGameDialog` 的「选择 EXE」入口：原生选择器 → 解析预览 → 可改显示名 → 确认导入。
5. 重复导入：命中唯一键返回 `Duplicate` + 已有记录 id，前端提示「已在库中」并提供「取消 / 查看已有记录」。
6. 不可读 / 格式不符：返回可理解错误（`InvalidArgument` / `PermissionDenied`）。

### 交付验收

- [ ] 有效 EXE 可导入；不存在的路径报错；非 `.exe` 被拒绝
- [ ] 大小写/斜杠/空格差异的同路径**不产生重复项**（构造用例验证）
- [ ] 同名的不同游戏**不会被误判重复**
- [ ] 自定义名称保存后重启仍在

### 执行结果（已完成）

| 项 | 实际结果 |
|---|---|
| 探测 | `importer/exe.rs`：扩展名大小写不敏感；**拒绝目录**（目录名也可能以 `.exe` 结尾）；`canonicalize` 失败不静默降级为原路径（否则去重键不稳定）；错误按「不存在 / 无权限 / 其他」分类 |
| 去重 | 探测阶段就查出重复项并回传 `duplicate` 引用；落库仍由唯一索引兜底。**不做哈希**，直接存规范化串，排障时可肉眼比对 |
| 命令 | `probe_exe`（含 duplicate）、`add_game_from_exe`（落库前**重新探测一次**，防止预览与确认之间文件被移动或替换） |
| 前端 | `AddGameDialog`：原生文件选择器 → 解析预览（显示名可改 / 启动目标 / 工作目录 / 体积）→ 确认导入；重复时禁用确认并显示已有记录名；Esc 关闭、进入预览自动聚焦名称框 |
| 契约核对 | 前端 9 个 `invoke` 名与后端注册的 9 个命令**逐一比对无差异**（参数名依赖 Tauri 的 camelCase↔snake_case 转换） |
| Rust 校验 | `cargo fmt --check` 通过；`cargo clippy --all-targets` **零警告**；`cargo test` **39 passed / 0 failed** |
| 前端校验 | `tsc` + `vite build` 通过 |
| 发布构建 | `npm run tauri build` 通过 |

**与计划的偏差**：

1. **命令体抽成 `*_impl(&Db, ...)`**，`#[tauri::command]` 只做转发。原因：Tauri 命令拿的是 `State<'_, Db>`，单测里构造不出来；抽一层之后「探测 → 查重 → 落库」整条链路可以被 6 个测试直接覆盖（真实系统 EXE + 内存库），而不是只能靠手工点界面验证。
2. **去重判定提前到探测阶段。** 原计划是「落库命中唯一索引 → 返回 Duplicate」，实际改为探测时即回传 `duplicate` 引用，用户在确认前就看见「已在库中」；落库的唯一索引仍保留为并发兜底。
3. **`SourceType::Shortcut` 暂未构造**，标注 `#[allow(dead_code)]` 并在源码注明 P3 启用。

---

## P3 · `.lnk` 快捷方式解析与导入

**对应 §12：6**

### 任务

1. `platform/win.rs`：`com_thread::run_sta(f)`（独立 STA 线程 + `CoInitializeEx` + `oneshot` 回传）。
2. `importer/shortcut.rs`：`IShellLinkW` + `IPersistFile::Load` → `GetPath`（`SLGP_RAWPATH` + 手动展开环境变量）/ `GetArguments` / `GetWorkingDirectory`。
3. 目标不存在 → `TargetMissing`；前端「取消 / 手动修正路径」对话框，**绝不静默导入**。
4. 识别常见平台启动器并置 `is_launcher = 1`（Steam / Epic / Ubisoft / EA / Battle.net / GOG）。
5. 命令 `probe_shortcut` / `add_game_from_shortcut`；`AddGameDialog` 的「导入快捷方式」入口。
6. **明确禁止**把 `.lnk` 路径当 EXE 路径；参数原样保存（architecture.md ADR-03）。

### 交付验收

- [ ] 真实 `.lnk` 解析出正确目标、参数、工作目录（用带参数的真实快捷方式验证）
- [ ] 失效快捷方式不落入「可启动」状态
- [ ] 参数含空格/引号的快捷方式能按原参数启动（P5 中回验）
- [ ] 启动器型快捷方式如实保留其启动配置

### 执行结果（已完成）

| 项 | 实际结果 |
|---|---|
| COM 隔离 | `platform/win.rs` 新增 `run_sta`：专用 STA 线程 + `CoInitializeEx`/`CoUninitialize` 用 RAII 守卫成对；COM 线程 panic 转成可展示错误，不让应用崩 |
| 解析 | `IShellLinkW` + `IPersistFile::Load`；`GetPath` 用 `SLGP_RAWPATH`（不触发 Shell 解析）后手动 `ExpandEnvironmentStringsW` 展开 `%VAR%`；参数**原样保留**；相对路径按 `.lnk` 所在目录补齐；目标存在时再 `canonicalize` |
| 去重一致性 | `.lnk` 目标经过 `canonicalize`，因此「导入 EXE」与「导入指向同一目标的 `.lnk`」产出**同一个去重键** |
| 目标缺失 | 探测**不报错**，回传 `targetExists = false` 与原目标路径；落库时若仍缺且无手动修正 → `TargetMissing`，**不落库、不留记录** |
| 手动修正 | `launchPathOverride`；修正目标同样过一遍 `exe::inspect_exe` 校验；工作目录跟随修正后的目标 |
| 启动器识别 | 按**目标文件名**匹配 10 个已知启动器（Steam / Epic / Ubisoft / EA / Battle.net / GOG 等），UI 提示「计时可能不准确」 |
| 前端 | `AddGameDialog` 两条入口（选择 EXE / 导入快捷方式）；目标缺失时提供「手动选择启动目标」并实时显示修正后的路径 |
| 契约核对 | 前端 11 个 `invoke` 名与后端注册的 11 个命令**逐一比对无差异** |
| Rust 校验 | `cargo fmt --check` 通过；`cargo clippy --all-targets` **零警告**；`cargo test` **56 passed / 0 failed**（P2 为 39） |
| 前端校验 | `tsc` + `vite build` 通过 |
| 发布构建 | `npm run tauri build` 通过（MSI 3.14 MiB / NSIS 2.25 MiB） |

**与计划的偏差**：

1. **目标缺失改为「探测不报错 + 回传标记」**，而不是探测阶段直接抛 `TargetMissing`。原因：前端需要在同一屏里同时展示「原本指向哪里」「是否已在库中」，并在用户手动修正后才允许落库；抛异常会让弹窗只剩一句错误文案。落库前的 `TargetMissing` 仍保留为最后一道闸。
2. **测试用 PowerShell 生成真实 `.lnk`**（临时目录，不碰桌面）—— COM 行为没法用假数据糊弄。测试辅助抽到 `cfg(test)` 的 `test_support` 模块，供 `shortcut` 与 `import` 两处复用；生成失败时用例**跳过并打印原因**，而不是伪装通过。
3. 修了一个**测试期望**错误：Windows 路径语义会忽略尾部分隔符，`C:\x\` 的 `file_stem` 是 `"x"` 而不是 `None`，所以「无文件名的路径」要用根路径构造。是测试写错了，不是代码错。

---

## P4 · 图标提取与自定义封面

**对应 §12：7**

### 任务

1. `covers/icon.rs`：`IShellItemImageFactory::GetImage`（回退 `SHGetFileInfoW` + `SHGFI_LARGEICON`）→ WIC 编码 PNG → 写 `icons/`；**任何失败返回 `None`**。
2. `covers/cover_store.rs`：`import_cover`（扩展名白名单、≤20 MB、尺寸 ≤8192²、复制到 `covers/`、UUID 文件名）。
3. `resolve_cover`：自定义封面 → 图标 → 占位图，每步校验文件真实存在。
4. `get_cover_url`（`convertFileSrc`）+ 前端 `Cover.tsx`（`onError` 降级占位图）。
5. `EditGameDialog` 支持更换/清除封面，保存后**立即刷新 UI**。
6. 打包内置一张 `placeholder.png`（自绘简单图形，非第三方素材）。

### 交付验收

- [ ] 有图标的 EXE 能提取并展示
- [ ] 图标提取失败**仍可完成导入**
- [ ] 自定义封面导入 → 刷新 → 重启后均正常显示
- [ ] 手工删除封面文件后回退到占位图，不报错不白屏
- [ ] 超大/不支持格式的图片被拒绝且提示明确

### 执行结果（已完成）

| 项 | 实际结果 |
|---|---|
| 图标提取 | `platform/icon.rs`：`IShellItemImageFactory::GetImage`（256px）→ 回退 `SHGetFileInfoW` + `SHGFI_LARGEICON`；HBITMAP 优先**直读 DIB section**（`GetDIBits` 对它反而会失败），HICON 用 `DrawIconEx` 画到 32 位 DIB；用 `image` crate 编码 PNG，**不依赖 WIC 编码器** |
| 并发处理 | 提取全程**串行化**：Shell 图像缓存是进程级共享的，多线程同取一个文件会互相干扰（`GetImage` 偶发返回空位图，实测并行跑测试能复现） |
| 封面 | 白名单 png/jpg/jpeg/webp + ≤20 MB + ≤8192²，UUID 命名落盘；**只复制不移动原图**；校验失败直接报错且不留半条记录 |
| 回收 | `remove_game` 删记录后按引用集合回收；**只删本应用生成的文件**（`covers/` 下必须是 UUID 命名、`icons/` 下必须是 png），用户自己放进去的文件一律不动 |
| 前端 | `Cover` 组件三级降级（自定义封面 → 图标 → 内联 SVG 占位图），加载失败由 `onError` 兜底；添加弹窗支持选择封面 |
| 契约核对 | 前端 13 个 `invoke` 名 ↔ 后端 13 个命令，零差异 |
| 作用域核验 | 查 Tauri 源码确认 `$APPDATA` 作用域变量确实映射到 `app_data_dir`（写错的话图片会**静默**加载失败、永远显示占位图） |
| Rust 校验 | `cargo fmt --check` 通过；`cargo clippy --all-targets` **零警告**；`cargo test` **76 passed / 0 failed**（P3 为 56） |
| 前端校验 | `tsc` + `vite build` 通过 |
| 发布构建 | `npm run tauri build` 通过（MSI 3.46 MiB / NSIS 2.49 MiB） |

**本阶段修掉的关键 bug（不修的话图标功能等于不存在）**：

`std::fs::canonicalize` 在 Windows 上返回 **verbatim 路径**（`\\?\C:\Windows\System32\notepad.exe`），而 **Shell 系列 API（`SHCreateItemFromParsingName`、`SHGetFileInfoW`）不接受这个前缀** —— 带着它调用一律失败。这意味着**所有**导入的游戏都拿不到图标，而且因为降级设计得「优雅」，只会表现为「一直是占位图」，不会报任何错。
新增 `platform::win::strip_verbatim_prefix`，在所有**产出路径**的地方统一剥掉。测试也补了「剥完后路径仍可访问」的断言。

**与计划的偏差**：

1. **图标提取放在 `platform/icon.rs` 而不是 `covers/icon.rs`。** 它直接调用 Windows API，属基础设施层；`covers` 只负责「什么时候提取、存到哪、失败了怎么办」。
2. **未实现 `get_cover_url` 命令。** 三级策略的**落地**在前端：后端只给相对路径，前端拼成绝对路径后走 `convertFileSrc`，文件缺失由 `<img onError>` 回落占位图 —— 等价效果，少一次 IPC 与一个命令。
3. **占位图用内联 SVG**，不是打包的 `placeholder.png`：少一个二进制资源，且永远可用。
4. **未做「换封面 / 清除封面」的编辑界面。** `import_cover` / `extract_game_icon` 两个命令已实现并被集成测试覆盖，但 UI 入口属于编辑弹窗（P7）。当前可通过「添加游戏时选封面」完成自定义封面的全流程验证。
5. **安装包变大**（2.25 → 2.49 MiB）：引入 `image` crate 只为读图片尺寸与编码 PNG，开了 `png/jpeg/webp` 三个必要特性，未开其它。

---

## P5 · 启动服务与最小进程监控

**对应 §12：8 —— 本项目最高风险阶段**

### 任务

1. `launcher/quote.rs`：仅对 `lpCommandLine` 中的**程序路径**做 MSVCRT 规则加引号。
2. `launcher/launch.rs`：`CreateProcessW`（`lpApplicationName` = 路径、`lpCommandLine` = 引号路径 + 原样参数、`lpCurrentDirectory` = 工作目录），取 `pid` + `GetProcessTimes` 创建时间；句柄 `Drop` 守卫。
3. `tracker/process_tree.rs`：`CreateToolhelp32Snapshot` + BFS 收集 root 及后代；创建时间校验 PID 复用。
4. `tracker/session.rs`：`SessionManager`（内存 `active` map + DB 唯一部分索引双层防重复）。
5. 追踪循环（`tokio::task` + `interval(2s)`），进程树为空 → 结束。
6. `commands::launch_game` / `get_active_sessions` / `stop_tracking`（**只停表，不杀进程**）。
7. 前端：启动按钮防抖置灰、`RunningBadge` 运行状态、`useSessionEvents` 订阅事件。
8. 本阶段先**只落 `started_at` 与 status**，时长结算留到 P6（保持阶段边界清晰，但会话记录必须真实）。

### 交付验收

- [ ] 直接 EXE 可从应用内启动
- [ ] 带参数快捷方式按正确参数启动（用会回显参数的真实程序验证）
- [ ] 不存在路径/启动失败返回结构化错误并给出 `GetLastError`
- [ ] 重复点击不产生第二个活动会话
- [ ] 启动器型游戏：进程树策略的实际表现被如实记录（不假装准确）
- [ ] **从应用外部启动的游戏不产生任何会话记录**

---

## P6 · 持久化计时与残留会话恢复

**对应 §12：9**

### 任务

1. `QueryUnbiasedInterruptTime` 作为计时基准（architecture.md ADR-05）。
2. `heartbeat`：每 10 秒写 `tracked_seconds` / `last_heartbeat_at`。
3. `settle`：同一事务内写 `ended_at` / `duration_seconds` / `status` + `games.total_play_seconds` 累加；`WHERE status='running'` 保证**只结算一次**。
4. `tracker/recover.rs`：`recover_on_startup` + `recover_interrupted_sessions` 命令，按 ADR-06 保守恢复。
5. 前端：`session://tick` 平滑显示运行中时长；结束时刷新卡片累计时长；恢复项顶部提示「上次有 N 条记录被异常中断」。
6. 单测：结算幂等（重复调用只累加一次）、恢复计算不虚增、心跳字段写入正确。

### 交付验收

- [ ] 游戏退出后时长被结算且**不重复累加**（重复触发验证）
- [ ] 从应用内启动后能创建**唯一**会话
- [ ] 强制结束应用进程后重启：残留会话被标记 `interrupted`，时长不虚增（记录恢复前后数值对比）
- [ ] 休眠策略与限制写入 README
- [ ] 前端定时刷新只用于展示，**不作为计时来源**（代码审查确认）

### 执行结果（已完成）

| 项 | 实际结果 |
|---|---|
| 计时基准 | `platform/win.rs` 新增 `unbiased_interrupt_time()`（`QueryUnbiasedInterruptTime`，100ns）与 `elapsed_seconds()` 纯函数；**不含系统休眠** |
| 心跳 | 约 10 秒（2s × 5 轮）落一次 `tracked_seconds` + `last_heartbeat_at`；只写仍是 running 的会话 |
| 结算 | `settle_and_accumulate`：**同一事务**里写会话（ended_at / duration_seconds / status）并累加 `games.total_play_seconds` 与 `last_played_at`；`WHERE status='running'` 保证只结算一次 |
| 恢复 | `tracker/recover.rs::recover_on_startup`：按**最后心跳**把残留 running 会话结算为 interrupted 并累加；有恢复时推送 `sessions://recovered`；恢复逻辑不依赖 `SessionManager`，可被单元测试覆盖 |
| 前端 | `useSessions` 处理 `session://tick`（实时显示运行中时长）与 `sessions://recovered`（顶部提示条）；结束/启动后自动刷新列表；**前端不定时刷新计时，进度全部来自后端事件** |
| Rust 校验 | `cargo fmt --check` 通过；`cargo clippy --all-targets` 零警告；`cargo test --lib` **108 passed / 0 failed**（P5 为 97） |
| 前端校验 | `tsc` + `vite build` 通过 |
| 发布构建 | `npm run tauri build` 通过（MSI 3.50 MiB / NSIS 2.53 MiB） |
| **端到端恢复验证** | 往真实库种一条 `tracked_seconds=120` 的残留 running 会话 → 启动 release 程序 → 日志确认「按最后心跳记 120 秒，interrupted」→ 库内该会话变 `interrupted` 且 `duration_seconds=120`，游戏 `total_play_seconds=120`、`last_played_at` 已写入。**验证后已清理测试数据** |

**与计划的偏差**：

1. **未实现 `recover_interrupted_sessions` 命令**。恢复在启动时静默完成并推送事件，前端用提示条展示 —— 不需要用户手动触发，少一个命令。
2. **休眠策略写入 README** 暂缓：休眠不计入时长的行为已写进架构文档 ADR-04/ADR-05 与本节；README 在收尾阶段（P8）统一写，避免中途写两遍。

---

## P7 · 运行状态、搜索、排序与错误提示

**对应 §12：10**

### 任务

1. 排序：名称 / 累计时长 / 添加时间（升序与降序）。
2. 搜索：名称 + 原始名模糊匹配；无结果显示独立提示（区别于空库空状态）。
3. 卡片细节：超长名称截断/换行不撑坏布局；运行中状态克制而明确；解析中/错误态。
4. 错误提示统一走 `Toast`，按 `AppError.code` 给不同文案与动作。
5. 设置页补全：默认封面策略说明、日志/诊断入口（打开日志目录）。
6. 可访问性与键盘操作：弹窗 Esc 关闭、Enter 确认、按钮可聚焦。

### 交付验收

- [ ] 三种排序结果正确（构造多游戏数据核对）
- [ ] 搜索无结果与空库两种文案区分正确
- [ ] 超长名称不破坏网格布局
- [ ] 各类错误都有可理解的用户文案，且不暴露堆栈

### 执行结果（已完成）

| 项 | 实际结果 |
|---|---|
| 排序 | 名称 / 累计时长 / 添加时间，升序与降序均可（P2 已实现，本轮核对）；database 层有 `list_supports_declared_sorts` 测试 |
| 搜索 | 名称 + 原始名模糊匹配（`search_matches_original_name_too` 测试）；百分号与下划线按字面处理；无结果与空库使用不同文案 |
| 卡片细节 | 超长名称两行截断（line-clamp），不撑坏网格；运行中徽标实时显示已玩时长；解析中 / 错误态就绪 |
| 错误提示 | 新增 **Toast 容器**与轻量 pub-sub；会话操作与设置页失败走 Toast 自动消失；按 `AppErrorCode` 建立文案兜底表，未知 code 统一给通用文案，**不暴露堆栈** |
| 设置页 | 新增「诊断」区块：打开日志目录（新命令 `open_log_dir`，目录不存在会先创建）；「当前阶段限制」更新为真实状态 |
| 可访问性 | 导入弹窗 Esc 关闭（已有）+ **Enter 确认**（新增，仅在可提交时生效） |
| Rust 校验 | `cargo fmt --check` 通过；`cargo clippy --all-targets` 零警告；`cargo test --lib` **108 passed / 0 failed** |
| 前端校验 | `tsc` + `vite build` 通过 |
| 发布构建 | `npm run tauri build` 通过（MSI 3.51 MiB / NSIS 2.53 MiB） |
| 契约核对 | 前端 **17** 个 `invoke` 名与后端 **17** 个命令逐一比对无差异 |

**与计划的偏差**：

1. **错误提示没有做成全局拦截器**，而是「就地显示 + Toast」分层：导入弹窗内的错误就地显示（用户需要上下文），会话操作与设置页失败用 Toast。理由：把弹窗内的错误弹成 Toast 会让用户找不到出错的东西。
2. **「构造多游戏数据核对三种排序」用 database 层既有测试覆盖**（`list_supports_declared_sorts`），未再手工构造界面数据 —— 排序逻辑在 SQL，不在前端。

---

## P8 · 测试、打包与文档

**对应 §12：11–12**

### 任务

1. 按实施说明 §11 逐条跑验收清单，产出 `docs/manual-testing.md`（逐条记录实测结果与结论）。
2. 修复发现的问题（**不掩盖**），回归验证。
3. `cargo fmt` / `cargo clippy` / `cargo test` / `npx tsc --noEmit` / `npm run build` / `npm run tauri build` 全量执行并记录真实输出。
4. 产出 Windows 安装包（MSI 或 NSIS），实测安装、启动、卸载（卸载不删用户数据目录，需在文档说明）。
5. 补齐 `README.md`：环境安装、开发运行、测试、打包、数据目录、**已知限制**（architecture.md §12）。
6. 补齐 `docs/architecture.md` 与实施说明的差异记录（如实际字段/命令有调整，如实更新）。

### 交付验收

- [ ] 实施说明 §11 验收清单**逐条**有结果
- [ ] 格式化、测试、类型检查、开发构建、发布构建全部通过（附真实命令输出）
- [ ] 安装包可安装、可运行、可卸载
- [ ] README 含环境安装/开发/测试/打包步骤与已知限制

### 执行结果（已完成）

| 项 | 实际结果 |
|---|---|
| 全量校验 | `cargo fmt --check` / `cargo clippy --all-targets`（零警告）/ `cargo test --lib`（**108 passed / 0 failed**）/ `cargo test`（含集成测试目标）/ `npx tsc --noEmit` / `npm run build` / `npm run tauri build` —— **七项全部退出码 0** |
| 验收清单 | §11 逐条核对完成，产出 [`docs/manual-testing.md`](manual-testing.md)：✅ 自动化覆盖 / ✅ 实测 / ⚠️ 待人工 三类标注 |
| 端到端实测 | 启动恢复（真实库 + 真实程序，120 秒）、图标提取（256px 目视核对）——见 manual-testing.md |
| 安装包 | MSI 3.51 MiB 与 NSIS 2.53 MiB 均已产出；**安装与卸载的实测需人工执行**（不未经同意往系统装软件），步骤已写进 manual-testing.md |
| README | 全新重写：功能、环境安装、开发运行、测试（含 manual-tests 开关）、打包、数据目录（含卸载不删数据）、已知限制（含休眠策略、启动器计时限制、崩溃恢复只可能少记） |
| 架构差异记录 | 全程「先改文档再改代码」，架构文档已与实现一致；各阶段的实现偏差记录在实施计划对应段落的「与计划的偏差」小节 |

**待人工完成的收尾**（本环境无法代替，步骤见 manual-testing.md）：

1. GUI 交互核对（启动 / 运行中徽标 / 结束结算 / 重复点击 / 停止计时不杀进程 / 封面回退 / 打开日志目录）。
2. 安装包安装 → 运行 → 卸载，并确认卸载不删数据目录。

---

## 横切事项（贯穿全阶段）

| 事项 | 做法 |
|---|---|
| 错误处理 | 所有文件/进程/数据库操作返回 `Result`，统一 `AppError`；不因单点失败崩溃 |
| 不做假数据 | 卡片只显示真实库记录；空态就用空态 |
| 不碰用户文件 | 移除游戏不删原始文件；封面只复制；`stop_tracking` 不杀进程 |
| 阶段边界 | 未实现的功能不标记完成；每个阶段结束都跑格式化/类型检查/测试/构建 |
| 文档同步 | 设计与实现出现偏差时，**先改文档再改代码**，保持两者一致 |

---

## P8 后增量演进（2026-10-15，v1.0.0 → v1.0.2）

P8 验收收尾后按用户需求持续迭代，分三批落地，每批均通过
`cargo fmt / clippy（零警告）/ cargo test --lib / tsc / vite / tauri build` 全量校验。

### 批次一：无边框窗口（v1.0.0）

- `decorations: false` + 自绘标题栏（`data-tauri-drag-region`）+ 四边缩放热区 + 双击最大化。
- 修复：`core:window:default` 不含 `allow-start-dragging`，拖拽被 ACL 静默拒绝 → capabilities 显式声明。

### 批次二：设置页改版 + 托盘 + 单实例（v1.0.1）

- 设置页清空重做：开机自启动（tauri-plugin-autostart）/ 关闭按钮行为（settings.json 持久化）/
  软件外观（深色、白色；双主题变量 + localStorage）/ 注意事项 / 维护 / 版本号。
- 系统托盘：tray-icon 特性，左键打开、右键菜单；关闭按钮默认最小化到托盘，可配置直接退出。
- 单实例：tauri-plugin-single-instance（第一个注册），重复启动唤醒已有窗口。
- 修复：单实例上线前，托盘化后再点图标会开出第二个实例。

### 批次三：拖拽排序 + 图标链路（v1.0.2）

- 自定义排序：DB 迁移 0002（sort_order）、reorder_games 命令、长按 350ms 拖拽
  （纯指针事件实现 —— HTML5 拖拽与长按手势冲突，两版均失败后弃用）、
  拖拽期间禁选文字/图片、动画（跟随指针 + 目标卡让位）、
  命中检测用几何最近距离（跟随动画后被拖卡会遮挡指针，elementFromPoint 失效）、
  排序偏好持久化（重启恢复）。
- 图标链路：提取归一化（可见内容裁剪 + 放大铺满，修复部分图标偏小）、
  启动工作目录统一取 exe 所在目录（修复巫师 3 等游戏静默退出，跨机器复现）、
  维护功能「重新提取缺失的图标」。
- 其他：应用图标重绘（深色圆角瓦片 + 手柄）、产品名改「游戏库」（NSIS-only）、
  卡面常驻禁选、游戏详情弹窗（改名/文件位置/打开所在位置）、
  GitHub 仓库 + Release 分发。

### 校验基线（v1.0.2）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 139 passed / 0 failed（143 条，4 条 manual-tests 门控） |
| `cargo clippy --all-targets` | 零警告 |
| `tsc` + `vite build` | 退出码 0 |
| `npm run tauri build` | 退出码 0（NSIS 2.71 MiB） |
