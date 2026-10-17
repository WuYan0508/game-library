//! AMD Software / Adrenalin 来源：**降级实现**。
//!
//! 阶段一调查（2026-10-16，docs/auto-discovery-design.md §10.1）：开发机未安装
//! AMD Software，其本地游戏配置格式**未经实机验证** —— 按实施要求，在拿到可验证
//! 的真实格式样本之前，本来源只做「安装痕迹探测 + 明确的不可用报告」，
//! 不实现任何格式解析，不产出候选，不伪造路径。
//!
//! 只读：不安装驱动、不绕过安全策略、不联网、不修改 AMD 软件数据。

use std::path::{Path, PathBuf};

use super::{DiscoverySource, SourceScanResult};

/// 阶段一排查过的 AMD Software 常见数据目录（只读探测）。
fn default_amd_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(program_files) = std::env::var("ProgramFiles") {
        dirs.push(PathBuf::from(&program_files).join("AMD"));
    }
    if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") {
        dirs.push(PathBuf::from(&program_files_x86).join("AMD"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        dirs.push(Path::new(&local).join("AMD"));
    }
    if let Ok(program_data) = std::env::var("PROGRAMDATA") {
        dirs.push(Path::new(&program_data).join("AMD"));
    }
    if let Some(system_drive) = std::env::var("SystemDrive")
        .ok()
        .map(|drive| drive.trim_end_matches('\\').to_owned())
    {
        dirs.push(PathBuf::from(format!("{system_drive}\\AMD")));
    }
    dirs.into_iter().filter(|dir| dir.is_dir()).collect()
}

/// 扫描 AMD 来源。
///
/// `detected_dirs` 传 `None` 时使用默认探测位置；测试注入合成目录。
/// 无 AMD 数据目录 → 「未安装」；有目录但格式未验证 → 明确的不可用报告（不产出候选）。
pub fn scan_amd(detected_dirs: Option<&[PathBuf]>) -> SourceScanResult {
    let dirs: Vec<PathBuf> = match detected_dirs {
        Some(dirs) => dirs.to_vec(),
        None => default_amd_dirs(),
    };

    if dirs.is_empty() {
        // 本机没有 AMD Software：不是故障，是正常状态
        return SourceScanResult::not_installed(DiscoverySource::Amd);
    }

    // 检测到安装痕迹，但配置格式未经实机验证（阶段一 §10.1）。
    // 不得猜测格式产出候选 —— 明确报告并引导用户使用其他导入方式。
    SourceScanResult::unavailable(
        DiscoverySource::Amd,
        "检测到 AMD Software 安装痕迹，但其本地游戏配置格式尚未实机验证，暂不支持自动发现；请使用目录扫描或手动导入".to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::SourceState;

    /// 合成 fixture（标注：模拟数据）。
    #[test]
    fn no_dirs_reports_not_installed() {
        let result = scan_amd(Some(&[]));
        assert!(matches!(result.state, SourceState::NotInstalled));
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn detected_dirs_report_unavailable_not_error() {
        let dir = std::env::temp_dir().join("gamelib-amd-test");
        std::fs::create_dir_all(&dir).expect("建目录");
        let result = scan_amd(Some(&[dir]));
        match &result.state {
            SourceState::Unavailable { reason } => {
                assert!(reason.contains("尚未实机验证"), "报告应说明原因: {reason}");
            }
            other => panic!("应报告 Unavailable，实际 {other:?}"),
        }
        assert!(result.candidates.is_empty(), "格式未验证，不得产出候选");
    }

    #[test]
    fn synthetic_data_never_produces_candidates() {
        // 即便目录里有疑似配置文件，未验证格式也不解析
        let dir = std::env::temp_dir().join("gamelib-amd-content-test");
        std::fs::create_dir_all(dir.join("CN")).expect("建目录");
        std::fs::write(dir.join("CN").join("some_config.xml"), b"<game/>").expect("写文件");
        let result = scan_amd(Some(&[dir]));
        assert!(result.candidates.is_empty());
    }
}
