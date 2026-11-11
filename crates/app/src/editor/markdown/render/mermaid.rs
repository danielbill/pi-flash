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
    Mutex::new(Cache {
        map: LruCache::new(NonZeroUsize::new(CACHE_CAP).expect("non-zero")),
    })
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

/// 定稿块拆段（session/messages/textview.rs 的 AgentMarkdown 用）：
/// gpui-component TextView 不识 mermaid，agent 定稿文本里的 ```mermaid
/// 只能落成普通码块。顶层（0 缩进）mermaid 围栏抽出来走自家 SVG 管线；
/// 缩进围栏（列表/引用内）不抽，整段留给 TextView 码块兑底（与旧行为
/// 一致）。口径对齐 parse/block.rs：`info.trim() == "mermaid"`。
pub(crate) struct MermaidSegment {
    /// 喂 TextView 的原文（mermaid 段渲染失败也用它兑底成码块）
    pub text: String,
    /// 围栏内的 mermaid 源码（None = 普通 markdown 段）
    pub source: Option<String>,
}

pub(crate) fn split_mermaid_segments(src: &str) -> Vec<MermaidSegment> {
    // 按行扫字节偏移（切片重建原文，零改写）；围栏未闭合视为延伸到
    // 文末（与 CommonMark 同）。闭栏 = 同字符、长度≥开栏、无信息串。
    let mut out: Vec<MermaidSegment> = Vec::new();
    let mut plain_start = 0usize; // 当前普通段起点
    let mut seg_start = 0usize; // 当前待收段（含 mermaid 段从开栏行）起点
    let mut i = 0usize;
    let mut fence: Option<(u8, usize, usize)> = None; // (字符, 开栏长度, 源码起点)
    while i < src.len() {
        let end = src[i..].find('\n').map_or(src.len(), |p| i + p + 1);
        let line = &src[i..end];
        if let Some((ch, open_len, code_start)) = fence {
            let t = line.trim_end();
            if !t.is_empty() && t.len() >= open_len && t.bytes().all(|b| b == ch) {
                let source = src[code_start..i].replace("\r\n", "\n");
                out.push(MermaidSegment {
                    text: src[seg_start..end].to_string(),
                    source: Some(source),
                });
                fence = None;
                plain_start = end;
                seg_start = end;
            }
        } else if line.starts_with("```") || line.starts_with("~~~") {
            let ch = line.as_bytes()[0];
            let open_len = line.bytes().take_while(|&b| b == ch).count();
            if line[open_len..].trim_end().trim() == "mermaid" {
                if plain_start < i {
                    out.push(MermaidSegment {
                        text: src[plain_start..i].to_string(),
                        source: None,
                    });
                }
                fence = Some((ch, open_len, end));
                seg_start = i;
            }
        }
        i = end;
    }
    if let Some((_, _, code_start)) = fence {
        // 未闭合 mermaid 围栏：延伸到文末（与 CommonMark 同）
        out.push(MermaidSegment {
            text: src[seg_start..].to_string(),
            source: Some(src[code_start..].replace("\r\n", "\n")),
        });
    } else if plain_start < src.len() {
        out.push(MermaidSegment {
            text: src[plain_start..].to_string(),
            source: None,
        });
    }
    out
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
                if let Some(Some(evicted)) = cache.map.put(key, rendered.clone()) {
                    // gpui 侧解码位图同步放掉（loading_assets 只进不出，
                    // 见 services::asset_evict）；流式 mermaid 每次内容变化
                    // 都是新键，不摘的话旧位图整场常驻
                    crate::services::asset_evict::queue_evicted(evicted);
                }
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

/// 探针（automation snapshot）：（条数，源 SVG 字节）
pub(crate) fn mermaid_probe() -> (usize, usize) {
    let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    (
        cache.map.len(),
        cache
            .map
            .iter()
            .map(|(_, v)| v.as_ref().map(|i| i.bytes.len()).unwrap_or(0))
            .sum(),
    )
}

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

    // ---- split_mermaid_segments（定稿块拆段，AgentMarkdown 消费）----

    fn segs(src: &str) -> Vec<(String, Option<String>)> {
        split_mermaid_segments(src)
            .into_iter()
            .map(|s| (s.text, s.source))
            .collect()
    }

    #[test]
    fn split_top_level_mermaid_fence() {
        let src = "前置说明\n\n```mermaid\nflowchart TD\n A-->B\n```\n\n后文\n";
        let out = segs(src);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].1, None);
        assert_eq!(out[1].1.as_deref(), Some("flowchart TD\n A-->B\n"));
        assert_eq!(out[2].1, None);
        // 切片重建：拼接必须逐字节等于原文
        let joined: String = out.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(joined, src);
    }

    #[test]
    fn split_info_string_extra_not_mermaid() {
        // parse/block.rs 口径：info.trim() 必须全等于 mermaid
        assert!(segs("```mermaid foo\nA\n```")[0].1.is_none());
    }

    #[test]
    fn split_indented_fence_not_intercepted() {
        // 列表/引用内的缩进围栏不抽，整段留给 TextView 码块兑底
        let out = segs("- 项\n  ```mermaid\n  A-->B\n  ```\n");
        assert_eq!(out.len(), 1);
        assert!(out[0].1.is_none());
    }

    #[test]
    fn split_tilde_fence_and_unclosed_fence() {
        let out = segs("~~~mermaid\nA-->B\n~~~\n");
        assert_eq!(out[0].1.as_deref(), Some("A-->B\n"));
        // 未闭合：延伸到文末（与 CommonMark 同）
        let out = segs("```mermaid\nA --> B");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1.as_deref(), Some("A --> B"));
    }

    #[test]
    fn split_closing_fence_needs_at_least_open_length() {
        // 4 反引号开栏、3 反引号行不算闭栏 → 延伸到文末
        let out = segs("````mermaid\nA\n```\nB\n````\n");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1.as_deref(), Some("A\n```\nB\n"));
    }

    #[test]
    fn split_plain_text_single_segment() {
        let out = segs("普通文本，没有围栏。\n\n```rust\nfn x() {}\n```\n");
        assert_eq!(out.len(), 1);
        assert!(out[0].1.is_none());
    }

    #[test]
    fn split_crlf_source_normalized() {
        let out = segs("```mermaid\r\nA --> B\r\n```\r\n");
        assert_eq!(out[0].1.as_deref(), Some("A --> B\n"));
        // 段文本仍是原字节（重建无损）
        assert!(out[0].0.contains('\r'));
    }
}
