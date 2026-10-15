//! 指定目录来源：只扫描用户明确选择的目录。
//!
//! 铁律（对应需求 §3.D）：不扫整块硬盘；有深度 / 数量 / 时长上限；
//! 排除系统与非游戏目录；**所有候选都视为「需要确认」**，置信度不超过 `medium`。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{DiscoveredGame, DiscoveryConfidence, DiscoverySource, SourceScanResult, SourceState};

/// 默认上限：可在命令参数里覆盖。
#[derive(Debug, Clone)]
pub struct ScanLimits {
    /// 目录递归深度（CS2 的 exe 在第 4 层，所以这里给 4）。
    pub max_depth: usize,
    /// 单个扫描最多检查的文件数。
    pub max_files: usize,
    /// 最多产出的候选数（防止把整个目录都塞给用户）。
    pub max_candidates: usize,
    /// 总时长上限。
    pub timeout: Duration,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_depth: 4,
            max_files: 8000,
            max_candidates: 200,
            timeout: Duration::from_secs(60),
        }
    }
}

/// 目录 / 文件名黑名单（不区分大小写，子串匹配）。规则集中在这里便于维护。
const NAME_BLOCKLIST: &[&str] = &[
    // 系统与用户数据
    "windows",
    "program files",
    "programdata",
    "appdata",
    "$recycle.bin",
    "system volume information",
    "intel",
    "nvidia",
    "amd",
    "drivers",
    // 开发与工具目录
    "node_modules",
    "__pycache__",
    ".git",
    "venv",
    "cargo",
    "rustup",
    // 游戏运行库与安装器目录（装完游戏后用不上）
    "redist",
    "_commonredist",
    "commonredist",
    "directx",
    "dxsetup",
    "dotnet",
    "vcredist",
    "crashreporter",
    "crashhandler",
    "battleye",
    "easyanticheat",
    // 明显不是游戏的位置
    "steamapps\\steamapps",
    "tmp",
    "temp",
    "cache",
    "logs",
    "saves",
];

/// exe 文件名黑名单（子串匹配，不区分大小写）。
const EXE_BLOCKLIST: &[&str] = &[
    "unins",
    "setup",
    "installer",
    "redist",
    "vcredist",
    "dxsetup",
    "dotnet",
    "crashhandler",
    "crashreport",
    "crashpad",
    "unitycrashhandler",
    "eac",
    "easyanticheat",
    "battleye",
    "launcher",
    "update",
    "patcher",
    "helper",
    "toast",
    "notifier",
    "unpacked",
    "oop-base",
    "debug",
];

/// 扫描若干个用户指定的目录。
pub fn scan_directories(dirs: &[PathBuf], limits: &ScanLimits) -> SourceScanResult {
    if dirs.is_empty() {
        return SourceScanResult::not_installed(DiscoverySource::Directory);
    }

    let started = Instant::now();
    // walk 收集 exe 路径，由这里统一过滤与计数，避免借用纠缠
    let mut candidates = Vec::new();
    let mut visited_files = 0usize;

    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        let mut found: Vec<PathBuf> = Vec::new();
        walk(dir, 0, limits, &started, &mut visited_files, &mut found);

        for exe in found {
            if candidates.len() >= limits.max_candidates {
                break;
            }
            if let Some(game) = candidate_from_exe(&exe) {
                candidates.push(game);
            }
        }

        if candidates.len() >= limits.max_candidates
            || visited_files >= limits.max_files
            || started.elapsed() > limits.timeout
        {
            break;
        }
    }

    if candidates.is_empty() && visited_files >= limits.max_files {
        return SourceScanResult::unavailable(
            DiscoverySource::Directory,
            "扫描因文件数量达到上限而中止，未发现候选".to_owned(),
        );
    }

    SourceScanResult {
        source: DiscoverySource::Directory,
        state: SourceState::Available,
        candidates,
        skipped: 0,
    }
}

fn walk(
    dir: &Path,
    depth: usize,
    limits: &ScanLimits,
    started: &Instant,
    visited: &mut usize,
    out: &mut Vec<PathBuf>,
) {
    if depth > limits.max_depth
        || *visited >= limits.max_files
        || started.elapsed() > limits.timeout
    {
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        if *visited >= limits.max_files || started.elapsed() > limits.timeout {
            return;
        }
        *visited += 1;

        let path = entry.path();
        let raw_name = entry.file_name().to_string_lossy().into_owned();
        let lower = raw_name.to_lowercase();

        // 目录名黑名单：子串匹配
        if NAME_BLOCKLIST.iter().any(|bad| lower.contains(bad)) {
            continue;
        }

        if path.is_dir() {
            walk(&path, depth + 1, limits, started, visited, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("exe") {
            out.push(path);
        }
    }
}

/// 由一个 exe 造候选。目录来源的候选**一律需要用户确认**。
fn candidate_from_exe(exe: &Path) -> Option<DiscoveredGame> {
    let file_name = exe.file_name()?.to_str()?.to_lowercase();
    if EXE_BLOCKLIST.iter().any(|bad| file_name.contains(bad)) {
        return None;
    }

    let name = exe.file_stem()?.to_str()?.to_owned();
    if name.trim().is_empty() {
        return None;
    }

    // exe 名与所在目录名相近 → 稍高一点的置信度（游戏主程序通常与其目录同名）
    let dir_name = exe
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|parent| parent.to_str())
        .map(|value| value.replace([' ', '_', '-'], "").to_lowercase())
        .unwrap_or_default();
    let stem_key = name.replace([' ', '_', '-'], "").to_lowercase();

    let confidence = if !dir_name.is_empty() && stem_key == dir_name {
        DiscoveryConfidence::Medium
    } else {
        DiscoveryConfidence::Low
    };

    Some(DiscoveredGame {
        name,
        launch_path: exe.to_string_lossy().into_owned(),
        launch_args: None,
        working_directory: exe
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned()),
        sources: vec![DiscoverySource::Directory],
        confidence,
        source_id: None,
        already_in_library: false,
        warnings: vec!["按目录启发式发现，请确认它确实是游戏且是正确的启动程序".to_owned()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("gamelib-dir-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("建目录");
        root
    }

    #[test]
    fn directory_candidates_are_low_or_medium_and_needs_confirmation() {
        let root = make_root("conf");

        // exe 与目录同名 → medium
        let game_dir = root.join("MyGame");
        std::fs::create_dir_all(&game_dir).expect("建目录");
        std::fs::write(game_dir.join("MyGame.exe"), b"mz").expect("写 exe");

        // exe 与目录不同名 → low
        let other = root.join("SomeDir");
        std::fs::create_dir_all(&other).expect("建目录");
        std::fs::write(other.join("weird-name.exe"), b"mz").expect("写 exe");

        let result = scan_directories(std::slice::from_ref(&root), &ScanLimits::default());
        assert!(matches!(result.state, SourceState::Available));
        assert_eq!(result.candidates.len(), 2);

        let medium = result
            .candidates
            .iter()
            .find(|game| game.name == "MyGame")
            .expect("同名候选");
        assert_eq!(medium.confidence, DiscoveryConfidence::Medium);
        let low = result
            .candidates
            .iter()
            .find(|game| game.name == "weird-name")
            .expect("异名候选");
        assert_eq!(low.confidence, DiscoveryConfidence::Low);

        // 所有候选都必须提示需要确认
        assert!(
            result
                .candidates
                .iter()
                .all(|game| game.warnings.iter().any(|w| w.contains("确认")))
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn blocklisted_dirs_and_exes_are_skipped() {
        let root = make_root("block");

        // 目录名黑名单
        let redist = root.join("Redist");
        std::fs::create_dir_all(&redist).expect("建目录");
        std::fs::write(redist.join("dxsetup.exe"), b"mz").expect("写 exe");

        // 文件名黑名单
        let game_dir = root.join("Game");
        std::fs::create_dir_all(&game_dir).expect("建目录");
        std::fs::write(game_dir.join("Uninstall.exe"), b"mz").expect("写 exe");
        std::fs::write(game_dir.join("UnityCrashHandler64.exe"), b"mz").expect("写 exe");

        let result = scan_directories(&[root], &ScanLimits::default());
        assert!(
            result.candidates.is_empty(),
            "黑名单项不应成为候选: {:?}",
            result.candidates
        );
    }

    #[test]
    fn only_scans_directories_the_user_chose() {
        let chosen = make_root("chosen");
        let not_chosen = make_root("notchosen");

        std::fs::write(chosen.join("ChosenGame.exe"), b"mz").expect("写 exe");
        std::fs::write(not_chosen.join("OtherGame.exe"), b"mz").expect("写 exe");

        let result = scan_directories(std::slice::from_ref(&chosen), &ScanLimits::default());
        assert_eq!(result.candidates.len(), 1, "只扫用户选的目录");
        assert!(result.candidates[0].launch_path.contains("ChosenGame"));

        // 未选择的目录不在结果里
        assert!(
            !result
                .candidates
                .iter()
                .any(|game| game.launch_path.contains("OtherGame"))
        );

        let _ = std::fs::remove_dir_all(chosen);
        let _ = std::fs::remove_dir_all(not_chosen);
    }
}
