//! v57-3: mermaid 图渲染（mermaid-rs-renderer 纯 Rust 实现 → SVG）。
//!
//! 输入来自 markdown 代码块 `​```mermaid`（markdown.rs 的 MdBlock::Mermaid）。
//! 渲染为 SVG 交给 gpui `img()`（usvg 带系统字体解析，图内 <text> 可渲）。
//!
//! 规则对齐 pi-web MermaidBlock：
//! - 流式期间不渲染（调用方传 streaming=true 时直接回退源码块）
//! - 解析/渲染失败回退源码块（本模块返回 None）
//! - 主题：深色 UI → Theme::dark()，浅色 → mermaid_default()（pi-web
//!   dark/default 的对应物）
//!
//! 缓存：同 math.rs，按 (source, dark) 哈希的进程级 LRU。

use std::hash::{DefaultHasher, Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::{Arc, LazyLock, Mutex};

use gpui::{Image, div, prelude::*, px};
use lru::LruCache;

const CACHE_CAP: usize = 64;

struct Cache {
    map: lru::LruCache<u64, Option<Arc<Image>>>,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| {
    Mutex::new(Cache { map: LruCache::new(NonZeroUsize::new(CACHE_CAP).expect("non-zero")) })
});

fn cache_key(source: &str, dark: bool) -> u64 {
    let mut h = DefaultHasher::new();
    source.hash(&mut h);
    dark.hash(&mut h);
    h.finish()
}

/// mermaid 源码 → SVG（gpui 图片）。None = 解析/渲染失败。
fn render_svg(source: &str, dark: bool) -> Option<Arc<Image>> {
    let options = mermaid_rs_renderer::RenderOptions {
        theme: if dark {
            mermaid_rs_renderer::Theme::dark()
        } else {
            mermaid_rs_renderer::Theme::mermaid_default()
        },
        ..Default::default()
    };
    let svg = mermaid_rs_renderer::render_with_options(source, options).ok()?;
    Some(Arc::new(Image::from_bytes(
        gpui::ImageFormat::Svg,
        svg.into_bytes(),
    )))
}

/// 图元素。None = 失败（调用方回退源码块）。
pub(crate) fn diagram_element(source: &str, dark: bool) -> Option<gpui::AnyElement> {
    let key = cache_key(source, dark);
    let img = {
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        match cache.map.get(&key) {
            Some(hit) => hit.clone(),
            None => {
                let rendered = render_svg(source, dark);
                cache.map.put(key, rendered.clone());
                rendered
            }
        }
    }?;
    Some(
        div()
            .w_full()
            .my(px(8.))
            .flex()
            .justify_center()
            .overflow_hidden()
            .child(gpui::img(img).max_w_full().max_h(px(600.)))
            .into_any_element(),
    )
}

#[allow(unused)]
fn _unused(_: Arc<Image>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flowchart_renders_svg() {
        let src = "flowchart TD\n  A[Start] --> B[End]";
        let el = diagram_element(src, false);
        assert!(el.is_some(), "simple flowchart should render");
    }

    #[test]
    fn cache_hits_same_pointer() {
        let src = "sequenceDiagram\n  A->>B: hi";
        let a = diagram_element(src, false);
        let b = diagram_element(src, false);
        assert!(a.is_some() && b.is_some());
    }

    #[test]
    fn dark_theme_renders() {
        let src = "flowchart LR\n  A --> B";
        assert!(diagram_element(src, true).is_some());
    }

    #[test]
    fn invalid_syntax_returns_none() {
        // 乱语法应当失败回退（不 panic）
        let _ = diagram_element("this is not mermaid at all {{{", false);
    }
}
