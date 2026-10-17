//! 应用入口与运行时装配。
//!
//! 分层约定见 docs/architecture.md §3：`commands → 业务模块 → 基础设施`。
//! 本文件只负责把各层装配起来，不放业务逻辑。

// 顶层模块对外可见，是为了让 `tests/` 下的集成测试能直接访问内部类型。
// 本项目是应用 crate、不对外发布，因此不构成封装负担；
// 反过来说，**把会话生命周期测试放进集成测试是必需的** —— 编进 lib 测试目标后
// 测试二进制会加载失败（见 tests/session_manager.rs 顶部说明）。
pub mod commands;
pub mod covers;
pub mod database;
pub mod discovery;
pub mod error;
pub mod importer;
pub mod launcher;
pub mod platform;
pub mod tracker;

#[cfg(test)]
mod test_support;

use tauri::Manager;

use database::Db;
use tracker::session::SessionManager;

/// 应用数据目录下由本应用管理的子目录。
///
/// 在 `setup` 阶段确保存在，避免后续写入封面/图标时才发现目录缺失。
/// 注意：日志目录**不在这里** —— Tauri 的 `app_log_dir()` 在 Windows 上位于
/// LocalAppData（Windows 惯例），且日志插件会自行创建，见下面插件配置。
const MANAGED_SUBDIRS: [&str; 2] = ["covers", "icons"];

/// 数据库文件名，位于应用数据目录下。
const DB_FILE_NAME: &str = "game_library.db";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 单实例：重复启动时唤醒已运行的窗口，而不是再开一个进程。
        // 官方要求该插件必须第一个注册。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                // `Builder::new()` 自带默认 targets（Stdout + 以包名为名的 LogDir），
                // 直接 `.target()` 是追加，会产出两个内容相同的日志文件，所以先清空。
                .clear_targets()
                .level(log::LevelFilter::Info)
                // 插件默认上限仅 40 KB，对诊断来说太小，会频繁轮转。
                .max_file_size(5 * 1024 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(3))
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("app".to_owned()),
                    },
                ))
                .build(),
        )
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;

            for name in MANAGED_SUBDIRS {
                let dir = data_dir.join(name);
                if !dir.exists() {
                    std::fs::create_dir_all(&dir)?;
                }
            }

            // 打开数据库并执行迁移。失败即中止启动 —— 没有数据层，应用没有可用的功能。
            let db_path = data_dir.join(DB_FILE_NAME);
            let db = Db::open(&db_path)?;

            // 先建会话管理器，再把上一轮异常退出留下的 running 会话保守结算掉
            let sessions = SessionManager::new(db.clone(), Some(app.handle().clone()));
            sessions.recover_on_startup()?;
            app.manage(sessions);
            app.manage(db);

            log::info!("应用数据目录就绪: {}", data_dir.display());
            log::info!("数据库就绪: {}", db_path.display());

            // 「关闭按钮行为」设置（settings.json），供窗口关闭拦截读取
            app.manage(commands::tray_settings::CloseBehavior::load(app.handle()));

            // 系统托盘：左键单击显示主窗口，右键菜单「打开游戏库 / 退出」
            use tauri::menu::{MenuBuilder, MenuItem};
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

            let open_item = MenuItem::with_id(app, "open", "打开游戏库", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = MenuBuilder::new(app)
                .item(&open_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let mut tray = TrayIconBuilder::with_id("main-tray")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("游戏库")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    // 左键单击 = 显示主窗口（与常见托盘应用一致）
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            log::info!("系统托盘已就绪");

            Ok(())
        })
        // 拦截主窗口关闭：按设置决定「隐藏到托盘」还是「真退出」
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                let app = window.app_handle();
                let behavior = app.state::<commands::tray_settings::CloseBehavior>();
                let close_to_tray = *behavior
                    .close_to_tray
                    .read()
                    .expect("close_to_tray 锁未被污染");
                if close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                    log::info!("窗口已隐藏到系统托盘");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::get_app_info,
            commands::system::get_app_data_path,
            commands::system::open_log_dir,
            commands::games::list_games,
            commands::games::get_game,
            commands::games::search_games,
            commands::games::update_game,
            commands::games::remove_game,
            commands::import::probe_exe,
            commands::import::add_game_from_exe,
            commands::import::probe_shortcut,
            commands::import::add_game_from_shortcut,
            commands::covers::import_cover,
            commands::covers::extract_game_icon,
            commands::games::reorder_games,
            commands::maintenance::reextract_missing_icons,
            commands::tray_settings::get_close_to_tray,
            commands::tray_settings::set_close_to_tray,
            commands::discovery::scan_games,
            commands::discovery::import_discovered_games,
            commands::sessions::launch_game,
            commands::sessions::get_active_sessions,
            commands::sessions::stop_tracking,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 显示并聚焦主窗口（托盘菜单 / 左键单击共用）。
fn show_main_window(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
