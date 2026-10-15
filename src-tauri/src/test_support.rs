//! 测试辅助。
//!
//! 快捷方式相关的测试需要**真实**的 `.lnk`（COM 无法用假数据糊弄），
//! 这里用 PowerShell 的 `WScript.Shell` 在临时目录里生成，不碰用户桌面。
//! 只在 `#[cfg(test)]` 下编译。

use std::path::{Path, PathBuf};

/// 本机上确实存在的系统可执行文件。返回 None 表示没有可用样本。
pub fn real_exe() -> Option<&'static str> {
    [
        r"C:\Windows\System32\notepad.exe",
        r"C:\Windows\System32\where.exe",
        r"C:\Windows\System32\mspaint.exe",
    ]
    .into_iter()
    .find(|path| Path::new(path).exists())
}

/// 在临时目录生成一个真实 `.lnk`。
///
/// 返回 `(lnk 路径, 临时目录)`；生成失败返回 None（调用方应跳过该用例，
/// 而不是把环境问题伪装成通过）。
pub fn make_shortcut(
    name: &str,
    target: &str,
    args: &str,
    working_dir: &str,
) -> Option<(PathBuf, PathBuf)> {
    let dir = std::env::temp_dir().join(format!("gamelib-lnk-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).ok()?;
    let lnk = dir.join(format!("{name}.lnk"));

    let script = format!(
        "$w = New-Object -ComObject WScript.Shell; \
         $s = $w.CreateShortcut('{}'); \
         $s.TargetPath = '{}'; \
         $s.Arguments = '{}'; \
         $s.WorkingDirectory = '{}'; \
         $s.Save()",
        lnk.display(),
        target,
        args,
        working_dir,
    );

    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .ok()?;

    if !status.success() || !lnk.exists() {
        std::fs::remove_dir_all(&dir).ok();
        return None;
    }

    Some((lnk, dir))
}

/// 清理 `make_shortcut` 产生的临时目录。
pub fn cleanup(dir: &Path) {
    std::fs::remove_dir_all(dir).ok();
}

/// 临时目录守卫，Drop 时自动清理。
///
/// 结构：`<root>/data`（模拟应用数据目录）、`<root>/source`（模拟用户文件所在目录）。
pub struct TempDir {
    root: PathBuf,
}

impl TempDir {
    pub fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!("gamelib-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("data")).expect("建 data 目录");
        std::fs::create_dir_all(root.join("source")).expect("建 source 目录");
        Self { root }
    }

    /// 模拟的应用数据目录。
    pub fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    /// 模拟的用户文件目录（放封面等素材）。
    pub fn source(&self) -> PathBuf {
        self.root.join("source")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

/// 在给定目录写出一张真实的小 PNG，返回其路径。
pub fn write_test_png(directory: &Path, name: &str, width: u32, height: u32) -> PathBuf {
    let path = directory.join(name);
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([200, 120, 60, 255]));
    image.save(&path).expect("写出测试图片");
    path
}
