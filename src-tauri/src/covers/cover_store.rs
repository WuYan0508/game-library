//! 自定义封面的校验与落盘，以及未被引用资源的回收。

use std::collections::HashSet;
use std::path::Path;

use crate::error::{AppError, AppResult};

use super::icon_store::ICONS_DIR;

/// 相对数据根目录的封面目录名。
pub const COVERS_DIR: &str = "covers";

/// 单个封面的体积上限。
pub const MAX_COVER_BYTES: u64 = 20 * 1024 * 1024;
/// 封面的单边像素上限，防止超大图解码时吃爆内存。
pub const MAX_COVER_DIMENSION: u32 = 8192;

/// 允许的图片扩展名（小写）。
const ALLOWED_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

/// 校验用户选择的封面并**复制**进应用数据目录。
///
/// 返回相对数据根目录的路径（`covers/<uuid>.<ext>`）。
/// 文件名用 UUID 生成，与游戏 id 解耦 —— 这样即使是先落盘封面、后写游戏记录，
/// 也不会出现半成品状态。
///
/// 注意：**只复制，不移动、不删除用户的原图。**
pub fn import(source_path: &str, data_dir: &Path) -> AppResult<String> {
    let trimmed = source_path.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_argument("没有选择封面文件"));
    }

    let path = Path::new(trimmed);

    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_lowercase)
        .filter(|ext| ALLOWED_EXTENSIONS.contains(&ext.as_str()))
        .ok_or_else(|| AppError::invalid_argument("封面只支持 PNG、JPG / JPEG、WebP 格式"))?;

    let metadata = std::fs::metadata(path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => {
            AppError::not_found("封面文件不存在或已被移动").with_detail(format!("{trimmed}: {err}"))
        }
        std::io::ErrorKind::PermissionDenied => {
            AppError::permission_denied("无法读取该封面文件，可能没有访问权限")
                .with_detail(format!("{trimmed}: {err}"))
        }
        _ => AppError::internal("读取封面文件信息失败").with_detail(format!("{trimmed}: {err}")),
    })?;

    if !metadata.is_file() {
        return Err(AppError::invalid_argument("请选择图片文件，而不是文件夹"));
    }

    if metadata.len() > MAX_COVER_BYTES {
        return Err(AppError::invalid_argument(format!(
            "封面文件过大（{:.1} MB），上限为 20 MB",
            metadata.len() as f64 / (1024.0 * 1024.0)
        )));
    }

    // 只读图片头部拿尺寸，不整图解码
    let (width, height) = image::image_dimensions(path).map_err(|err| {
        AppError::invalid_argument("无法识别这个图片文件，可能已损坏或格式不受支持")
            .with_detail(format!("{trimmed}: {err}"))
    })?;

    if width > MAX_COVER_DIMENSION || height > MAX_COVER_DIMENSION {
        return Err(AppError::invalid_argument(format!(
            "封面尺寸过大（{width}×{height}），单边上限为 {MAX_COVER_DIMENSION} 像素"
        )));
    }

    if width == 0 || height == 0 {
        return Err(AppError::invalid_argument("封面图片尺寸无效"));
    }

    // jpeg 统一存成 .jpg，避免同名不同扩展
    let stored_extension = if extension == "jpeg" {
        "jpg"
    } else {
        &extension
    };
    let file_name = format!("{}.{stored_extension}", uuid::Uuid::new_v4());

    let directory = data_dir.join(COVERS_DIR);
    std::fs::create_dir_all(&directory).map_err(|err| {
        AppError::internal("无法创建封面目录")
            .with_detail(format!("{}: {err}", directory.display()))
    })?;

    let destination = directory.join(&file_name);
    std::fs::copy(path, &destination).map_err(|err| {
        AppError::internal("复制封面文件失败").with_detail(format!(
            "{} -> {}: {err}",
            path.display(),
            destination.display()
        ))
    })?;

    Ok(format!("{COVERS_DIR}/{file_name}"))
}

/// 删除某个已落盘的封面 / 图标文件（不存在时静默跳过）。
pub fn remove_stored(data_dir: &Path, relative_path: &str) {
    let absolute = data_dir.join(relative_path.replace('/', std::path::MAIN_SEPARATOR_STR));
    if absolute.exists()
        && let Err(err) = std::fs::remove_file(&absolute)
    {
        log::warn!("删除资源文件失败 {}: {err}", absolute.display());
    }
}

/// 回收 `covers/` 与 `icons/` 中**没有任何记录引用**的文件。
///
/// 安全约束：只删「确实由本应用生成」的文件 ——
/// - `covers/` 下必须是 `<UUID>.<允许的扩展名>`；
/// - `icons/` 下必须是 `*.png`。
///
/// 用户自己丢进来的文件一律不动。
pub fn gc(referenced: &HashSet<String>, data_dir: &Path) -> AppResult<usize> {
    let mut removed = 0;

    for directory_name in [COVERS_DIR, ICONS_DIR] {
        let directory = data_dir.join(directory_name);
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            // 目录不存在 = 没有可回收的东西
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let relative = format!("{directory_name}/{file_name}");

            if referenced.contains(&relative) || !is_managed_asset(directory_name, &file_name) {
                continue;
            }

            match std::fs::remove_file(entry.path()) {
                Ok(()) => {
                    removed += 1;
                    log::info!("回收未被引用的资源: {relative}");
                }
                Err(err) => log::warn!("回收资源失败 {relative}: {err}"),
            }
        }
    }

    Ok(removed)
}

/// 判断文件是否由本应用生成 —— 决定能不能删。
fn is_managed_asset(directory_name: &str, file_name: &str) -> bool {
    let lower = file_name.to_lowercase();

    if directory_name == COVERS_DIR {
        let Some((stem, extension)) = lower.rsplit_once('.') else {
            return false;
        };
        return ALLOWED_EXTENSIONS.contains(&extension) && uuid::Uuid::parse_str(stem).is_ok();
    }

    lower.ends_with(".png")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(std::path::PathBuf, std::path::PathBuf);

    impl TempDir {
        /// 建一个临时「数据目录」，内部再放一个用于存放源图片的目录。
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("gamelib-cover-{}", uuid::Uuid::new_v4()));
            let data = root.join("data");
            let source = root.join("source");
            std::fs::create_dir_all(&data).expect("建数据目录");
            std::fs::create_dir_all(&source).expect("建源目录");
            Self(data, source)
        }

        fn data(&self) -> &Path {
            &self.0
        }

        fn source(&self) -> &Path {
            &self.1
        }

        /// 写一张真实的小 PNG，返回其路径。
        fn write_png(&self, name: &str, width: u32, height: u32) -> std::path::PathBuf {
            let path = self.1.join(name);
            let image = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
            image.save(&path).expect("写出测试图片");
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(self.0.parent().unwrap_or(&self.0)).ok();
        }
    }

    #[test]
    fn imports_valid_png_into_covers_directory() {
        let temp = TempDir::new();
        let source = temp.write_png("cover.png", 64, 64);

        let relative = import(&source.to_string_lossy(), temp.data()).expect("导入应成功");
        assert!(relative.starts_with("covers/"), "实际 {relative}");
        assert!(relative.ends_with(".png"));

        let absolute = temp
            .data()
            .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        assert!(absolute.exists(), "封面应已被复制到数据目录");
        assert!(source.exists(), "**用户原图必须还在** —— 我们只复制不移动");
    }

    #[test]
    fn rejects_unsupported_extension() {
        let temp = TempDir::new();
        let source = temp.source().join("animation.gif");
        std::fs::write(&source, b"GIF89a").expect("写文件");

        let err = import(&source.to_string_lossy(), temp.data()).expect_err("gif 应被拒绝");
        assert_eq!(err.code(), crate::error::AppErrorCode::InvalidArgument);
        assert!(err.message().contains("PNG"), "错误说明应列出支持的格式");
    }

    #[test]
    fn rejects_missing_file() {
        let temp = TempDir::new();
        let missing = temp.source().join("nope.png");

        let err = import(&missing.to_string_lossy(), temp.data()).expect_err("不存在应报错");
        assert_eq!(err.code(), crate::error::AppErrorCode::NotFound);
    }

    #[test]
    fn rejects_corrupted_image() {
        let temp = TempDir::new();
        let source = temp.source().join("broken.png");
        std::fs::write(&source, vec![0x42u8; 4096]).expect("写坏文件");

        let err = import(&source.to_string_lossy(), temp.data()).expect_err("坏图应被拒绝");
        assert_eq!(err.code(), crate::error::AppErrorCode::InvalidArgument);
    }

    #[test]
    fn rejects_oversized_file_before_reading_it() {
        let temp = TempDir::new();
        let source = temp.source().join("huge.png");
        // 体积检查在解码之前，所以内容是垃圾也无所谓
        let file = std::fs::File::create(&source).expect("建文件");
        file.set_len(MAX_COVER_BYTES + 1).expect("撑大文件");

        let err = import(&source.to_string_lossy(), temp.data()).expect_err("超大文件应被拒绝");
        assert_eq!(err.code(), crate::error::AppErrorCode::InvalidArgument);
        assert!(err.message().contains("20 MB"));
    }

    #[test]
    fn gc_removes_unreferenced_managed_files_only() {
        let temp = TempDir::new();
        let covers = temp.data().join(COVERS_DIR);
        let icons = temp.data().join(ICONS_DIR);
        std::fs::create_dir_all(&covers).expect("建 covers");
        std::fs::create_dir_all(&icons).expect("建 icons");

        let kept = format!("{}.png", uuid::Uuid::new_v4());
        let orphan = format!("{}.png", uuid::Uuid::new_v4());
        let orphan_icon = "game-1234.png".to_owned();

        std::fs::write(covers.join(&kept), b"x").expect("写 kept");
        std::fs::write(covers.join(&orphan), b"x").expect("写 orphan");
        // 不是 UUID 命名 —— 不是我们生成的文件，绝不能删
        std::fs::write(covers.join("user-put-this-here.png"), b"x").expect("写用户文件");
        std::fs::write(icons.join(&orphan_icon), b"x").expect("写 orphan icon");

        let referenced: HashSet<String> = [format!("{COVERS_DIR}/{kept}")].into_iter().collect();
        let removed = gc(&referenced, temp.data()).expect("回收应成功");

        assert_eq!(removed, 2, "应回收 1 个封面 + 1 个图标");
        assert!(covers.join(&kept).exists(), "被引用的封面不能删");
        assert!(!covers.join(&orphan).exists(), "无引用的封面应被回收");
        assert!(
            covers.join("user-put-this-here.png").exists(),
            "**用户自己放进去的文件一律不动**"
        );
    }

    #[test]
    fn gc_on_missing_directories_is_a_noop() {
        let temp = TempDir::new();
        let referenced = HashSet::new();
        assert_eq!(gc(&referenced, temp.data()).expect("不应报错"), 0);
    }

    #[test]
    fn managed_asset_recognition() {
        let uuid_name = format!("{}.png", uuid::Uuid::new_v4());
        assert!(is_managed_asset(COVERS_DIR, &uuid_name));
        assert!(is_managed_asset(COVERS_DIR, &uuid_name.to_uppercase()));
        assert!(!is_managed_asset(COVERS_DIR, "not-a-uuid.png"));
        assert!(!is_managed_asset(
            COVERS_DIR,
            &format!("{}.gif", uuid::Uuid::new_v4())
        ));
        assert!(is_managed_asset(ICONS_DIR, "abc.png"));
        assert!(!is_managed_asset(ICONS_DIR, "abc.jpg"));
    }
}
