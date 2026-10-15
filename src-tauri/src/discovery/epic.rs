//! Epic 来源：读 `%PROGRAMDATA%\Epic\EpicGamesLauncher\Data\Manifests\*.item`。
//!
//! **实测状态（docs/auto-discovery-design.md §2.2）**：本机 Epic 启动器已安装但清单目录为空，
//! 字段结构依据公开资料（`DisplayName` / `InstallLocation` / `LaunchExecutable` / `bIsIncomplete`），
//! **未经本机真值验证**。因此解析器对一切字段按「可选 + 容错」处理，任何异常都跳过该条目，
//! 整体失败降级为来源不可用。

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{DiscoveredGame, DiscoveryConfidence, DiscoverySource, SourceScanResult, SourceState};

/// Epic 清单目录的默认位置（ProgramData 是机器级共享，启动器把清单写在这里）。
pub fn manifests_dir() -> Option<PathBuf> {
    let program_data = std::env::var_os("PROGRAMDATA")?;
    Some(
        PathBuf::from(program_data)
            .join("Epic")
            .join("EpicGamesLauncher")
            .join("Data")
            .join("Manifests"),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Item {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    install_location: Option<String>,
    #[serde(default)]
    launch_executable: Option<String>,
    #[serde(default)]
    launch_parameters: Option<Vec<String>>,
    #[serde(default, rename = "bIsIncomplete")]
    is_incomplete: bool,
}

/// 扫描 Epic 清单目录。
pub fn scan_epic(dir: Option<&Path>) -> SourceScanResult {
    let Some(dir) = dir.map(|p| p.to_path_buf()).or_else(manifests_dir) else {
        return SourceScanResult::not_installed(DiscoverySource::Epic);
    };
    let dir = dir.as_path();
    if !dir.is_dir() {
        // 启动器未安装（或从未装过任何游戏）
        return SourceScanResult::not_installed(DiscoverySource::Epic);
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            return SourceScanResult::unavailable(
                DiscoverySource::Epic,
                format!("无法读取清单目录 {}: {err}", dir.display()),
            );
        }
    };

    let mut candidates = Vec::new();
    let mut skipped = 0usize;

    let mut items: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("item"))
        .collect();
    items.sort();

    for item_path in items {
        let raw = match std::fs::read_to_string(&item_path) {
            Ok(raw) => raw,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };

        let item: Item = match serde_json::from_str(&raw) {
            Ok(item) => item,
            Err(err) => {
                // 清单损坏：跳过这一条，不影响其它
                log::warn!("Epic 清单损坏 {}: {err}", item_path.display());
                skipped += 1;
                continue;
            }
        };

        // 未下载完整 / 损坏的安装跳过（卸载残留保护）
        if item.is_incomplete {
            skipped += 1;
            continue;
        }

        let Some(name) = item.display_name.filter(|name| !name.trim().is_empty()) else {
            skipped += 1;
            continue;
        };

        let Some(exe) = item.launch_executable.filter(|exe| !exe.trim().is_empty()) else {
            skipped += 1;
            continue;
        };
        let Some(install) = item.install_location.filter(|dir| !dir.trim().is_empty()) else {
            skipped += 1;
            continue;
        };

        let launch_path = PathBuf::from(&install).join(&exe);
        if !launch_path.is_file() {
            // 卸载残留：清单还在、文件没了
            skipped += 1;
            continue;
        }

        let args = item
            .launch_parameters
            .unwrap_or_default()
            .join(" ")
            .trim()
            .to_owned();
        let launch_args = if args.is_empty() { None } else { Some(args) };

        candidates.push(DiscoveredGame {
            name,
            launch_path: launch_path.to_string_lossy().into_owned(),
            launch_args,
            working_directory: Some(install),
            sources: vec![DiscoverySource::Epic],
            confidence: DiscoveryConfidence::High,
            source_id: item_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned),
            already_in_library: false,
            warnings: Vec::new(),
        });
    }

    SourceScanResult {
        source: DiscoverySource::Epic,
        state: SourceState::Available,
        candidates,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_item(dir: &Path, file: &str, body: &str) {
        std::fs::write(dir.join(file), body).expect("写清单");
    }

    #[test]
    fn empty_dir_means_not_installed() {
        let dir = std::env::temp_dir().join(format!("gamelib-epic-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");

        let result = scan_epic(Some(&dir));
        assert!(
            matches!(result.state, SourceState::Available),
            "目录存在但为空 → 可用且无候选"
        );
        assert!(result.candidates.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_dir_means_not_installed() {
        let result = scan_epic(Some(Path::new(r"C://gamelib-epic-missing")));
        assert!(matches!(result.state, SourceState::NotInstalled));
    }

    #[test]
    fn valid_incomplete_and_corrupted_items() {
        let dir = std::env::temp_dir().join(format!("gamelib-epic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");

        // 1) 有效条目（exe 真实存在）
        let exe = dir.join("Game.exe");
        std::fs::write(&exe, b"mz").expect("写 exe");
        write_item(
            &dir,
            "good.item",
            r#"{"DisplayName":"Epic Game","InstallLocation":"__DIR__","LaunchExecutable":"Game.exe","LaunchParameters":["-log"],"bIsIncomplete":false}"#,
        );

        // 2) 未下载完整
        write_item(
            &dir,
            "incomplete.item",
            r#"{"DisplayName":"Ghost","InstallLocation":"C://x","LaunchExecutable":"g.exe","bIsIncomplete":true}"#,
        );

        // 3) 损坏的 JSON
        write_item(&dir, "broken.item", "{ not json");

        // 往 JSON 里插 Windows 路径必须把 \ 转义成 \\，否则 JSON 直接非法
        let body = std::fs::read_to_string(dir.join("good.item"))
            .expect("读回")
            .replace("__DIR__", &dir.to_string_lossy().replace('\\', "\\\\"));
        std::fs::write(dir.join("good.item"), body).expect("替换路径");

        let result = scan_epic(Some(&dir));
        assert!(matches!(result.state, SourceState::Available));
        assert_eq!(
            result.candidates.len(),
            1,
            "实际候选: {:?}",
            result.candidates
        );
        assert_eq!(result.candidates[0].name, "Epic Game");
        assert_eq!(result.candidates[0].launch_args.as_deref(), Some("-log"));
        assert_eq!(result.candidates[0].confidence, DiscoveryConfidence::High);
        assert_eq!(result.skipped, 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
