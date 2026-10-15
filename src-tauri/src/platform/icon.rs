//! 图标提取：把可执行文件的 Shell 图标取出并编码为 PNG。
//!
//! 两条路径：
//! 1. `IShellItemImageFactory::GetImage` —— 拿 256px 图标，质量最好（首选）；
//! 2. `SHGetFileInfoW` + `SHGFI_ICON | SHGFI_LARGEICON` —— 回退到系统小图标。
//!
//! 两条路径最终都归一到「32 位 BGRA 位图 → RGBA → PNG」，编码用 `image` crate，
//! 因此**不依赖 WIC 编码器**（少一层 COM 流与帧的样板代码）。
//!
//! 任何一步失败都返回 `Ok(None)` —— **图标提取失败绝不能阻断游戏导入**。

use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DIBSECTION, DeleteDC, DeleteObject, GetDC, GetDIBits, GetObjectW, HBITMAP, HDC, HGDIOBJ,
    ReleaseDC, SelectObject,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
    SHGetFileInfoW, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx, HICON};
use windows::core::PCWSTR;

use crate::error::AppResult;
use crate::platform::win::{run_sta, to_wide};

/// 首选图标边长。256px 在卡片上放大后仍然清晰。
const PREFERRED_ICON_SIZE: i32 = 256;
/// 回退路径的绘制边长（系统大图标通常是 32px，用 48 兜住 DPI 缩放）。
const FALLBACK_ICON_SIZE: i32 = 48;

/// 提取串行锁。
///
/// Shell 的图像缓存是**进程级共享**的：多个线程同时提取同一个文件会互相干扰，
/// 表现为 `GetImage` 偶发返回空位图（实测在并行跑测试时能复现）。
/// 图标提取本身很快、也不高频，直接串行化，换取确定性。
static EXTRACTION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 一张已解码为 RGBA 的位图。
#[derive(Debug, Clone)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    /// 长度为 `width * height * 4`，顺序 R,G,B,A。
    pub pixels: Vec<u8>,
}

/// 提取图标并编码为 PNG。拿不到图标返回 `Ok(None)`。
pub fn extract_icon_png(file_path: &str) -> AppResult<Option<Vec<u8>>> {
    let owned = file_path.to_owned();

    // 串行化：见 EXTRACTION_LOCK 的说明。锁被污染时不让应用崩，继续走。
    let _guard = EXTRACTION_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // COM + GDI 都在专用 STA 线程里完成，只把纯数据带回来
    let image = run_sta(move || Ok(grab_rgba(&owned)))?;

    match image {
        Some(image) => Ok(encode_png(&normalize_visible_content(&image))),
        None => Ok(None),
    }
}

/// 可见像素判定阈值：alpha 低于它的视作透明留白。
const VISIBLE_ALPHA_THRESHOLD: u8 = 16;
/// 可见内容占画布超过这个比例就不再放大（本来就满的图标二次放大会糊）。
const FULL_CONTENT_RATIO: f32 = 0.92;

/// 求可见（alpha > 阈值）像素的包围盒，返回 `(min_x, min_y, max_x_excl, max_y_excl)`。
fn visible_bbox(image: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;

    for y in 0..image.height {
        for x in 0..image.width {
            let alpha = image.pixels[((y * image.width + x) * 4 + 3) as usize];
            if alpha > VISIBLE_ALPHA_THRESHOLD {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + 1);
                max_y = max_y.max(y + 1);
            }
        }
    }

    if max_x == 0 {
        None
    } else {
        Some((min_x, min_y, max_x, max_y))
    }
}

/// 把「logo 居中在透明画布上」的图标裁剪到可见内容并等比放大铺满画布。
///
/// 真实案例：TCLS\client.exe 的图标只有 ~88×88 的 logo 居中在 256×256 透明画布里，
/// 卡片上看起来比其它图标小一圈。按可见 alpha 求包围盒，裁剪后用 Lanczos
/// 重采样放回原尺寸画布。已经铺满的图标原样返回，不做二次放大。
fn normalize_visible_content(image: &RgbaImage) -> RgbaImage {
    let Some((min_x, min_y, max_x, max_y)) = visible_bbox(image) else {
        // 全透明：交给上游 image_is_usable 拒绝
        return image.clone();
    };

    let content_w = (max_x - min_x) as f32;
    let content_h = (max_y - min_y) as f32;
    let canvas = image.width.max(image.height) as f32;
    if content_w / canvas >= FULL_CONTENT_RATIO && content_h / canvas >= FULL_CONTENT_RATIO {
        return image.clone();
    }

    let Some(buffer) = image::RgbaImage::from_raw(image.width, image.height, image.pixels.clone())
    else {
        return image.clone();
    };
    let cropped =
        image::imageops::crop_imm(&buffer, min_x, min_y, max_x - min_x, max_y - min_y).to_image();

    // 等比放大到长边铺满画布
    let target = image.width.max(image.height);
    let (new_w, new_h) = if cropped.width() >= cropped.height() {
        (
            target,
            (cropped.height() as f32 * target as f32 / cropped.width() as f32)
                .round()
                .max(1.0) as u32,
        )
    } else {
        (
            (cropped.width() as f32 * target as f32 / cropped.height() as f32)
                .round()
                .max(1.0) as u32,
            target,
        )
    };
    let scaled = image::imageops::resize(
        &cropped,
        new_w,
        new_h,
        image::imageops::FilterType::Lanczos3,
    );

    // 居中贴回透明画布
    let mut canvas_img = image::RgbaImage::new(image.width, image.height);
    let offset_x = (image.width.saturating_sub(scaled.width())) / 2;
    let offset_y = (image.height.saturating_sub(scaled.height())) / 2;
    image::imageops::overlay(&mut canvas_img, &scaled, offset_x as i64, offset_y as i64);

    RgbaImage {
        width: image.width,
        height: image.height,
        pixels: canvas_img.into_raw(),
    }
}

/// 编码为 PNG。编码本身不碰 COM，放在 STA 线程之外做。
fn encode_png(image: &RgbaImage) -> Option<Vec<u8>> {
    let buffer = image::RgbaImage::from_raw(image.width, image.height, image.pixels.clone())?;
    let mut out = std::io::Cursor::new(Vec::new());
    buffer.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

fn grab_rgba(file_path: &str) -> Option<RgbaImage> {
    let wide = to_wide(file_path);

    // SAFETY: 所有句柄都在各自函数内创建并释放；缓冲区按 API 返回的尺寸分配。
    unsafe {
        let primary = match grab_via_shell_item(&wide) {
            Ok(image) => Some(image),
            Err(reason) => {
                // 降级是允许的，但要知道为什么降级 —— 否则「图标很糊」会变成无头案
                log::debug!("大图标提取失败，回退到系统小图标：{reason}");
                None
            }
        };

        // 主路径的已知缺陷（2026-10-14 实测）：图标组缺少大尺寸资源时，Windows 返回
        // 「小 logo 居中在带框画布上」的图 —— 可见像素占比极低（TCLS client.exe 实测 ~12%），
        // 而系统小图标（SHGetFileInfoW，48×48）反而是**按请求尺寸铺满**的，观感更好。
        // 所以主路径产出「稀疏」图标时，尝试回退路径并择优。
        let needs_fallback = primary
            .as_ref()
            .map(|image| visible_pixel_ratio(image) < 0.35)
            .unwrap_or(true);

        if !needs_fallback {
            return primary;
        }

        match grab_via_file_info(&wide) {
            Ok(small) => {
                let small_ratio = visible_pixel_ratio(&small);
                let primary_ratio = primary.as_ref().map(visible_pixel_ratio).unwrap_or(0.0);
                if small_ratio >= primary_ratio {
                    log::debug!(
                        "主路径图标内容稀疏（{}%），改用系统小图标（{}%）",
                        (primary_ratio * 100.0).round(),
                        (small_ratio * 100.0).round()
                    );
                    Some(small)
                } else {
                    primary
                }
            }
            Err(reason) => {
                log::debug!("系统小图标也拿不到，沿用主路径结果或使用占位图：{reason}");
                primary
            }
        }
    }
}

/// 可见像素占比（alpha > `VISIBLE_ALPHA_THRESHOLD` 的像素比例）。
fn visible_pixel_ratio(image: &RgbaImage) -> f32 {
    let total = (image.width as usize) * (image.height as usize);
    if total == 0 {
        return 0.0;
    }
    let visible = image
        .pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[3] > VISIBLE_ALPHA_THRESHOLD)
        .count();
    visible as f32 / total as f32
}

/// 首选路径：`IShellItemImageFactory`，可拿到 256px 大图标。
unsafe fn grab_via_shell_item(wide_path: &[u16]) -> Result<RgbaImage, String> {
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(PCWSTR(wide_path.as_ptr()), None) }
            .map_err(|err| format!("SHCreateItemFromParsingName 失败: {err}"))?;

    let requested = SIZE {
        cx: PREFERRED_ICON_SIZE,
        cy: PREFERRED_ICON_SIZE,
    };
    let flags = SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK;

    let bitmap: HBITMAP = unsafe { factory.GetImage(requested, flags) }
        .map_err(|err| format!("GetImage 失败: {err}"))?;

    let image = unsafe { hbitmap_to_rgba(bitmap) };
    // SAFETY: bitmap 由 GetImage 产生，用完即释放
    let _ = unsafe { DeleteObject(HGDIOBJ(bitmap.0)) };

    let image = image.ok_or_else(|| "位图转换失败（可能返回了 0 尺寸位图）".to_owned())?;
    if !image_is_usable(&image) {
        return Err("返回的位图全透明".to_owned());
    }
    Ok(image)
}

/// 回退路径：`SHGetFileInfoW` 拿系统图标，再画到 32 位 DIB 上取像素。
unsafe fn grab_via_file_info(wide_path: &[u16]) -> Result<RgbaImage, String> {
    let mut info = SHFILEINFOW::default();
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide_path.as_ptr()),
            Default::default(),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if result == 0 {
        return Err("SHGetFileInfoW 返回 0".to_owned());
    }
    if info.hIcon.is_invalid() {
        return Err("SHGetFileInfoW 未返回图标句柄".to_owned());
    }

    let icon: HICON = info.hIcon;
    let image = unsafe { hicon_to_rgba(icon) };

    let _ = unsafe { DestroyIcon(icon) };
    image.ok_or_else(|| "HICON 绘制到位图失败".to_owned())
}

/// HBITMAP → RGBA。
///
/// 先尝试直读 DIB section 的位数据 —— `IShellItemImageFactory::GetImage` 返回的正是
/// DIB section，直读 `bmBits` 最可靠（实测 `GetDIBits` 对它反而会失败）。
/// 不是 DIB section（普通 DDB）时再走 `GetDIBits` 兜底。
unsafe fn hbitmap_to_rgba(bitmap: HBITMAP) -> Option<RgbaImage> {
    if let Some(image) = unsafe { read_dib_section(bitmap) } {
        return Some(image);
    }

    let hdc: HDC = unsafe { GetDC(None) };
    if hdc.is_invalid() {
        return None;
    }

    let image = unsafe { read_dib_pixels(hdc, bitmap) };
    unsafe { ReleaseDC(None, hdc) };
    image
}

/// 直读 32 位 DIB section 的像素。
unsafe fn read_dib_section(bitmap: HBITMAP) -> Option<RgbaImage> {
    let mut section = DIBSECTION::default();
    let copied = unsafe {
        GetObjectW(
            HGDIOBJ(bitmap.0),
            std::mem::size_of::<DIBSECTION>() as i32,
            Some(&mut section as *mut DIBSECTION as *mut core::ffi::c_void),
        )
    };
    if copied == 0 || section.dsBm.bmBits.is_null() {
        return None;
    }

    // 只直读 32 位；其它位深交给 GetDIBits 兜底
    if section.dsBmih.biBitCount != 32 {
        return None;
    }

    let width = section.dsBm.bmWidth;
    let height = section.dsBm.bmHeight;
    if width <= 0 || height == 0 {
        return None;
    }

    let w = width as usize;
    let h = height.unsigned_abs() as usize;
    let stride = w * 4;

    // SAFETY: bmBits 指向该 DIB section 的像素区，大小为 stride * h
    let raw = unsafe { std::slice::from_raw_parts(section.dsBm.bmBits as *const u8, stride * h) };

    // 正高度表示自下而上存储，需要逐行翻正；负高度已经是自上而下
    let top_down = section.dsBmih.biHeight < 0;
    let mut ordered = vec![0u8; stride * h];
    for row in 0..h {
        let source_row = if top_down { row } else { h - 1 - row };
        ordered[row * stride..(row + 1) * stride]
            .copy_from_slice(&raw[source_row * stride..(source_row + 1) * stride]);
    }

    Some(unsafe { bgra_to_rgba(&ordered, w as u32, h as u32) })
}

/// HICON → RGBA：新建一张 32 位 DIB，先把像素清零（保证完全透明），
/// 再用 `DrawIconEx` 以 `DI_NORMAL` 画上去 —— 这样 alpha 由 GDI 正确合成，
/// 不必自己去处理图标掩码（AND mask）。
unsafe fn hicon_to_rgba(icon: HICON) -> Option<RgbaImage> {
    let screen: HDC = unsafe { GetDC(None) };
    if screen.is_invalid() {
        return None;
    }
    let mem_dc: HDC = unsafe { CreateCompatibleDC(Some(screen)) };
    if mem_dc.is_invalid() {
        unsafe { ReleaseDC(None, screen) };
        return None;
    }

    let width = FALLBACK_ICON_SIZE;
    let height = FALLBACK_ICON_SIZE;

    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // 负高度 = 自上而下，与 RGBA 的行序一致，省一次翻转
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let dib = unsafe { CreateDIBSection(Some(mem_dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) };

    let image = match dib {
        Ok(dib) if !bits.is_null() => {
            let pixel_count = (width as usize) * (height as usize);
            let slice = unsafe { std::slice::from_raw_parts_mut(bits as *mut u8, pixel_count * 4) };
            slice.fill(0);

            let previous = unsafe { SelectObject(mem_dc, HGDIOBJ(dib.0)) };
            let drawn = unsafe {
                DrawIconEx(mem_dc, 0, 0, icon, width, height, 0, None, DI_NORMAL).is_ok()
            };
            unsafe { SelectObject(mem_dc, previous) };

            if drawn {
                Some(unsafe { bgra_to_rgba(slice, width as u32, height as u32) })
            } else {
                None
            }
        }
        _ => None,
    };

    if let Ok(dib) = dib {
        let _ = unsafe { DeleteObject(HGDIOBJ(dib.0)) };
    }
    let _ = unsafe { DeleteDC(mem_dc) };
    unsafe { ReleaseDC(None, screen) };
    image
}

/// 用 `GetDIBits` 读出位图的 32 位像素。
unsafe fn read_dib_pixels(hdc: HDC, bitmap: HBITMAP) -> Option<RgbaImage> {
    let mut header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };

    // 先问尺寸：传空的 bits 指针，biHeight 会回填真实高度
    let probe = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };
    let mut probe = probe;
    if unsafe { GetDIBits(hdc, bitmap, 0, 0, None, &mut probe, DIB_RGB_COLORS) } == 0 {
        return None;
    }
    if probe.bmiHeader.biWidth <= 0 || probe.bmiHeader.biHeight == 0 {
        return None;
    }

    let width = probe.bmiHeader.biWidth as u32;
    let height = probe.bmiHeader.biHeight.unsigned_abs();
    header.biWidth = probe.bmiHeader.biWidth;
    header.biHeight = -(height as i32); // 自上而下
    header.biSizeImage = width * height * 4;

    let mut buffer = vec![0u8; (width as usize) * (height as usize) * 4];
    let mut info = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };
    let copied = unsafe {
        GetDIBits(
            hdc,
            bitmap,
            0,
            height,
            Some(buffer.as_mut_ptr() as *mut core::ffi::c_void),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    if copied == 0 {
        return None;
    }

    Some(unsafe { bgra_to_rgba(&buffer, width, height) })
}

/// BGRA → RGBA，就地交换 R 与 B。
///
/// 全透明像素的 RGB 一并清零：`IShellItemImageFactory` 输出的透明区域可能带
/// 未定义的颜色值（甚至是预乘后的残值），留着会在某些渲染路径下渗出色边。
unsafe fn bgra_to_rgba(bgra: &[u8], width: u32, height: u32) -> RgbaImage {
    let mut pixels = bgra.to_vec();
    for chunk in pixels.as_chunks_mut::<4>().0 {
        chunk.swap(0, 2);
        if chunk[3] == 0 {
            chunk[0] = 0;
            chunk[1] = 0;
            chunk[2] = 0;
        }
    }

    RgbaImage {
        width,
        height,
        pixels,
    }
}

/// 判断位图里是否真的有像素。`IShellItemImageFactory` 对没有图标的文件
/// 有时会返回一张 0 尺寸或全透明的位图。
pub fn image_is_usable(image: &RgbaImage) -> bool {
    !image.pixels.is_empty() && image.pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real_exe() -> Option<&'static str> {
        [
            r"C:\Windows\System32\notepad.exe",
            r"C:\Windows\System32\mspaint.exe",
            r"C:\Windows\System32\where.exe",
        ]
        .into_iter()
        .find(|path| std::path::Path::new(path).exists())
    }

    #[test]
    fn extracts_a_real_icon_as_png() {
        let Some(exe) = real_exe() else {
            eprintln!("跳过：本机找不到可用的系统 exe");
            return;
        };

        let bytes = extract_icon_png(exe)
            .expect("提取不应报错")
            .expect("系统 exe 应该有图标");
        assert!(
            bytes.len() > 100,
            "PNG 太小，可能不是有效图片，实际 {} 字节",
            bytes.len()
        );
        // PNG magic number
        assert_eq!(
            &bytes[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
            "输出必须是合法 PNG"
        );
    }

    #[test]
    fn decoded_png_has_expected_size_and_visible_pixels() {
        let Some(exe) = real_exe() else {
            return;
        };
        let bytes = extract_icon_png(exe).expect("提取").expect("有图标");

        let decoded = image::load_from_memory(&bytes).expect("应能解码回图片");
        assert!(decoded.width() >= 16 && decoded.height() >= 16);
        assert_eq!(
            decoded.width(),
            decoded.height(),
            "图标应当是正方形，实际 {}x{}",
            decoded.width(),
            decoded.height()
        );

        let rgba = decoded.to_rgba8();
        let opaque = rgba.pixels().filter(|pixel| pixel.0[3] > 0).count();
        assert!(opaque > 0, "整张图全透明，说明转换链路有问题");
    }

    #[test]
    fn missing_file_yields_none_or_error_without_panicking() {
        let missing = std::env::temp_dir().join("no-such-icon-4b71.exe");
        // 允许返回 None，也允许返回错误；关键是**不能 panic、不能崩**
        match extract_icon_png(&missing.to_string_lossy()) {
            Ok(value) => assert!(value.is_none(), "不存在的文件不应产出图标"),
            Err(err) => {
                let _ = err.message();
            }
        }
    }

    #[test]
    fn image_is_usable_rejects_fully_transparent() {
        let blank = RgbaImage {
            width: 2,
            height: 2,
            pixels: vec![0; 16],
        };
        assert!(!image_is_usable(&blank));

        let mut opaque = vec![0u8; 16];
        opaque[3] = 255;
        assert!(image_is_usable(&RgbaImage {
            width: 2,
            height: 2,
            pixels: opaque,
        }));
    }

    #[test]
    fn bgra_to_rgba_swaps_channels_and_clears_transparent_rgb() {
        // BGRA 输入：B=1, G=2, R=3, A=255 → 期望 RGBA: R=3, G=2, B=1
        let input = [1u8, 2, 3, 255, 9, 8, 7, 0];
        let image = unsafe { bgra_to_rgba(&input, 2, 1) };
        assert_eq!(&image.pixels[0..4], &[3, 2, 1, 255]);
        assert_eq!(
            &image.pixels[4..8],
            &[0, 0, 0, 0],
            "全透明像素的 RGB 应被清零"
        );
    }

    /// 诊断两条路径各自的结果，用于排查「为什么降级成了小图标」。
    /// `cargo test -- --ignored diagnose_extraction`
    #[test]
    #[ignore = "手动运行：诊断图标提取的两条路径"]
    fn diagnose_extraction_paths() {
        let Some(exe) = real_exe() else {
            return;
        };
        let owned = exe.to_owned();

        let (shell, fallback) = crate::platform::win::run_sta(move || {
            let wide = to_wide(&owned);
            let describe = |result: Result<RgbaImage, String>| match result {
                Ok(image) => format!("OK {}x{}", image.width, image.height),
                Err(reason) => format!("FAIL {reason}"),
            };
            let a = describe(unsafe { grab_via_shell_item(&wide) });
            let b = describe(unsafe { grab_via_file_info(&wide) });
            Ok((a, b))
        })
        .expect("run_sta 应成功");

        println!("来源          : {exe}");
        println!("IShellItem    : {shell}");
        println!("SHGetFileInfo : {fallback}");
    }

    /// 把样本图标导出到临时目录，供**人工目视检查**。
    ///
    /// 自动断言只能证明「是合法 PNG 且有可见像素」，证明不了通道顺序、
    /// 透明度合成是否正确 —— 那些只有看图才知道。默认忽略，需要时显式运行：
    /// `cargo test -- --ignored dump_sample_icon`
    #[test]
    #[ignore = "手动运行：导出样本图标供人工查看"]
    fn dump_sample_icon_for_visual_check() {
        let Some(exe) = real_exe() else {
            eprintln!("跳过：没有可用样本");
            return;
        };

        let bytes = extract_icon_png(exe).expect("提取").expect("有图标");
        let out = std::env::temp_dir().join("gamelib-icon-sample.png");
        std::fs::write(&out, &bytes).expect("写出样本");

        let decoded = image::load_from_memory(&bytes).expect("解码");
        println!("样本已写出: {}", out.display());
        println!("尺寸: {}x{}", decoded.width(), decoded.height());
        println!("来源: {exe}");
    }
}

#[cfg(test)]
mod normalize_tests {
    use super::*;

    /// 造一张 256×256 透明画布，中央放 size×size 的不透明方块。
    fn canvas_with_centered_logo(size: u32) -> RgbaImage {
        let mut image = RgbaImage {
            width: 256,
            height: 256,
            pixels: vec![0; 256 * 256 * 4],
        };
        let start = (256 - size) / 2;
        for y in start..start + size {
            for x in start..start + size {
                let offset = ((y * 256 + x) * 4) as usize;
                image.pixels[offset..offset + 4].copy_from_slice(&[200, 30, 30, 255]);
            }
        }
        image
    }

    fn visible_bbox_size(image: &RgbaImage) -> u32 {
        let (_, _, max_x, max_y) = visible_bbox(image).expect("应有可见内容");
        (max_x - min_of(image)).max(max_y - min_of(image))
    }

    fn min_of(image: &RgbaImage) -> u32 {
        let (min_x, min_y, _, _) = visible_bbox(image).expect("应有可见内容");
        min_x.min(min_y)
    }

    #[test]
    fn small_centered_logo_is_enlarged_to_fill() {
        // ~88×88 的 logo 居中（对应 TCLS client.exe 的真实情况）
        let image = canvas_with_centered_logo(88);
        let normalized = normalize_visible_content(&image);

        let (min_x, min_y, max_x, max_y) = visible_bbox(&normalized).expect("应有可见内容");
        let w = max_x - min_x;
        let h = max_y - min_y;
        assert!(
            w >= 230 && h >= 230,
            "放大后内容应接近铺满画布，实际 {w}x{h}"
        );
        assert_eq!(normalized.width, 256);
        assert_eq!(normalized.height, 256);
    }

    #[test]
    fn full_bleed_icon_is_untouched() {
        // 整幅不透明的图标（Steam 游戏图标的常见形态）必须原样返回
        let mut image = RgbaImage {
            width: 256,
            height: 256,
            pixels: vec![0; 256 * 256 * 4],
        };
        for pixel in image.pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[120, 120, 200, 255]);
        }
        let before = image.pixels.clone();

        let normalized = normalize_visible_content(&image);
        assert_eq!(normalized.pixels, before, "铺满的图标不应被二次处理");
    }

    #[test]
    fn fully_transparent_passes_through() {
        let image = RgbaImage {
            width: 64,
            height: 64,
            pixels: vec![0; 64 * 64 * 4],
        };
        let normalized = normalize_visible_content(&image);
        assert_eq!(normalized.width, 64);
        assert!(normalized.pixels.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn visible_bbox_tracks_alpha_threshold() {
        // alpha=16 以下视作透明
        let mut image = RgbaImage {
            width: 8,
            height: 8,
            pixels: vec![0; 8 * 8 * 4],
        };
        // (1,1) 放一个 alpha=16 的像素（不算可见）
        image.pixels[(1 * 8 + 1) * 4 + 3] = 16;
        // (5,5) 放一个 alpha=17 的像素（可见）
        image.pixels[(5 * 8 + 5) * 4 + 3] = 17;
        image.pixels[(5 * 8 + 5) * 4] = 200;

        let (min_x, min_y, max_x, max_y) = visible_bbox(&image).expect("应有可见内容");
        assert_eq!((min_x, min_y, max_x, max_y), (5, 5, 6, 6));
        let _ = canvas_with_centered_logo(1); // 保持 helper 被使用
        let _ = visible_bbox_size(&canvas_with_centered_logo(88));
    }
}

#[cfg(test)]
mod diagnose_tests {
    use super::*;

    /// 临时诊断：把 client.exe 各提取路径的输出导出成 PNG 对比。
    /// cargo test --lib diagnose_client -- --ignored --nocapture
    #[test]
    #[ignore = "临时诊断：交互式查看各提取路径的输出"]
    fn diagnose_client_exe() {
        let exe = r"D:\WeGameApps\英雄联盟\TCLS\client.exe";
        let out_dir = std::env::temp_dir().join("gamelib-icon-diagnose");
        let _ = std::fs::remove_dir_all(&out_dir);
        std::fs::create_dir_all(&out_dir).expect("建目录");

        let owned = exe.to_owned();
        let results = crate::platform::win::run_sta(move || {
            let wide = to_wide(&owned);

            // 路径 A：现有主路径
            let a = unsafe { grab_via_shell_item(&wide) };

            // 路径 B：GetImage 不带 BIGGERSIZEOK
            let b = (|| -> Result<RgbaImage, String> {
                let factory: IShellItemImageFactory =
                    unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }
                        .map_err(|err| format!("{err}"))?;
                let requested = SIZE { cx: 256, cy: 256 };
                let bitmap: HBITMAP = unsafe { factory.GetImage(requested, SIIGBF_ICONONLY) }
                    .map_err(|err| format!("{err}"))?;
                let image = unsafe { hbitmap_to_rgba(bitmap) };
                let _ = unsafe { DeleteObject(HGDIOBJ(bitmap.0)) };
                image.ok_or_else(|| "位图转换失败".to_owned())
            })();

            // 路径 C：现有回退路径
            let c = unsafe { grab_via_file_info(&wide) };

            Ok((a, b, c))
        })
        .expect("run_sta 应成功");

        for (name, result) in [
            ("A-shellitem-biggersizeok", &results.0),
            ("B-shellitem-icononly", &results.1),
            ("C-shgetfileinfo", &results.2),
        ] {
            match result {
                Ok(image) => {
                    let (min_x, min_y, max_x, max_y) = visible_bbox(image).unwrap_or((0, 0, 0, 0));
                    println!(
                        "{name}: {}x{}，可见 bbox=({min_x},{min_y})-({max_x},{max_y})",
                        image.width, image.height
                    );
                    let buffer =
                        image::RgbaImage::from_raw(image.width, image.height, image.pixels.clone())
                            .expect("像素长度正确");
                    let path = out_dir.join(format!("{name}.png"));
                    buffer.save(&path).expect("保存");
                    println!("    已导出 {}", path.display());
                }
                Err(reason) => println!("{name}: 失败 —— {reason}"),
            }
        }
    }
}
