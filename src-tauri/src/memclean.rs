//! 内存优化（Mem Reduct 同款逻辑）。
//!
//! 清理三块：进程工作集（SystemMemoryListInformation / MemoryEmptyWorkingSets）、
//! 系统缓存工作集（SystemFileCacheInformation，最小/最大工作集设为 SIZE_MAX）、
//! 备用页列表（MemoryPurgeStandbyList）。
//!
//! 权限：这些调用需要管理员 + SeProfileSingleProcessPrivilege。
//! 应用本体不提权（保持免管理员设计）——点击「优化」时用 ShellExecuteExW(runas)
//! 拉起自身的提权子进程（`--mem-clean` 参数）执行，清理完成后写结果文件并退出。

#![cfg(windows)]

use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};

/// SystemMemoryListInformation 的 NtQuerySystemInformation 信息类别。
const SYSTEM_MEMORY_LIST_INFORMATION: i32 = 80;
/// SystemFileCacheInformation 的信息类别。
const SYSTEM_FILE_CACHE_INFORMATION: i32 = 21;
/// SystemMemoryListInformation 命令：清空全部进程工作集。
const MEMORY_EMPTY_WORKING_SETS: i32 = 2;
/// SystemMemoryListInformation 命令：清除备用页列表。
const MEMORY_PURGE_STANDBY_LIST: i32 = 4;
/// SeProfileSingleProcessPrivilege 的特权 LUID 索引（winnt.h 表）。
const SE_PROFILE_SINGLE_PROCESS_PRIVILEGE: u32 = 13;
/// SeIncreaseQuotaPrivilege 的特权 LUID 索引（收缩系统缓存工作集需要）。
const SE_INCREASE_QUOTA_PRIVILEGE: u32 = 5;
/// 提权子进程等待超时（毫秒）。
const CHILD_WAIT_TIMEOUT_MS: u32 = 20_000;

type NtSetSystemInformationFn =
    unsafe extern "system" fn(system_information_class: i32, information: *mut core::ffi::c_void, length: u32) -> i32;
type RtlAdjustPrivilegeFn = unsafe extern "system" fn(
    privilege: u32,
    enable: i32,
    current_thread: i32,
    was_enabled: *mut i32,
) -> i32;

/// 可用物理内存（MB）。
fn available_mb() -> u64 {
    unsafe {
        let mut status: MEMORYSTATUSEX = std::mem::zeroed();
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        let _ = GlobalMemoryStatusEx(&mut status);
        status.ullAvailPhys / (1024 * 1024)
    }
}

/// 从 ntdll 解析未公开导出。
fn ntdll_proc(name: &str) -> Option<*const core::ffi::c_void> {
    unsafe {
        let ntdll_name: windows::core::HSTRING = "ntdll.dll".into();
        let module = LoadLibraryW(PCWSTR(ntdll_name.as_ptr())).ok()?;
        // GetProcAddress 用 ANSI（PCSTR）—— 这些导出名均为 ASCII
        let name_bytes: Vec<u8> = name.as_bytes().to_vec();
        let proc_addr = GetProcAddress(module, windows::core::PCSTR(name_bytes.as_ptr()));
        proc_addr.map(|p| p as *const core::ffi::c_void)
    }
}

/// 启用单个特权（LUID 索引）。
fn enable_privilege(privilege: u32) -> Result<(), String> {
    let proc_addr = ntdll_proc("RtlAdjustPrivilege").ok_or("未找到 RtlAdjustPrivilege（ntdll）")?;
    let rtl_adjust_privilege: RtlAdjustPrivilegeFn = unsafe { std::mem::transmute(proc_addr) };
    let mut was_enabled = 0i32;
    let status = unsafe { rtl_adjust_privilege(privilege, 1, 0, &mut was_enabled) };
    if status != 0 {
        return Err(format!("启用特权 {privilege} 失败（NTSTATUS 0x{status:08X}）"));
    }
    Ok(())
}

/// 启用清理所需的全部特权：备用页清理要 SeProfileSingleProcessPrivilege，
/// 系统缓存收缩要 SeIncreaseQuotaPrivilege。
fn enable_needed_privileges() -> Result<(), String> {
    let mut errors = Vec::new();
    for privilege in [SE_PROFILE_SINGLE_PROCESS_PRIVILEGE, SE_INCREASE_QUOTA_PRIVILEGE] {
        if let Err(err) = enable_privilege(privilege) {
            errors.push(err);
        }
    }
    if errors.len() == 2 {
        Err(errors.join("；"))
    } else {
        Ok(())
    }
}

/// NtSetSystemInformation 封装。
fn nt_set_system_information(class: i32, info: *mut core::ffi::c_void, len: u32) -> Result<(), String> {
    let proc_addr = ntdll_proc("NtSetSystemInformation").ok_or("未找到 NtSetSystemInformation（ntdll）")?;
    let nt_set: NtSetSystemInformationFn = unsafe { std::mem::transmute(proc_addr) };
    let status = unsafe { nt_set(class, info, len) };
    if status != 0 {
        return Err(format!("NtSetSystemInformation 失败（NTSTATUS 0x{status:08X}）"));
    }
    Ok(())
}

/// 执行三步清理（需要已具备管理员权限）。返回 (优化前, 优化后, 失败步骤)。
/// 每步独立容错：单步失败不影响其余步骤，全部失败才视为整体失败。
fn perform_clean() -> Result<(u64, u64, Vec<String>), String> {
    enable_needed_privileges()?;
    let before = available_mb();
    let mut warnings = Vec::new();

    // 1) 清空全部进程工作集
    let command = MEMORY_EMPTY_WORKING_SETS;
    if let Err(err) = nt_set_system_information(SYSTEM_MEMORY_LIST_INFORMATION, &command as *const i32 as *mut _, std::mem::size_of::<i32>() as u32) {
        warnings.push(format!("进程工作集清理失败（{err}）"));
    }

    // 2) 收缩系统缓存工作集（最小/最大工作集设为 SIZE_MAX）
    #[repr(C)]
    struct SystemFileCacheInfo {
        current_size: usize,
        peak_size: usize,
        page_fault_count: u32,
        minimum_working_set: usize,
        maximum_working_set: usize,
    }
    let mut cache_info: SystemFileCacheInfo = unsafe { std::mem::zeroed() };
    cache_info.minimum_working_set = usize::MAX;
    cache_info.maximum_working_set = usize::MAX;
    if let Err(err) = nt_set_system_information(SYSTEM_FILE_CACHE_INFORMATION, &cache_info as *const SystemFileCacheInfo as *mut _, std::mem::size_of::<SystemFileCacheInfo>() as u32) {
        warnings.push(format!("系统缓存清理失败（{err}）"));
    }

    // 3) 清除备用页列表（最大降幅来源）
    let command = MEMORY_PURGE_STANDBY_LIST;
    if let Err(err) = nt_set_system_information(SYSTEM_MEMORY_LIST_INFORMATION, &command as *const i32 as *mut _, std::mem::size_of::<i32>() as u32) {
        warnings.push(format!("备用页列表清理失败（{err}）"));
    }

    // 内存统计需要一点时间收敛
    std::thread::sleep(std::time::Duration::from_millis(500));
    let after = available_mb();
    if warnings.len() == 3 {
        Err(warnings.join("；"))
    } else {
        Ok((before, after, warnings))
    }
}

/// 提权子进程入口：清理并把结果写入指定文件，返回进程退出码。
pub fn child_entry(result_path: Option<&str>) -> i32 {
    let Some(path) = result_path else {
        return 2;
    };
    match perform_clean() {
        Ok((before, after, warnings)) => {
            let mut payload = json_line(before, after);
            if !warnings.is_empty() {
                payload = payload.trim_end_matches('}').to_string();
                payload.push_str(&format!(",\"warn\":{}", json_string(&warnings.join("；"))));
                payload.push('}');
            }
            if write_result(Path::new(path), &payload).is_err() {
                return 3;
            }
            0
        }
        Err(message) => {
            let payload = format!("{{\"error\":{}}}", json_string(&message));
            let _ = write_result(Path::new(path), &payload);
            1
        }
    }
}

fn json_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn json_line(before: u64, after: u64) -> String {
    format!("{{\"before\":{before},\"after\":{after}}}")
}

fn write_result(path: &Path, payload: &str) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(payload.as_bytes())
}

/// 通过 ShellExecuteExW(runas) 拉起提权子进程并等待其完成，返回 (before_mb, after_mb)。
pub fn optimize_with_elevation() -> Result<(u64, u64, Option<String>), String> {
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOCLOSEPROCESS};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let exe = std::env::current_exe().map_err(|err| format!("无法定位应用路径: {err}"))?;
    let result_path: PathBuf = std::env::temp_dir().join("game-library-memclean.json");
    let _ = std::fs::remove_file(&result_path);

    let exe_wide: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    let verb_wide: Vec<u16> = "runas".encode_utf16().chain([0]).collect();
    let params = format!("--mem-clean \"{}\"", result_path.display());
    let params_wide: Vec<u16> = params.encode_utf16().chain([0]).collect();

    let mut exec_info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    exec_info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    exec_info.fMask = SEE_MASK_NOCLOSEPROCESS;
    exec_info.lpVerb = PCWSTR(verb_wide.as_ptr());
    exec_info.lpFile = PCWSTR(exe_wide.as_ptr());
    exec_info.lpParameters = PCWSTR(params_wide.as_ptr());
    exec_info.nShow = SW_HIDE.0;

    unsafe {
        if ShellExecuteExW(&mut exec_info).is_err() {
            return Err("未能启动提权子进程".into());
        }
        let handle = exec_info.hProcess;
        if handle.is_invalid() {
            return Err("已取消 —— 未授予管理员权限".into());
        }
        let wait = WaitForSingleObject(handle, CHILD_WAIT_TIMEOUT_MS);
        let _ = CloseHandle(handle);
        if wait == WAIT_TIMEOUT {
            return Err("内存优化超时".into());
        }

        let mut exit_code = 1u32;
        let _ = GetExitCodeProcess(handle, &mut exit_code);
        let raw = std::fs::read_to_string(&result_path).map_err(|_| "未读取到清理结果")?;
        let _ = std::fs::remove_file(&result_path);
        let parsed: serde_json::Value =
            serde_json::from_str(raw.trim()).map_err(|_| "清理结果格式异常")?;
        if let Some(error) = parsed.get("error").and_then(|v| v.as_str()) {
            return Err(format!("内存优化失败：{error}"));
        }
        if exit_code != 0 {
            return Err(format!("内存优化子进程异常退出（{exit_code}）"));
        }
        let before = parsed.get("before").and_then(|v| v.as_u64()).ok_or("结果缺少 before")?;
        let after = parsed.get("after").and_then(|v| v.as_u64()).ok_or("结果缺少 after")?;
        let warn = parsed.get("warn").and_then(|v| v.as_str()).map(String::from);
        Ok((before, after, warn))
    }
}


