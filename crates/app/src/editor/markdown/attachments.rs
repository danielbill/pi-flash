//! 025 Markdown 插图·P0：md 源码态 Ctrl+V 剪贴板图片 → 落盘
//! `<md同级>/assets/<md名去扩展>/file-<17位时间戳>.png` → 光标处插相对
//! 引用（进 undo 栈）。对齐 Obsidian 归档结构（文档 §3.1）。
//!
//! 拍板（勿改）：不设体积上限（落盘本地 png 自压）；非 md 文件不设钩子
//! （自动不做，文本粘贴照旧）；转不了的格式（svg 等）静默回落文本粘贴；
//! 文件树不加徽标（png 普通文件名显示）。
//!
//! 接线：`editor/view.rs ensure_file_editor` 仅在 `ext == "md"` 时挂
//! [`image_paste_hook`]；vendor 缝 = `input/state.rs paste` 图片分支
//! （PF-025 标记，升级重放）。

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use chrono::{DateTime, Local};
use gpui::{Image, ImageFormat};
use gpui_component::input::ImagePasteHook;

/// 目标目录：`<md同级>/assets/<md文件名去扩展>/`（各目录各自归档，
/// 不上浮到根——拍板，搬运文章时图随文走）。
pub(crate) fn target_dir(md_path: &Path) -> PathBuf {
    let stem = md_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("未命名");
    md_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("assets")
        .join(stem)
}

/// 命名：`file-yyyymmddHHmmssSSS.png`（17 位本地时间，与 Obsidian 截图
/// `file-20260927113156645` 同构）；同名已存在则追加 `-1/-2…` 防撞。
pub(crate) fn unique_png_path(dir: &Path, now: DateTime<Local>) -> PathBuf {
    let ts = now.format("%Y%m%d%H%M%S%3f").to_string();
    let mut path = dir.join(format!("file-{ts}.png"));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("file-{ts}-{n}.png"));
        n += 1;
    }
    path
}

/// 光标处插入串：相对 md 文件的 `/` 分隔路径（图片就在 md 同级 assets 下，
/// strip 基准 = md 父目录；失败退化为绝对路径，仍可渲染）。含空格/括号
/// 的路径用 `<>` 包裹——否则 pulldown 在空格处截断、整条解析失败（025 P1）。
pub(crate) fn image_ref_markdown(md_path: &Path, img_path: &Path) -> String {
    let base = md_path.parent().unwrap_or_else(|| Path::new("."));
    let rel = img_path.strip_prefix(base).unwrap_or(img_path);
    let rel = rel.to_string_lossy().replace('\\', "/");
    if rel.contains([' ', '(', ')', '<', '>']) {
        format!("![](<{rel}>)")
    } else {
        format!("![]({rel})")
    }
}

/// 剪贴板图 → png 字节：png 直存；jpg/gif/webp/bmp 经 image crate 转码；
/// 其余格式（svg 等）None = 静默不做。
pub(crate) fn to_png_bytes(img: &Image) -> Option<Vec<u8>> {
    let fmt = match img.format {
        ImageFormat::Png => return Some(img.bytes.clone()),
        ImageFormat::Jpeg => image::ImageFormat::Jpeg,
        ImageFormat::Gif => image::ImageFormat::Gif,
        ImageFormat::Webp => image::ImageFormat::WebP,
        ImageFormat::Bmp => image::ImageFormat::Bmp,
        _ => return None,
    };
    let decoded = image::load_from_memory_with_format(&img.bytes, fmt).ok()?;
    let mut out = Cursor::new(Vec::new());
    decoded
        .write_to(&mut out, image::ImageFormat::Png)
        .ok()?;
    Some(out.into_inner())
}

/// 生成挂在 md 编辑器 `InputState` 上的图片粘贴钩子（PF-025 vendor 缝）。
/// 链路：转 png → 逐级建目录 → 写盘 → 返回光标处插入串（undo 栈由
/// vendor paste 自行处理——钩子是纯函数，不碰 cx/entity 防重入崩溃）。
pub(crate) fn image_paste_hook(md_path: PathBuf) -> ImagePasteHook {
    Rc::new(move |img: &Image| {
        let png = to_png_bytes(img)?;
        let dir = target_dir(&md_path);
        std::fs::create_dir_all(&dir).ok()?;
        let file = unique_png_path(&dir, Local::now());
        // 写失败不返回引用串（不留悬空引用）
        std::fs::write(&file, &png).ok()?;
        Some(image_ref_markdown(&md_path, &file))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一张 2×2 红色小图并按格式编码（测试 fixture 自造，不入库）。
    fn test_image_bytes(fmt: image::ImageFormat) -> Vec<u8> {
        let img = image::DynamicImage::new_rgba8(2, 2);
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, fmt).unwrap();
        out.into_inner()
    }

    #[test]
    fn target_dir_is_sibling_assets_per_md() {
        assert_eq!(
            target_dir(Path::new("D:/vault/posts/文章.md")),
            PathBuf::from("D:/vault/posts/assets/文章")
        );
        // 根级 md：assets 在根
        assert_eq!(
            target_dir(Path::new("D:/vault/文章.md")),
            PathBuf::from("D:/vault/assets/文章")
        );
    }

    #[test]
    fn naming_17_digit_timestamp_with_collision_suffix() {
        let dir = std::env::temp_dir().join(format!("pf025-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = DateTime::from_timestamp_millis(1_790_000_000_645)
            .unwrap()
            .with_timezone(&Local);
        let p1 = unique_png_path(&dir, now);
        let n1 = p1.file_name().unwrap().to_str().unwrap().to_string();
        // 形如 file-<17位>.png，毫秒尾数 = 时间戳毫秒
        let digits = n1
            .strip_prefix("file-")
            .and_then(|s| s.strip_suffix(".png"))
            .unwrap();
        assert_eq!(digits.len(), 17, "命名应为 17 位: {n1}");
        assert!(digits.ends_with("645"), "毫秒尾数应为 645: {n1}");
        assert!(digits.chars().all(|c| c.is_ascii_digit()), "应全为数字: {n1}");
        // 首个路径已存在 → 追加序号
        std::fs::write(&p1, b"x").unwrap();
        let p2 = unique_png_path(&dir, now);
        assert_eq!(p2.file_name().unwrap().to_str().unwrap(), format!("file-{digits}-1.png"));
        std::fs::write(&p2, b"x").unwrap();
        let p3 = unique_png_path(&dir, now);
        assert_eq!(p3.file_name().unwrap().to_str().unwrap(), format!("file-{digits}-2.png"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn insert_ref_relative_posix_path() {
        let s = image_ref_markdown(
            Path::new("D:/vault/posts/a.md"),
            Path::new("D:/vault/posts/assets/a/file-1.png"),
        );
        assert_eq!(s, "![](assets/a/file-1.png)");
    }

    #[test]
    fn insert_ref_wraps_spaced_path_in_angle_brackets() {
        // md 文件名含空格 → 归档目录含空格 → 引用必须 <> 包裹
        let s = image_ref_markdown(
            Path::new("D:/vault/文章 一.md"),
            Path::new("D:/vault/assets/文章 一/file-2.png"),
        );
        assert_eq!(s, "![](<assets/文章 一/file-2.png>)");
    }

    #[test]
    fn convert_png_passthrough_jpeg_to_png_svg_skipped() {
        // png 直存：字节原样返回
        let png = test_image_bytes(image::ImageFormat::Png);
        let gp = Image::from_bytes(ImageFormat::Png, png.clone());
        assert_eq!(to_png_bytes(&gp).unwrap(), png);
        // jpeg → png：转码产物应是合法 png（8 字节 png 头 + 能解码）
        let jpg = test_image_bytes(image::ImageFormat::Jpeg);
        let gj = Image::from_bytes(ImageFormat::Jpeg, jpg);
        let out = to_png_bytes(&gj).unwrap();
        assert_eq!(&out[..8], b"\x89PNG\r\n\x1a\n");
        assert!(image::load_from_memory_with_format(&out, image::ImageFormat::Png).is_ok());
        // svg → None（静默不做）
        let gs = Image::from_bytes(ImageFormat::Svg, b"<svg/>".to_vec());
        assert!(to_png_bytes(&gs).is_none());
    }

    #[test]
    fn convert_corrupt_bytes_returns_none() {
        let bad = Image::from_bytes(ImageFormat::Jpeg, b"not-a-jpeg".to_vec());
        assert!(to_png_bytes(&bad).is_none());
    }

    #[test]
    fn hook_pipeline_dir_file_ref() {
        // 不进 gpui 上下文，验证链路的纯函数部分：目录 + 文件 + 引用串组合
        let dir = std::env::temp_dir().join(format!("pf025h-{}", std::process::id()));
        let md = dir.join("文章.md");
        let tdir = target_dir(&md);
        std::fs::create_dir_all(&tdir).unwrap();
        let png = test_image_bytes(image::ImageFormat::Png);
        let file = unique_png_path(&tdir, Local::now());
        std::fs::write(&file, &png).unwrap();
        assert!(file.exists());
        let text = image_ref_markdown(&md, &file);
        assert!(text.starts_with("![](assets/文章/file-"));
        assert!(text.ends_with(".png)"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
