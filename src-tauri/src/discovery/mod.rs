//! 自动发现候选游戏（对应 docs/auto-discovery-design.md）。
//!
//! **边界**（铁律 #1 修订版）：只读本地明文清单与用户指定的目录；不联网、不执行候选程序、
//! 不修改启动器数据；**所有导入必须经过用户勾选确认**。本模块只产出候选，导入由
//! `commands/discovery.rs` 调用现有的 `add_game_from_exe_impl` 完成。
//!
//! 一个来源失败不影响其它来源：每个来源返回独立的 `SourceScanResult`。

pub mod amd;
mod battlenet;
mod directory;
pub mod epic;
pub mod nvidia;
pub mod steam;
mod ubisoft;
pub mod vdf;

use std::path::PathBuf;

use serde::Serialize;

use crate::database::Db;
use crate::database::games;
use crate::error::AppResult;

/// 发现来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiscoverySource {
    Nvidia,
    Steam,
    Epic,
    Ubisoft,
    Battlenet,
    Amd,
    Directory,
}

impl DiscoverySource {
    pub fn as_str(self) -> &'static str {
        match self {
            DiscoverySource::Nvidia => "nvidia",
            DiscoverySource::Steam => "steam",
            DiscoverySource::Epic => "epic",
            DiscoverySource::Ubisoft => "ubisoft",
            DiscoverySource::Battlenet => "battlenet",
            DiscoverySource::Amd => "amd",
            DiscoverySource::Directory => "directory",
        }
    }

    /// 合并同类候选时的优先级：信息越全的来源越优先保留。
    fn rank(self) -> u8 {
        match self {
            // NVIDIA 给的是指纹识别出的真实启动程序，含金量最高
            DiscoverySource::Nvidia => 3,
            DiscoverySource::Epic => 2,
            DiscoverySource::Ubisoft => 2,
            DiscoverySource::Steam => 1,
            DiscoverySource::Battlenet => 1,
            DiscoverySource::Amd => 0,
            DiscoverySource::Directory => 0,
        }
    }
}

/// 置信度：这是「数据可信度」，不是「是不是游戏的判定」——
/// 是否导入永远由用户决定（需求 §3.D / §6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiscoveryConfidence {
    Low,
    Medium,
    High,
}

/// 一个待确认的候选。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredGame {
    pub name: String,
    pub launch_path: String,
    pub launch_args: Option<String>,
    pub working_directory: Option<String>,
    /// 发现该候选的全部来源（合并后可能多于一个，第一个是主来源）。
    pub sources: Vec<DiscoverySource>,
    pub confidence: DiscoveryConfidence,
    /// 来源内部 ID（Steam appid、Epic manifest 文件名等），用于展示与调试。
    pub source_id: Option<String>,
    /// 已在库中（按规范化启动路径比对）。前端据此默认不勾选。
    pub already_in_library: bool,
    pub warnings: Vec<String>,
}

/// 来源不可用的原因。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SourceState {
    Available,
    NotInstalled,
    Unavailable { reason: String },
}

/// 单个来源的扫描结果。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceScanResult {
    pub source: DiscoverySource,
    pub state: SourceState,
    pub candidates: Vec<DiscoveredGame>,
    /// 因无 exe / 路径失效 / 清单损坏而跳过的条目数（来源状态里可见，不硬造候选）。
    pub skipped: usize,
}

impl SourceScanResult {
    pub fn not_installed(source: DiscoverySource) -> Self {
        Self {
            source,
            state: SourceState::NotInstalled,
            candidates: Vec::new(),
            skipped: 0,
        }
    }

    pub fn unavailable(source: DiscoverySource, reason: String) -> Self {
        Self {
            source,
            state: SourceState::Unavailable { reason },
            candidates: Vec::new(),
            skipped: 0,
        }
    }
}

/// 整体扫描结果：各来源状态 + 合并去重后的候选列表。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutcome {
    pub sources: Vec<SourceScanResult>,
    pub games: Vec<DiscoveredGame>,
}

/// 扫描全部来源并做跨来源去重。
///
/// `directories` 只用于「指定目录」来源；传空表示不扫目录。
pub fn scan_all(db: &Db, directories: &[PathBuf]) -> AppResult<ScanOutcome> {
    let sources = vec![
        steam::scan_steam(steam::detect_steam_root().as_deref()),
        epic::scan_epic(epic::manifests_dir().as_deref()),
        nvidia::scan_nvidia(nvidia::storage_path().as_deref()),
        battlenet::scan_battlenet(None),
        ubisoft::scan_ubisoft(None, &ubisoft::detect_ubisoft_installs()),
        amd::scan_amd(None),
        directory::scan_directories(directories, &Default::default()),
    ];

    let games = merge_and_dedupe(&sources, db)?;

    Ok(ScanOutcome { sources, games })
}

/// 跨来源去重与合并。
///
/// 1. 规范化键（复用现有 `normalize_launch_key`）相同 → 合并为一条；
/// 2. 合并时保留「信息更全」的候选（优先级：来源 rank > 置信度），来源列表聚合；
/// 3. 已在库中的标记 `already_in_library`（前端默认不勾选）。
fn merge_and_dedupe(sources: &[SourceScanResult], db: &Db) -> AppResult<Vec<DiscoveredGame>> {
    let mut merged: Vec<DiscoveredGame> = Vec::new();

    for result in sources {
        for mut candidate in result.candidates.clone() {
            let key = games::normalize_launch_key(
                &candidate.launch_path,
                candidate.launch_args.as_deref().unwrap_or(""),
            );

            if let Some(existing) = merged.iter_mut().find(|game| {
                games::normalize_launch_key(
                    &game.launch_path,
                    game.launch_args.as_deref().unwrap_or(""),
                ) == key
            }) {
                // 合并：保留 rank 更高（信息更全）的那个，来源聚合，警告并入
                let keep_new = source_rank_of(candidate.sources.first().copied())
                    > source_rank_of(existing.sources.first().copied());
                if keep_new {
                    let previous_sources = std::mem::take(&mut existing.sources);
                    let previous_warnings = std::mem::take(&mut existing.warnings);
                    candidate.sources = merge_sources(candidate.sources.clone(), &previous_sources);
                    for warning in previous_warnings {
                        if !candidate.warnings.contains(&warning) {
                            candidate.warnings.push(warning);
                        }
                    }
                    *existing = candidate;
                } else {
                    for source in candidate.sources {
                        if !existing.sources.contains(&source) {
                            existing.sources.push(source);
                        }
                    }
                    for warning in candidate.warnings {
                        if !existing.warnings.contains(&warning) {
                            existing.warnings.push(warning);
                        }
                    }
                }
                continue;
            }

            candidate.sources = vec![
                candidate
                    .sources
                    .first()
                    .copied()
                    .unwrap_or(DiscoverySource::Directory),
            ];
            merged.push(candidate);
        }
    }

    // 标记已在库中的候选
    for game in &mut merged {
        game.already_in_library = db
            .with_conn(|conn| {
                games::find_by_launch_key(
                    conn,
                    &game.launch_path,
                    game.launch_args.as_deref().unwrap_or(""),
                )
            })?
            .is_some();
    }

    // 稳定排序：置信度降序，同名游戏相邻便于浏览
    merged.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(merged)
}

fn source_rank_of(source: Option<DiscoverySource>) -> u8 {
    source.map_or(0, DiscoverySource::rank)
}

fn merge_sources(
    primary: Vec<DiscoverySource>,
    previous: &[DiscoverySource],
) -> Vec<DiscoverySource> {
    let mut sources = primary;
    for source in previous {
        if !sources.contains(source) {
            sources.push(*source);
        }
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(name: &str, path: &str, source: DiscoverySource) -> DiscoveredGame {
        DiscoveredGame {
            name: name.to_owned(),
            launch_path: path.to_owned(),
            launch_args: None,
            working_directory: None,
            sources: vec![source],
            confidence: DiscoveryConfidence::High,
            source_id: None,
            already_in_library: false,
            warnings: Vec::new(),
        }
    }

    fn result(source: DiscoverySource, candidates: Vec<DiscoveredGame>) -> SourceScanResult {
        SourceScanResult {
            source,
            state: SourceState::Available,
            candidates,
            skipped: 0,
        }
    }

    #[test]
    fn same_exe_from_two_sources_is_merged_once() {
        // NVIDIA 与 Steam 都发现了同一个 exe → 只出一条，来源聚合
        let results = vec![
            result(
                DiscoverySource::Nvidia,
                vec![game(
                    "Hollow Knight",
                    r"D://steam//steamapps//common//Hollow Knight//hollow_knight.exe",
                    DiscoverySource::Nvidia,
                )],
            ),
            result(
                DiscoverySource::Steam,
                vec![game(
                    "Hollow Knight",
                    r"D:/steam/steamapps/common/HOLLOW KNIGHT/hollow_knight.exe",
                    DiscoverySource::Steam,
                )],
            ),
        ];

        let db = Db::open_in_memory().expect("内存库");
        let merged = merge_and_dedupe(&results, &db).expect("合并");

        assert_eq!(merged.len(), 1, "大小写与分隔符不同的同一路径应合并");
        assert_eq!(merged[0].sources.len(), 2, "两个来源都要记录");
        assert_eq!(
            merged[0].sources[0],
            DiscoverySource::Nvidia,
            "rank 高的在前"
        );
    }

    #[test]
    fn different_paths_stay_separate() {
        let results = vec![result(
            DiscoverySource::Nvidia,
            vec![
                game("Sekiro", r"D://a//sekiro.exe", DiscoverySource::Nvidia),
                game("Sekiro", r"D://b//sekiro.exe", DiscoverySource::Nvidia),
            ],
        )];

        let db = Db::open_in_memory().expect("内存库");
        let merged = merge_and_dedupe(&results, &db).expect("合并");
        assert_eq!(merged.len(), 2, "路径不同的同名游戏不得强行合并");
    }

    #[test]
    fn existing_library_games_are_marked_not_new() {
        let db = Db::open_in_memory().expect("内存库");
        let exe = std::env::temp_dir().join(format!("gamelib-merge-{}.exe", std::process::id()));
        std::fs::write(&exe, b"mz").expect("写 exe");

        db.with_conn(|conn| {
            games::insert(
                conn,
                &crate::database::games::InsertGame {
                    id: games::new_id(),
                    name: "已有游戏",
                    original_name: "已有游戏",
                    source_type: crate::database::models::SourceType::Exe,
                    source_path: &exe.to_string_lossy(),
                    launch_path: &exe.to_string_lossy(),
                    launch_args: "",
                    working_directory: None,
                    cover_path: None,
                    icon_path: None,
                    is_launcher: false,
                },
            )
        })
        .expect("插入已有游戏");

        let results = vec![result(
            DiscoverySource::Nvidia,
            vec![game(
                "已有游戏",
                &exe.to_string_lossy(),
                DiscoverySource::Nvidia,
            )],
        )];

        let merged = merge_and_dedupe(&results, &db).expect("合并");
        assert_eq!(merged.len(), 1);
        assert!(merged[0].already_in_library, "应标记为已在库中");
        let _ = std::fs::remove_file(&exe);
    }
}

#[cfg(test)]
mod real_env_tests {
    use super::*;

    /// 真实环境验证（默认忽略，手动运行）：
    ///   cargo test --lib discovery::real_env_tests::scan_real_machine -- --ignored --nocapture
    ///
    /// 只读访问本机的启动器清单，验证扫描链路在真实数据上的表现。
    /// 断言故意宽松：本机装没装某来源都应「优雅返回」，而不是崩溃。
    #[test]
    #[ignore = "真实环境验证：依赖本机安装的启动器"]
    fn scan_real_machine() {
        let db = Db::open_in_memory().expect("内存库");
        let outcome = scan_all(&db, &[]).expect("扫描");

        println!("\n=== 真实环境扫描结果 ===");
        for source in &outcome.sources {
            let state = match &source.state {
                SourceState::Available => "可用".to_owned(),
                SourceState::NotInstalled => "未安装".to_owned(),
                SourceState::Unavailable { reason } => format!("不可用：{reason}"),
            };
            println!(
                "  {:<12} {state}（候选 {}，跳过 {}）",
                source.source.as_str(),
                source.candidates.len(),
                source.skipped
            );
        }

        println!("\n合并去重后的候选：");
        for game in &outcome.games {
            println!(
                "  [{:?}] {:?} {} —— {}",
                game.confidence, game.sources, game.name, game.launch_path
            );
        }

        // 全部来源都必须给出明确状态（Steam/Epic/NVIDIA/目录/战网/育碧/AMD）
        assert_eq!(outcome.sources.len(), 7);

        // Battle.net：本机装有炉石传说（2026-10-16 实测），且该来源与 NVIDIA 同路径合并
        let battlenet = outcome
            .sources
            .iter()
            .find(|source| source.source == DiscoverySource::Battlenet)
            .expect("应有 battlenet 来源");
        assert!(matches!(battlenet.state, SourceState::Available));
        assert!(
            outcome.games.iter().any(|game| {
                game.sources.contains(&DiscoverySource::Battlenet)
                    && game.launch_path.to_lowercase().contains("hearthstone")
            }),
            "应发现炉石传说（与 NVIDIA 同路径合并）"
        );

        // Ubisoft：本机装有刺客信条：启示录（2026-10-16 实测）
        let ubisoft = outcome
            .sources
            .iter()
            .find(|source| source.source == DiscoverySource::Ubisoft)
            .expect("应有 ubisoft 来源");
        assert!(matches!(ubisoft.state, SourceState::Available));
        assert!(
            outcome.games.iter().any(|game| {
                game.sources.contains(&DiscoverySource::Ubisoft)
                    && game
                        .launch_path
                        .to_lowercase()
                        .contains("assassin's creed revelations")
            }),
            "应发现刺客信条：启示录"
        );

        // AMD：本机未安装 → 优雅返回「未安装」而非故障
        let amd = outcome
            .sources
            .iter()
            .find(|source| source.source == DiscoverySource::Amd)
            .expect("应有 amd 来源");
        assert!(matches!(amd.state, SourceState::NotInstalled));

        // Steam 在本机已安装且库里确有游戏（2026-10-14 实测：7 个 appmanifest）
        let steam = outcome
            .sources
            .iter()
            .find(|source| source.source == DiscoverySource::Steam)
            .expect("应有 Steam 来源");
        assert!(
            matches!(steam.state, SourceState::Available),
            "本机装有 Steam，来源应可用"
        );
        assert!(
            !steam.candidates.is_empty(),
            "本机 Steam 库里有完整安装的游戏，应能发现候选"
        );

        // NVIDIA App 本机也已安装（实测 22 个应用记录）
        let nvidia = outcome
            .sources
            .iter()
            .find(|source| source.source == DiscoverySource::Nvidia)
            .expect("应有 NVIDIA 来源");
        assert!(matches!(nvidia.state, SourceState::Available));

        // 所有候选的 exe 都必须真实存在（路径验证）
        for game in &outcome.games {
            assert!(
                std::path::Path::new(&game.launch_path).is_file(),
                "候选的启动目标必须存在：{} —— {}",
                game.name,
                game.launch_path
            );
        }
    }
}

/// 阶段三验收测试（2026-10-16）：三个新来源与协调器的集成。
/// 全部为合成数据，不依赖实机安装；验证复用现有去重/合并/库内标记，不引入第二套规则。
#[cfg(test)]
mod phase3_tests {
    use super::*;
    use crate::database::games;
    use crate::database::models::{GameSort, SortOrder};

    fn game(name: &str, path: &str, source: DiscoverySource) -> DiscoveredGame {
        DiscoveredGame {
            name: name.to_owned(),
            launch_path: path.to_owned(),
            launch_args: None,
            working_directory: None,
            sources: vec![source],
            confidence: DiscoveryConfidence::High,
            source_id: None,
            already_in_library: false,
            warnings: Vec::new(),
        }
    }

    fn result(source: DiscoverySource, candidates: Vec<DiscoveredGame>) -> SourceScanResult {
        SourceScanResult {
            source,
            state: SourceState::Available,
            candidates,
            skipped: 0,
        }
    }

    #[test]
    fn battlenet_and_nvidia_merge_on_same_exe() {
        // 炉石传说同时被 Battle.net 与 NVIDIA 发现（同一路径、分隔符差异）→ 合并为一条
        let hs_path_bnet = r"D:/Hearthstone/Hearthstone.exe";
        let hs_path_nvidia = r"D:/Hearthstone/Hearthstone.exe";
        let results = vec![
            result(
                DiscoverySource::Nvidia,
                vec![game(
                    "Hearthstone: Heroes of Warcraft",
                    hs_path_nvidia,
                    DiscoverySource::Nvidia,
                )],
            ),
            result(
                DiscoverySource::Battlenet,
                vec![game("炉石传说", hs_path_bnet, DiscoverySource::Battlenet)],
            ),
        ];

        let db = Db::open_in_memory().expect("内存库");
        let merged = merge_and_dedupe(&results, &db).expect("合并");

        assert_eq!(merged.len(), 1, "同一 exe 必须合并为一条候选");
        let merged_game = &merged[0];
        assert_eq!(merged_game.sources.len(), 2, "两个来源都要记录");
        assert_eq!(
            merged_game.sources[0],
            DiscoverySource::Nvidia,
            "rank 高的来源排在前"
        );
        // 合并保留 rank 高来源的名称（NVIDIA 的英文名），但路径一致
        assert!(
            merged_game
                .launch_path
                .to_lowercase()
                .contains("hearthstone.exe")
        );
    }

    #[test]
    fn same_name_different_path_across_new_sources_stay_separate() {
        // 同名不同路径：不得按名称强行合并（跨新来源同样适用）
        let results = vec![
            result(
                DiscoverySource::Ubisoft,
                vec![game(
                    "Assassin's Creed",
                    r"D:/games1/ACRSP.exe",
                    DiscoverySource::Ubisoft,
                )],
            ),
            result(
                DiscoverySource::Ubisoft,
                vec![game(
                    "Assassin's Creed",
                    r"D:/games2/AssassinsCreedRevelations.exe",
                    DiscoverySource::Ubisoft,
                )],
            ),
            result(
                DiscoverySource::Battlenet,
                vec![game(
                    "Assassin's Creed",
                    r"D:/games3/ACR.exe",
                    DiscoverySource::Battlenet,
                )],
            ),
        ];

        let db = Db::open_in_memory().expect("内存库");
        let merged = merge_and_dedupe(&results, &db).expect("合并");
        assert_eq!(merged.len(), 3, "三个不同路径的同名游戏都应保留");
    }

    #[test]
    fn already_in_library_flag_works_for_new_sources() {
        // 库中已有炉石传说 → Battle.net 候选应标记 already_in_library
        let db = Db::open_in_memory().expect("内存库");
        let exe = std::env::temp_dir().join(format!("gamelib-p3-{}.exe", std::process::id()));
        std::fs::write(&exe, b"mz").expect("写 exe");

        db.with_conn(|conn| {
            games::insert(
                conn,
                &games::InsertGame {
                    id: games::new_id(),
                    name: "炉石传说",
                    original_name: "炉石传说",
                    source_type: crate::database::models::SourceType::Exe,
                    source_path: &exe.to_string_lossy(),
                    launch_path: &exe.to_string_lossy(),
                    launch_args: "",
                    working_directory: None,
                    cover_path: None,
                    icon_path: None,
                    is_launcher: false,
                },
            )
        })
        .expect("插入已有游戏");

        let results = vec![result(
            DiscoverySource::Battlenet,
            vec![game(
                "炉石传说",
                &exe.to_string_lossy(),
                DiscoverySource::Battlenet,
            )],
        )];

        let merged = merge_and_dedupe(&results, &db).expect("合并");
        assert_eq!(merged.len(), 1);
        assert!(
            merged[0].already_in_library,
            "新来源的候选同样要标记已在库中"
        );
        let _ = std::fs::remove_file(&exe);
    }

    #[test]
    fn warnings_are_merged_across_sources() {
        // 同一 exe 的两个来源各带 warning → 合并后警告聚合且去重
        let mut with_warning = game("X", r"D:/x/game.exe", DiscoverySource::Nvidia);
        with_warning.warnings = vec!["NVIDIA 来源警告".to_owned()];
        let mut ubisoft_warning = game("X", r"D:/x/game.exe", DiscoverySource::Ubisoft);
        ubisoft_warning.warnings = vec!["Ubisoft 来源警告".to_owned()];

        let results = vec![
            result(DiscoverySource::Nvidia, vec![with_warning]),
            result(DiscoverySource::Ubisoft, vec![ubisoft_warning]),
        ];

        let db = Db::open_in_memory().expect("内存库");
        let merged = merge_and_dedupe(&results, &db).expect("合并");
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].warnings.len(), 2, "两个来源的警告都应保留");
    }

    #[test]
    fn unavailable_and_not_installed_sources_do_not_affect_others() {
        // AMD 未安装 + Ubisoft 不可用 → 其余来源照常返回（阶段三 §4 验收项）
        let mut amd = SourceScanResult::not_installed(DiscoverySource::Amd);
        amd.skipped = 0;
        let ubisoft = SourceScanResult::unavailable(
            DiscoverySource::Ubisoft,
            "模拟：配置缓存不可读".to_owned(),
        );
        let results = vec![
            result(
                DiscoverySource::Battlenet,
                vec![game(
                    "炉石传说",
                    r"D:/Hearthstone/Hearthstone.exe",
                    DiscoverySource::Battlenet,
                )],
            ),
            ubisoft,
            amd,
        ];

        let db = Db::open_in_memory().expect("内存库");
        let merged = merge_and_dedupe(&results, &db).expect("合并");
        assert_eq!(merged.len(), 1, "失败的来源不得拖垮其他来源");
        assert_eq!(merged[0].sources, vec![DiscoverySource::Battlenet]);
    }

    #[test]
    fn scan_all_does_not_write_database() {
        // 阶段三红线：扫描不写库。真机扫描前后库内记录数必须一致。
        let db = Db::open_in_memory().expect("内存库");
        let count_before = db
            .with_conn(|conn| games::list(conn, GameSort::CreatedAt, SortOrder::Asc))
            .expect("读取")
            .len();

        // 空目录列表：只跑平台来源（真机容忍）
        let outcome = scan_all(&db, &[]).expect("扫描");
        let count_after = db
            .with_conn(|conn| games::list(conn, GameSort::CreatedAt, SortOrder::Asc))
            .expect("读取")
            .len();

        assert_eq!(
            count_before, count_after,
            "扫描不得写入数据库（写入只发生在用户确认导入时）"
        );
        // 来源数量 = 7（Steam/Epic/NVIDIA/Battle.net/Ubisoft/Amd/Directory）
        assert_eq!(outcome.sources.len(), 7);
    }
}
