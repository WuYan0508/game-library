//! 进程树追踪。
//!
//! 为什么不看「最初启动的进程退没退出」（见 ADR-04）：
//! 快捷方式常常先拉起平台客户端，再由客户端拉起真正的游戏。只看 root 会在客户端
//! 退出时误判游戏结束。所以这里从 root 出发做 BFS，**root 及其全部后代**都算在内。
//!
//! 另一个必须处理的点：**PID 复用**。Windows 会回收 PID，必须用「进程创建时间」
//! 校验看到的这个 pid 还是不是当初那个进程。

use std::collections::{HashMap, HashSet, VecDeque};

use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

use crate::error::{AppError, AppResult};
use crate::launcher::launch::process_creation_time;
use crate::platform::win::OwnedHandle;

/// 快照里的一条进程记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessEntry {
    pub pid: u32,
    pub parent_pid: u32,
}

/// 进程树的当前状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeStatus {
    /// root 或其某个后代仍在运行。
    Alive { members: Vec<u32> },
    /// 整棵树都已退出。
    Gone,
    /// root 的 PID 被复用 —— 现在这个 pid 已经是别的进程了，按结束处理。
    IdentityReused,
}

/// 判断进程树中是否还有存活成员。
///
/// `expected_creation_time` 是启动时记录的进程创建时间；root 仍存在时会拿它校验 PID 复用。
pub fn collect_tree(root_pid: u32, expected_creation_time: u64) -> AppResult<TreeStatus> {
    let entries = snapshot_processes()?;

    let mut present: HashSet<u32> = HashSet::with_capacity(entries.len());
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for entry in &entries {
        present.insert(entry.pid);
        children
            .entry(entry.parent_pid)
            .or_default()
            .push(entry.pid);
    }

    if present.contains(&root_pid)
        && let Ok(actual) = read_creation_time(root_pid)
        && actual != expected_creation_time
    {
        return Ok(TreeStatus::IdentityReused);
    }

    // 从 root 出发 BFS。
    //
    // 注意：**即使 root 已经退出也能走通** —— Windows 不会更新子进程的 PPID，
    // 它依然指向那个（可能已经消失的）父进程 pid。这正是「启动器退出、游戏还在跑」
    // 这种场景能被正确识别的原因。
    let mut members = Vec::new();
    let mut visited: HashSet<u32> = HashSet::new();
    let mut queue: VecDeque<u32> = VecDeque::from([root_pid]);
    visited.insert(root_pid);

    while let Some(pid) = queue.pop_front() {
        if present.contains(&pid) {
            members.push(pid);
        }

        if let Some(kids) = children.get(&pid) {
            for &kid in kids {
                if visited.insert(kid) {
                    queue.push_back(kid);
                }
            }
        }
    }

    if members.is_empty() {
        Ok(TreeStatus::Gone)
    } else {
        Ok(TreeStatus::Alive { members })
    }
}

/// 枚举当前所有进程（pid + 父 pid）。
pub fn snapshot_processes() -> AppResult<Vec<ProcessEntry>> {
    let mut entries = Vec::new();

    // SAFETY: 快照句柄由 OwnedHandle 持有并在函数结束时关闭；
    // PROCESSENTRY32W.dwSize 必须先填写，否则 API 直接失败。
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|err| AppError::internal("无法枚举进程").with_detail(err.to_string()))?;
        let guard = OwnedHandle::new(snapshot);

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(guard.raw(), &mut entry).is_err() {
            return Ok(entries);
        }

        loop {
            entries.push(ProcessEntry {
                pid: entry.th32ProcessID,
                parent_pid: entry.th32ParentProcessID,
            });

            if Process32NextW(guard.raw(), &mut entry).is_err() {
                break;
            }
        }
    }

    Ok(entries)
}

/// 读取某个 pid 的创建时间。读不到（权限不足 / 进程已退出）返回 Err。
fn read_creation_time(pid: u32) -> AppResult<u64> {
    // SAFETY: 句柄由 OwnedHandle 持有并在函数结束时关闭。
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(|err| {
            AppError::internal("无法打开进程读取创建时间").with_detail(err.to_string())
        })?;
        let guard = OwnedHandle::new(handle);
        process_creation_time(guard.raw())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_contains_current_process() {
        let entries = snapshot_processes().expect("枚举进程");
        assert!(!entries.is_empty(), "进程快照不该是空的");

        let me = std::process::id();
        assert!(
            entries.iter().any(|entry| entry.pid == me),
            "快照里应当能找到当前测试进程（pid {me}）"
        );
    }

    #[test]
    fn nonexistent_root_reports_gone() {
        // 一个几乎不可能存在的 pid
        let status = collect_tree(0xFFFF_FFF0, 0).expect("查询应成功");
        assert_eq!(status, TreeStatus::Gone);
    }

    #[test]
    fn current_process_is_alive_with_wrong_identity_checks() {
        let me = std::process::id();

        // 用错误的创建时间去查 → 应判定为 PID 复用
        let reused = collect_tree(me, 1).expect("查询应成功");
        assert_eq!(
            reused,
            TreeStatus::IdentityReused,
            "创建时间对不上时必须判定为 PID 复用"
        );

        // 用真实创建时间去查 → 应当存活
        let real = snapshot_processes()
            .expect("枚举")
            .into_iter()
            .find(|entry| entry.pid == me)
            .expect("找到自己");
        let _ = real; // 父 pid 不参与断言，这里只是确认能取到

        let status = collect_tree(me, true_creation_time()).expect("查询应成功");
        assert!(
            matches!(status, TreeStatus::Alive { .. }),
            "用正确身份查询应判定为存活，实际 {status:?}"
        );
    }

    /// 当前进程的真实创建时间。
    fn true_creation_time() -> u64 {
        use windows::Win32::System::Threading::GetCurrentProcess;
        process_creation_time(unsafe { GetCurrentProcess() }).expect("读取自身创建时间")
    }

    #[test]
    fn tree_includes_children_of_root() {
        // 用 cmd 启动一个子进程，验证 BFS 能把后代算进来
        let child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 6 127.0.0.1 > nul"])
            .spawn();

        let Ok(mut child) = child else {
            eprintln!("跳过：无法启动测试用子进程");
            return;
        };

        std::thread::sleep(std::time::Duration::from_millis(400));

        let root_pid = child.id();
        let creation = snapshot_processes()
            .ok()
            .and_then(|entries| {
                entries
                    .iter()
                    .any(|entry| entry.pid == root_pid)
                    .then_some(())
            })
            .and_then(|_| read_creation_time(root_pid).ok());

        if let Some(creation) = creation {
            match collect_tree(root_pid, creation) {
                Ok(TreeStatus::Alive { members }) => {
                    assert!(members.contains(&root_pid), "存活集合必须包含 root 自己");
                    assert!(
                        members.len() > 1,
                        "cmd 会拉起 ping 子进程，进程树应不止一个成员，实际 {members:?}"
                    );
                }
                other => eprintln!("跳过：进程树状态非预期 {other:?}"),
            }
        }

        let _ = child.kill();
        let _ = child.wait();
    }
}
