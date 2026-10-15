# 架构设计 · Windows 个人游戏库（手动导入版）

> 对应实施说明 MVP 1.0 · 目标平台 Windows 10/11 x64
> 技术栈：Tauri 2 + React + TypeScript + Rust + SQLite
> 状态：设计已定稿，待评审后进入脚手架阶段

---

## 1. 设计目标

| 目标 | 说明 |
|---|---|
| 单一数据源 | 所有持久化状态（游戏、会话、设置）只由 Rust 后端写 SQLite，前端不直连数据库 |
| 计时可信 | 时长必须由后端计算与落库，崩溃/重启不得虚增时长 |
| 崩溃不扩散 | 单条记录损坏、图标提取失败、启动失败都不得让应用整体崩溃 |
| 不碰用户资产 | 只读用户的 EXE / `.lnk`，绝不修改、移动、删除原始文件 |
| 可扩展 | 启动器型游戏的进程识别留有明确扩展点，不为了全覆盖而在第一版过度设计 |

**明确不做**（继承实施说明 §1，其中自动扫描一项已于 2026-10-14 修订，见下）：账号与云同步、未经确认的联网抓封面。

**铁律 #1 修订（2026-10-14，用户决定）**：原「不做 NVIDIA / Steam / Epic 自动扫描」修订为
「**不做未经用户确认的自动导入**」。自动发现（`discovery/`）只扫描本地明文清单（Steam `appmanifest`、
Epic `.item`、NVIDIA App `ApplicationStorage.json`）与用户明确指定的目录，**只读、不联网、不执行候选程序、
不修改启动器数据**，所有导入必须经过预览与用户勾选。调查结论与数据源真实验证记录见
[`docs/auto-discovery-design.md`](auto-discovery-design.md)。

> **阶段二/三 已实现（2026-10-14）**：`discovery/` 模块（steam / epic / nvidia / directory / vdf）
> 与命令 `scan_games` / `import_discovered_games`；前端「扫描游戏」弹窗（来源状态、预览勾选、
> 确认导入）。导入复用现有 `add_game_from_exe_impl`，已在库中的候选默认不勾选且后端兜底跳过。
> 真实环境验证：Steam 候选 6、NVIDIA 候选 20，跨来源合并生效，全部启动目标真实存在。

---

## 2. 关键架构决策（ADR）

### ADR-01：数据库只由 Rust 侧访问（rusqlite），不用 `tauri-plugin-sql`

- **决策**：在 Rust 后端使用 `rusqlite`（bundled SQLite），通过 `Mutex<Connection>` 持有单一连接，配合 `rusqlite_migration` 管理迁移。
- **理由**：
  - 实施说明要求「计时会话必须由 Rust 后端持久化和管理」，且「前端组件不得直接访问 SQLite」。用 `tauri-plugin-sql` 会把 SQL 散落到前端，与本要求冲突。
  - 「结算会话 + 更新累计时长」必须落在同一事务里，放在 Rust 侧最自然。
  - 避免前端与 Rust 两套代码同时维护同一批业务表。
- **代价**：需要自己写一层命令封装（本来也要写）。
- **备选**：`sqlx`（异步、编译期校验）。因引入额外异步复杂度与 SQLite 驱动依赖，MVP 不采用。

### ADR-02：进程启动使用 `CreateProcessW`，不用 `Command::new` 拼接

- **决策**：通过 `windows` crate 直接调用 `CreateProcessW`。
  - `lpApplicationName` = 规范化后的 `launch_path`（程序路径单独传递）
  - `lpCommandLine` = `"<launch_path>"` + 原样的 `launch_args`
  - `lpCurrentDirectory` = `working_directory`
- **理由**：
  - 需要拿到进程句柄与 `dwProcessId`，并读取进程创建时间作为**启动身份**，用于识别 PID 复用；`std::process::Command` 拿不到这些。
  - **不经过 cmd.exe / shell**，因此不存在命令注入面；路径与参数各自独立传递，不做二次转义（避免转义错误反而破坏参数）。
- **代价**：需处理 `STARTUPINFOW`/`PROCESS_INFORMATION` 内存与句柄释放（用 `Drop` 守卫封装）。
- **2026-10-15 补充（工作目录规则）**：启动时工作目录**一律取 exe 所在目录**
  （`effective_working_directory`，与资源管理器双击行为一致），库内存的
  working_directory 仅作展示用途。实测巫师 3：以「游戏根目录」为 cwd 时进程
  静默退出（退出码 0），该规则在其他电脑上同样复现 —— 统一在启动器层面生效，
  所有存量记录无需迁移即被修复。
- **2026-10-14 补充（740 兜底）**：要求管理员权限的 exe（如 TCLS\client.exe）会被
  `CreateProcessW` 以 740（ERROR_ELEVATION_REQUIRED）拒绝 —— 应用本身不提权（铁律 #6 不变），
  此时**自动改走 `ShellExecuteExW(runas)`**：只对这一个程序弹一次 UAC，用户同意后进程以
  管理员身份运行；`SEE_MASK_NOCLOSEPROCESS` 拿到进程句柄，计时追踪照常。用户在 UAC 点「否」
  则报「已取消管理员授权」。普通程序（asInvoker）行为完全不变。

### ADR-03：`launch_args` 以「原样参数串」存储，不做分词

- **决策**：`launch_args` 存**字符串**（默认空串）。EXE 导入时默认为空、由用户手填；`.lnk` 导入时取 `IShellLinkW::GetArguments` 的**原始串**，原样保存。
- **理由**：Windows 命令行参数的引号/反斜杠转义规则非常微妙。把 Shell 记录的原始串**原样回放**，比「解析成 token 再重新拼接」更不容易出错，也不会因为重新转义而改变原本正确的参数。
- **边界**：因为程序路径通过 `lpApplicationName` 独立传参，参数串本身不存在「污染可执行文件身份」的风险。
- **禁止**：任何形式的 `format!("{} {}", exe, args)` 再交给 shell 执行。

### ADR-04：进程识别采用「进程树追踪 + 启动身份校验」

- **决策**：
  - 记录 root 进程 `pid` 与 **创建时间（FILETIME → u64 字符串）** 作为 `process_start_identity`；
  - 运行时周期性用 `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` 枚举进程，按 `th32ParentProcessID` 从 root 出发做 BFS，得到 **root + 全部后代** 的存活集合；
  - **集合为空** 才判定游戏结束；root 存活但 pid 创建时间与记录不符 → 判定为 PID 复用，按结束处理。
- **理由**：实施说明 §8.2 明确「不能把最初启动的进程退出无条件视为游戏结束」，因为快捷方式常先拉起平台客户端再由客户端拉起游戏。进程树追踪能在**不猜测游戏进程名**的前提下覆盖绝大多数「启动器 → 游戏」场景。
- **已知限制**：若启动器把游戏交给另一个进程树（例如通过系统服务、计划任务拉起，已脱离父子链），本策略会漏算。此类情况在 README 中标注，并由 `is_launcher` 标记 + UI 提示「该游戏通过启动器启动，计时可能不准确」。

### ADR-05：计时基准用 `QueryUnbiasedInterruptTime`（单调、不含休眠）

- **决策**：
  - 会话进行中的时长 = `QueryUnbiasedInterruptTime()` 差值（单位 100ns，系统启动后计时，**不包含系统睡眠/休眠时间**）；
  - 落库的 `started_at` / `ended_at` / `last_heartbeat_at` 使用 **UTC 墙钟**，仅用于历史展示；
  - **绝不用墙钟差值计时**，避免用户改系统时间导致时长错误。
- **理由**：一次性满足「单调时钟」与「扣除休眠时间」两条要求，比监听 `WM_POWERBROADCAST` 简单且更可靠。
- **代价**：`QueryUnbiasedInterruptTime` 是开机相对时间，重启后归零——只用于**单次会话内**的差值，不跨会话比较，因此无影响。

### ADR-06：心跳落库 + 保守恢复，替代"崩溃后用墙钟补齐"

- **决策**：会话运行期间每 **10 秒** 把 `tracked_seconds`（可信累计）与 `last_heartbeat_at` 写入数据库。启动时扫描遗留 `running` 会话：
  - 一律结算为 `interrupted`；
  - `duration_seconds` 取 **最后一次心跳的 `tracked_seconds`**，**不**用「当前时间 − `started_at`」；
  - UI 明确提示该次记录为异常中断，时长按保守值计入。
- **理由**：实施说明 §8.3 禁止「把当前时间减去旧开始时间并无条件全部计入」，也禁止自动夸大时长。
- **代价**：崩溃时最多丢失 10 秒。

### ADR-07：COM 调用隔离到独立 STA 线程

- **决策**：`.lnk` 解析（`IShellLinkW`）与图标提取（`IShellItemImageFactory`）统一放在 `std::thread::spawn` 出来的**专用线程**中，线程内 `CoInitializeEx(COINIT_APARTMENTTHREADED)` → 执行 → `CoUninitialize`，结果通过 `oneshot` channel 回传。
- **理由**：Tauri 使用多线程 tokio runtime，在 worker 线程上随意初始化 COM apartment 会带来难以排查的线程模型问题（STA 要求消息泵、跨 apartment 封送）。隔离线程是最省事且正确的做法。

### ADR-08：封面/图标落盘用应用自有目录 + 相对路径存库

- **决策**：自定义封面复制到 `<app_data_dir>/covers/<uuid>.<ext>`；提取的图标存 `<app_data_dir>/icons/<game_id>.png`。DB 只存**相对路径**，统一用正斜杠。
  - 封面用 UUID 命名，与游戏 id 解耦 —— 这样「先落文件、后写记录」也不会出现名字对不上；
  - 图标用 `game_id` 命名：它与游戏一一对应，回收时一眼能看出归属。
- **理由**：避免存原图临时路径导致文件被移动后封面失效；相对路径保证换机/改数据目录时仍可用。所有文件均由应用生成，移除游戏时可安全回收（先算引用集合，再删未引用的）。
- **展示**：通过 Tauri asset protocol 暴露，scope 只放行这两个子目录；前端把相对路径拼成绝对路径后交给 `convertFileSrc()`，**文件缺失由 `<img onError>` 回落占位图**。
- **占位图**：内联 SVG，随前端一起打包 —— 不依赖额外的二进制资源文件，永远可用。

### ADR-09：数据目录与项目路径

- **决策**：数据分两处存放，遵循 Windows 惯例：
  - **数据根目录** `%APPDATA%\com.wuayn.gamelibrary\` —— 放 `game_library.db`、`covers/`、`icons/`、`placeholder.png`（应用自管资源，可漫游）。
  - **日志目录** `%LOCALAPPDATA%\com.wuayn.gamelibrary\logs\` —— 放 `app.log`（日志不漫游）。
  - 两者**不是父子关系**。任何地方都不得用数据根目录去拼日志路径；查询一律走 Tauri 的 `app_data_dir()` / `app_log_dir()`，不要自行复刻其解析规则。
- **工程根目录**：直接使用工作区根 `E:/workbuddy数据/游戏库/`（即 `src/`、`src-tauri/` 位于根下）。
- **P0 实测备注（tauri-plugin-log）**：
  - `Builder::new()` 自带默认 targets（Stdout + 以包名为名的 LogDir）。直接 `.target()` 是**追加**而非替换，会同时产出 `app.log` 与 `<包名>.log` 两个内容相同的文件 —— 必须先 `clear_targets()`。
  - 默认 `max_file_size` 仅 **40 KB**，做诊断太小、会频繁轮转，需显式调大（本项目取 5 MB，`RotationStrategy::KeepSome(3)`）。
- **风险**：工程路径含中文字符。Cargo / Vite / Tauri 均支持，但少数第三方构建工具在非 ASCII 路径下可能异常；若构建失败且定位为路径问题，则改用 ASCII 路径并在此文档更新。

### ADR-10：编译并发度上限（开发机内存约束）

- **决策**：在 `CARGO_HOME/config.toml` 中固定 `[build] jobs = 6`，不让 cargo 按 CPU 核心数自由并发。
- **理由**：开发机有 **24 个逻辑核心，但物理内存仅 15.8 GB**（编译期实际可用约 5 GB）。cargo 默认按核心数并发调起 rustc，24 个进程各占数百 MB 会直接耗尽内存，rustc 以 `STATUS_ACCESS_VIOLATION (0xc0000005)` 崩溃。**关键在于这种崩溃在输出里表现为「编译失败」，极易被误判为代码问题而浪费时间。**
- **代价**：全量编译变慢；换来的是可稳定完成的构建。
- **排查提示**：日后若再出现 rustc 崩溃（而非编译错误），第一反应应是降低 `jobs`，而不是查代码。

---

## 3. 分层架构

```
┌──────────────────────────────────────────────────────────────┐
│  前端（WebView2 / React + TS）                                │
│   pages: LibraryPage / SettingsPage                           │
│   components: GameGrid / GameCard / AddGameDialog /           │
│               EditGameDialog / EmptyState                     │
│   services/gameApi.ts  ← 唯一的 invoke 出口，全类型化          │
│   hooks: useGames / useLaunch / useSessionEvents              │
└───────────────────────────┬──────────────────────────────────┘
                            │  Tauri IPC（commands 调用 + events 推送）
┌───────────────────────────▼──────────────────────────────────┐
│  commands 层（src-tauri/src/commands/）                        │
│   games.rs / covers.rs / sessions.rs / system.rs              │
│   职责：参数校验、错误映射、DTO 转换；不含业务规则             │
└───────────────────────────┬──────────────────────────────────┘
┌───────────────────────────▼──────────────────────────────────┐
│  业务模块层                                                    │
│   importer/（exe.rs, shortcut.rs, dedupe.rs）                  │
│   covers/（icon.rs, cover_store.rs）                           │
│   launcher/（launch.rs, quote.rs）                             │
│   tracker/（session.rs, process_tree.rs, recover.rs）          │
└───────────────────────────┬──────────────────────────────────┘
┌───────────────────────────▼──────────────────────────────────┐
│  基础设施层                                                    │
│   database/（mod.rs, migrations/, models.rs）                  │
│   platform/（win.rs: 进程句柄、快照、单调时钟、COM 线程助手）  │
│   error.rs（AppError）/ logging.rs                            │
└───────────────────────────┬──────────────────────────────────┘
                            │  rusqlite（同步，Mutex<Connection>）
                        SQLite 文件
```

**依赖方向单向向下**：`commands → 业务模块 → 基础设施`。业务模块之间通过明确的函数签名交互，禁止循环依赖；`platform/` 是唯一直接使用 `windows` crate 的目录。

---

## 4. 后端模块设计

### 4.1 `database/`

| 项 | 内容 |
|---|---|
| 连接 | `Db(Arc<Mutex<Connection>>)`，在 `tauri::Builder::setup` 中打开并跑迁移，作为 managed state 注入 |
| 连接参数 | `journal_mode=WAL`、`foreign_keys=ON`、`busy_timeout=5000ms` |
| 迁移 | `rusqlite_migration` + `migrations/0001_init.sql`，版本记录在 `schema_migrations` |
| 事务 | 结算相关逻辑统一走 `db.with_tx(...)` 辅助函数，保证原子性 |

### 4.2 `importer/`

- `exe.rs`
  - `inspect_exe(path) -> ExeProbe`：校验扩展名 `.exe`（大小写不敏感）→ `fs::metadata`（把「不存在 / 无权限 / 其他」翻译成不同的错误分类）→ 拒绝目录（目录名也可能以 `.exe` 结尾）→ `canonicalize` 规范化 → 派生默认名（去 `.exe`）与默认工作目录（父目录）→ 返回文件体积。
  - `canonicalize` 失败时**不静默降级**为原路径 —— 否则去重键会随调用时机而变，去重就不可靠了。
  - 不做 PE 头解析（MVP 不需要），只做存在性与可访问性校验。
  - 探测本身**只读**：不改动任何文件，也不落库。
- `shortcut.rs`
  - `resolve_shortcut(path) -> ShortcutProbe`：在隔离 STA 线程内用 `IShellLinkW` + `IPersistFile::Load` 读取：
    - `GetPath` 用 `SLGP_RAWPATH` 拿「存进去的原样路径」，**不触发 Shell 的链接解析**（解析可能去访问网络位置或光驱，本地小工具不值得），随后用 `ExpandEnvironmentStringsW` 手动展开 `%VAR%`；
    - 注意 `IShellLinkW::GetPath` 实际是 **3 个参数**：中间的 `WIN32_FIND_DATAW` 输出参数在只带 `SLGP_RAWPATH` 时传空指针；
    - `GetArguments`（原样串，见 ADR-03）；`GetWorkingDirectory`（为空则回退到目标所在目录）；
    - 目标若是相对路径，按 `.lnk` 所在目录补成绝对路径；目标存在时再 `canonicalize` 一次 —— 既得到长路径，也让「导入 EXE」与「导入指向同一目标的 `.lnk`」产出**同一个去重键**；
    - 显示名回退顺序：`.lnk` 文件名 → 目标文件名 → 未命名游戏。
  - **目标不存在时探测不报错**：返回 `targetExists = false` 并保留原目标路径供用户判断，由前端提供「取消 / 手动选择启动目标」。真正落库时若既无手动修正、目标又不存在，则返回 `TargetMissing` —— **绝不把失效目标以「可启动」的状态入库**。
  - 手动修正的目标同样要过一遍 `exe::inspect_exe`，不因为「手动」就放松校验；工作目录跟随修正后的目标。
  - 启动器判定看**目标文件名**（不看 `.lnk` 名字），命中 Steam / Epic / Ubisoft / EA / Battle.net / GOG 等则置 `is_launcher = true`，UI 据此提示「计时可能不准确」。
- `dedupe.rs`
  - **规范化规则不在这里定义**：去重键由 `database::games::normalize_launch_key` 给出（它是唯一索引 `idx_games_dedupe` 的依据），必须与落库时逐字一致，所以只保留一处实现。
    规则：`canonicalize`（失败则退化为朴素路径规范化）→ 统一为 `\` → 去尾随分隔符 → 转小写 → 有参数时用 `\x1f` 拼接小写参数串。
    **不做哈希** —— 直接存规范化字符串，排障时能肉眼比对。
  - 本模块提供 `find_existing(conn, launch_path, args) -> Option<Game>` 与给前端展示的 `DuplicateRef`。
  - 去重键存 `games.launch_path_key` 并建**唯一索引**，作为并发下的最终兜底。

### 4.3 `covers/`

- `cover_store.rs`
  - `import(source_path, data_dir) -> String`：校验扩展名 ∈ {png, jpg/jpeg, webp} → 体积 ≤ 20 MB → 用 `image` crate **只读头部**拿宽高，拒绝 > 8192×8192 → 复制到 `covers/<uuid>.<ext>`（jpeg 统一存成 `.jpg`），返回相对路径。
    - **只复制，绝不移动或删除用户的原图。**
    - 校验失败**直接报错**：封面是用户明确选的，不该被悄悄丢掉。
    - 落封面发生在写库**之前**，所以校验失败不会留下半成品记录。
  - `gc(referenced, data_dir) -> usize`：回收没有任何记录引用的文件。**只删本应用生成的** —— `covers/` 下必须是 `<UUID>.<允许扩展名>`，`icons/` 下必须是 `*.png`；用户自己丢进来的文件一律不动。
  - 注意：`gc` 不接收 `Db`，引用集合由调用方传入，避免业务模块反向依赖数据层。
- `icon_store.rs`
  - `extract_and_store(launch_path, data_dir, game_id) -> Option<String>`：调用 `platform::icon` 取 PNG，写入 `icons/<game_id>.png`，返回相对路径。
  - **任何失败都返回 `Ok(None)`**（提取失败 / 体积异常 / 写盘失败），由调用方降级到占位图，**绝不阻断导入**。

图标提取的 Windows 调用本身在 `platform/icon.rs`（基础设施层，见 §4.7）；本模块只决定「什么时候提取、存到哪、失败了怎么办」。

**三级封面策略**：自定义封面 → 提取的图标 → 占位图。

前两级由后端给出**相对路径**，前端拼成 asset URL 后交给 `<img>` 加载；**文件缺失由 `onError` 兜底回落到占位图**，因此不需要额外的「文件是否存在」查询命令。占位图是**内联 SVG**，不依赖额外的二进制资源文件。

### 4.4 `launcher/`

- `launch.rs`（已实现）
  - `LaunchRequest { program_path, raw_args, working_directory }` → `launch() -> LaunchedProcess`：
    1. 校验 `launch_path` 非空、存在且是文件（错误按「不存在 / 无权限 / 其它」分类）；
    2. 用 `quote.rs` 拼 `lpCommandLine`；
    3. `CreateProcessW`：**`lpApplicationName` = 程序路径（泛型 `Param<PCWSTR>`，直接传 `PCWSTR`，不包 `Option`）**、`lpCommandLine` = 引号路径 + 原样参数（必须是**可写**宽字符缓冲，Windows 会就地改它）、`lpCurrentDirectory` = 工作目录；`dwCreationFlags` 不加 `CREATE_NEW_CONSOLE`，不请求提权；
    4. 失败时映射为 `LaunchFailed` 结构化错误；
    5. `GetProcessTimes` → 进程创建时间（FILETIME ticks）作为 `process_start_identity`；
    6. 返回 `{ pid, creation_time, OwnedHandle }`。线程句柄用不到，立即关闭。
  - **`AlreadyRunning` 不在这里判**：那是 `SessionManager` 的职责（还要查数据库），`launch()` 只管把进程拉起来。
  - `process_creation_time(handle)` 同时暴露为公共函数，进程树校验也用它。
- `quote.rs`（已实现）
  - `quote_argument()`：完整 MSVCRT 规则 —— 不含空格 / 制表符 / 引号则原样返回；否则 `"` 包住，「n 个反斜杠 + 引号」写成 `2n+1` 个反斜杠，**结尾的连续反斜杠加倍**（否则会转义收尾引号）。
  - `quote_program_path()`：给程序路径加引号（`lpCommandLine` 的第一段）。
  - `build_command_line(program_path, raw_args)`：引号路径 + **原样参数**；空参数不留多余空格。**参数不做任何加工**（ADR-03）。

### 4.5 `tracker/`

- `session.rs`（已实现）
  - `SessionManager`（Tauri managed state，可克隆）：
    - `active: Arc<Mutex<HashMap<GameId, ActiveSession>>>` —— 内存级防抖；`start` **全程持锁**（检查 → 启动 → 写库 → 登记），消除「检查—写入」竞态；
    - `start(game)`：先查内存集合，再查数据库残留的 `running` 会话（都可能返回 `AlreadyRunning`）→ `launch()` → 写会话 → 登记 → `emit("session://started")` → 派发追踪任务。**写库失败会显式记日志**——进程已经起来了，绝不能变成「游戏在跑、应用却不知情」的静默状态；
    - `stop_tracking(game_id)`：置 `stop_requested` 标志通知追踪任务退出，然后结算为 `interrupted`、从活动集合移除、`emit("session://ended")`。**绝不调用 `TerminateProcess`**。幂等：第二次返回 `None` 不报错；
    - 无 `AppHandle` 时也能工作（不发事件）—— 这是单元测试的构造方式。
  - **结算幂等**：`settle` 的 SQL 带 `WHERE id = ? AND status = 'running'`，只有第一个到达者能改写；`stop_tracking` 与追踪循环天然互斥，不会重复结算。
  - **P5 的 `duration_seconds` 恒为 0** —— 时长计算属 P6（`QueryUnbiasedInterruptTime`），这一步只落「会话存在与状态」。
- `process_tree.rs`（已实现）
  - `snapshot_processes()`：`CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` + `Process32FirstW/NextW`；
  - `collect_tree(root_pid, expected_creation_time) -> TreeStatus { Alive{members} | Gone | IdentityReused }`：BFS 收集 root 及全部后代；root 仍在时用 `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `GetProcessTimes` 校验创建时间，识别 PID 复用。
  - **关键点**：BFS 从 root 出发**即使 root 已经退出也能走通** —— Windows 不更新子进程的 PPID，它仍指向已退出的父 pid。这正是「启动器退出、游戏还在跑」能被正确识别的原因。BFS 时只把「快照里仍存在」的 pid 计入成员。
- 追踪循环（已实现）：每个会话一个 `tauri::async_runtime::spawn` 任务，`tokio::time::interval(2s)`，第一次 tick 立即返回需跳过：
  1. `stop_requested` 置位 → 直接退出（`stop_tracking` 已结算）；
  2. `spawn_blocking(collect_tree)`（枚举进程是同步系统调用，别占着 runtime）；
  3. `Alive` → 继续；`Gone` / `IdentityReused` → `settle(completed)` 并 `emit("session://ended")`；
  4. 查询失败按**暂态**处理、下一轮重试 —— 不能因一次失败误判游戏结束。
- `recover.rs`（**P6 实现**）：启动时扫描 `status='running'` 的残留会话，按 ADR-06 保守结算。当前只在应用启动时打一条 warn 日志。

### 4.6 `commands/`

见 §7 接口契约。命令层只做：反序列化参数 → 调业务模块 → 把 `AppError` 映射为 `{ code, message }` → 返回 DTO（`serde` 驼峰命名，与 TS 类型一一对应）。

### 4.7 `platform/`

集中封装所有 Windows 专有调用，便于日后替换或测试。**这是唯一直接使用 `windows` crate 的目录。**

`win.rs`：

- `run_sta(f)`：把需要 COM 的工作丢到一个**专用 STA 线程**执行并取回结果。Tauri 使用多线程 tokio runtime，在 worker 线程上初始化 apartment 会带来 STA 消息泵与跨 apartment 封送的隐患，隔离到自己的线程最省事。`CoInitializeEx` / `CoUninitialize` 用 RAII 守卫成对出现，闭包提前返回也不会漏；COM 线程 panic 会被转成可展示的错误，不让整个应用崩掉。
- `to_wide` / `from_wide`：Rust 字符串与 NUL 结尾宽字符缓冲互转（已覆盖非 ASCII 路径用例）。
- `strip_verbatim_prefix(path)`：剥掉 `\\?\` / `\\?\UNC\` 前缀。
  **这条很关键**：`std::fs::canonicalize` 在 Windows 上返回的是 verbatim 路径，而 **Shell 系列 API（`SHCreateItemFromParsingName`、`SHGetFileInfoW`）不接受这个前缀** —— 带着它调用会直接失败，表现为「所有导入的游戏都提取不到图标」。因此所有**产出路径**的地方（`inspect_exe`、`resolve_shortcut`）都统一剥掉它，存库与展示也不再带这个前缀。

`icon.rs`（图标提取）：

- 首选 `IShellItemImageFactory::GetImage`（256×256，质量最好）→ HBITMAP；
- 回退 `SHGetFileInfoW` + `SHGFI_ICON | SHGFI_LARGEICON` → HICON → `DrawIconEx` 画到 32 位 DIB；
- 两条路径都归一到「BGRA → RGBA」，再用 `image` crate 编码 PNG —— **不依赖 WIC 编码器**，省掉一整层 COM 流与帧的样板代码。
- 读取 HBITMAP 像素优先**直读 DIB section 的 `bmBits`**：`GetImage` 返回的正是 DIB section，而 `GetDIBits` 对它反而会失败；不是 DIB section 时才回退到 `GetDIBits`。
- 提取全程**串行化**（进程内互斥锁）：Shell 的图像缓存是进程级共享的，多线程同时提取同一文件会互相干扰，表现为 `GetImage` 偶发返回空位图。
- 任何失败都返回 `Ok(None)`，由调用方降级到占位图。

`QueryUnbiasedInterruptTime`（P6 已实现，`unbiased_interrupt_time()`）与 `elapsed_seconds()`：**无偏中断时间不含系统休眠**，电脑睡 8 小时不会被记成玩了 8 小时。`CreateProcessW`、`GetProcessTimes`、`CreateToolhelp32Snapshot`、`OpenProcess` 已在 P5 于 `launcher/` 与 `tracker/` 中使用。

---

## 5. 数据模型

```sql
-- migrations/0001_init.sql

CREATE TABLE games (
  id                TEXT    PRIMARY KEY,              -- UUID v4
  name              TEXT    NOT NULL,                 -- 用户可编辑显示名
  original_name     TEXT    NOT NULL,                 -- 导入时解析的原始名
  source_type       TEXT    NOT NULL CHECK (source_type IN ('exe','shortcut')),
  source_path       TEXT    NOT NULL,                 -- 用户选择的原始 EXE / .lnk 路径
  launch_path       TEXT    NOT NULL,                 -- 实际启动目标
  launch_args       TEXT    NOT NULL DEFAULT '',      -- 原样参数串（ADR-03）
  working_directory TEXT,                             -- 可为空
  cover_path        TEXT,                             -- 相对 app_data_dir，可为空
  icon_path         TEXT,                             -- 相对 app_data_dir，可为空
  launch_path_key   TEXT    NOT NULL,                 -- 规范化去重键
  is_launcher       INTEGER NOT NULL DEFAULT 0,       -- 目标疑似平台启动器
  total_play_seconds INTEGER NOT NULL DEFAULT 0 CHECK (total_play_seconds >= 0),
  last_played_at    TEXT,                             -- UTC ISO8601
  created_at        TEXT    NOT NULL,
  updated_at        TEXT    NOT NULL
);

CREATE UNIQUE INDEX idx_games_dedupe ON games(launch_path_key);
CREATE INDEX        idx_games_name   ON games(name);

CREATE TABLE play_sessions (
  id                    TEXT    PRIMARY KEY,
  game_id               TEXT    NOT NULL REFERENCES games(id) ON DELETE CASCADE,
  started_at            TEXT    NOT NULL,             -- UTC ISO8601（墙钟，仅展示）
  ended_at              TEXT,                         -- UTC ISO8601，可为空
  duration_seconds      INTEGER NOT NULL DEFAULT 0 CHECK (duration_seconds >= 0),
  tracked_seconds       INTEGER NOT NULL DEFAULT 0 CHECK (tracked_seconds >= 0),
  last_heartbeat_at     TEXT,                         -- UTC ISO8601
  status                TEXT    NOT NULL CHECK (status IN ('running','completed','interrupted')),
  process_id            INTEGER NOT NULL,
  process_start_identity TEXT   NOT NULL,             -- FILETIME 创建时间（100ns ticks 字符串）
  created_at            TEXT    NOT NULL
);

CREATE INDEX idx_sessions_game   ON play_sessions(game_id, started_at DESC);
CREATE INDEX idx_sessions_status ON play_sessions(status);
-- 同一游戏同时只允许一个活动会话（数据库级防重复计时）
CREATE UNIQUE INDEX idx_sessions_single_running
  ON play_sessions(game_id) WHERE status = 'running';

CREATE TABLE settings (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
-- 迁移版本不额外建表：由 rusqlite_migration 通过 PRAGMA user_version 自行维护。
-- P1 实测日志：`[rusqlite_migration][INFO] Database migrated to version 1`
```

**P1 实测补充**（在真实应用库里核验过，与上面的 DDL 一致）：

- `PRAGMA journal_mode` 返回 `wal`，数据目录下会同时出现 `game_library.db-wal` 与 `-shm` —— 这是 WAL 生效的证据。
- `PRAGMA foreign_keys` 返回 1（`configure` 里显式开启，否则级联删除不生效）。
- `idx_sessions_single_running` 的 `WHERE status = 'running'` 被原样保留，确认部分唯一索引可用。
- `play_sessions.game_id → games.id` 的 `on_delete = CASCADE` 确认生效。

**一致性规则**（对应实施说明 §5）：

1. 时长一律整数秒；前端只做 `12 小时 34 分钟` 形式的格式化。
2. 结算与累计时长在**同一事务**中完成；结算带 `status='running'` 条件，天然幂等。
3. 索引覆盖 `game_id`、会话状态、去重键。
4. 启动时扫描并保守恢复残留 `running` 会话。
5. 表结构只通过迁移变更，不手工建库。

---

## 6. 关键流程

### 6.1 导入 EXE

```
用户点「选择 EXE」
 → tauri-plugin-dialog 原生文件选择器（过滤 *.exe）
 → commands::add_game_from_exe(path)
 → importer::exe::inspect_exe  （存在性 / 类型 / 规范化 / 默认名 / 默认工作目录）
 → dedupe::find_existing       （命中 → 探测结果带 duplicate 引用，前端在用户确认前就提示「已在库中」）
 → covers::icon::extract_icon  （失败静默为 None）
 → 写入 games → 返回 GameDto → 前端插入卡片
```

### 6.2 导入 `.lnk`

```
用户点「导入快捷方式」
 → 原生选择器（过滤 *.lnk）
 → commands::probe_shortcut(path)          ← 只读探测，不落库
 → run_sta(IShellLinkW + IPersistFile::Load
           → GetPath(SLGP_RAWPATH) / GetArguments / GetWorkingDirectory)
 → 展开 %VAR%；相对路径按 .lnk 所在目录补齐；目标存在则 canonicalize
 → 目标缺失 → 结果带 targetExists=false，前端提示「取消 / 手动选择启动目标」
 → 目标存在 → dedupe::find_existing 查重（命中则带 duplicate 引用）
 → 用户确认 → commands::add_game_from_shortcut(path, name, launchPathOverride?)
 → 仍缺目标且无 override → 返回 TargetMissing，不落库
 → 写入 games（source_type = shortcut，参数原样保留，is_launcher 按目标文件名判定）
```

### 6.3 启动与计时

```
用户点「启动」
 → 前端按钮立即置灰（防抖）
 → commands::launch_game(game_id)
 → 校验 + 检查活动会话（内存 Set + DB 唯一索引）
 → CreateProcessW  → 取 pid + 创建时间 + 句柄
 → 事务：INSERT play_sessions(status='running')
 → emit("session://started", {gameId, sessionId, startedAt})
 → spawn tracker task
     每 2s : collect_tree → 空则结算；否则累计并 emit("session://tick")
     每 10s: 写 tracked_seconds / last_heartbeat_at
 → 结束时事务：UPDATE session（带 status='running' 条件）+ UPDATE games 累加
 → emit("session://ended", {gameId, durationSeconds, totalPlaySeconds})
 → 前端刷新卡片时长，恢复按钮
```

### 6.4 异常退出恢复

```
应用启动 setup()
 → recover_on_startup(db)
     对每条 status='running' 会话：
       duration_seconds := 最后一次心跳的 tracked_seconds（保守值，不用墙钟补齐）
       status := 'interrupted'，ended_at := last_heartbeat_at（或 started_at 兜底）
       事务内同时累加 games.total_play_seconds
 → 若有恢复项 → emit("sessions://recovered", [...]) → 前端顶部提示条
```

---

## 7. 接口契约

### 7.1 Tauri Commands

| 命令 | 入参 | 返回 | 说明 |
|---|---|---|---|
| `list_games` | `{ sort: 'name'\|'playTime'\|'createdAt', order: 'asc'\|'desc' }` | `GameDto[]` | 库列表 |
| `get_game` | `{ id }` | `GameDto` | 单条 |
| `search_games` | `{ keyword }` | `GameDto[]` | 名称 + 原始名模糊匹配（`LIKE` 转义 `% _`） |
| `probe_exe` | `{ path }` | `ExeProbeDto` | 只读探测；结果含 `duplicate`（已在库中的记录引用） |
| `probe_shortcut` | `{ path }` | `ShortcutProbeDto` | 解析目标/参数/工作目录；含 `isLauncher`、`targetExists`、`duplicate` |
| `add_game_from_exe` | `{ path, name? }` | `GameDto` | 落库前**重新探测**；重复返回 `Duplicate` |
| `add_game_from_shortcut` | `{ path, name?, launchPathOverride? }` | `GameDto` | `launchPathOverride` 用于目标缺失时的手动修正；仍缺则返回 `TargetMissing` |
| `update_game` | `{ id, name?, launchPath?, launchArgs?, workingDirectory?, coverSourcePath?, clearCover? }` | `GameDto` | 改启动配置前先校验 |
| `remove_game` | `{ id }` | `{ removedSessions: number }` | **不删原始文件**，附带封面 GC |
| `import_cover` | `{ gameId, sourcePath }` | `{ coverPath }` | 校验并复制进应用目录；只复制原图 |
| `extract_game_icon` | `{ gameId }` | `{ iconPath \| null }` | 重新提取；提不到返回 null 并清空旧路径 |
| （无 `get_cover_url`） | — | — | 三级策略的**落地**在前端：后端只给相对路径，前端拼绝对路径后走 `convertFileSrc`，加载失败由 `onError` 回落占位图 |
| `launch_game` | `{ gameId }` | `{ sessionId, pid, startedAt }` | 失败返回结构化错误 |
| `get_active_sessions` | — | `ActiveSessionDto[]` | 运行中会话 |
| `get_play_sessions` | `{ gameId, limit? }` | `PlaySessionDto[]` | 历史会话 |
| `stop_tracking` | `{ gameId }` | `{ durationSeconds }` | 只停表，**不杀进程** |
| `recover_interrupted_sessions` | — | `RecoveredDto[]` | 手动触发恢复扫描 |
| `get_app_info` | — | `{ version, tauriVersion, buildProfile }` | 设置页 |
| `get_app_data_path` | — | `{ dataDir, dbPath, logDir }` | 设置页，附「打开目录」入口 |

### 7.2 事件（后端 → 前端）

| 事件 | 载荷 | 状态 |
|---|---|---|
| `session://started` | `{ gameId, sessionId, startedAt, processId }` | P5 已实现 |
| `session://ended` | `{ gameId, sessionId, status, startedAt, endedAt }` | P5 已实现 |
| `session://tick` | `{ gameId, sessionId, elapsedSeconds }` | P6 已实现（约每 2 秒） |
| `sessions://recovered` | `{ items: RecoveredSession[] }` | P6 已实现（仅当确有恢复时推送） |

### 7.3 错误契约

```ts
type AppErrorCode =
  | 'NotFound' | 'PermissionDenied' | 'InvalidArgument' | 'TargetMissing'
  | 'Duplicate' | 'DatabaseError' | 'Internal';

interface AppError { code: AppErrorCode; message: string; }
```

- Rust 侧 `AppError` 实现 `serde::Serialize`，只输出 `{ code, message }`；`detail` 留作日志，**不给用户看堆栈**。
- 前端按 `code` 决定提示文案与可执行动作（例如 `TargetMissing` → 提示「取消 / 手动选择启动目标」）。
- 已实现的 code：`NotFound`、`PermissionDenied`、`InvalidArgument`、`TargetMissing`、`Duplicate`、`DatabaseError`、`Internal`。
  后续阶段会补 `AlreadyRunning`、`LaunchFailed`、`CoverInvalid`、`ShortcutParseFailed`。
  **未实现的 code 不预先加进枚举**，避免出现「看起来支持其实没有」的假象。

---

## 8. 前端架构

```
src/
├── components/
│   ├── GameCard.tsx       封面 + 名称（截断/换行）+ 累计时长 + 启动按钮 + ⋯菜单
│   ├── GameGrid.tsx       响应式网格（auto-fill, minmax(180px, 1fr)）
│   ├── AddGameDialog.tsx  「选择 EXE」/「导入快捷方式」双入口 + 解析预览 + 名称/封面编辑
│   ├── EditGameDialog.tsx 名称 / 封面 / 启动配置；保存失败保留输入并显示错误
│   ├── EmptyState.tsx     空库 / 搜索无结果两种文案
│   ├── Cover.tsx          统一封面组件：加载失败 onError → 占位图
│   ├── RunningBadge.tsx   克制的运行状态指示（小圆点 + 计时）
│   └── Toast.tsx          错误/提示
├── pages/
│   ├── LibraryPage.tsx    顶部栏（应用名 / 搜索 / 添加 / 设置）+ 网格 + 运行提示
│   └── SettingsPage.tsx   版本 / 数据目录 / 封面策略 / 日志入口
├── hooks/
│   ├── useGames.ts        列表 + 搜索 + 排序（本地排序，量级小）
│   ├── useLaunch.ts       启动 + 防抖 + 错误提示
│   └── useSessionEvents.ts 订阅 4 个事件，驱动运行状态与时长刷新
├── services/gameApi.ts    唯一 invoke 出口，全部函数带 TS 类型
├── types/game.ts          DTO 与 AppError 类型（与 Rust 侧对齐）
└── styles/                CSS Modules：深色主题 token 集中于 theme.css
```

- **状态管理**：不引入 Redux。列表用 `useState` + `useGames`，运行状态用事件驱动的小型 `sessionStore`（`useSyncExternalStore`）。量级（几十~几百条）下足够。
- **时长展示**：前端只做格式化；运行中的秒数来自 `session://tick`，不自行计时作为权威来源（自行 tick 仅用于平滑插值，可省略）。
- **深色主题**：`theme.css` 定义 `--bg / --surface / --border / --text / --accent` 等变量；整体简洁，无复杂仪表盘与多余动画。

---

## 9. 并发与线程模型

| 关注点 | 方案 |
|---|---|
| 数据库 | `Arc<Mutex<Connection>>`；同步短事务，配 `busy_timeout`；不做长事务 |
| 启动 | `commands` 内直接调用 `CreateProcessW`（快，不需要 `spawn_blocking`） |
| 追踪 | 每会话一个 `tokio::task`，`tokio::time::interval(2s)`；快照枚举在 `spawn_blocking` 中执行，避免阻塞 runtime |
| COM | 隔离 STA 线程（ADR-07），`oneshot` 回传 |
| 缩放/清理 | 会话结束即 `abort`/自然退出 tracker task，并释放进程句柄；`active` map 同步移除 |
| 防重复 | 三层：前端按钮置灰 → 内存 `active` 集合 → DB 唯一部分索引 |

---

## 10. 错误处理、日志与可观测性

- 全部 `Result<T, AppError>`；`AppError` 携带 `code / message / detail`。
- 日志：`tauri-plugin-log` 写 `<data>/logs/app.log`（滚动 5 MB × 3），记录错误细节与启动/结算事件；`detail` 不返回前端。
- 关键埋点（写日志，不引外部遥测）：导入成功/失败原因、图标提取是否失败、启动 `GetLastError`、会话结算来源（正常 / 恢复 / 手动停止）。
- **崩溃隔离**：导入与图标提取失败不影响应用启动；单条记录字段异常（例如封面路径指向已删文件）由 `resolve_cover` 降级处理。

---

## 11. 安全与隐私

| 项 | 措施 |
|---|---|
| 不联网 | 无任何网络请求；不做封面在线抓取 |
| 不提权 | 不请求管理员权限；`CreateProcessW` 不做提权 |
| 不碰用户文件 | 只读原始 EXE / `.lnk`；移除游戏不删原始文件；封面仅复制 |
| 命令执行 | 不走 shell，路径与参数分离传递（ADR-02/03） |
| 参数输入 | 拒绝含 NUL 的参数串；不做转义改写 |
| 资源暴露 | asset protocol 仅放行 `<data>/covers`、`<data>/icons` |
| 图片处理 | 限制扩展名、文件大小（≤20 MB）与像素尺寸（≤8192²） |

---

## 12. 已知限制（需写入 README）

1. **启动器型游戏**：进程树追踪覆盖不到「已脱离父子链」的拉起方式（服务/计划任务），此类游戏时长可能偏短；UI 对 `is_launcher` 游戏显示提示。
2. **休眠**：使用 `QueryUnbiasedInterruptTime`，睡眠时间不计入；若目标机上该 API 行为异常，退化表现为时长偏短（偏保守，不会虚增）。
3. **崩溃丢失**：最多丢失 10 秒（心跳间隔）。
4. **异常退出恢复**：一律标记 `interrupted`，不继续跟踪，不自动补齐时长。
5. **PID 复用**：通过创建时间校验识别；极端情况下（创建时间精度不足）可能误判。
6. **移除游戏**：`play_sessions` 采用 `ON DELETE CASCADE`，MVP 不提供「保留统计历史」选项。

---

## 13. 依赖清单（预计）

**Rust（`src-tauri/Cargo.toml`）**

| crate | 用途 |
|---|---|
| `tauri` = 2 | 桌面框架 |
| `tauri-plugin-dialog` | 原生文件/文件夹选择器 |
| `tauri-plugin-log` + `log` | 日志 |
| `rusqlite`（features: `bundled`） | SQLite |
| `rusqlite_migration` | 迁移 |
| `windows`（features: Threading, ProcessStatus, Diagnostics_ToolHelp, UI_Shell, Storage_FileSystem, Graphics_Gdi, Graphics_Imaging, UI_WindowsAndMessaging, System_Com） | Windows API |
| `serde` / `serde_json` | 序列化 |
| `uuid`（v4） | 主键 |
| `thiserror` | 错误类型 |
| `tokio`（rt-multi-thread, time） | 异步任务 |
| `image` | 封面尺寸校验 |
| `once_cell` | 全局静态 |

**Node（`package.json`）**：`@tauri-apps/api`、`@tauri-apps/cli`、`@tauri-apps/plugin-dialog`、`react`、`react-dom`、`typescript`、`vite`、`@vitejs/plugin-react`。
