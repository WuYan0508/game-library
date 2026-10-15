//! Steam 来源：`libraryfolders.vdf` 定位库目录，`appmanifest_<appid>.acf` 读游戏清单。
//!
//! **实测结论（见 docs/auto-discovery-design.md §2.1）**：`name` / `installdir` / `StateFlags`
//! 可靠，但**启动 exe 的位置千差万别**（巫师 3 在第 3 层、CS2 在第 4 层、旁边还有
//! `REDprelauncher.exe` / `UnityCrashHandler64.exe` 干扰项），因此这里做的是
//! 「有限深度的启发式猜测」，猜不到的条目计入 `skipped`，不硬造候选。

use std::path::{Path, PathBuf};

use super::vdf;
use super::{DiscoveredGame, DiscoveryConfidence, DiscoverySource, SourceScanResult, SourceState};

/// 已知不是游戏的 Steam 应用（Steamworks 公共可再分发组件，本机实测存在）。
const REDIST_APPIDS: &[&str] = &["228980"];

/// exe 扫描的最大目录深度（CS2 的 `game\bin\win64\cs2.exe` 在第 4 层）。
const MAX_EXE_DEPTH: usize = 4;

/// 单个安装目录最多检查的 exe 数量，防止扫爆巨型目录。
const MAX_EXES_PER_GAME: usize = 400;

/// 明显不是游戏主程序的 exe 名片段（不区分大小写）。
const EXE_BLOCKLIST: &[&str] = &[
    "redist",
    "dxsetup",
    "vcredist",
    "directx",
    "dotnet",
    "crashhandler",
    "crashreport",
    "crashpad",
    "unins",
    "setup",
    "installer",
    "eac",
    "easyanticheat",
    "battleye",
    "launcher",
    // 真实数据里踩到的：巫师 3 重制版被选成了 D3D12StateObjectCompiler.exe
    "d3d12",
    "shader",
    "compiler",
    "stateobject",
    "editor",
    "tools",
];

/// 从注册表与常见安装位置推断 Steam 根目录。
pub fn detect_steam_root() -> Option<PathBuf> {
    use crate::platform::win::{RegRoot, registry_string};

    // 1) 注册表（覆盖非默认安装位置；本机是 D:\Steam）
    for (root, subkey, value) in [
        (RegRoot::CurrentUser, r"Software\Valve\Steam", "SteamPath"),
        (
            RegRoot::LocalMachine,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
    ] {
        if let Some(path) = registry_string(root, subkey, value)
            && !path.trim().is_empty()
        {
            let candidate = PathBuf::from(path.trim());
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
    }

    // 2) 常见安装位置兜底
    let common = [r"C:\Program Files (x86)\Steam", r"C:\Program Files\Steam"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_dir());
    if common.is_some() {
        return common;
    }

    // 3) 注册表与常见位置都落空时的兜底：逐个盘根检查 Steam 的标志性文件。
    //    只查每个盘根下 2 个固定路径，不递归扫描，开销可忽略；
    //    只认「确实有 steamapps\libraryfolders.vdf」的目录，不凭盘符猜。
    for drive in drive_roots() {
        for candidate in [drive.join("Steam"), drive.join("SteamLibrary")] {
            if candidate
                .join("steamapps")
                .join("libraryfolders.vdf")
                .is_file()
            {
                return Some(candidate);
            }
        }
    }

    None
}

/// 当前机器上存在的盘根（A~Z 逐个探测 `X:\` 是否存在）。
fn drive_roots() -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .map(|letter| PathBuf::from(format!("{}:\\", letter as char)))
        .filter(|root| root.is_dir())
        .collect()
}

/// 读 `libraryfolders.vdf`，返回全部库根目录（含 Steam 自身目录）。
pub fn library_roots(steam_root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let vdf_path = steam_root.join("steamapps").join("libraryfolders.vdf");
    let Ok(text) = std::fs::read_to_string(&vdf_path) else {
        // 读不到就退回 Steam 根目录本身（老版本 Steam 的游戏就在根下）
        return vec![steam_root.to_path_buf()];
    };

    if let Ok(parsed) = vdf::parse(&text)
        && let Some(folders) = vdf::get_table(&parsed, "libraryfolders")
    {
        for (_, value) in folders {
            if let Some(table) = value.as_table()
                && let Some(path) = vdf::get_str(table, "path")
            {
                let root = PathBuf::from(path);
                if root.is_dir() {
                    roots.push(root);
                }
            }
        }
    }

    if roots.is_empty() {
        roots.push(steam_root.to_path_buf());
    }
    roots
}

/// 一个 appmanifest 的关键字段。
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub appid: String,
    pub name: String,
    pub installdir: String,
    pub state_flags: u32,
}

/// 解析单个 appmanifest 文本。缺关键字段的条目返回 `None`（损坏清单）。
pub fn parse_manifest(text: &str) -> Option<Manifest> {
    let parsed = vdf::parse(text).ok()?;
    let state = vdf::get_table(&parsed, "AppState")?;

    let appid = vdf::get_str(state, "appid")?.to_owned();
    let name = vdf::get_str(state, "name")?.to_owned();
    let installdir = vdf::get_str(state, "installdir")?.to_owned();
    let state_flags = vdf::get_str(state, "StateFlags")?.parse::<u32>().ok()?;

    Some(Manifest {
        appid,
        name,
        installdir,
        state_flags,
    })
}

/// 扫描 Steam 全部库目录。
pub fn scan_steam(steam_root: Option<&Path>) -> SourceScanResult {
    let Some(steam_root) = steam_root else {
        return SourceScanResult::not_installed(DiscoverySource::Steam);
    };
    if !steam_root.is_dir() {
        return SourceScanResult::not_installed(DiscoverySource::Steam);
    }

    let mut candidates = Vec::new();
    let mut skipped = 0usize;

    for root in library_roots(steam_root) {
        let apps_dir = root.join("steamapps");
        let entries = match std::fs::read_dir(&apps_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        let mut manifests: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("appmanifest_") && name.ends_with(".acf"))
            })
            .collect();
        manifests.sort();

        for manifest_path in manifests {
            let text = match std::fs::read_to_string(&manifest_path) {
                Ok(text) => text,
                Err(err) => {
                    log::warn!("读取 Steam 清单失败 {}: {err}", manifest_path.display());
                    skipped += 1;
                    continue;
                }
            };

            let Some(manifest) = parse_manifest(&text) else {
                log::warn!("Steam 清单无法解析: {}", manifest_path.display());
                skipped += 1;
                continue;
            };

            // StateFlags 4 = 完整安装；更新中 / 需要更新 / 损坏的跳过（卸载残留保护）
            if manifest.state_flags != 4 {
                skipped += 1;
                continue;
            }
            if REDIST_APPIDS.contains(&manifest.appid.as_str()) {
                skipped += 1;
                continue;
            }

            let install_dir = root
                .join("steamapps")
                .join("common")
                .join(&manifest.installdir);
            if !install_dir.is_dir() {
                // 卸载残留：清单还在、目录没了
                skipped += 1;
                continue;
            }

            let (exe, exe_warning) = guess_launch_exe(&install_dir, &manifest);
            let Some(launch_path) = exe else {
                // 找不到 exe：不硬造候选，计入 skipped（来源状态里可见）
                skipped += 1;
                continue;
            };

            let mut warnings = Vec::new();
            if let Some(warning) = exe_warning {
                warnings.push(warning);
            }
            if manifest.name.starts_with("appid_") {
                warnings.push("Steam 未提供该应用的名称，只有占位串".to_owned());
            }

            // 工作目录 = exe 所在目录（与资源管理器双击一致）。
            // 实测巫师 3：工作目录设为游戏根目录会导致进程静默退出（退出码 0），
            // 只有以 bin\x64_dx12 这类 exe 自己的目录为 cwd 才能正常启动。
            let working_directory = Path::new(&launch_path)
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned());

            candidates.push(DiscoveredGame {
                name: manifest.name,
                launch_path,
                launch_args: None,
                working_directory,
                sources: vec![DiscoverySource::Steam],
                confidence: if warnings.is_empty() {
                    DiscoveryConfidence::Medium
                } else {
                    DiscoveryConfidence::Low
                },
                source_id: Some(manifest.appid),
                already_in_library: false,
                warnings,
            });
        }
    }

    SourceScanResult {
        source: DiscoverySource::Steam,
        state: SourceState::Available,
        candidates,
        skipped,
    }
}

/// 在安装目录里启发式地猜启动 exe。
///
/// 返回 `(候选路径, 警告)`；找不到返回 `(None, None)`。
fn guess_launch_exe(install_dir: &Path, manifest: &Manifest) -> (Option<String>, Option<String>) {
    let mut exes: Vec<PathBuf> = Vec::new();
    collect_exes(install_dir, 0, &mut exes);
    if exes.is_empty() {
        return (None, None);
    }

    let dir_key = manifest
        .installdir
        .replace([' ', '_', '-'], "")
        .to_lowercase();
    let name_key = manifest
        .name
        .replace([' ', '_', '-', ':', '—'], "")
        .to_lowercase();

    let mut best: Option<(PathBuf, u32)> = None;
    let mut found_count = 0usize;

    for exe in exes {
        found_count += 1;
        let Some(stem) = exe.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let stem_key = stem.replace([' ', '_', '-'], "").to_lowercase();

        // 命中黑名单的直接出局
        if EXE_BLOCKLIST
            .iter()
            .any(|bad| stem_key.contains(bad) || stem.to_lowercase().contains(bad))
        {
            continue;
        }

        let mut score = 1u32;
        if !dir_key.is_empty() && stem_key == dir_key {
            score += 10;
        } else if !dir_key.is_empty() && dir_key.contains(&stem_key) {
            // 目录名包含 exe 名（如 TheWitcher3 ⊃ witcher3）—— 主程序通常如此
            score += 6;
        }
        if !name_key.is_empty() && stem_key.contains(&name_key) {
            score += 6;
        }
        // 根目录的 exe 通常比 bin/ 下的更像主程序（Unity 游戏正是如此）
        if exe.parent() == Some(install_dir) {
            score += 3;
        }

        if best.is_none() || score > best.as_ref().map_or(0, |(_, s)| *s) {
            best = Some((exe, score));
        }
    }

    let Some((path, _score)) = best else {
        return (None, None);
    };

    let warning = if found_count > 1 {
        Some(format!(
            "安装目录里发现 {found_count} 个可执行文件，已选择最像主程序的：{}",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
        ))
    } else {
        None
    };

    (Some(path.to_string_lossy().into_owned()), warning)
}

fn collect_exes(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_EXE_DEPTH || out.len() >= MAX_EXES_PER_GAME {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // 常见非游戏目录直接跳过
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if matches!(
                name.as_str(),
                "redist"
                    | "_commonredist"
                    | "commonredist"
                    | "directx"
                    | "dotnet"
                    | "dxsetup"
                    | "vcredist"
                    | "binaries"
                    | "crashreporter"
                    | "__macosx"
            ) {
                continue;
            }
            collect_exes(&path, depth + 1, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("exe") {
            out.push(path);
            if out.len() >= MAX_EXES_PER_GAME {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在临时目录里搭一个假的 Steam 库。
    /// 返回 (根目录, 清理函数)。
    fn make_steam_root(tag: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("gamelib-steam-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("steamapps")).expect("建 steamapps");
        (root.clone(), root)
    }

    fn write_manifest(root: &Path, appid: &str, name: &str, installdir: &str, state: &str) {
        let text = format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"name\"\t\t\"{name}\"\n\t\"installdir\"\t\t\"{installdir}\"\n\t\"StateFlags\"\t\t\"{state}\"\n}}\n"
        );
        std::fs::write(
            root.join("steamapps")
                .join(format!("appmanifest_{appid}.acf")),
            text,
        )
        .expect("写清单");
    }

    #[test]
    fn parses_real_world_manifest_shape() {
        let manifest = parse_manifest(
            "\"AppState\"\n{\n\t\"appid\"\t\t\"292030\"\n\t\"name\"\t\t\"The Witcher 3 — Remastered\"\n\t\"installdir\"\t\t\"The Witcher 3\"\n\t\"StateFlags\"\t\t\"4\"\n}\n",
        )
        .expect("解析");
        assert_eq!(manifest.appid, "292030");
        assert_eq!(manifest.state_flags, 4);
        assert_eq!(manifest.installdir, "The Witcher 3");
    }

    #[test]
    fn skips_redist_uninstalled_and_incomplete_and_warns_on_placeholder() {
        let (root, _keep) = make_steam_root("filter");
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"{ROOT}\"\n\t}\n}"
            .replace("{ROOT}", &root.to_string_lossy().replace('\\', "\\\\"));
        std::fs::write(root.join("steamapps").join("libraryfolders.vdf"), vdf).expect("写 vdf");

        // 1) 正常游戏（exe 在根目录）
        let good_dir = root.join("steamapps").join("common").join("Hollow Knight");
        std::fs::create_dir_all(&good_dir).expect("建目录");
        std::fs::write(good_dir.join("hollow_knight.exe"), b"mz").expect("写 exe");
        write_manifest(&root, "367520", "Hollow Knight", "Hollow Knight", "4");

        // 2) Steamworks 公共组件（appid 黑名单）
        write_manifest(
            &root,
            "228980",
            "Steamworks Common Redistributables",
            "Steamworks Shared",
            "4",
        );

        // 3) 更新中（StateFlags != 4）
        write_manifest(&root, "730", "Counter-Strike 2", "CS2", "2");

        // 4) 卸载残留（清单在、目录没了）
        write_manifest(&root, "1", "Uninstalled Game", "Ghost Dir", "4");

        // 5) 名称占位串（appid_431960）
        let we_dir = root
            .join("steamapps")
            .join("common")
            .join("wallpaper_engine");
        std::fs::create_dir_all(&we_dir).expect("建目录");
        std::fs::write(we_dir.join("wallpaper32.exe"), b"mz").expect("写 exe");
        write_manifest(&root, "431960", "appid_431960", "wallpaper_engine", "4");

        let result = scan_steam(Some(&root));
        assert!(matches!(result.state, SourceState::Available));

        assert_eq!(result.candidates.len(), 2, "只应剩正常游戏 + 占位名游戏");
        let names: Vec<&str> = result
            .candidates
            .iter()
            .map(|game| game.name.as_str())
            .collect();
        assert!(names.contains(&"Hollow Knight"));
        assert!(names.contains(&"appid_431960"));

        // 卸载残留 / 更新中 / 黑名单 appid 都进了 skipped
        assert_eq!(result.skipped, 3, "实际 skipped = {}", result.skipped);

        // 占位名要带警告
        let placeholder = result
            .candidates
            .iter()
            .find(|game| game.name == "appid_431960")
            .expect("占位名候选");
        assert!(
            placeholder.warnings.iter().any(|w| w.contains("占位")),
            "应有占位名警告: {:?}",
            placeholder.warnings
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn exe_guess_prefers_name_match_and_flags_multiple() {
        let (root, _keep) = make_steam_root("guess");
        let game_dir = root.join("steamapps").join("common").join("The Witcher 3");
        std::fs::create_dir_all(game_dir.join("bin").join("x64_dx12")).expect("建目录");
        std::fs::write(
            game_dir.join("bin").join("x64_dx12").join("witcher3.exe"),
            b"mz",
        )
        .expect("写 exe");
        std::fs::write(game_dir.join("REDprelauncher.exe"), b"mz").expect("写干扰 exe");

        let (exe, warning) = guess_launch_exe(
            &game_dir,
            &Manifest {
                appid: "292030".to_owned(),
                name: "The Witcher 3".to_owned(),
                installdir: "The Witcher 3".to_owned(),
                state_flags: 4,
            },
        );

        let exe = exe.expect("应猜到 exe");
        assert!(
            exe.ends_with("witcher3.exe"),
            "应选中 witcher3.exe 而不是 REDprelauncher.exe：{exe}"
        );
        assert!(warning.is_some(), "多个 exe 时应有警告");
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod unicode_path_tests {
    use super::*;

    /// 很多中文系统的用户名是中文（如 C://Users//张三//...），启动器清单也可能装在
    /// 带空格的目录里。Steam 根路径带中文与空格时必须照常工作。
    #[test]
    fn steam_root_with_chinese_and_spaces_works() {
        let root = std::env::temp_dir()
            .join("游戏库 测试")
            .join("Steam 启动器");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("steamapps")).expect("建目录");

        let vdf = format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}",
            // vdf 里反斜杠要转义成 \\
            root.to_string_lossy().replace('\\', "\\\\")
        );
        std::fs::write(root.join("steamapps").join("libraryfolders.vdf"), vdf).expect("写 vdf");

        let game_dir = root
            .join("steamapps")
            .join("common")
            .join("黑暗之魂 REMASTERED");
        std::fs::create_dir_all(&game_dir).expect("建目录");
        std::fs::write(game_dir.join("DarkSoulsRemastered.exe"), b"mz").expect("写 exe");

        let text = format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\t\"570940\"\n\t\"name\"\t\t\"DARK SOULS: REMASTERED\"\n\t\"installdir\"\t\t\"黑暗之魂 REMASTERED\"\n\t\"StateFlags\"\t\t\"4\"\n}}\n"
        );
        std::fs::write(root.join("steamapps").join("appmanifest_570940.acf"), text)
            .expect("写清单");

        let result = scan_steam(Some(&root));
        assert!(matches!(result.state, SourceState::Available));
        assert_eq!(result.candidates.len(), 1, "实际: {:?}", result.candidates);
        assert!(result.candidates[0].launch_path.contains("黑暗之魂"));
        assert_eq!(result.candidates[0].name, "DARK SOULS: REMASTERED");

        let _ = std::fs::remove_dir_all(&root);
    }
}
