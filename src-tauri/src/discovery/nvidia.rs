//! NVIDIA App 来源：读 `%LOCALAPPDATA%\NVIDIA Corporation\NVIDIA app\NvBackend\ApplicationStorage.json`。
//!
//! **实测结论（docs/auto-discovery-design.md §2.3，本机 22 个条目）**：
//! - `DetectedFiles` 里第一个 `.exe` 就是正确的启动程序（含 CS2 第 4 层深路径，且不会混入
//!   `REDprelauncher.exe` / `UnityCrashHandler64.exe` 这类干扰项）；
//! - 数据里**混着非游戏应用**（直播伴侣、钉钉、腾讯会议、Logitech G Hub 都真实出现过），
//!   且 `IsCreativeApplication` 能标记创作类软件 —— 两者都用于降置信度；
//! - 真实脏数据：`DisplayName` 是 `xxx.url` 的条目、`DetectedFiles` 为空的条目（WindowsApps）。
//!
//! 该文件是 NVIDIA App 的私有本地缓存，格式无官方承诺：解析全程容错，任何失败都降级为
//! 「来源不可用」，绝不报错阻断其它来源。只读访问，不写、不锁、不删。

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{DiscoveredGame, DiscoveryConfidence, DiscoverySource, SourceScanResult, SourceState};

/// NVIDIA `ApplicationStorage.json` 的全部候选位置，按优先级排列。
///
/// 不同机器上可能只装了新版 NVIDIA App 或旧版 GeForce Experience，两者的
/// NvBackend 数据落点不同（都叫 ApplicationStorage.json、结构相同，本机已验证新版；
/// 旧版按同构解析、失败自动降级为来源不可用）。全部基于环境变量，与安装盘符无关。
pub fn storage_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        // 新版 NVIDIA App（本机实测的位置）
        candidates.push(
            local
                .join("NVIDIA Corporation")
                .join("NVIDIA app")
                .join("NvBackend")
                .join("ApplicationStorage.json"),
        );
    }

    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        let program_data = PathBuf::from(program_data);
        // NVIDIA App 也可能把共享数据放 ProgramData
        candidates.push(
            program_data
                .join("NVIDIA Corporation")
                .join("NVIDIA app")
                .join("NvBackend")
                .join("ApplicationStorage.json"),
        );
        // 旧版 GeForce Experience（用户没升级到 NVIDIA App 的机器）
        candidates.push(
            program_data
                .join("NVIDIA Corporation")
                .join("NVIDIA GeForce Experience")
                .join("NvBackend")
                .join("ApplicationStorage.json"),
        );
    }

    candidates
}

/// 兼容旧接口：返回第一个候选位置。
pub fn storage_path() -> Option<PathBuf> {
    storage_candidates().into_iter().next()
}

// NVIDIA 的 JSON 键全部是 PascalCase（DisplayName / InstallDirectory / ...）
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Storage {
    #[serde(default)]
    applications: Vec<StorageEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StorageEntry {
    #[serde(default)]
    application: Option<Application>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Application {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    install_directory: Option<String>,
    #[serde(default)]
    detected_files: Option<Vec<String>>,
    #[serde(default)]
    is_creative_application: bool,
}

/// 扫描 NVIDIA App 的应用记录。
pub fn scan_nvidia(storage: Option<&Path>) -> SourceScanResult {
    // 逐个候选尝试：装的是新版还是旧版 GFE、数据在不在，都从「文件是否存在」判断
    let candidates: Vec<PathBuf> = match storage {
        Some(explicit) => vec![explicit.to_path_buf()],
        None => storage_candidates(),
    };
    let path = match candidates.iter().find(|path| path.is_file()) {
        Some(path) => path.clone(),
        None => return SourceScanResult::not_installed(DiscoverySource::Nvidia),
    };

    let raw = match std::fs::read(&path) {
        Ok(raw) => raw,
        Err(err) => {
            return SourceScanResult::unavailable(
                DiscoverySource::Nvidia,
                format!("无法读取数据文件 {}: {err}", path.display()),
            );
        }
    };

    // 文件带 UTF-8 BOM
    let text = String::from_utf8_lossy(&raw);
    let storage: Storage = match serde_json::from_str(&text) {
        Ok(storage) => storage,
        Err(err) => {
            // 格式变了或文件损坏：按需求 §3.A 优雅跳过，绝不阻断其它来源
            return SourceScanResult::unavailable(
                DiscoverySource::Nvidia,
                format!("数据格式无法解析（NVIDIA App 版本可能已变化）：{err}"),
            );
        }
    };

    let mut candidates = Vec::new();
    let mut skipped = 0usize;

    for entry in storage.applications {
        let Some(app) = entry.application else {
            continue;
        };

        let Some(name) = app
            .display_name
            .clone()
            .filter(|name| !name.trim().is_empty())
        else {
            skipped += 1;
            continue;
        };

        // 只取 .exe 结尾的项，第一个作为启动目标（实测就是正确答案）
        let exe = app
            .detected_files
            .unwrap_or_default()
            .into_iter()
            .find(|file| file.to_lowercase().ends_with(".exe"));

        let Some(launch_path) = exe else {
            // 如 Minecraft（WindowsApps，读不到 exe）：跳过并计数
            skipped += 1;
            continue;
        };

        if !Path::new(&launch_path).is_file() {
            // 路径已失效（卸载残留 / 移动过）
            skipped += 1;
            continue;
        }

        let mut warnings = Vec::new();
        let mut confidence = DiscoveryConfidence::High;

        // 创作类软件（本机实测：Workbuddy、BCUT 被标为 true）
        if app.is_creative_application {
            confidence = DiscoveryConfidence::Low;
            warnings.push("NVIDIA 将其标记为创作类应用，可能不是游戏".to_owned());
        }
        // 真实脏数据：DisplayName 是「Assassin's Creed Revelations.url」这种文件名
        let lower_name = name.to_lowercase();
        if lower_name.ends_with(".url")
            || lower_name.ends_with(".lnk")
            || lower_name.ends_with(".exe")
        {
            confidence = DiscoveryConfidence::Low;
            warnings.push("名称疑似文件名而非游戏名，请确认".to_owned());
        }
        if !warnings.is_empty() {
            // 通用提示：来源不区分游戏与工具
            warnings.insert(
                0,
                "NVIDIA 的数据不区分游戏与工具类软件，请确认后再导入".to_owned(),
            );
        }

        candidates.push(DiscoveredGame {
            name,
            launch_path,
            launch_args: None,
            working_directory: app.install_directory.filter(|dir| !dir.trim().is_empty()),
            sources: vec![DiscoverySource::Nvidia],
            confidence,
            source_id: None,
            already_in_library: false,
            warnings,
        });
    }

    SourceScanResult {
        source: DiscoverySource::Nvidia,
        state: SourceState::Available,
        candidates,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个假的 ApplicationStorage.json。entries 是 (name, exe, is_creative) 三元组，
    /// exe 传 None 表示该条目没有 DetectedFiles。
    pub(super) fn make_storage(dir: &Path, entries: &[(&str, Option<&str>, bool)]) -> PathBuf {
        let apps: Vec<serde_json::Value> = entries
            .iter()
            .map(|(name, exe, creative)| {
                let files: Vec<String> = exe.iter().map(|p| p.to_string()).collect();
                serde_json::json!({
                    "LocalId": 1,
                    "Application": {
                        "DisplayName": name,
                        "InstallDirectory": exe.and_then(|p| Path::new(p).parent())
                            .map(|p| p.to_string_lossy().into_owned()),
                        "DetectedFiles": files,
                        "IsCreativeApplication": creative,
                        "IsFingerprintDetected": true
                    }
                })
            })
            .collect();

        let path = dir.join("ApplicationStorage.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({ "Applications": apps })).expect("序列化"),
        )
        .expect("写文件");
        path
    }

    #[test]
    fn parses_creative_url_and_missing_exe_entries() {
        let dir = std::env::temp_dir().join(format!("gamelib-nv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");

        // 一个真实存在的游戏 exe
        let game_exe = dir.join("hollow_knight.exe");
        std::fs::write(&game_exe, b"mz").expect("写 exe");

        let storage = make_storage(
            &dir,
            &[
                (
                    "Hollow Knight",
                    Some(game_exe.to_string_lossy().as_ref()),
                    false,
                ),
                // 注意：路径必须确定不存在 —— 别用真实存在的路径（本机 E:\workbuddy\workbuddy.exe 就真的在）
                ("Workbuddy", Some(r"C:\gamelib-missing\workbuddy.exe"), true),
                (
                    "Assassin's Creed Revelations.url",
                    Some(r"C:\gamelib-missing\a.exe"),
                    false,
                ),
                ("Minecraft", None, false),
            ],
        );

        let result = scan_nvidia(Some(&storage));
        assert!(matches!(result.state, SourceState::Available));

        assert_eq!(
            result.candidates.len(),
            1,
            "只应有有效游戏（创作类降置信度但保留）"
        );
        let game = &result.candidates[0];
        assert_eq!(game.name, "Hollow Knight");
        assert_eq!(game.confidence, DiscoveryConfidence::High);

        // Minecraft 无 exe、Workbuddy 的 exe 不存在 → 都进了 skipped
        assert_eq!(result.skipped, 3, "实际 skipped = {}", result.skipped);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_means_not_installed() {
        let missing = std::env::temp_dir().join("no-such-nvidia-storage.json");
        let result = scan_nvidia(Some(&missing));
        assert!(matches!(result.state, SourceState::NotInstalled));
    }

    #[test]
    fn corrupted_json_degrades_to_unavailable() {
        let path = std::env::temp_dir().join(format!("gamelib-nv-bad-{}.json", std::process::id()));
        std::fs::write(&path, b"{ not valid json").expect("写坏文件");

        let result = scan_nvidia(Some(&path));
        assert!(
            matches!(result.state, SourceState::Unavailable { .. }),
            "损坏数据应降级为不可用而不是报错"
        );
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod unicode_path_tests {
    use super::*;

    /// 中文用户名 / 带空格的数据目录必须照常工作（不同机器的 LOCALAPPDATA 会不同）。
    #[test]
    fn storage_at_chinese_path_works() {
        let dir = std::env::temp_dir().join("英伟达 数据 目录");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");

        let game_exe = dir.join("只狼 影逝二度.exe");
        std::fs::write(&game_exe, b"mz").expect("写 exe");

        let storage = super::tests::make_storage(
            &dir,
            &[(
                "Sekiro: Shadows Die Twice",
                Some(game_exe.to_string_lossy().as_ref()),
                false,
            )],
        );
        // 顺手验证：中文数据文件也应能读（不是只有 ASCII 文件名才行）
        let renamed = dir.join("ApplicationStorage 数据.json");
        std::fs::rename(&storage, &renamed).expect("改名");

        let result = scan_nvidia(Some(&renamed));
        assert!(matches!(result.state, SourceState::Available));
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].name, "Sekiro: Shadows Die Twice");
        assert!(result.candidates[0].launch_path.contains("只狼"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
