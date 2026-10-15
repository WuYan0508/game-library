//! `.lnk` 快捷方式解析。
//!
//! 用 Windows Shell Link（`IShellLinkW`）读取，行为与系统一致；
//! COM 调用统一走 `platform::win::run_sta` 的专用 STA 线程。
//!
//! 两条铁律：
//! 1. **绝不把 `.lnk` 文件本身当作启动目标**；
//! 2. 目标不存在时**不静默导入**，而是把「目标缺失」如实回传，由用户决定取消或手动修正。

use std::path::{Path, PathBuf};

use serde::Serialize;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ,
};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::UI::Shell::{IShellLinkW, SLGP_RAWPATH, ShellLink};
use windows::core::{Interface, PCWSTR};

use crate::error::{AppError, AppResult};
use crate::platform::win::{from_wide, run_sta, strip_verbatim_prefix, to_wide};

/// 路径缓冲。`GetPath` 等接口按 NUL 结尾返回，取足够大的定长缓冲避免截断。
const PATH_BUFFER_LEN: usize = 1024;
/// 参数串可能比路径长得多。
const ARGS_BUFFER_LEN: usize = 4096;

/// 已知平台启动器的可执行文件名（小写、不含扩展名）。
///
/// 命中后 `is_launcher = true`：这类目标往往是「启动器」而非游戏本体，
/// 会影响进程跟踪的准确性，需要如实告知用户而不是假装计时完全准确。
const KNOWN_LAUNCHER_STEMS: [&str; 10] = [
    "steam",
    "steamwebhelper",
    "epicgameslauncher",
    "ubisoftconnect",
    "uplay",
    "eadesktop",
    "origin",
    "battle.net",
    "galaxyclient",
    "riotclientservices",
];

/// 快捷方式解析结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutProbe {
    /// 用户选择的 `.lnk` 路径。
    pub source_path: String,
    /// 解析出的启动目标。目标缺失时这里仍是「原本应指向的路径」，供用户判断与手动修正。
    pub launch_path: String,
    /// 从快捷方式读到的**原样参数串**，不分词、不重新转义。
    pub launch_args: String,
    /// 工作目录；快捷方式未设置时回退到目标所在目录。
    pub working_directory: String,
    /// 显示名：`.lnk` 文件名 → 目标文件名。
    pub default_name: String,
    /// 目标疑似平台启动器。
    pub is_launcher: bool,
    /// 目标文件当前是否存在。为 false 时前端必须让用户「取消 / 手动选择启动目标」。
    pub target_exists: bool,
}

/// COM 读回的原始字段（未做任何加工）。
struct RawLink {
    target: String,
    args: String,
    working_directory: String,
}

/// 解析 `.lnk`。
pub fn resolve_shortcut(raw_path: &str) -> AppResult<ShortcutProbe> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_argument("没有选择文件"));
    }

    let path = Path::new(trimmed);
    let is_lnk = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"));
    if !is_lnk {
        return Err(AppError::invalid_argument("请选择 .lnk 快捷方式"));
    }

    let metadata = std::fs::metadata(path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => AppError::not_found("快捷方式不存在或已被移动，请重新选择")
            .with_detail(format!("{trimmed}: {err}")),
        std::io::ErrorKind::PermissionDenied => {
            AppError::permission_denied("无法读取该快捷方式，可能没有访问权限")
                .with_detail(format!("{trimmed}: {err}"))
        }
        _ => AppError::internal("读取快捷方式信息失败").with_detail(format!("{trimmed}: {err}")),
    })?;

    if !metadata.is_file() {
        return Err(AppError::invalid_argument("请选择 .lnk 文件，而不是文件夹"));
    }

    let raw = read_link(trimmed.to_owned())?;

    // 快捷方式里可能存的是 `%ProgramFiles%\...` 这类未展开的路径（用了 SLGP_RAWPATH，
    // 正是为了不触发 Shell 的链接解析），这里手动展开。
    let expanded = expand_env_vars(&raw.target);

    // 相对路径按 `.lnk` 所在目录解析
    let absolute = absolutize(&expanded, path);

    // 目标存在时做一次规范化：既得到长路径，也让「导入 EXE」与「导入同目标的 .lnk」
    // 产出同一个去重键。canonicalize 的 verbatim 前缀（`\\?\`）要剥掉 ——
    // 否则展示难看、Shell API 也不认。
    let launch_path = match &absolute {
        Some(target) => std::fs::canonicalize(target)
            .ok()
            .map(|canonical| strip_verbatim_prefix(&canonical.to_string_lossy()))
            .unwrap_or_else(|| target.to_string_lossy().into_owned()),
        None => expanded.clone(),
    };

    let target_exists = absolute.as_ref().is_some_and(|target| target.exists());

    let working_directory = if raw.working_directory.trim().is_empty() {
        absolute
            .as_ref()
            .and_then(|target| target.parent())
            .map(|parent| strip_verbatim_prefix(&parent.to_string_lossy()))
            .unwrap_or_default()
    } else {
        expand_env_vars(&raw.working_directory)
    };

    Ok(ShortcutProbe {
        source_path: trimmed.to_owned(),
        default_name: default_name(path, &launch_path),
        is_launcher: is_launcher_target(&launch_path),
        launch_args: raw.args,
        working_directory,
        launch_path,
        target_exists,
    })
}

/// 通过 `IShellLinkW` 读取快捷方式。COM 调用整体放在专用 STA 线程内。
fn read_link(lnk_path: String) -> AppResult<RawLink> {
    run_sta(move || {
        let wide_path = to_wide(&lnk_path);

        // SAFETY: 已在本线程初始化 STA apartment；所有接口指针在作用域结束前有效。
        unsafe {
            let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|err| {
                    AppError::internal("无法创建 Shell Link 对象").with_detail(err.to_string())
                })?;

            let persist: IPersistFile = shell_link.cast().map_err(|err| {
                AppError::internal("快捷方式接口不可用").with_detail(err.to_string())
            })?;

            persist
                .Load(PCWSTR(wide_path.as_ptr()), STGM_READ)
                .map_err(|err| {
                    AppError::invalid_argument("无法读取该快捷方式，文件可能已损坏")
                        .with_detail(format!("{lnk_path}: {err}"))
                })?;

            // SLGP_RAWPATH：拿「存进去的原样路径」，不触发 Shell 的链接解析
            // （解析可能去访问网络位置或光驱，对一个本地小工具来说不值得）。
            // 中间的 WIN32_FIND_DATAW 输出参数只在带 SLGP_RAWPATH 以外的标志时才需要，这里传空指针。
            let mut path_buffer = [0u16; PATH_BUFFER_LEN];
            shell_link
                .GetPath(
                    &mut path_buffer,
                    std::ptr::null_mut(),
                    SLGP_RAWPATH.0 as u32,
                )
                .map_err(|err| {
                    AppError::internal("读取快捷方式目标失败").with_detail(err.to_string())
                })?;

            let mut args_buffer = [0u16; ARGS_BUFFER_LEN];
            shell_link.GetArguments(&mut args_buffer).map_err(|err| {
                AppError::internal("读取快捷方式参数失败").with_detail(err.to_string())
            })?;

            let mut dir_buffer = [0u16; PATH_BUFFER_LEN];
            shell_link
                .GetWorkingDirectory(&mut dir_buffer)
                .map_err(|err| {
                    AppError::internal("读取快捷方式工作目录失败").with_detail(err.to_string())
                })?;

            Ok(RawLink {
                target: from_wide(&path_buffer),
                args: from_wide(&args_buffer),
                working_directory: from_wide(&dir_buffer),
            })
        }
    })
}

/// 展开 `%VAR%`。不含 `%` 时直接返回，避免无谓的系统调用。
fn expand_env_vars(input: &str) -> String {
    if !input.contains('%') {
        return input.to_owned();
    }

    let wide = to_wide(input);

    // SAFETY: 传入的是以 NUL 结尾的宽字符串；缓冲区按首次调用返回的长度分配。
    unsafe {
        let needed = ExpandEnvironmentStringsW(PCWSTR(wide.as_ptr()), None);
        if needed == 0 {
            return input.to_owned();
        }

        let mut buffer = vec![0u16; needed as usize];
        let written = ExpandEnvironmentStringsW(PCWSTR(wide.as_ptr()), Some(&mut buffer));
        if written == 0 {
            return input.to_owned();
        }

        from_wide(&buffer)
    }
}

/// 把可能是相对路径的目标，按 `.lnk` 所在目录补成绝对路径。
fn absolutize(target: &str, lnk_path: &Path) -> Option<PathBuf> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }

    lnk_path.parent().map(|parent| parent.join(path))
}

/// 显示名回退顺序：`.lnk` 文件名 → 目标文件名。
fn default_name(lnk_path: &Path, launch_path: &str) -> String {
    let from_lnk = lnk_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty());

    if let Some(name) = from_lnk {
        return name.to_owned();
    }

    Path::new(launch_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("未命名游戏")
        .to_owned()
}

/// 目标是否像平台启动器（而非游戏本体）。
///
/// 命中后 `is_launcher = true`：这类目标会影响进程跟踪的准确性，
/// 需要如实告知用户，而不是假装计时完全准确。
pub fn is_launcher_target(launch_path: &str) -> bool {
    Path::new(launch_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_lowercase())
        .is_some_and(|stem| KNOWN_LAUNCHER_STEMS.contains(&stem.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppErrorCode;
    use crate::test_support::{cleanup, make_shortcut, real_exe};

    #[test]
    fn rejects_non_lnk_extension() {
        let err =
            resolve_shortcut("C:\\Windows\\System32\\notepad.exe").expect_err("非 lnk 应被拒绝");
        assert_eq!(err.code(), AppErrorCode::InvalidArgument);
    }

    #[test]
    fn rejects_missing_shortcut() {
        let missing = std::env::temp_dir().join("nope-1a2b.lnk");
        let err = resolve_shortcut(&missing.to_string_lossy()).expect_err("不存在应报错");
        assert_eq!(err.code(), AppErrorCode::NotFound);
    }

    #[test]
    fn parses_real_shortcut_with_target_args_and_working_dir() {
        let Some(exe) = real_exe() else {
            eprintln!("跳过：本机找不到可用的系统 exe");
            return;
        };
        let Some((lnk, dir)) =
            make_shortcut("我的游戏", exe, "-windowed -novid", r"C:\Windows\System32")
        else {
            eprintln!("跳过：无法生成测试用 .lnk");
            return;
        };

        let probe = resolve_shortcut(&lnk.to_string_lossy()).expect("解析应成功");

        assert!(
            probe.launch_path.to_lowercase().ends_with(".exe"),
            "启动目标必须是 exe，实际 {}",
            probe.launch_path
        );
        assert_ne!(
            probe.launch_path.to_lowercase(),
            lnk.to_string_lossy().to_lowercase(),
            "绝不能把 .lnk 自身当作启动目标"
        );
        assert_eq!(probe.launch_args, "-windowed -novid", "参数必须原样保留");
        assert!(
            probe.working_directory.to_lowercase().ends_with("system32"),
            "工作目录应来自快捷方式，实际 {}",
            probe.working_directory
        );
        assert_eq!(probe.default_name, "我的游戏", "显示名优先取 .lnk 文件名");
        assert!(probe.target_exists);
        assert!(!probe.is_launcher);

        cleanup(&dir);
    }

    #[test]
    fn shortcut_without_args_or_working_dir_still_resolves() {
        let Some(exe) = real_exe() else {
            return;
        };
        let Some((lnk, dir)) = make_shortcut("空参数", exe, "", "") else {
            return;
        };

        let probe = resolve_shortcut(&lnk.to_string_lossy()).expect("解析应成功");
        assert_eq!(probe.launch_args, "");
        assert!(
            probe.working_directory.to_lowercase().ends_with("system32"),
            "工作目录为空时应回退到目标所在目录，实际 {}",
            probe.working_directory
        );

        cleanup(&dir);
    }

    #[test]
    fn missing_target_is_reported_not_silently_accepted() {
        let bogus = r"C:\definitely-missing-dir-9f3a\game.exe";
        let Some((lnk, dir)) = make_shortcut("失效链接", bogus, "", "") else {
            eprintln!("跳过：无法生成指向不存在目标的 .lnk");
            return;
        };

        let probe = resolve_shortcut(&lnk.to_string_lossy()).expect("解析本身不应失败");
        assert!(!probe.target_exists, "目标缺失必须被标记出来");
        assert!(
            probe.launch_path.contains("definitely-missing-dir-9f3a"),
            "应保留原始目标路径供用户判断，实际 {}",
            probe.launch_path
        );

        cleanup(&dir);
    }

    #[test]
    fn detects_known_launcher_target() {
        assert!(is_launcher_target(
            r"C:\Program Files (x86)\Steam\steam.exe"
        ));
        assert!(is_launcher_target(r"D:\Epic\EpicGamesLauncher.exe"));
        assert!(is_launcher_target(
            r"C:\Program Files\Battle.net\Battle.net.exe"
        ));
        assert!(!is_launcher_target(r"C:\Games\CoolGame\game.exe"));
        assert!(!is_launcher_target(""));
    }

    #[test]
    fn default_name_falls_back_to_target_then_placeholder() {
        // .lnk 文件名优先
        assert_eq!(
            default_name(Path::new(r"C:\x\我的游戏.lnk"), r"C:\y\target.exe"),
            "我的游戏"
        );

        // 注意 Windows 路径语义会忽略尾部分隔符（`C:\x\` 的 file_stem 是 "x"），
        // 所以「没有文件名的路径」要用根路径来构造。
        assert_eq!(
            default_name(Path::new("C:\\"), r"C:\y\target.exe"),
            "target"
        );
        assert_eq!(default_name(Path::new("C:\\"), ""), "未命名游戏");
    }

    #[test]
    fn expand_env_vars_handles_plain_and_percent_strings() {
        assert_eq!(expand_env_vars("C:\\plain\\path"), "C:\\plain\\path");

        let expanded = expand_env_vars("%SystemRoot%\\System32");
        if expanded.contains('%') {
            eprintln!("跳过：本机 %SystemRoot% 未能展开");
        } else {
            assert!(expanded.to_lowercase().contains("system32"));
        }
    }
}
