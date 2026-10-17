//! 进程/COM 相关的基础设施。

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RegEnumKeyExW, RegOpenKeyExW,
    RegQueryValueExW,
};
use windows::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime;

use crate::error::{AppError, AppResult};

/// Windows 句柄的所有权包装：`Drop` 时 `CloseHandle`，提前返回也不会泄漏。
pub struct OwnedHandle(HANDLE);

impl OwnedHandle {
    /// 接管一个句柄的所有权。
    ///
    /// # Safety
    ///
    /// `handle` 必须是有效句柄，且调用方放弃对它的所有权（不得再自行关闭）。
    pub unsafe fn new(handle: HANDLE) -> Self {
        Self(handle)
    }

    pub fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: 句柄由 `new` 接管，且只在这里关闭一次。
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

// SAFETY: Windows 句柄是**进程级**的、与线程无关的不透明引用，可以从任意线程使用。
// windows-rs 把 HANDLE 定义成裸指针而没有标 Send，但这里确实需要跨线程持有进程句柄
// （追踪任务要在别的线程上判断进程是否还活着）。「只关闭一次」由 Drop 守卫保证。
unsafe impl Send for OwnedHandle {}

/// 当前线程的 STA COM apartment。
///
/// 用 RAII 保证 `CoUninitialize` 与 `CoInitializeEx` 成对出现 ——
/// 即使闭包里提前 `return` 或 `?` 早退，也不会漏掉反初始化。
struct StaApartment;

impl StaApartment {
    fn init() -> AppResult<Self> {
        // SAFETY: 只在本线程调用，并与 Drop 中的 CoUninitialize 成对。
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr.is_err() {
            return Err(AppError::internal("COM 初始化失败").with_detail(format!("{hr:?}")));
        }
        Ok(Self)
    }
}

impl Drop for StaApartment {
    fn drop(&mut self) {
        // SAFETY: 与上面的 CoInitializeEx 成对。
        unsafe { CoUninitialize() };
    }
}

/// 读取「无偏中断时间」（单位 100ns）。
///
/// 单调递增，且**不把系统休眠的时间算进去** —— 这正是游玩时长该用的基准：
/// 电脑睡眠 8 小时不应被记成玩了 8 小时（ADR-04）。墙钟只用于展示时间戳。
pub fn unbiased_interrupt_time() -> AppResult<u64> {
    let mut value = 0u64;
    // SAFETY: 输出指针指向本地变量，调用本身没有其它副作用。
    let ok = unsafe { QueryUnbiasedInterruptTime(&mut value) };
    if ok.as_bool() {
        Ok(value)
    } else {
        Err(AppError::internal("读取系统计时基准失败"))
    }
}

/// 由两个无偏时间基准点计算已流逝的整秒数。
///
/// 100ns 为单位的差值除以 10^7。起点晚于终点（不该发生）时按 0 处理。
pub fn elapsed_seconds(start_unbiased: u64, now_unbiased: u64) -> i64 {
    const TICKS_PER_SECOND: u64 = 10_000_000;
    (now_unbiased.saturating_sub(start_unbiased) / TICKS_PER_SECOND) as i64
}

/// 在一个**专用 STA 线程**里执行需要 COM 的工作并取回结果。
///
/// 为什么要另起线程：Tauri 使用多线程 tokio runtime，在 worker 线程上初始化
/// apartment 会带来 STA 消息泵与跨 apartment 封送的隐患。隔离到自己的线程最省事、
/// 也最不容易出难查的问题。
pub fn run_sta<T, F>(job: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    let (sender, receiver) = std::sync::mpsc::channel();

    std::thread::Builder::new()
        .name("com-sta".to_owned())
        .spawn(move || {
            let result = match StaApartment::init() {
                Ok(_apartment) => job(),
                Err(err) => Err(err),
            };
            // 接收端可能已放弃等待；发送失败不是错误
            let _ = sender.send(result);
        })
        .map_err(|err| AppError::internal("无法创建 COM 工作线程").with_detail(err.to_string()))?;

    // 线程 panic 会让发送端断开，这里统一转成可展示的错误，不让应用崩掉
    receiver.recv().map_err(|err| {
        AppError::internal("COM 调用未返回结果")
            .with_detail(format!("worker channel closed: {err}"))
    })?
}

/// Rust 字符串转以 NUL 结尾的宽字符缓冲。
pub fn to_wide(input: &str) -> Vec<u16> {
    input.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 去掉 Windows 的 verbatim 路径前缀（`\\?\` 与 `\\?\UNC\`）。
///
/// 为什么必须剥掉：`std::fs::canonicalize` 在 Windows 上返回的是 verbatim 路径
/// （`\\?\C:\Windows\System32\notepad.exe`），而 **Shell 系列 API
/// （`SHCreateItemFromParsingName`、`SHGetFileInfoW`）不接受这个前缀** ——
/// 传进去会直接失败，表现为「所有游戏都提取不到图标」。
///
/// 展示与存库也都不该带这个前缀。因此统一在**产出路径**的地方剥掉它。
pub fn strip_verbatim_prefix(path: &str) -> String {
    // UNC 形式要单独处理，否则会得到 `\\server\share` 少了前缀的错路径
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = path.strip_prefix(r"\\?\") {
        return rest.to_owned();
    }
    path.to_owned()
}

/// 从宽字符缓冲读出 Rust 字符串，遇到 NUL 即止。
pub fn from_wide(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sta_thread_returns_value() {
        let value = run_sta(|| Ok(7)).expect("应能取回结果");
        assert_eq!(value, 7);
    }

    #[test]
    fn sta_thread_propagates_error() {
        let err = run_sta::<u8, _>(|| Err(AppError::invalid_argument("故意失败")))
            .expect_err("错误应被原样传回");
        assert_eq!(err.message(), "故意失败");
    }

    #[test]
    fn wide_round_trip_including_non_ascii() {
        let original = "E:\\workbuddy数据\\游戏库";
        let wide = to_wide(original);
        assert_eq!(wide.last().copied(), Some(0), "必须以 NUL 结尾");
        assert_eq!(from_wide(&wide), original);
    }

    #[test]
    fn strips_verbatim_prefix() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\C:\Windows\System32\notepad.exe"),
            r"C:\Windows\System32\notepad.exe"
        );
        assert_eq!(
            strip_verbatim_prefix(r"\\?\E:\workbuddy数据\游戏库\src-tauri"),
            r"E:\workbuddy数据\游戏库\src-tauri"
        );
        // UNC 要还原成 `\\server\share` 而不是丢掉前缀
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\server\share\game.exe"),
            r"\\server\share\game.exe"
        );
        // 已经不带前缀的路径原样返回
        assert_eq!(strip_verbatim_prefix(r"C:\plain\path"), r"C:\plain\path");
    }

    #[test]
    fn canonicalize_output_is_stripped_for_shell_apis() {
        let Some(path) = [
            r"C:\Windows\System32\notepad.exe",
            r"C:\Windows\System32\where.exe",
        ]
        .into_iter()
        .find(|candidate| std::path::Path::new(candidate).exists()) else {
            eprintln!("跳过：本机找不到可用的系统 exe");
            return;
        };

        let canonical = std::fs::canonicalize(path).expect("canonicalize");
        let cleaned = strip_verbatim_prefix(&canonical.to_string_lossy());

        assert!(
            !cleaned.starts_with(r"\\?\"),
            "剥掉前缀后不能再残留 verbatim 标记，实际 {cleaned}"
        );
        assert!(
            std::path::Path::new(&cleaned).exists(),
            "剥前缀后路径仍然要能访问，实际 {cleaned}"
        );
    }
}

#[cfg(test)]
mod timing_tests {
    use super::*;

    #[test]
    fn unbiased_time_is_monotonic_and_positive() {
        let a = unbiased_interrupt_time().expect("读取计时基准");
        std::thread::sleep(std::time::Duration::from_millis(20));
        let b = unbiased_interrupt_time().expect("读取计时基准");
        assert!(a > 0, "无偏时间不应该是 0");
        assert!(b >= a, "无偏时间必须单调不减（a={a}, b={b}）");
    }

    #[test]
    fn elapsed_seconds_converts_100ns_ticks() {
        // 1 秒 = 10^7 个 100ns tick
        assert_eq!(elapsed_seconds(0, 10_000_000), 1);
        assert_eq!(elapsed_seconds(0, 25_000_000), 2, "不足一秒的部分向下取整");
        assert_eq!(elapsed_seconds(0, 0), 0);
        assert_eq!(elapsed_seconds(5_000_000, 0), 0, "终点早于起点时按 0 处理");
    }

    #[test]
    fn elapsed_seconds_measures_real_sleep() {
        let start = unbiased_interrupt_time().expect("读取计时基准");
        std::thread::sleep(std::time::Duration::from_millis(120));
        let now = unbiased_interrupt_time().expect("读取计时基准");
        let seconds = elapsed_seconds(start, now);
        assert!(
            (0..=2).contains(&seconds),
            "睡 120ms 折算秒数应在 0~2 之间，实际 {seconds}"
        );
    }
}

/// 读取注册表里的一个 REG_SZ 字符串值（只读）。
///
/// 用于定位第三方启动器的安装目录（如 Steam 的 `HKCU\Software\Valve\Steam : SteamPath`）。
/// 键不存在、值不是 REG_SZ 或读取失败都返回 `None` —— 调用方应继续尝试其它定位手段。
pub fn registry_string(root: RegRoot, subkey: &str, value_name: &str) -> Option<String> {
    let predefined: HKEY = match root {
        RegRoot::CurrentUser => HKEY_CURRENT_USER,
        RegRoot::LocalMachine => HKEY_LOCAL_MACHINE,
    };

    let mut subkey_wide: Vec<u16> = subkey.encode_utf16().collect();
    subkey_wide.push(0);

    // SAFETY: subkey 以 NUL 结尾；成功后句柄由 OwnedHandle 接管并在函数尾关闭。
    let mut handle = HKEY::default();
    let opened = unsafe {
        RegOpenKeyExW(
            predefined,
            windows::core::PCWSTR(subkey_wide.as_ptr()),
            None,
            KEY_READ,
            &mut handle,
        )
    };
    if opened.is_err() {
        return None;
    }
    // SAFETY: 句柄由 RegOpenKeyExW 返回，所有权移交 OwnedHandle 并在本函数尾关闭。
    let guard = unsafe { OwnedHandle::new(HANDLE(handle.0)) };

    let mut value_wide: Vec<u16> = value_name.encode_utf16().collect();
    value_wide.push(0);

    let mut kind = REG_SZ;
    let mut buffer = [0u8; 2048];
    let mut size = buffer.len() as u32;
    let queried = unsafe {
        RegQueryValueExW(
            HKEY(guard.raw().0),
            windows::core::PCWSTR(value_wide.as_ptr()),
            None,
            Some(&mut kind),
            Some(buffer.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if queried.is_err() || kind != REG_SZ || size == 0 {
        return None;
    }

    let usable = (size as usize).min(buffer.len());
    let chars = &buffer[..usable.saturating_sub(usable % 2)];
    let (pairs, _rest) = chars.as_chunks::<2>();
    let wide: Vec<u16> = pairs
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|c| *c != 0)
        .collect();
    Some(String::from_utf16_lossy(&wide))
}

/// 枚举注册表某个键下的全部子键名（只读，一层）。
///
/// 用于发现类适配器遍历安装记录（如 Ubisoft 的
/// `HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\<id>`）。键不存在返回空列表。
pub fn registry_subkeys(root: RegRoot, subkey: &str) -> Vec<String> {
    let predefined: HKEY = match root {
        RegRoot::CurrentUser => HKEY_CURRENT_USER,
        RegRoot::LocalMachine => HKEY_LOCAL_MACHINE,
    };

    let mut subkey_wide: Vec<u16> = subkey.encode_utf16().collect();
    subkey_wide.push(0);

    // SAFETY: subkey 以 NUL 结尾；成功后句柄由 OwnedHandle 接管并在函数尾关闭。
    let mut handle = HKEY::default();
    let opened = unsafe {
        RegOpenKeyExW(
            predefined,
            windows::core::PCWSTR(subkey_wide.as_ptr()),
            None,
            KEY_READ,
            &mut handle,
        )
    };
    if opened.is_err() {
        return Vec::new();
    }
    let guard = unsafe { OwnedHandle::new(HANDLE(handle.0)) };

    let mut names = Vec::new();
    let mut index = 0u32;
    loop {
        let mut buffer = [0u16; 256];
        let mut size = buffer.len() as u32;
        // SAFETY: buffer/size 成对传入；枚举到末尾（ERROR_NO_MORE_ITEMS）即停止。
        let result = unsafe {
            RegEnumKeyExW(
                HKEY(guard.raw().0),
                index,
                Some(windows::core::PWSTR(buffer.as_mut_ptr())),
                &mut size,
                None,
                None,
                None,
                None,
            )
        };
        if result.is_err() {
            break;
        }
        let wide = &buffer[..size as usize];
        names.push(String::from_utf16_lossy(wide));
        index += 1;
        if index > 4096 {
            break; // 防御：异常注册表不至于死循环
        }
    }
    names
}

/// 注册表根键（避免把 windows 的 HKEY 类型暴露给业务层）。
#[derive(Debug, Clone, Copy)]
pub enum RegRoot {
    CurrentUser,
    LocalMachine,
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    use std::path::Path;

    /// 验证注册表读取真的能工作（Steam 一定会写这个键，且值就是它的安装目录）。
    /// 如果本机没装 Steam，只验证「读取不崩、返回 None」。
    #[test]
    fn reads_steam_path_from_registry() {
        let value = registry_string(RegRoot::CurrentUser, r"Software\Valve\Steam", "SteamPath");
        println!("HKCU\\Software\\Valve\\Steam : SteamPath = {value:?}");
        // 只要不崩就算通过；有值时应当是一个存在的目录
        if let Some(path) = value {
            assert!(
                Path::new(&path).is_dir(),
                "注册表里的 Steam 路径应是真实存在的目录：{path}"
            );
        }
    }
}
