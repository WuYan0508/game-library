# 自动发现游戏：调查结论与设计

> 对应增量需求：`windows_game_library_auto_discovery_implementation.md`
> 本文档是**阶段一（审查与验证）**的产出。所有数据源结论都来自 2026-10-14 在本机（Windows 11 26300）上的真实检查，未做任何假设或伪造。

## 1. 结论速览

| 来源 | 可行性 | 数据位置 | 关键字段 |
|---|---|---|---|
| **Steam** | ✅ 可行 | `<SteamRoot>\steamapps\libraryfolders.vdf` + `appmanifest_<appid>.acf` | `name`、`installdir`、`StateFlags` |
| **Epic** | ⚠️ 已安装但本机无游戏 | `%PROGRAMDATA%\Epic\EpicGamesLauncher\Data\Manifests\*.item` | （本机清单为空，字段设计依据公开格式） |
| **NVIDIA App** | ✅ **可行且质量超出预期** | `%LOCALAPPDATA%\NVIDIA Corporation\NVIDIA app\NvBackend\ApplicationStorage.json` | `DisplayName`、`DetectedFiles`、`InstallDirectory`、`IsCreativeApplication` |
| 指定目录扫描 | ✅ 可行（启发式） | 用户选择的目录 | — |
| 手动导入 | 已存在 | — | — |

**本机环境**：Steam 装在 `D:\Steam`（非默认盘，1 个库目录、7 个 appmanifest）；Epic 启动器已安装但清单目录为空；NVIDIA App 已安装。

## 2. 逐来源调查记录

### 2.1 Steam（已验证）

实际读取了 `D:\Steam\steamapps\libraryfolders.vdf`（404 字节）与全部 7 个 `appmanifest_*.acf`。

**`libraryfolders.vdf` 结构**（Valve 文本 VDF 格式，`"` 键值对 + `{}` 嵌套）：

```vdf
"libraryfolders"
{
	"0"
	{
		"path"		"D:\\steam"
		"apps"
		{
			"730"		"76040199332"
			...
		}
	}
}
```

- `path` 是库根目录（注意值里的 `\\` 是 VDF 转义，需还原为 `\`）；
- `apps` 的键是 appid（值是 manifest 大小，可忽略）；
- 多库时是 `"0"`、`"1"`、`"2"`… 多个子块；**也可能出现 `"ContentStore"` 等非数字键**，解析时要按「值里有 `path` 的块」判断，不要按数字键遍历。

**`appmanifest_<appid>.acf` 结构**（同为文本 VDF，顶层是 `"AppState"`）：

| 字段 | 实测值示例 | 用途 |
|---|---|---|
| `appid` | `292030` | 来源 ID |
| `name` | `The Witcher 3: Wild Hunt — Remastered` | 显示名（含 em-dash 等 Unicode，文件为 UTF-8） |
| `installdir` | `The Witcher 3` | 相对 `steamapps\common\` |
| `StateFlags` | `4` | **4 = 完整安装**；其它值（更新中/需要更新/损坏）应跳过 |
| `LastPlayed` / `SizeOnDisk` | — | 不使用 |

**实测发现的真实坑**（全部来自本机数据）：

1. `appid 228980` = `Steamworks Common Redistributables` —— **不是游戏**，必须按 appid 或名称排除；
2. `appid 431960` = Wallpaper Engine，`name` 字段是占位串 `appid_431960` —— 工具类应用会混进来；
3. exe 位置**千差万别**，启发式扫描不可靠：
   - 巫师 3：`bin\x64_dx12\witcher3.exe`（第 3 层，且同目录还有 `REDprelauncher.exe` 干扰项）
   - CS2：`game\bin\win64\cs2.exe`（**第 4 层**，深度 3 以内根本扫不到）
   - Hollow Knight Silksong：根目录就有，但旁边还有 `UnityCrashHandler64.exe` 陷阱
4. 结论：**Steam 来源不承诺自动找到启动 exe**。`name` + `installdir` 高置信度；exe 交给 NVIDIA 来源（它有精确路径）或目录启发式 + 用户确认。

### 2.2 Epic（已安装，本机无游戏）

- `%PROGRAMDATA%\Epic\EpicGamesLauncher\Data\Manifests\` 存在但 `.item` 数量为 0 —— **这是「已安装但没装游戏」的真实样本**，正好验证了「清单为空 → 优雅返回空结果」的路径。
- 实现按公开已知的 `.item` JSON 结构（`DisplayName`、`InstallLocation`、`LaunchExecutable`、`bIsIncomplete`）编写解析器，全部字段视为可选、缺失即跳过该条目。本机无法验证字段真值，**此来源标记为「格式依据公开资料，未经本机实测」**，待用户机器上有 Epic 游戏后再实测。
- 另检查了 `%PROGRAMDATA%\Epic\` 下的 `EpicOnlineServices`、`UnrealEngineLauncher` 等目录，无游戏清单。

### 2.3 NVIDIA App（已验证，质量超出预期）

**数据位置**：`%LOCALAPPDATA%\NVIDIA Corporation\NVIDIA app\NvBackend\ApplicationStorage.json`（本机 31 KB，UTF-8 带 BOM 的明文 JSON，普通用户权限可读）。

> 同目录的 `AppicationsMetadata.json`（55 字节全零）与 `ApplicationState\*.json`（每个应用的**显卡设置**）都不是我们要的数据；`ApplicationStorage.json` 才是。

**结构**：

```json
{
  "Applications": [
    {
      "LocalId": 21969271,
      "Application": {
        "DisplayName": "The Witcher 3: Wild Hunt",
        "ShortName": "...",
        "Distributor": "Standalone",
        "InstallDirectory": "D:\\steam\\steamapps\\common\\The Witcher 3\\",
        "DetectedFiles": [
          "D:\\steam\\steamapps\\common\\The Witcher 3\\bin\\x64_dx12\\witcher3.exe",
          "D:\\steam\\steamapps\\common\\The Witcher 3\\bin\\ddi\\Steam.dll"
        ],
        "IsCreativeApplication": false,
        "IsManuallyAdded": false,
        "IsFingerprintDetected": true,
        "LastLaunchTimeISO": "..."
      }
    }
  ]
}
```

**本机实测（22 个条目）的关键验证点**：

1. `DetectedFiles[0]` **就是正确的启动 exe**，且解决了 Steam 自身解决不了的问题：
   - 巫师 3 → `bin\x64_dx12\witcher3.exe`（NVIDIA 知道真正的启动项，`REDprelauncher.exe` 没混进来）
   - CS2 → `game\bin\win64\cs2.exe`（第 4 层深度）
   - Hollow Knight → `hollow_knight.exe`（正确排除了同目录的 `UnityCrashHandler64.exe`）
2. `DetectedFiles` 混有 dll（如 `steam_api64.dll`）→ **只取 `.exe` 结尾的项**，且取第一个；
3. `IsCreativeApplication` 字段区分创作类软件（本机 Workbuddy、BCUT 为 `true`，游戏均为 `false`）→ 用它做初筛，但**不硬过滤**（万一误标），改为降置信度并提示；
4. **工具类应用会混进来**（本机实测有：Bilibili 直播伴侣 ×2、Logitech G Hub、钉钉、腾讯会议）→ 印证了需求文档「不要把目录中的所有 exe 都当成游戏」的要求：**来源给的是「NVIDIA 认识的应用」，不是「游戏」**，必须由用户勾选确认；
5. **脏数据样本**（必须处理的边界情况，全部来自真实文件）：
   - `DisplayName` 为 `Assassin's Creed Revelations.url` 的条目（用户手动从桌面添加的快捷方式，`IsManuallyAdded=true`）→ 名称里带 `.url` 的要降置信度并提示；
   - Sekiro 出现两次（两个不同的 `InstallDirectory`）→ 交给统一去重 + 用户确认；
   - Minecraft（WindowsApps）`DetectedFiles` 为空数组 → 无 exe 的条目跳过并说明原因；
6. 来源**兼容性风险**（如实记录）：这是 NVIDIA App 的私有本地缓存，**格式无官方承诺**，App 升级可能改变结构。缓解：解析器容错（缺字段即跳过）、解析失败时该来源标记为「不可用」而不是报错、版本号写入日志。**只读访问，不写、不锁、不删**，符合需求文档 §3.A 的合规要求。

### 2.4 指定目录扫描（设计，不需外部验证）

按需求 §3.D：只扫用户选的目录；默认深度 3、单目录文件数上限 2000、总时限 60 秒（三者均可调，防止扫爆）；排除 `Windows`、`Program Files`、`$Recycle.Bin`、`AppData`、`node_modules`、`__pycache__`、`CommonRedist`、`Redist`、`_CommonRedist`、`DotNet`、`DXSETUP` 等目录与文件名（规则集中在一个常量表里，便于维护）。

候选判定（启发式，产出的都是低/中置信度）：
- 目录名包含疑似游戏关键词，或 exe 与目录名相近 → `medium`；
- 其余 exe 一律 `low` 并标「需要确认」；
- **不做**文件大小、图标资源分析之外的「智能判断」。

## 3. 统一数据结构（Rust）

```rust
/// 发现来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiscoverySource {
    Nvidia,
    Steam,
    Epic,
    Directory,
}

/// 置信度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiscoveryConfidence {
    High,
    Medium,
    Low,
}

/// 一个待确认的候选。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredGame {
    pub name: String,
    pub launch_path: String,
    pub launch_args: Option<String>,
    pub working_directory: Option<String>,
    pub source: DiscoverySource,
    pub confidence: DiscoveryConfidence,
    /// 来源内部 ID（Steam appid / NVIDIA LocalId 等），用于展示与调试。
    pub source_id: Option<String>,
    /// 已在库中（按规范化启动路径比对），前端据此默认不勾选。
    pub already_in_library: bool,
    pub warnings: Vec<String>,
}

/// 每个来源的扫描结果：状态与候选分开，来源失败不影响其它来源。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceScanResult {
    pub source: DiscoverySource,
    pub status: SourceStatus,   // Available / NotInstalled / Unavailable { reason }
    pub candidates: Vec<DiscoveredGame>,
}
```

模块位置（与现有 `importer/` 同级，独立成 `discovery/`，不改动现有导入路径）：

```
src-tauri/src/discovery/
├── mod.rs          // DiscoverySource/Confidence/DiscoveredGame + 协调器 scan_all()
├── steam.rs        // libraryfolders.vdf + appmanifest 解析
├── epic.rs         // Manifests/*.item 解析
├── nvidia.rs       // ApplicationStorage.json 解析
├── directory.rs    // 用户指定目录启发式
└── vdf.rs          // 极简文本 VDF 解析（Steam 两个文件共用）
```

## 4. 去重与保护

1. **与库内比对**：复用现有 `normalize_launch_path` 逻辑（P2 的 `launch_path_key`，已处理大小写、分隔符、`\\?\` 前缀），命中即 `already_in_library = true`，前端默认不勾选；
2. **来源间合并**：同一规范化路径被多个来源发现 → 合并为一条，来源列表聚合（展示用），字段取「信息更全的那个」（有 exe 的优先于只有 installdir 的）；
3. **不强行合并**：路径不同但名字相近（如两个 Sekiro）→ 各自展示，让用户决定；
4. **不覆盖**：已存在条目在预览里只读展示，勾选框禁用；**绝不写库**；
5. **批量导入**：逐条调用现有 `add_game_from_exe_impl`（含去重、名称、封面提取），**不绕过业务逻辑**；单条失败记录原因并继续。

## 5. 拟修改文件清单

| 文件 | 改动 |
|---|---|
| `src-tauri/src/discovery/*`（新增） | 上述 6 个模块 |
| `src-tauri/src/commands/discovery.rs`（新增） | `scan_games(sources, directories)`、`import_discovered_games(items)` 两个命令 |
| `src-tauri/src/lib.rs` | 注册 2 个命令 + `pub mod discovery;` |
| `src/types/discovery.ts`（新增） | DTO 对应 |
| `src/services/gameApi.ts` | 2 个 API 封装 |
| `src/components/ScanDialog.tsx`（新增） | 扫描入口、来源状态、进度、预览列表、勾选、确认导入 |
| `src/pages/LibraryPage.tsx` | 「添加游戏」弹窗里加「扫描游戏」入口（不改变现有手动导入） |
| `docs/architecture.md` | **铁律 #1 改写** + §4 新增 `discovery/` 小节 + §7.1 命令表补充 |
| `docs/manual-testing.md` | 增加扫描/预览/导入的验证步骤 |

**不改动**：数据库迁移（无新表，`launch_path_key` 已够用）、启动、计时、封面、现有导入流程。

## 6. 测试计划（全部用夹具，不依赖真实启动器）

夹具：`src-tauri/src/discovery/testdata/`（或 `tests/fixtures/`）下手工构造——
`libraryfolders.vdf`（含非数字键与多库）、`appmanifest_*.acf`（含 StateFlags≠4、name 占位、Unicode 名）、
`ApplicationStorage.json`（含创作类、.url 脏数据、无 exe 条目、重复条目）、空 `.item`、损坏 JSON。

| # | 用例 | 对应需求 |
|---|---|---|
| 1 | NVIDIA 不可用（文件缺失/损坏）时 Steam 仍能扫出 | §9.1 |
| 2 | Steam 未安装 / vdf 缺失 / acf 损坏 → 状态 Unavailable，不崩 | §9.2 |
| 3 | 目录扫描只访问指定根（用临时目录验证不越界） | §9.3 |
| 4 | Steamworks Redistributables / 钉钉等工具类候选被降置信度并带警告 | §9.4 |
| 5 | 同一 exe 多来源只出一条；`.lnk` 目标与扫描 exe 去重 | §9.5 |
| 6 | 大小写 / `\\` 与 `/` / `\\?\` / 中文与空格路径规范化正确 | §9.6 |
| 7 | 已在库中的候选 `already_in_library = true` 且导入时跳过 | §9.7 |
| 8 | 导入不触碰已有记录的名称 / 封面 / 时长（复用现有 `add_game_from_exe` 测试） | §9.8 |
| 9 | 扫描取消不写库（纯内存结果，天然满足；UI 层验证） | §9.9 |
| 10 | 批量导入部分失败：成功项入库，失败项带原因返回 | §9.10 |
| 11 | 导入后的游戏走现有启动/计时路径（类型为 exe，行为一致） | §9.11 |
| 12 | 现有 108 条测试全部保持通过（回归） | §9.12 |

**本环境无法做到、需留到真实环境验证的**：Epic 有游戏时的 `.item` 字段实测；NVIDIA App 升级后格式是否仍兼容（已把解析器设计为容错 + 可整体跳过）。

## 7. 限制与风险（如实声明）

1. **NVIDIA App 数据是私有缓存**，格式无官方承诺，可能随版本变化。缓解：容错解析、失败降级为来源不可用、不做任何写操作；
2. **NVIDIA 来源不区分「游戏」与「NVIDIA 认识的应用」**（直播伴侣、钉钉都在），只能靠 `IsCreativeApplication` 降置信度 + 用户勾选兜底；
3. **Epic 字段未经本机实测**（本机无 Epic 游戏），解析器按公开格式编写，标记为待实测；
4. **Steam 不承诺 exe**：只有 NVIDIA 来源覆盖到的 Steam 游戏才有精确 exe；其余以「安装目录 + 需要确认」的形式出现，用户需手动挑一次；
5. **registry 访问在开发沙箱里被安全中心拦截**（`reg.exe` 在程序黑名单），本调查未读注册表；实现里 Steam 根目录的定位将采用「注册表 `HKCU\Software\Valve\Steam\SteamPath` → 常见路径探测」的组合，注册表部分在真实环境验证。

## 7.1 阶段三实施记录（2026-10-14 完成）

- 后端：`commands/discovery.rs` —— `scan_games`（扫描）与 `import_discovered_games`（批量导入）。
  批量导入逐条调用现有 `add_game_from_exe_impl`，**单条失败记录原因并继续**；
  导入前用 `find_by_launch_key` 再查一次重（后端兜底）；发现来源带的启动参数 / 工作目录
  通过现有 `games::update` 补写到刚导入的记录上。
- 前端：`ScanDialog.tsx`（来源状态行、预览列表、全选/单个勾选、已在库中默认不勾选且禁用、
  导入结果摘要 + 失败 Toast）、LibraryPage 顶栏「扫描游戏」入口（手动导入入口原样保留）。
- **真实环境验证**（`cargo test --lib discovery::real_env_tests::scan_real_machine -- --ignored --nocapture`）：
  Steam 候选 6 / 跳过 1，Epic 可用 0 候选，NVIDIA 候选 20 / 跳过 2；跨来源合并生效
  （CS2、Hollow Knight、PvZ 均为 `[Nvidia, Steam]`）；**全部候选的启动目标都真实存在**。
- **真实数据反哺的修正**：Steam 的 exe 启发式曾把巫师 3 重制版错配成 `D3D12StateObjectCompiler.exe`，
  已扩充 exe 黑名单（`d3d12` / `shader` / `compiler` 等）并把打分改为「目录名包含 exe 名」——
  修正后巫师 3 与 NVIDIA 来源正确合并为一条。
- 全量校验：fmt / clippy（零警告）/ test（127 passed）/ tsc / vite / tauri build 全部通过。

## 7.2 通用性核对（2026-10-14，回应「不同机器目录不同」）

| 项 | 定位方式 | 是否与安装盘符/位置无关 |
|---|---|---|
| Steam 根目录 | ① 注册表 `HKCU\Software\Valve\Steam : SteamPath`（Steam 安装时自己写，**任意位置都能定位**；本机实测返回 `d:/steam`）② 注册表 `HKLM\...\Valve\Steam : InstallPath` ③ Program Files 两个默认位置 ④ **盘根兜底**：A~Z 逐个探测 `X://Steam` / `X://SteamLibrary` 下是否存在 `steamapps\libraryfolders.vdf`（只查固定路径，不递归扫描） | ✅ |
| Steam 库目录 | `libraryfolders.vdf` 里列出的**全部**库根（任意盘符的多个 Library 都覆盖） | ✅ |
| Epic 清单 | `%PROGRAMDATA%\Epic\EpicGamesLauncher\Data\Manifests` —— 启动器无论装在哪都写这个机器级固定位置 | ✅ |
| NVIDIA 数据 | 三个候选按序尝试：`%LOCALAPPDATA%\...\NVIDIA app\NvBackend\`（新版 App，本机实测）→ `%PROGRAMDATA%\...\NVIDIA app\NvBackend\` → `%PROGRAMDATA%\...\NVIDIA GeForce Experience\NvBackend\`（**旧版 GFE**，没升级的机器） | ✅ |
| 多用户 | Steam 按当前用户的 HKCU；Epic / NVIDIA 按环境变量（PROGRAMDATA 机器级 + LOCALAPPDATA 用户级） | ✅ |

**验证**：`registry_string` 在本机实测成功（返回 `d:/steam` 且目录存在）——此前只验证过「能编译」；
加盘根兜底后真实环境扫描仍全绿（Steam 6 / NVIDIA 20 / Epic 0 候选）。

**仍覆盖不了的**（如实）：Steam 装在完全自定义的多级目录且注册表被清（如 `D://Games//Steam`）——
盘根兜底只认 `X://Steam` / `X://SteamLibrary` 两种布局，此时会优雅返回「未安装」而不是报错；
NVIDIA App 未来版本改格式 → 解析失败降级为来源不可用（已文档化）。

## 8. 铁律修订记录

原铁律 #1「不做 NVIDIA / Steam / Epic 自动扫描」由用户于 2026-10-14 决定推翻，修订为：

> **不做未经用户确认的自动导入**；自动发现仅扫描本地明文清单与用户指定目录，只读、不联网、不执行候选程序、不修改启动器数据；所有导入必须经过预览与用户勾选。

其余铁律不变。
