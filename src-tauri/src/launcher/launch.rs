//! 用 `CreateProcessW` 启动目标程序。
//!
//! 为什么不用 `std::process::Command`（见 ADR-02）：
//! - 需要**进程句柄**与 `dwProcessId`，还要读进程创建时间作为「启动身份」；
//! - 需要把程序路径、参数、工作目录**分别传递**，绝不拼成一条交给 shell。

use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::Threading::{
    CreateProcessW, GetProcessId, GetProcessTimes, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION,
    STARTUPINFOW,
};
use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, PWSTR};

use crate::error::{AppError, AppResult};
use crate::launcher::quote::build_command_line;
use crate::platform::win::{OwnedHandle, to_wide};

/// 一次成功启动的结果。
pub struct LaunchedProcess {
    pub pid: u32,
    /// 进程创建时间（FILETIME，100ns 为单位）。用于识别 PID 复用。
    pub creation_time: u64,
    /// 进程句柄。持有它可确保「进程真的结束了」这一判定不被 PID 复用干扰。
    _handle: OwnedHandle,
}

impl std::fmt::Debug for LaunchedProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchedProcess")
            .field("pid", &self.pid)
            .field("creation_time", &self.creation_time)
            .finish_non_exhaustive()
    }
}

impl LaunchedProcess {
    /// 进程创建时间的字符串形式，直接对应 `play_sessions.process_start_identity`。
    pub fn creation_identity(&self) -> String {
        self.creation_time.to_string()
    }
}

/// 启动请求。三个字段分别对应 `CreateProcessW` 的三个独立参数。
pub struct LaunchRequest<'a> {
    pub program_path: &'a str,
    /// 原样参数串，不做分词或重新转义（ADR-03）。
    pub raw_args: &'a str,
    pub working_directory: Option<&'a str>,
}

/// 启动程序。
///
/// 普通程序走 `CreateProcessW`；**要求管理员权限的 exe**（如 TCLS\client.exe）会被
/// Windows 以 740（ERROR_ELEVATION_REQUIRED）拒绝 —— 此时自动改走
/// `ShellExecuteExW(runas)`，只对这一个程序弹 UAC。应用本身**不提权**（铁律 #6）：
/// 绝大多数游戏保持普通权限运行，不会踩反作弊对提权进程的限制。
pub fn launch(request: &LaunchRequest<'_>) -> AppResult<LaunchedProcess> {
    let program = request.program_path.trim();
    if program.is_empty() {
        return Err(AppError::invalid_argument("启动目标为空"));
    }

    validate_target(program)?;

    // 工作目录一律 = exe 所在目录（与资源管理器双击行为一致）。
    // 实测：巫师 3 等游戏以「游戏根目录」为 cwd 时进程会静默退出（退出码 0），
    // 只有以 exe 自己的目录为 cwd 才能正常启动。该规则在启动器层面统一生效，
    // 不依赖库内存的旧数据 —— 其他机器上的存量记录也因此一并修复。
    // 库内存的 working_directory 仅作展示用途。
    let effective_working_directory = effective_working_directory(program);

    let request = LaunchRequest {
        program_path: program,
        raw_args: request.raw_args,
        working_directory: effective_working_directory.as_deref(),
    };

    match try_create_process(&request) {
        Ok(launched) => Ok(launched),
        Err(err) => {
            let code = win32_error_code(&err);
            if code == Some(ERROR_ELEVATION_REQUIRED) {
                launch_elevated(&request)
            } else {
                Err(
                    AppError::launch_failed(describe_launch_failure(code)).with_detail(format!(
                        "{program}（cmdline: {}）: win32_code={code:?} {err}",
                        build_command_line(program, request.raw_args)
                    )),
                )
            }
        }
    }
}

/// 计算启动工作目录：exe 所在目录（ADR-02）。
fn effective_working_directory(program: &str) -> Option<String> {
    Path::new(program.trim())
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
}

/// 740 = ERROR_ELEVATION_REQUIRED：exe 的 manifest 要求管理员权限。
const ERROR_ELEVATION_REQUIRED: u32 = 740;
/// 1223 = ERROR_CANCELLED：用户在 UAC 弹窗里点了「否」。
const ERROR_CANCELLED: u32 = 1223;

/// 用 `CreateProcessW` 直接启动。失败时原样返回 windows 错误，由上层决定如何翻译。
fn try_create_process(
    request: &LaunchRequest<'_>,
) -> Result<LaunchedProcess, windows::core::Error> {
    let program = request.program_path.trim();
    let working_directory = request
        .working_directory
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let command_line = build_command_line(program, request.raw_args);

    let program_wide = to_wide(program);
    // CreateProcessW 会**就地修改** lpCommandLine，必须给它一块可写缓冲
    let mut command_line_wide = to_wide(&command_line);
    let directory_wide = working_directory.map(to_wide);

    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();

    // SAFETY: 三个宽字符串缓冲在调用期间保持有效；句柄由下方接管并在 Drop 中关闭。
    // 注意 CreateProcessW 的路径参数走泛型 `Param<PCWSTR>`，直接传 PCWSTR，不能包 Option。
    unsafe {
        CreateProcessW(
            PCWSTR(program_wide.as_ptr()),
            Some(PWSTR(command_line_wide.as_mut_ptr())),
            None,
            None,
            false,
            PROCESS_CREATION_FLAGS(0),
            None,
            directory_wide
                .as_ref()
                .map_or(PCWSTR::null(), |wide| PCWSTR(wide.as_ptr())),
            &startup,
            &mut info,
        )?;
    }

    // 线程句柄用不到，立刻关闭；进程句柄交给 OwnedHandle 持有
    unsafe {
        let _ = CloseHandle(info.hThread);
    }
    // SAFETY: 句柄由 CreateProcessW 返回，所有权移交给 OwnedHandle
    let handle = unsafe { OwnedHandle::new(info.hProcess) };

    let creation_time = process_creation_time_raw(handle.raw())?;

    Ok(LaunchedProcess {
        pid: info.dwProcessId,
        creation_time,
        _handle: handle,
    })
}

/// 通过 `ShellExecuteExW(runas)` 启动要求管理员权限的程序。
///
/// 只在 `CreateProcessW` 返回 740 时走到这里：弹一次 UAC，用户同意后进程以
/// 管理员身份运行。`SEE_MASK_NOCLOSEPROCESS` 让我们拿到进程句柄，计时追踪照常。
fn launch_elevated(request: &LaunchRequest<'_>) -> AppResult<LaunchedProcess> {
    let program = request.program_path.trim();
    let args = request.raw_args.trim();

    let verb = to_wide("runas");
    let file = to_wide(program);
    let args_wide = to_wide(args);
    let directory_wide = request.working_directory.map(|value| to_wide(value.trim()));

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: if args.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(args_wide.as_ptr())
        },
        lpDirectory: directory_wide
            .as_ref()
            .map_or(PCWSTR::null(), |wide| PCWSTR(wide.as_ptr())),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    // SAFETY: 各宽字符串缓冲在调用期间保持有效；成功时 hProcess 由下方接管。
    let result = unsafe { ShellExecuteExW(&mut info) };
    if let Err(err) = result {
        let code = win32_error_code(&err);
        let message = if code == Some(ERROR_CANCELLED) {
            "已取消管理员授权，程序未启动"
        } else {
            describe_launch_failure(Some(ERROR_ELEVATION_REQUIRED))
        };
        return Err(AppError::launch_failed(message)
            .with_detail(format!("{program}: win32_code={code:?} {err}")));
    }

    if info.hProcess.is_invalid() {
        // 理论上 SEE_MASK_NOCLOSEPROCESS 成功就应有句柄；防御一下，别静默丢时长
        return Err(AppError::launch_failed(
            "该程序已通过管理员授权启动，但本应用无法获取其进程信息，时长将不会被统计",
        ));
    }

    // SAFETY: 句柄由 ShellExecuteExW 返回，所有权移交给 OwnedHandle
    let handle = unsafe { OwnedHandle::new(info.hProcess) };
    let creation_time = process_creation_time(handle.raw())?;
    // SAFETY: 句柄有效
    let pid = unsafe { GetProcessId(handle.raw()) };

    log::info!("已通过 UAC 提权启动: {program}（pid={pid}），计时追踪照常");

    Ok(LaunchedProcess {
        pid,
        creation_time,
        _handle: handle,
    })
}

/// 从 windows crate 的错误里提取原始 Win32 错误码。
///
/// HRESULT 的 Win32 设施（FACILITY_WIN32 = 7）编码为 `0x8007xxxx`，低 16 位就是错误码。
fn win32_error_code(err: &windows::core::Error) -> Option<u32> {
    let hr = err.code().0;
    if (hr >> 16) & 0x1FFF == 7 {
        Some((hr & 0xFFFF) as u32)
    } else {
        None
    }
}

/// 把 CreateProcessW 的失败翻译成人话。
///
/// 真实案例：TCLS\\client.exe（英雄联盟启动器）的 manifest 要求
/// `requireAdministrator`，Windows 拒绝非提权调用并返回 740 ——
/// 这不是本应用的 bug，而是它真的不能被普通权限进程拉起（ADR：本应用不申请管理员权限）。
fn describe_launch_failure(code: Option<u32>) -> &'static str {
    match code {
        // 740 = ERROR_ELEVATION_REQUIRED
        Some(740) => {
            "该程序要求以管理员身份运行，本应用无法以管理员身份启动它。请改用它的官方启动器（如 WeGame）进入游戏，或导入游戏本体的 exe"
        }
        // 5 = ERROR_ACCESS_DENIED
        Some(5) => "系统拒绝访问该程序（可能被安全软件拦截，或需要管理员权限）",
        // 193 = ERROR_BAD_EXE_FORMAT
        Some(193) => "该文件不是当前系统可运行的程序（位数或架构不匹配）",
        _ => "无法启动该程序，请检查文件是否存在或是否被系统阻止",
    }
}

/// 读取某个进程的创建时间（FILETIME ticks）。
///
/// 也供进程树校验使用：句柄上的这个值稳定不变，是识别 PID 复用的可靠依据。
pub fn process_creation_time(process: HANDLE) -> AppResult<u64> {
    process_creation_time_raw(process)
        .map_err(|err| AppError::internal("读取进程创建时间失败").with_detail(err.to_string()))
}

/// 同上，但错误类型是 windows 原生错误（供 `try_create_process` 用 `?` 直通）。
fn process_creation_time_raw(process: HANDLE) -> Result<u64, windows::core::Error> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();

    // SAFETY: 传入的是有效进程句柄；四个 FILETIME 都是本地可写变量。
    unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user)? };

    Ok(filte_time_to_u64(creation))
}

/// FILETIME → u64（100ns ticks）。
pub fn filte_time_to_u64(time: FILETIME) -> u64 {
    ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64
}

fn validate_target(program: &str) -> AppResult<()> {
    let path = Path::new(program);

    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(AppError::invalid_argument("启动目标不是文件")),
        Err(err) => Err(match err.kind() {
            std::io::ErrorKind::NotFound => AppError::not_found("启动目标不存在或已被移动")
                .with_detail(format!("{program}: {err}")),
            std::io::ErrorKind::PermissionDenied => {
                AppError::permission_denied("无法访问启动目标，可能没有权限")
                    .with_detail(format!("{program}: {err}"))
            }
            _ => AppError::internal("无法访问启动目标").with_detail(format!("{program}: {err}")),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppErrorCode;

    /// 一个「启动后会自己退出」的真实系统程序，用来验证启动链路而不留残留进程。
    fn quick_exit_program() -> Option<&'static str> {
        [
            r"C:\Windows\System32\where.exe",
            r"C:\Windows\System32\hostname.exe",
            r"C:\Windows\System32\whoami.exe",
        ]
        .into_iter()
        .find(|path| Path::new(path).exists())
    }

    #[test]
    fn rejects_empty_target() {
        let err = launch(&LaunchRequest {
            program_path: "   ",
            raw_args: "",
            working_directory: None,
        })
        .expect_err("空路径应被拒绝");
        assert_eq!(err.code(), AppErrorCode::InvalidArgument);
    }

    #[test]
    fn rejects_missing_target() {
        let missing = std::env::temp_dir().join("no-such-app-71ba.exe");
        let err = launch(&LaunchRequest {
            program_path: &missing.to_string_lossy(),
            raw_args: "",
            working_directory: None,
        })
        .expect_err("不存在的路径应报错");
        assert_eq!(err.code(), AppErrorCode::NotFound);
    }

    #[test]
    fn launches_real_process_and_reports_identity() {
        let Some(program) = quick_exit_program() else {
            eprintln!("跳过：本机找不到可用的短命程序");
            return;
        };

        let launched = launch(&LaunchRequest {
            program_path: program,
            // 传给 where.exe 一个必然不存在的名字，让它立刻退出
            raw_args: "__gamelib_no_such_pattern__",
            working_directory: None,
        })
        .expect("启动应成功");

        assert!(launched.pid > 0, "应拿到有效 PID");
        assert!(
            launched.creation_time > 0,
            "应读到进程创建时间（PID 复用识别的依据）"
        );
        assert_eq!(
            launched.creation_identity(),
            launched.creation_time.to_string()
        );
    }

    #[test]
    fn launch_with_path_containing_spaces_works() {
        // 复制一个真实 exe 到带空格的临时目录，验证引号处理是否真的能启动
        let Some(program) = quick_exit_program() else {
            return;
        };

        let dir = std::env::temp_dir().join("gamelib space test");
        std::fs::create_dir_all(&dir).expect("建带空格的目录");
        let target = dir.join("my tool.exe");
        std::fs::copy(program, &target).expect("复制程序");

        let launched = launch(&LaunchRequest {
            program_path: &target.to_string_lossy(),
            raw_args: "__gamelib_no_such_pattern__",
            working_directory: Some(&dir.to_string_lossy()),
        });

        let result = launched.map(|process| process.pid);
        std::fs::remove_dir_all(&dir).ok();

        assert!(
            result.is_ok(),
            "带空格的路径必须能启动（引号处理错了就会失败）: {:?}",
            result.err().map(|err| err.message().to_owned())
        );
    }

    #[test]
    fn stored_working_directory_is_ignored_in_favor_of_exe_parent() {
        let Some(program) = quick_exit_program() else {
            return;
        };
        let missing_dir = std::env::temp_dir().join("no-such-dir-9f3a");

        // 工作目录一律取 exe 所在目录：库里存的旧值（哪怕无效）会被忽略，
        // 这是「巫师 3 静默退出」问题的通用修复
        let result = launch(&LaunchRequest {
            program_path: program,
            raw_args: "",
            working_directory: Some(&missing_dir.to_string_lossy()),
        });

        assert!(
            result.is_ok(),
            "无效的存量工作目录应被忽略并正常启动: {:?}",
            result.err().map(|err| err.message().to_owned())
        );
    }

    #[test]
    fn effective_working_directory_is_exe_parent() {
        assert_eq!(
            effective_working_directory(r"D:\games\The Witcher 3\bin\x64\witcher3.exe"),
            Some(r"D:\games\The Witcher 3\bin\x64".to_owned())
        );
        // 盘符根下的 exe：父目录是盘符根
        assert_eq!(
            effective_working_directory(r"C:\game.exe"),
            Some(r"C:\".to_owned())
        );
        // 前后空白会被裁掉
        assert_eq!(
            effective_working_directory("  D:\\a\\b.exe  "),
            Some(r"D:\a".to_owned())
        );
    }
}

#[cfg(test)]
mod error_mapping_tests {
    use super::*;

    #[test]
    fn elevation_required_maps_to_clear_message() {
        // 740 = ERROR_ELEVATION_REQUIRED → HRESULT 0x800702E4
        let err = windows::core::Error::from_hresult(windows::core::HRESULT(0x8007_02E4u32 as i32));
        assert_eq!(win32_error_code(&err), Some(740));
        let message = describe_launch_failure(Some(740));
        assert!(message.contains("管理员"), "提示要提到管理员：{message}");
        assert!(message.contains("启动器"), "要给出替代方案：{message}");
    }

    #[test]
    fn access_denied_maps_to_clear_message() {
        // 5 = ERROR_ACCESS_DENIED
        let err = windows::core::Error::from_hresult(windows::core::HRESULT(0x8007_0005u32 as i32));
        assert_eq!(win32_error_code(&err), Some(5));
        assert!(describe_launch_failure(Some(5)).contains("拒绝访问"));
    }

    #[test]
    fn non_win32_error_falls_back_to_generic_message() {
        // 设施码不是 WIN32（比如自定义 HRESULT 0x80004005 E_FAIL）
        let err = windows::core::Error::from_hresult(windows::core::HRESULT(0x8000_4005u32 as i32));
        assert_eq!(win32_error_code(&err), None);
        assert!(describe_launch_failure(None).contains("无法启动"));
    }

    #[test]
    fn missing_target_is_caught_by_pre_validation() {
        // 目标不存在时预检先报 NotFound（还没走到 CreateProcessW 的 740/5 映射）
        let request = LaunchRequest {
            program_path: r"C:\gamelib-definitely-missing\nope.exe",
            raw_args: "",
            working_directory: None,
        };
        let err = launch(&request).expect_err("应失败");
        assert_eq!(err.code(), crate::error::AppErrorCode::NotFound);
    }
}
