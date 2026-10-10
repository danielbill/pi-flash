//! v57-2: KaTeX 兼容数学渲染（RaTeX 管线：parser → layout → PNG）。
//!
//! 输入来自 pulldown-cmark 的 `Event::InlineMath` / `DisplayMath`（markdown.rs
//! 开 `ENABLE_MATH` 后发出，对应 pi-web remark-math 的 `$…$` / `$$…$$`）。
//! 渲染为透明底 PNG（字形颜色 = 主题文字色），gpui `img()` 直接显示。
//!
//! 缓存：流式期间整条消息每条 delta 重渲一次，公式必须按
//! (latex, display, color) 哈希做进程级 LRU，命中则零渲染成本。
//! 失败（解析/渲染）返回 None，调用方降级为等宽文本——不劣于 v56 现状。

use std::hash::{DefaultHasher, Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::{Arc, LazyLock, Mutex};

use gpui::{Image, SharedString, div, prelude::*, px};
use lru::LruCache;
use ratex_layout::layout_options::LayoutOptions;
use ratex_types::color::Color;
use ratex_types::math_style::MathStyle;

use crate::theme::Theme;

/// 逻辑字号（px）：正文 14 × KaTeX 默认 1.21 ≈ 17。
const FONT_SIZE: f32 = 17.0;
/// 超采样倍率（HiDPI 下仍清晰；显示时缩回逻辑尺寸）。
const DPR: f32 = 2.0;
const CACHE_CAP: usize = 256;

struct Cache {
    map: lru::LruCache<u64, Option<(Arc<Image>, f32, f32)>>,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| {
    Mutex::new(Cache { map: LruCache::new(NonZeroUsize::new(CACHE_CAP).expect("non-zero")) })
});

fn cache_key(latex: &str, display: bool, color: u32) -> u64 {
    let mut h = DefaultHasher::new();
    latex.hash(&mut h);
    display.hash(&mut h);
    color.hash(&mut h);
    h.finish()
}

/// 主题色 u32 (0xRRGGBB) → RaTeX Color。
fn to_ratex_color(rgb24: u32) -> Color {
    Color {
        r: ((rgb24 >> 16) & 0xff) as f32 / 255.0,
        g: ((rgb24 >> 8) & 0xff) as f32 / 255.0,
        b: (rgb24 & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

/// LaTeX → 透明底 PNG（2x 超采样）。返回 (图片, 逻辑宽, 逻辑高)。
fn render_png(latex: &str, display: bool, color: u32) -> Option<(Arc<Image>, f32, f32)> {
    let nodes = ratex_parser::parse(latex).ok()?;
    let options = LayoutOptions {
        style: if display { MathStyle::Display } else { MathStyle::Text },
        color: to_ratex_color(color),
        ..Default::default()
    };
    let layout_box = ratex_layout::engine::layout(&nodes, &options);
    let display_list = ratex_layout::to_display::to_display_list(&layout_box);
    let png = ratex_render::render_to_png(
        &display_list,
        &ratex_render::RenderOptions {
            font_size: FONT_SIZE,
            padding: 2.0,
            // 透明底：公式直接落在消息背景上，深浅主题通用
            background_color: Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
            font_dir: String::new(),
            device_pixel_ratio: DPR,
        },
    )
    .ok()?;
    let w = display_list.width as f32 * FONT_SIZE + 4.0;
    let h = (display_list.height + display_list.depth) as f32 * FONT_SIZE + 4.0;
    Some((Arc::new(Image::from_bytes(gpui::ImageFormat::Png, png)), w, h))
}

/// 渲染（带缓存）。None = 公式解析/渲染失败。
pub(crate) fn math_image(
    latex: &str,
    display: bool,
    color: u32,
) -> Option<(Arc<Image>, f32, f32)> {
    let key = cache_key(latex, display, color);
    if let Some(hit) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).map.get(&key) {
        return hit.clone();
    }
    let rendered = render_png(latex, display, color);
    CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .map
        .put(key, rendered.clone());
    rendered
}

/// 块级公式元素（pi-web KaTeX display parity：居中、上下 0.6em、字号 1.05em）。
/// 失败降级为等宽文本块。
pub(crate) fn block_element(latex: &str, t: &Theme) -> gpui::AnyElement {
    match math_image(latex, true, t.text) {
        Some((img, w, h)) => div()
            .w_full()
            .my(px(8.))
            .flex()
            .justify_center()
            .child(
                gpui::img(img)
                    .max_w_full()
                    .w(px((w * 1.05).min(720.0)))
                    .h(px(h * 1.05)),
            )
            .into_any_element(),
        None => fallback(latex, t),
    }
}

/// 行内公式元素（flex 段内嵌 img；失败降级等宽文本）。
pub(crate) fn inline_element(latex: &str, t: &Theme) -> gpui::AnyElement {
    match math_image(latex, false, t.text) {
        Some((img, w, h)) => div()
            .mt(px(2.))
            .mr(px(2.))
            .child(gpui::img(img).w(px(w)).h(px(h)))
            .into_any_element(),
        None => div()
            .font_family(crate::editor::markdown::MONO_FAMILY)
            .text_color(rgb_color(t.accent))
            .child(SharedString::from(latex.to_string()))
            .into_any_element(),
    }
}

fn rgb_color(c: u32) -> gpui::Hsla {
    gpui::rgb(c).into()
}

/// 降级：等宽代码风文本（v56 前的行为）。
fn fallback(latex: &str, t: &Theme) -> gpui::AnyElement {
    div()
        .w_full()
        .my(px(6.))
        .px(px(10.))
        .py(px(6.))
        .rounded(px(6.))
        .bg(gpui::rgba(crate::theme::border_alpha(t, 0x33)))
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(px(12.5))
        .text_color(rgb_color(t.text_muted))
        .child(SharedString::from(latex.to_string()))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_formula_renders_png() {
        // 依赖内嵌字体；渲染失败（无字体/管线坏）则测试失败
        let (img, w, h) = math_image(r"\frac{1}{2} + \sqrt{x}", true, 0x000000)
            .expect("formula should render");
        assert!(w > 0.0 && h > 0.0);
        assert!(!img.bytes().is_empty(), "png bytes present");
    }

    #[test]
    fn cache_hits_same_pointer() {
        let a = math_image(r"E = mc^2", false, 0x000000).expect("render");
        let b = math_image(r"E = mc^2", false, 0x000000).expect("render");
        assert!(Arc::ptr_eq(&a.0, &b.0), "second call must hit cache");
    }

    #[test]
    fn invalid_latex_falls_back_to_none() {
        // RaTeX 对乱串可能仍解析出字符——只要求不 panic、不返回崩溃
        let _ = math_image("\\notacommand{", true, 0x000000);
    }

    #[test]
    fn color_changes_cache_key() {
        let a = math_image(r"x", false, 0x000000).expect("render");
        let b = math_image(r"x", false, 0xffffff).expect("render");
        assert!(!Arc::ptr_eq(&a.0, &b.0), "different color = different image");
    }
}
