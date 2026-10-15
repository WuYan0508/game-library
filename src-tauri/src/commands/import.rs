//! 导入命令（对应 docs/architecture.md §7.1「游戏管理」的导入部分）。
//!
//! 命令体很薄：真正的编排放在 `*_impl` 里，且**只接收纯数据参数**（`&Db`、`&Path`），
//! 这样「探测 → 查重 → 落封面/图标 → 写库」整条链路可以被单元测试直接覆盖。

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::covers::{cover_store, icon_store};
use crate::database::Db;
use crate::database::games::{self, InsertGame};
use crate::database::models::{Game, SourceType};
use crate::error::{AppError, AppResult};
use crate::importer::{dedupe, exe, shortcut};

use super::app_data_dir;

/// 探测结果 + 重复提示。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExeProbeDto {
    #[serde(flatten)]
    pub probe: exe::ExeProbe,
    /// 已存在等价记录时给出其引用，前端在确认前就提示「已在库中」。
    pub duplicate: Option<dedupe::DuplicateRef>,
}

/// 快捷方式探测结果 + 重复提示。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutProbeDto {
    #[serde(flatten)]
    pub probe: shortcut::ShortcutProbe,
    pub duplicate: Option<dedupe::DuplicateRef>,
}

/// 只读探测：校验路径、派生默认名称与工作目录，并检查是否已在库中。
#[tauri::command]
pub fn probe_exe(db: State<'_, Db>, path: String) -> AppResult<ExeProbeDto> {
    probe_exe_impl(&db, &path)
}

/// 落库导入 EXE。
///
/// 会**重新探测一次**：从预览到用户点确认之间，文件可能已被移动或替换。
#[tauri::command]
pub fn add_game_from_exe(
    app: AppHandle,
    db: State<'_, Db>,
    path: String,
    name: Option<String>,
    cover_source_path: Option<String>,
) -> AppResult<Game> {
    add_game_from_exe_impl(&db, &app_data_dir(&app)?, &path, name, cover_source_path)
}

/// 只读探测 `.lnk`。
///
/// 目标缺失**不报错**：把 `targetExists = false` 如实回传，
/// 由前端让用户决定「取消」或「手动选择启动目标」。
#[tauri::command]
pub fn probe_shortcut(db: State<'_, Db>, path: String) -> AppResult<ShortcutProbeDto> {
    probe_shortcut_impl(&db, &path)
}

/// 落库导入快捷方式。
///
/// `launch_path_override`：目标缺失时用户手动选定的启动目标（一个真实存在的 EXE）。
#[tauri::command]
pub fn add_game_from_shortcut(
    app: AppHandle,
    db: State<'_, Db>,
    path: String,
    name: Option<String>,
    launch_path_override: Option<String>,
    cover_source_path: Option<String>,
) -> AppResult<Game> {
    add_game_from_shortcut_impl(
        &db,
        &app_data_dir(&app)?,
        &path,
        name,
        launch_path_override,
        cover_source_path,
    )
}

pub(crate) fn probe_exe_impl(db: &Db, path: &str) -> AppResult<ExeProbeDto> {
    let probe = exe::inspect_exe(path)?;

    // EXE 导入没有参数，去重键的参数部分为空串
    let duplicate = db
        .with_conn(|conn| dedupe::find_existing(conn, &probe.launch_path, ""))?
        .as_ref()
        .map(dedupe::DuplicateRef::from);

    Ok(ExeProbeDto { probe, duplicate })
}

pub(crate) fn probe_shortcut_impl(db: &Db, path: &str) -> AppResult<ShortcutProbeDto> {
    let probe = shortcut::resolve_shortcut(path)?;

    // 目标不存在时不去查重：拿一个失效路径去比对没有意义
    let duplicate = if probe.target_exists {
        db.with_conn(|conn| dedupe::find_existing(conn, &probe.launch_path, &probe.launch_args))?
            .as_ref()
            .map(dedupe::DuplicateRef::from)
    } else {
        None
    };

    Ok(ShortcutProbeDto { probe, duplicate })
}

pub(crate) fn add_game_from_exe_impl(
    db: &Db,
    data_dir: &Path,
    path: &str,
    name: Option<String>,
    cover_source_path: Option<String>,
) -> AppResult<Game> {
    let probe = exe::inspect_exe(path)?;
    let display_name = normalise_name(name, &probe.default_name);

    let game_id = games::new_id();
    let (cover_path, icon_path) = prepare_assets(
        data_dir,
        &game_id,
        &probe.launch_path,
        cover_source_path.as_deref(),
    )?;

    db.with_conn(|conn| {
        games::insert(
            conn,
            &InsertGame {
                id: game_id,
                name: &display_name,
                original_name: &probe.default_name,
                source_type: SourceType::Exe,
                source_path: &probe.source_path,
                launch_path: &probe.launch_path,
                launch_args: "",
                working_directory: Some(probe.working_directory.as_str()),
                cover_path: cover_path.as_deref(),
                icon_path: icon_path.as_deref(),
                is_launcher: false,
            },
        )
    })
}

pub(crate) fn add_game_from_shortcut_impl(
    db: &Db,
    data_dir: &Path,
    path: &str,
    name: Option<String>,
    launch_path_override: Option<String>,
    cover_source_path: Option<String>,
) -> AppResult<Game> {
    let probe = shortcut::resolve_shortcut(path)?;

    let override_path = launch_path_override
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    let launch_path = match &override_path {
        // 用户手动指定的目标同样要过一遍 EXE 校验，不能因为是「手动修正」就放松
        Some(value) => exe::inspect_exe(value)?.launch_path,
        None => {
            if !probe.target_exists {
                // 最后一道闸：绝不让失效目标以「可启动」的状态入库
                return Err(AppError::target_missing(
                    "快捷方式指向的目标不存在，请手动选择启动目标，或取消导入",
                ));
            }
            probe.launch_path.clone()
        }
    };

    // 手动指定目标时，工作目录跟随新目标；否则沿用快捷方式里的设置
    let working_directory = if override_path.is_some() {
        Path::new(&launch_path)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
    } else {
        Some(probe.working_directory.clone()).filter(|value| !value.trim().is_empty())
    };

    let display_name = normalise_name(name, &probe.default_name);
    // 启动器标记按**最终生效的目标**重新判定
    let is_launcher = shortcut::is_launcher_target(&launch_path);

    let game_id = games::new_id();
    let (cover_path, icon_path) = prepare_assets(
        data_dir,
        &game_id,
        &launch_path,
        cover_source_path.as_deref(),
    )?;

    db.with_conn(|conn| {
        games::insert(
            conn,
            &InsertGame {
                id: game_id,
                name: &display_name,
                original_name: &probe.default_name,
                source_type: SourceType::Shortcut,
                source_path: &probe.source_path,
                launch_path: &launch_path,
                // 参数原样保留：快捷方式的启动配置如实带过来，不做分词或重新转义
                launch_args: &probe.launch_args,
                working_directory: working_directory.as_deref(),
                cover_path: cover_path.as_deref(),
                icon_path: icon_path.as_deref(),
                is_launcher,
            },
        )
    })
}

fn normalise_name(name: Option<String>, fallback: &str) -> String {
    name.map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

/// 准备封面与图标，返回二者的相对路径。
///
/// 二者的失败策略**刻意不同**：
/// - **封面**是用户明确选的，失败就直接报错 —— 让用户重选，而不是悄悄丢掉；
/// - **图标**是「有更好、没有也能用」，失败返回 None。提不到图标绝不能阻断导入。
fn prepare_assets(
    data_dir: &Path,
    game_id: &str,
    launch_path: &str,
    cover_source_path: Option<&str>,
) -> AppResult<(Option<String>, Option<String>)> {
    let cover = match cover_source_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(source) => Some(cover_store::import(source, data_dir)?),
        None => None,
    };

    let icon = icon_store::extract_and_store(launch_path, data_dir, game_id)?;

    Ok((cover, icon))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::models::{GameSort, SortOrder};
    use crate::error::AppErrorCode;
    use crate::test_support::{TempDir, cleanup, make_shortcut, real_exe, write_test_png};

    fn real_exe_stem(exe_path: &str) -> String {
        Path::new(exe_path)
            .file_stem()
            .expect("有文件名")
            .to_string_lossy()
            .into_owned()
    }

    fn list_all(db: &Db) -> Vec<Game> {
        db.with_conn(|conn| games::list(conn, GameSort::Name, SortOrder::Asc))
            .expect("列出")
    }

    // ---------- EXE ----------

    #[test]
    fn probe_reports_no_duplicate_on_empty_library() {
        let Some(exe_path) = real_exe() else {
            eprintln!("跳过：本机找不到可用的系统 exe");
            return;
        };
        let db = Db::open_in_memory().expect("内存库");

        let result = probe_exe_impl(&db, exe_path).expect("探测应成功");
        assert!(result.duplicate.is_none(), "空库不应报重复");
        assert!(!result.probe.default_name.is_empty());
        assert!(result.probe.size_bytes > 0);
    }

    #[test]
    fn import_then_probe_reports_duplicate() {
        let Some(exe_path) = real_exe() else {
            return;
        };
        let temp = TempDir::new("import");
        let db = Db::open_in_memory().expect("内存库");

        let imported =
            add_game_from_exe_impl(&db, &temp.data(), exe_path, Some("我的游戏".into()), None)
                .expect("首次导入应成功");

        assert_eq!(imported.name, "我的游戏");
        assert_eq!(imported.source_type, "exe");
        assert_eq!(imported.original_name, real_exe_stem(exe_path));
        assert_eq!(imported.total_play_seconds, 0);

        let again = probe_exe_impl(&db, exe_path).expect("再次探测");
        assert_eq!(again.duplicate.expect("应检测到重复").id, imported.id);
    }

    #[test]
    fn duplicate_import_is_rejected_and_leaves_one_record() {
        let Some(exe_path) = real_exe() else {
            return;
        };
        let temp = TempDir::new("dup");
        let db = Db::open_in_memory().expect("内存库");

        add_game_from_exe_impl(&db, &temp.data(), exe_path, None, None).expect("首次导入");
        let err =
            add_game_from_exe_impl(&db, &temp.data(), exe_path, Some("换个名字".into()), None)
                .expect_err("第二次导入应被拒绝");

        assert_eq!(err.code(), AppErrorCode::Duplicate, "改显示名也必须判重");
        assert_eq!(list_all(&db).len(), 1);
    }

    #[test]
    fn blank_name_falls_back_to_file_stem() {
        let Some(exe_path) = real_exe() else {
            return;
        };
        let temp = TempDir::new("blank");
        let db = Db::open_in_memory().expect("内存库");

        let imported =
            add_game_from_exe_impl(&db, &temp.data(), exe_path, Some("   ".into()), None)
                .expect("空白名称应回退默认名");
        assert_eq!(imported.name, real_exe_stem(exe_path));
    }

    #[test]
    fn import_missing_file_reports_not_found() {
        let temp = TempDir::new("missing");
        let db = Db::open_in_memory().expect("内存库");
        let missing = std::env::temp_dir().join("gone-4b71.exe");

        let err = add_game_from_exe_impl(&db, &temp.data(), &missing.to_string_lossy(), None, None)
            .expect_err("文件不存在应报错");
        assert_eq!(err.code(), AppErrorCode::NotFound);
    }

    #[test]
    fn import_rejects_non_exe() {
        let temp = TempDir::new("non-exe");
        let db = Db::open_in_memory().expect("内存库");
        let err = add_game_from_exe_impl(
            &db,
            &temp.data(),
            r"C:\Windows\System32\drivers\etc\hosts",
            None,
            None,
        )
        .expect_err("非 exe 应被拒绝");
        assert_eq!(err.code(), AppErrorCode::InvalidArgument);
    }

    // ---------- 图标与封面 ----------

    #[test]
    fn import_extracts_and_stores_icon_for_real_exe() {
        let Some(exe_path) = real_exe() else {
            return;
        };
        let temp = TempDir::new("icon");
        let db = Db::open_in_memory().expect("内存库");

        let game = add_game_from_exe_impl(&db, &temp.data(), exe_path, None, None).expect("导入");

        let relative = game.icon_path.as_deref().expect("系统 exe 应能提取到图标");
        assert!(relative.starts_with("icons/"), "实际 {relative}");

        let absolute = temp
            .data()
            .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        assert!(absolute.exists(), "图标应已落盘: {}", absolute.display());

        let metadata = std::fs::metadata(&absolute).expect("读元数据");
        assert!(metadata.len() > 100, "图标文件太小，可能是空文件");
    }

    #[test]
    fn import_with_cover_stores_copy_and_keeps_original() {
        let Some(exe_path) = real_exe() else {
            return;
        };
        let temp = TempDir::new("cover");
        let db = Db::open_in_memory().expect("内存库");
        let source = write_test_png(&temp.source(), "my-cover.png", 320, 480);

        let game = add_game_from_exe_impl(
            &db,
            &temp.data(),
            exe_path,
            None,
            Some(source.to_string_lossy().into_owned()),
        )
        .expect("带封面导入应成功");

        let relative = game.cover_path.as_deref().expect("应有封面路径");
        assert!(relative.starts_with("covers/"));

        let stored = temp
            .data()
            .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        assert!(stored.exists(), "封面应已复制进数据目录");
        assert!(source.exists(), "**用户的原图必须原封不动** —— 我们只复制");
    }

    #[test]
    fn invalid_cover_aborts_import_without_leaving_a_record() {
        let Some(exe_path) = real_exe() else {
            return;
        };
        let temp = TempDir::new("bad-cover");
        let db = Db::open_in_memory().expect("内存库");

        let bad_cover = temp.source().join("not-an-image.png");
        std::fs::write(&bad_cover, vec![0x7fu8; 2048]).expect("写坏图");

        let err = add_game_from_exe_impl(
            &db,
            &temp.data(),
            exe_path,
            None,
            Some(bad_cover.to_string_lossy().into_owned()),
        )
        .expect_err("坏封面应导致导入失败");
        assert_eq!(err.code(), AppErrorCode::InvalidArgument);

        assert!(
            list_all(&db).is_empty(),
            "封面准备失败时不应留下任何游戏记录"
        );
    }

    // ---------- 快捷方式 ----------

    #[test]
    fn shortcut_import_preserves_source_type_and_args() {
        let Some(exe) = real_exe() else {
            return;
        };
        let Some((lnk, dir)) =
            make_shortcut("快捷方式游戏", exe, "-windowed", r"C:\Windows\System32")
        else {
            eprintln!("跳过：无法生成测试用 .lnk");
            return;
        };
        let temp = TempDir::new("lnk");
        let db = Db::open_in_memory().expect("内存库");

        let game = add_game_from_shortcut_impl(
            &db,
            &temp.data(),
            &lnk.to_string_lossy(),
            None,
            None,
            None,
        )
        .expect("导入应成功");

        assert_eq!(game.source_type, "shortcut");
        assert_eq!(game.name, "快捷方式游戏");
        assert_eq!(game.launch_args, "-windowed", "快捷方式的参数必须原样保留");
        assert!(game.launch_path.to_lowercase().ends_with(".exe"));
        assert_ne!(
            game.source_path.to_lowercase(),
            game.launch_path.to_lowercase(),
            "source_path 是 .lnk，launch_path 必须是解析后的目标"
        );

        cleanup(&dir);
    }

    #[test]
    fn shortcut_probe_reports_duplicate_on_second_look() {
        let Some(exe) = real_exe() else {
            return;
        };
        let Some((lnk, dir)) = make_shortcut("重复检查", exe, "-abc", "") else {
            return;
        };
        let temp = TempDir::new("lnk-dup");
        let db = Db::open_in_memory().expect("内存库");

        let first = probe_shortcut_impl(&db, &lnk.to_string_lossy()).expect("首次探测");
        assert!(first.duplicate.is_none());

        let imported = add_game_from_shortcut_impl(
            &db,
            &temp.data(),
            &lnk.to_string_lossy(),
            None,
            None,
            None,
        )
        .expect("导入");

        let second = probe_shortcut_impl(&db, &lnk.to_string_lossy()).expect("再次探测");
        assert_eq!(second.duplicate.expect("应检测到重复").id, imported.id);

        let err = add_game_from_shortcut_impl(
            &db,
            &temp.data(),
            &lnk.to_string_lossy(),
            None,
            None,
            None,
        )
        .expect_err("重复导入应被拒");
        assert_eq!(err.code(), AppErrorCode::Duplicate);
        assert_eq!(list_all(&db).len(), 1);

        cleanup(&dir);
    }

    #[test]
    fn missing_target_without_override_is_rejected() {
        let Some((lnk, dir)) =
            make_shortcut("失效链接", r"C:\definitely-missing-9f3a\game.exe", "", "")
        else {
            return;
        };
        let temp = TempDir::new("lnk-missing");
        let db = Db::open_in_memory().expect("内存库");

        let probe = probe_shortcut_impl(&db, &lnk.to_string_lossy()).expect("探测应成功");
        assert!(!probe.probe.target_exists);

        let err = add_game_from_shortcut_impl(
            &db,
            &temp.data(),
            &lnk.to_string_lossy(),
            None,
            None,
            None,
        )
        .expect_err("失效目标不得静默入库");
        assert_eq!(err.code(), AppErrorCode::TargetMissing);
        assert!(list_all(&db).is_empty(), "被拦下的导入不应留下记录");

        cleanup(&dir);
    }

    #[test]
    fn missing_target_with_exe_override_is_accepted() {
        let Some(exe) = real_exe() else {
            return;
        };
        let Some((lnk, dir)) =
            make_shortcut("修正目标", r"C:\definitely-missing-9f3a\game.exe", "", "")
        else {
            return;
        };
        let temp = TempDir::new("lnk-override");
        let db = Db::open_in_memory().expect("内存库");

        let game = add_game_from_shortcut_impl(
            &db,
            &temp.data(),
            &lnk.to_string_lossy(),
            None,
            Some(exe.to_owned()),
            None,
        )
        .expect("手动修正后应导入成功");

        assert!(game.launch_path.to_lowercase().contains("system32"));
        assert_eq!(game.source_type, "shortcut");
        assert!(
            game.working_directory
                .as_deref()
                .is_some_and(|path| path.to_lowercase().contains("system32")),
            "工作目录应跟随修正后的目标"
        );

        cleanup(&dir);
    }

    #[test]
    fn override_to_non_exe_is_rejected() {
        let Some((lnk, dir)) =
            make_shortcut("坏修正", r"C:\definitely-missing-9f3a\game.exe", "", "")
        else {
            return;
        };
        let temp = TempDir::new("lnk-bad-override");
        let db = Db::open_in_memory().expect("内存库");

        let err = add_game_from_shortcut_impl(
            &db,
            &temp.data(),
            &lnk.to_string_lossy(),
            None,
            Some(r"C:\Windows\System32\drivers\etc\hosts".to_owned()),
            None,
        )
        .expect_err("手动修正的目标同样必须是 exe");
        assert_eq!(err.code(), AppErrorCode::InvalidArgument);

        cleanup(&dir);
    }

    #[test]
    fn lnk_name_does_not_affect_launcher_flag() {
        let Some(exe) = real_exe() else {
            return;
        };
        let Some((lnk, dir)) = make_shortcut("steam", exe, "", "") else {
            return;
        };
        let temp = TempDir::new("lnk-launcher");
        let db = Db::open_in_memory().expect("内存库");

        let game = add_game_from_shortcut_impl(
            &db,
            &temp.data(),
            &lnk.to_string_lossy(),
            None,
            None,
            None,
        )
        .expect("导入应成功");
        assert!(
            !game.is_launcher,
            "目标不是启动器，不该因为 .lnk 叫 steam 就被打标记"
        );

        cleanup(&dir);
    }
}
