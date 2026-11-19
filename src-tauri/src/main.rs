// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 内存优化提权子进程：UAC 提权后以 `--mem-clean <结果文件>` 运行，
    // 执行完清理写结果文件即退出 —— 不初始化 Tauri / 数据库 / 托盘。
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 && args[1] == "--mem-clean" {
        let code = game_library_lib::mem_clean_child(args.get(2).map(String::as_str));
        std::process::exit(code);
    }
    game_library_lib::run()
}
