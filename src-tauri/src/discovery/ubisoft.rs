//! Ubisoft Connect 来源：解析本地配置缓存 + 注册表安装记录。
//!
//! 格式结论来自 2026-10-16 阶段一调查（docs/auto-discovery-design.md §10.2）：
//! - `%LOCALAPPDATA%\Ubisoft Game Launcher\cache\configuration\configurations`
//!   为 protobuf 信封内嵌的 **YAML 配置块序列**（实测 463 块）。解析采用内容扫描
//!   （`version: 2.0` → `localizations: {}`），不依赖顶层 protobuf framing。
//! - 有意义的字段：`root.sort_string` / `shortcut_name`（显示名）、
//!   `start_game.*.executables[].path.relative`（启动 exe 相对路径）。
//! - **实际安装判定**：`HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\<数字产品ID>\InstallDir`。
//!   注册表用数字产品 ID，YAML 用 GUID space_id，两者不直接对应 —— 关联靠
//!   「InstallDir 下存在某块声明的 exe」这一事实校验（见 scan_ubisoft）。
//!
//! 只读：不读凭据、不启动客户端、不修改平台文件。
//!
//! ⚠️ 实现教训：本文件的测试 fixture 一度把源码里的 `\r\n` 转义写成了字面量 `/r/n`
//! （工具链流转改写），导致解析全灭。CRLF 一律用数字码构造（`crlf()`），不要在源码里写转义。

use std::path::{Path, PathBuf};

use super::{DiscoveredGame, DiscoveryConfidence, DiscoverySource, SourceScanResult, SourceState};
use crate::platform::win::{RegRoot, registry_string, registry_subkeys};

/// 一段 YAML 配置块解析出的可执行定义。
#[derive(Debug, Clone)]
struct ExecutableDef {
    shortcut_name: Option<String>,
    relative: String,
}

/// 一段 YAML 配置块。
#[derive(Debug, Clone, Default)]
struct ConfigBlock {
    root_name: Option<String>,
    sort_string: Option<String>,
    executables: Vec<ExecutableDef>,
}

/// 从 configurations 字节流中提取全部 YAML 配置块。
///
/// 采用内容扫描而非严格 protobuf framing（阶段一实测顶层 framing 不规则）：
/// 每块以 `version: 2.0` 开始、以 `localizations: {}` 结束。
fn parse_configuration_blocks(data: &[u8]) -> Vec<ConfigBlock> {
    let mut blocks = Vec::new();
    for start in find_block_starts(data) {
        // 注意 "localizations: {}" 是 17 字节 —— 此前误写 windows(19) 导致永不匹配
        let end = find_sub(data, start, b"localizations: {}")
            .map(|offset| offset + 17)
            .unwrap_or_else(|| (start + 4000).min(data.len()));
        let text = String::from_utf8_lossy(&data[start..end]).into_owned();
        let parsed = parse_block(&text);
        blocks.push(parsed);
    }
    blocks
}

fn find_block_starts(data: &[u8]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut from = 0usize;
    while let Some(offset) = find_sub(data, from, b"version: 2.0") {
        starts.push(offset);
        from = offset + 12;
        if starts.len() > 4096 {
            break; // 防御
        }
    }
    starts
}

fn find_sub(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| from + offset)
}

fn parse_block(text: &str) -> ConfigBlock {
    let mut block = ConfigBlock::default();
    // executables 区块以内层缩进为界：shortcut_name / relative 同属一个条目
    let mut pending_shortcut: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        if let Some(value) = trimmed.strip_prefix("name: ") {
            // root.name 在 root: 的直接子层级（实测缩进 2；放宽到 ≤4 兼容变体）
            if indent <= 4 && block.root_name.is_none() {
                block.root_name = Some(value.trim_matches('"').to_owned());
            }
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("sort_string: ") {
            if indent <= 4 && block.sort_string.is_none() {
                block.sort_string = Some(value.trim_matches('"').to_owned());
            }
            continue;
        }
        let stripped = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        if let Some(value) = stripped.strip_prefix("shortcut_name: ") {
            pending_shortcut = Some(value.trim_matches('"').to_owned());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("relative: ") {
            let relative = value.trim().to_owned();
            if !relative.is_empty() {
                block.executables.push(ExecutableDef {
                    shortcut_name: pending_shortcut.take(),
                    relative,
                });
            }
            continue;
        }
        // 离开 executables 区块（出现更低缩进的新键）时丢弃未配对的 shortcut
        if indent <= 6 && !trimmed.is_empty() {
            pending_shortcut = None;
        }
    }
    block
}

/// 注册表中的育碧安装记录。
#[derive(Debug, Clone)]
pub struct UbiInstall {
    /// 注册表子键名（数字产品 ID，如 "40"）。
    pub product_id: String,
    /// InstallDir（已验证目录存在）。
    pub install_dir: PathBuf,
}

/// 从注册表枚举实际安装的育碧游戏（只读）。
pub fn detect_ubisoft_installs() -> Vec<UbiInstall> {
    const INSTALLS: &str = r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs";
    let mut installs = Vec::new();
    for product_id in registry_subkeys(RegRoot::LocalMachine, INSTALLS) {
        let subkey = format!("{INSTALLS}\\{product_id}");
        let Some(dir) = registry_string(RegRoot::LocalMachine, &subkey, "InstallDir") else {
            continue;
        };
        let dir = PathBuf::from(dir.trim());
        if dir.is_dir() {
            installs.push(UbiInstall {
                product_id,
                install_dir: dir,
            });
        }
    }
    installs
}

fn default_configurations_path() -> Option<PathBuf> {
    let local = std::env::var("LOCALAPPDATA").ok()?;
    let path = Path::new(&local).join(r"Ubisoft Game Launcher\cache\configuration\configurations");
    path.is_file().then_some(path)
}

/// 扫描 Ubisoft Connect 来源。
///
/// `configurations` 传 `None` 时使用默认位置；`installs` 由调用方注入
/// （生产走 `detect_ubisoft_installs()`，测试传合成数据）。
pub fn scan_ubisoft(configurations: Option<&Path>, installs: &[UbiInstall]) -> SourceScanResult {
    if installs.is_empty() {
        // 客户端可能装了但没有任何游戏安装（缓存里的配置块只是历史记录）
        return SourceScanResult::not_installed(DiscoverySource::Ubisoft);
    }
    let Some(path) = configurations
        .map(Path::to_path_buf)
        .or_else(default_configurations_path)
    else {
        return SourceScanResult::unavailable(
            DiscoverySource::Ubisoft,
            "未找到 Ubisoft Connect 的配置缓存（configurations）".to_owned(),
        );
    };
    let Ok(data) = std::fs::read(&path) else {
        return SourceScanResult::unavailable(
            DiscoverySource::Ubisoft,
            format!("无法读取配置缓存 {}", path.display()),
        );
    };
    let blocks = parse_configuration_blocks(&data);

    let mut candidates = Vec::new();
    let mut skipped = 0usize;

    for install in installs {
        let dir_name = install
            .install_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();

        // 关联方式（阶段一验证）：InstallDir 下真实存在某块声明的 exe → 该块即此安装的配置
        let mut valid: Vec<(String, PathBuf)> = Vec::new(); // (显示名, exe 全路径)
        for block in &blocks {
            for executable in &block.executables {
                let exe = install.install_dir.join(&executable.relative);
                if !exe.is_file() {
                    continue;
                }
                let name = executable
                    .shortcut_name
                    .clone()
                    .or_else(|| block.sort_string.clone())
                    .unwrap_or_else(|| dir_name.clone());
                if !valid.iter().any(|(_, existing)| *existing == exe) {
                    valid.push((name, exe));
                }
            }
        }

        if valid.is_empty() {
            // 安装目录存在但没有任何配置块声明可用的启动 exe：不伪造
            skipped += 1;
            continue;
        }

        let multi = valid.len() > 1;
        for (name, exe) in valid {
            // 同一安装目录出现多个有效 exe 时，把 exe 名附在显示名后供用户区分
            let display = if multi {
                let stem = exe
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                format!("{name}（{stem}）")
            } else {
                name.clone()
            };
            candidates.push(DiscoveredGame {
                name: display,
                launch_path: exe.to_string_lossy().into_owned(),
                launch_args: None,
                working_directory: Some(install.install_dir.to_string_lossy().into_owned()),
                sources: vec![DiscoverySource::Ubisoft],
                confidence: DiscoveryConfidence::High,
                source_id: Some(install.product_id.clone()),
                already_in_library: false,
                warnings: Vec::new(),
            });
        }
    }

    SourceScanResult {
        source: DiscoverySource::Ubisoft,
        state: SourceState::Available,
        candidates,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成 fixture（标注：模拟数据，非真实 configurations）。
    /// 结构按阶段一实测的 YAML 字段构造。
    /// 构造 CRLF：用数字码而非源码转义（工具链流转会改写 \r\n 转义，教训见实现说明）。
    fn crlf() -> String {
        let mut out = String::new();
        out.push(13u8 as char);
        out.push(10u8 as char);
        out
    }

    fn block(name: &str, shortcut: &str, relative: &str) -> String {
        let crlf = crlf();
        format!(
            "version: 2.0{crlf}root:{crlf}  name: {name}{crlf}  sort_string: {name}{crlf}             start_game:{crlf}    executables:{crlf}- shortcut_name: {shortcut}{crlf}             path:{crlf}        relative: {relative}{crlf}localizations: {{}}{crlf}",
            crlf = crlf
        )
    }

    #[test]
    fn parses_blocks_from_synthetic_data() {
        let mut data = block("Assassin's Creed 04", "AC Revelations", "ACR.exe");
        data += &block("Another Game", "Another", "another.exe");
        let blocks = parse_configuration_blocks(data.as_bytes());
        assert_eq!(blocks.len(), 2);
        assert_eq!(
            blocks[0].sort_string.as_deref(),
            Some("Assassin's Creed 04")
        );
        assert_eq!(blocks[0].executables.len(), 1);
        assert_eq!(blocks[0].executables[0].relative, "ACR.exe");
    }

    #[test]
    fn scan_associates_registry_install_with_config_exe() {
        let dir = std::env::temp_dir().join("gamelib-ubi-test");
        let install = dir.join("games").join("AC Revelations");
        std::fs::create_dir_all(&install).expect("建目录");
        let exe = install.join("ACR.exe");
        std::fs::write(&exe, b"stub").expect("写占位 exe");

        let configurations = format!(
            "{}{}",
            block("Assassin's Creed 04", "AC Revelations", "ACR.exe"),
            block("Other Game", "Other", "other.exe")
        );
        let config_path = dir.join("configurations");
        std::fs::write(&config_path, configurations).expect("写 fixture");

        let installs = vec![UbiInstall {
            product_id: "40".to_owned(),
            install_dir: install.clone(),
        }];

        let result = scan_ubisoft(Some(&config_path), &installs);
        assert!(matches!(result.state, SourceState::Available));
        assert_eq!(result.candidates.len(), 1, "只有 ACR.exe 与安装目录匹配");
        assert_eq!(result.candidates[0].name, "AC Revelations");
        assert_eq!(result.candidates[0].source_id.as_deref(), Some("40"));
    }

    #[test]
    fn multiple_valid_exes_all_listed_with_suffix() {
        let dir = std::env::temp_dir().join("gamelib-ubi-multi-test");
        let install = dir.join("games").join("Some Game");
        std::fs::create_dir_all(&install).expect("建目录");
        std::fs::write(install.join("Main.exe"), b"stub").expect("写 exe");
        std::fs::write(install.join("Launcher.exe"), b"stub").expect("写 exe");

        let configurations = block("Some Game", "Some Game", "Main.exe")
            + &block("Some Game", "Some Game", "Launcher.exe");
        let config_path = dir.join("configurations");
        std::fs::write(&config_path, configurations).expect("写 fixture");

        let installs = vec![UbiInstall {
            product_id: "1".to_owned(),
            install_dir: install,
        }];

        let result = scan_ubisoft(Some(&config_path), &installs);
        assert_eq!(
            result.candidates.len(),
            2,
            "多个有效 exe 全部列出交用户确认"
        );
        assert!(
            result.candidates[0].name != result.candidates[1].name,
            "显示名应带 exe 后缀区分"
        );
    }

    #[test]
    fn uninstall_leftover_dir_produces_skipped_not_candidate() {
        let dir = std::env::temp_dir().join("gamelib-ubi-leftover-test");
        std::fs::create_dir_all(&dir).expect("建目录");
        let configurations = block("Gone Game", "Gone Game", "gone.exe");
        let config_path = dir.join("configurations");
        std::fs::write(&config_path, configurations).expect("写 fixture");

        // 注册表记录存在，但 InstallDir 下没有该 exe（卸载残留）
        let installs = vec![UbiInstall {
            product_id: "7".to_owned(),
            install_dir: dir.join("games").join("Gone Game"),
        }];

        let result = scan_ubisoft(Some(&config_path), &installs);
        assert!(matches!(result.state, SourceState::Available));
        assert!(result.candidates.is_empty(), "卸载残留不得产出候选");
        assert!(result.skipped >= 1);
    }

    #[test]
    fn no_installs_reports_not_installed() {
        // 实测场景：客户端在、缓存里有 463 个配置块，但注册表零安装
        let dir = std::env::temp_dir().join("gamelib-ubi-none-test");
        std::fs::create_dir_all(&dir).expect("建目录");
        let config_path = dir.join("configurations");
        std::fs::write(&config_path, block("X", "X", "x.exe")).expect("写 fixture");

        let result = scan_ubisoft(Some(&config_path), &[]);
        assert!(matches!(result.state, SourceState::NotInstalled));
    }

    #[test]
    fn non_ascii_paths_work() {
        let dir = std::env::temp_dir().join("gamelib-ubi-中文路径测试");
        let install = dir.join("游戏").join("子目录");
        std::fs::create_dir_all(&install).expect("建目录");
        let exe = install.join("游戏.exe");
        std::fs::write(&exe, b"stub").expect("写 exe");

        let crlf = crlf();
        let configurations = format!(
            "version: 2.0{crlf}root:{crlf}  name: 某游戏{crlf}  start_game:{crlf}    executables:{crlf}- shortcut_name: 某游戏{crlf}      path:{crlf}        relative: 游戏.exe{crlf}localizations: {{}}{crlf}",
            crlf = crlf
        );
        let config_path = dir.join("configurations");
        std::fs::write(&config_path, configurations).expect("写 fixture");

        let installs = vec![UbiInstall {
            product_id: "9".to_owned(),
            install_dir: install,
        }];

        let result = scan_ubisoft(Some(&config_path), &installs);
        assert_eq!(result.candidates.len(), 1, "中文路径应正常关联");
        assert!(result.candidates[0].launch_path.contains("游戏.exe"));
    }
}
