//! Markdown renderer for chat messages and md file preview (pi-web
//! MarkdownBody parity: globals.css `.markdown-body` spec + CodeBlock
//! structure). Streaming-friendly: whole-message re-render.
//!
//! pi-web 规格（app/globals.css + MermaidBlock.tsx CodeBlock）：
//! - 正文 14px / line-height 1.7；块间距 = CSS margin 折叠 max(mb, mt)，
//!   末块不挂 mb（p:last-child 同效）
//! - 标题 600 字重、h1 1.16em / h2 1.08em / h3 0.98em 88% 混色
//! - strong = 700 + 88% 混 accent；em = muted；marker = accent 72% 混 muted
//! - 行内 code = mono 0.92em 等宽盒（拆段渲染）；代码块 = 外框圆角 7 +
//!   头部 + 行号 + 12.5px/1.62，阴影 0 1px 0
//! - 语法高亮：浅色 InspiredGitHub / 深色 VS Dark+（代码构建，pi-web 用
//!   Prism vscDarkPlus）

pub mod attachments;
pub mod render;
pub mod wysiwyg;

use gpui::{
    AnyElement, FontStyle, FontWeight, HighlightStyle, InteractiveText, SharedString, StyledText,
    TextStyle, div, prelude::*, px, relative, rgb, rgba,
};
use pulldown_cmark::{Event, Options, Parser};
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::ops::Range;

use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::ui::ScrollAxisExt;
#[cfg(test)]
mod real_file_tests;
#[cfg(test)]
mod tests;

mod doc;
pub(crate) mod highlight;
mod links;
mod parse;

pub(crate) use doc::*;
pub(crate) use highlight::*;
pub(crate) use links::*;
use parse::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Style {
    Normal,
    Bold,
    Italic,
    BoldItalic,
    Code,
    Link,
    Strike,
    /// 标题（024 P1）：level 1-6——源码态装饰用（字重 + 行级字号上浮），
    /// 预览渲染走 Heading 块不经过此样式。
    Heading(u8),
    /// 行内公式标记（v57-2）：Run.text 存 LaTeX 源码，渲染期拆段成图
    Math,
    /// 块级公式标记（$$…$$ 可能出现在段落事件流内，多行块 pulldown 也发在
    /// Paragraph 里）：渲染期独立成整行图
    DisplayMath,
}

#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub(crate) text: String,
    pub(crate) style: Style,
    /// `Style::Link` 的跳转目标（autolink = 文本本身），其余样式恒 None。
    /// 渲染层据此接 InteractiveText → cx.open_url。
    pub(crate) url: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum MdBlock {
    Heading {
        level: u8,
        runs: Vec<Run>,
    },
    Paragraph {
        runs: Vec<Run>,
    },
    Code {
        lang: String,
        code: String,
    },
    Quote {
        blocks: Vec<MdBlock>,
    },
    ListItem {
        depth: usize,
        marker: String,
        runs: Vec<Run>,
        task: Option<bool>,
    },
    Table {
        head: Vec<Vec<Run>>,
        rows: Vec<Vec<Vec<Run>>>,
    },
    Image {
        url: String,
        alt: Vec<Run>,
        width: Option<f32>,
    },

    /// 块级公式（$$…$$，v57-2）
    Math {
        latex: String,
    },
    /// mermaid 图代码块（v57-3）
    Mermaid {
        source: String,
    },
    Rule,
}

/// 样式栈元素：(样式, 链接目标)。URL 随样式进栈——链接内嵌粗体等嵌套
/// 场景不丢跳转目标，渲染层按 run.url 接点击。
pub(crate) const MONO_FAMILY: &str = "JetBrains Mono";

pub(crate) type StyleEntry = (Style, Option<String>);

/// markdown 基准字号（pi-web: 14px + chat-font-size-offset；本项目以
/// 会话/文件槽位字号为基准做绝对像素差换算）
const BASE: f32 = 14.;

thread_local! {
    /// 当前 markdown 渲染的字体规格：聊天路径（render/render_user）= 会话
    /// 字体；文件预览（render_themed）= markdown 字体。入口设置、构建期
    /// 读取（base_style 等在 Chat::render 内同步执行）；未设置时回退
    /// markdown 槽位。UI 单线程，thread_local 兜底后台路径。
    static MD_SPEC: std::cell::RefCell<Option<crate::services::workspace::FontSpec>> =
        std::cell::RefCell::new(None);
}

fn active_md_spec() -> crate::services::workspace::FontSpec {
    MD_SPEC
        .with(|c| c.borrow().clone())
        .unwrap_or_else(crate::appearance::file_font)
}

fn set_md_spec(s: crate::services::workspace::FontSpec) {
    MD_SPEC.with(|c| *c.borrow_mut() = Some(s));
}

fn base_style(size: f32, line_h: f32, color: u32, weight: FontWeight) -> TextStyle {
    // 聊天正文跟随「会话字体」，文件预览跟随「文件字体」（绝对像素差：
    // 生效字号 = 槽位设置值 + (size - BASE)，设计稿基准 14px）。字重必须
    // 显式入参：容器 font_weight 同样传不进 runs（同 font_size 的限制）
    let spec = active_md_spec();
    TextStyle {
        // 容器 .text_color 不会传进 StyledText（自带 base style 覆盖继承），
        // 颜色必须显式入参——h3 混色/引用块 muted 都靠它落到 run 上
        color: rgb(color).into(),
        font_family: spec.family.clone().into(),
        font_size: px(spec.size + (size - BASE)).into(),
        font_weight: weight,
        line_height: relative(line_h),
        ..Default::default()
    }
}

pub(crate) fn highlight(style: Style, t: &Theme) -> Option<HighlightStyle> {
    let h = match style {
        Style::Normal => return None,
        // pi-web strong: 700 字重 + color-mix(text 88%, accent)
        Style::Bold => HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            color: Some(rgb(crate::appearance::theme::mix_rgb(t.text, t.accent, 0.88)).into()),
            ..Default::default()
        },
        // pi-web em: var(--text-muted)
        Style::Italic => HighlightStyle {
            font_style: Some(FontStyle::Italic),
            color: Some(rgb(t.text_muted).into()),
            ..Default::default()
        },
        Style::BoldItalic => HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            font_style: Some(FontStyle::Italic),
            color: Some(rgb(crate::appearance::theme::mix_rgb(t.text, t.accent, 0.88)).into()),
            ..Default::default()
        },
        // note: gpui 0.2.2 highlights cannot change font family; 正文的行内 code
        // 走 paragraph_element 拆段成等宽盒，此高亮只是表格/标题内的兜底
        Style::Code => HighlightStyle {
            background_color: Some(rgb(t.tool_bg).into()),
            ..Default::default()
        },
        // pi-web a: 下划线 45% 透明（offset gpui 无对应）
        Style::Link => HighlightStyle {
            color: Some(rgb(t.accent).into()),
            underline: Some(gpui::UnderlineStyle {
                thickness: px(1.),
                color: Some(gpui::rgba((t.accent << 8) | 0x73).into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        // 024 P1 标题：正文色 + 字重（字号上浮由 Decorations.line_scale
        // 行级 shaping 承担，行高 uniform 内 cap）
        Style::Heading(_) => HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            color: Some(rgb(t.text).into()),
            ..Default::default()
        },
        Style::Strike => HighlightStyle {
            strikethrough: Some(gpui::StrikethroughStyle {
                thickness: px(1.),
                ..Default::default()
            }),
            ..Default::default()
        },
        // 公式在渲染前已拆段为图片；防御性落到代码风
        Style::Math | Style::DisplayMath => HighlightStyle {
            background_color: Some(rgb(t.tool_bg).into()),
            ..Default::default()
        },
    };
    Some(h)
}

fn styled_text(
    runs: &[Run],
    t: &Theme,
    size: f32,
    line_h: f32,
    color: u32,
    weight: FontWeight,
) -> StyledText {
    let mut s = String::new();
    let mut highlights = Vec::new();
    for r in runs {
        let start = s.len();
        s.push_str(&r.text);
        let end = s.len();
        if let Some(h) = highlight(r.style, t) {
            highlights.push((start..end, h));
        }
    }
    StyledText::new(s).with_default_highlights(&base_style(size, line_h, color, weight), highlights)
}

/// 链接 run 的（拼接后字节区间, URL）平行列表，供 InteractiveText 命中。
fn link_ranges(runs: &[Run]) -> (Vec<Range<usize>>, Vec<String>) {
    let mut ranges = Vec::new();
    let mut urls = Vec::new();
    let mut pos = 0usize;
    for r in runs {
        let end = pos + r.text.len();
        if let Some(url) = &r.url {
            ranges.push(pos..end);
            urls.push(url.clone());
        }
        pos = end;
    }
    (ranges, urls)
}

/// runs → 文本元素：含链接 run 时包 InteractiveText（点击 = cx.open_url 调
/// 系统默认浏览器；悬停手型光标由 InteractiveText 自动处理），否则纯
/// StyledText（多数块零开销）。element id 用拼接文本哈希——渲染无状态，
/// 靠内容稳定性保住 InteractiveText 跨帧的 mouse down/up 状态（流式中
/// 正在增长的段落点击可能失效，定稿即恢复）。
fn runs_element(
    runs: &[Run],
    t: &Theme,
    size: f32,
    line_h: f32,
    color: u32,
    weight: FontWeight,
) -> AnyElement {
    let text = styled_text(runs, t, size, line_h, color, weight);
    let (ranges, urls) = link_ranges(runs);
    if ranges.is_empty() {
        return text.into_any_element();
    }
    let mut hasher = DefaultHasher::new();
    hasher.write(runs_text(runs).as_bytes());
    let id = gpui::ElementId::named_usize("md-link", hasher.finish() as usize);
    let target = md_target();
    InteractiveText::new(id, text)
        .on_click(ranges, move |ix, _window, cx| {
            let Some(url) = urls.get(ix) else { return };
            // 点击归链接自己消费（本地文件开 tab，其余开浏览器），冒泡到
            // md 预览容器的「点击进编辑」不得再触发
            cx.stop_propagation();
            // href 指向本地文件 → 内置文件标签页（pi-web 拦截 file 链接同型）
            if let Some(path) = as_file_path(url) {
                open_path_with(target.clone(), path, cx);
            } else {
                cx.open_url(url);
            }
        })
        .into_any_element()
}

/// 正文块（段落/表格/代码等）：字号必须挂在容器 div 上——gpui 0.2.2 的
/// StyledText 排版字号取 `window.text_style()`（容器继承链），runs 里的
/// font_size 只决定字族/字重/颜色，对字形尺寸无效。此前 base_style 里算好
/// 的字号对正文/表格/代码块从未生效（一直画容器继承的默认值），只有像
/// 标题、列表标号那样把 text_size 挂在容器上的元素才真正随设置变。
fn sized_text(
    runs: &[Run],
    t: &Theme,
    size: f32,
    line_h: f32,
    color: u32,
    weight: FontWeight,
) -> AnyElement {
    let spec = active_md_spec();
    div()
        .w_full()
        .text_size(px(spec.size + (size - BASE)))
        .line_height(relative(line_h))
        .font_weight(weight)
        .child(runs_element(runs, t, size, line_h, color, weight))
        .into_any_element()
}

/// pi-web 标题字号（em 相对 14px 正文）。
fn size_for_level(level: u8) -> f32 {
    match level {
        1 => BASE * 1.16,
        2 => BASE * 1.08,
        3 => BASE * 0.98,
        _ => BASE,
    }
}

fn runs_text(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

/// 各块的 CSS margin (mt, mb)，pi-web globals.css .markdown-body 规格。
/// flex 不做外边距折叠——render_blocks 用 max(prev.mb, cur.mt) 复现 CSS
/// 兄弟折叠，末块自然无 mb（= p:last-child { margin-bottom: 0 } 同效，
/// 此前逐块挂 margin 导致块间距系统性偏大：8+6=14 而 CSS 取 8）。
fn block_margins(b: &MdBlock) -> (f32, f32) {
    match b {
        MdBlock::Heading { .. } => (10., 5.),
        MdBlock::Paragraph { .. } => (0., 8.),
        MdBlock::Code { .. } | MdBlock::Mermaid { .. } => (6., 6.),
        MdBlock::Quote { .. } => (6., 6.),
        // 列表容器 ul/ol { margin: 5px 0 8px }
        MdBlock::ListItem { .. } => (5., 8.),
        MdBlock::Table { .. } => (8., 8.),
        // pi-web img { margin: 8px 0 }
        MdBlock::Image { .. } => (8., 8.),
        // katex-display { margin: 0.6em }
        MdBlock::Math { .. } => (8., 8.),
        MdBlock::Rule => (12., 12.),
    }
}

fn render_blocks(
    blocks: &[MdBlock],
    depth: usize,
    t: &Theme,
    streaming: bool,
    color: u32,
) -> gpui::Div {
    let mut col = div().flex().flex_col();
    // 间距 = max(前块 mb, 本块 mt)；首块保留自身 mt（引用块内有 padding，
    // 首元素 margin 不折叠出去，与 CSS 一致）
    let mut prev_mb: Option<f32> = None;
    let mut push = |col: gpui::Div, el: gpui::AnyElement, mt: f32, mb: f32| -> gpui::Div {
        let space = prev_mb.map_or(mt, |p| p.max(mt));
        prev_mb = Some(mb);
        col.child(div().mt(px(space)).child(el))
    };
    // 连续 ListItem 收进列表容器（li 间距 3px 用 gap）
    let mut i = 0;
    while i < blocks.len() {
        if matches!(blocks[i], MdBlock::ListItem { .. }) {
            let mut list = div().flex().flex_col().gap(px(3.));
            while i < blocks.len() && matches!(blocks[i], MdBlock::ListItem { .. }) {
                list = list.child(render_block(&blocks[i], depth, t, streaming, color));
                i += 1;
            }
            col = push(col, list.into_any_element(), 5., 8.);
        } else {
            let (mt, mb) = block_margins(&blocks[i]);
            let el = render_block(&blocks[i], depth, t, streaming, color);
            col = push(col, el, mt, mb);
            i += 1;
        }
    }
    col
}

mod blocks;
mod code_block;

pub(crate) use blocks::{local_img_probe, render_block};
pub(crate) use code_block::{inline_code_box, render_code_block, render_code_block_doc};

/// pi-web MAX_MARKDOWN_CHARS（v56-3 c16）：超限跳过管线，退纯文本。
/// 只约束聊天气泡；文件预览走 doc_blocks/render_doc_item 虚拟化，不设上限。
const MAX_MARKDOWN_CHARS: usize = 100_000;

/// Render a markdown string as a vertical stack of styled GPUI elements.
/// `streaming`（v56-3 c15）：流式中的消息跳过 syntect 高亮与行号
/// （pi-web CodeBlock：流式期间 Prism 逐 chunk 重分词是最贵开销）。
pub fn render(src: &str, t: &Theme, streaming: bool) -> AnyElement {
    set_md_spec(crate::appearance::session_font());
    render_impl(src, t, streaming, false)
}

/// 用户气泡专用（v57）：HTML 标签不渲染、按原文显示——用户消息是发出
/// 内容的凭证，气泡吞标签会让用户无法核对 agent 实际收到的文本。
pub fn render_user(src: &str, t: &Theme) -> AnyElement {
    set_md_spec(crate::appearance::session_font());
    render_impl(src, t, false, false)
}

fn render_impl(src: &str, t: &Theme, streaming: bool, html: bool) -> AnyElement {
    if src.chars().count() > MAX_MARKDOWN_CHARS {
        return render_oversize(src, t);
    }
    let blocks = cached_blocks(src, html);
    if blocks.is_empty() {
        return div().into_any_element();
    }
    render_blocks(&blocks, 1, t, streaming, t.text).into_any_element()
}

// 解析结果缓存：gpui List 每帧对可见条目重建元素树，pulldown 解析是其中
// 最贵的纯 CPU 段（大 markdown 消息数百 µs～ms 级），滚动/流式时每帧白打。
// key = 源文哈希 + 长度 + html 标志，命中后全等校验防碰撞；线程局部
// VecDeque 当 LRU（元素构建只在主线程）。流式期间末条消息源文每 delta
// 一变，最多占满队头被挤出，不影响命中态。
thread_local! {
    static MD_PARSE_CACHE: std::cell::RefCell<
        std::collections::VecDeque<((u64, usize, bool), (String, std::rc::Rc<Vec<MdBlock>>))>,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
const MD_PARSE_CACHE_CAP: usize = 128;
/// 字节预算（源文 32MB）：大粘贴场景 128 条 × MB 级源文＋解析树的组合曾把
/// 稳态推高上百 MB；超预算从头驱逐（LRU），解析树随条目一起释放
const MD_PARSE_CACHE_BUDGET_BYTES: usize = 32 * 1024 * 1024;

/// FNV-1a 按 8 字节块处理（进程内去重键，非加密；命中另有全等校验兜底）
pub(crate) fn hash_str(s: &str) -> u64 {
    hash_bytes(s.as_bytes())
}

/// 字节版 FNV-1a（同 [`hash_str`]；无 str 场景如本地图片缓存用）
pub(crate) fn hash_bytes(b: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325;
    let mut chunks = b.chunks_exact(8);
    for chunk in &mut chunks {
        let mut word = u64::from_le_bytes(chunk.try_into().unwrap());
        for _ in 0..8 {
            h = (h ^ (word & 0xff)).wrapping_mul(0x100000001b3);
            word >>= 8;
        }
    }
    for &byte in chunks.remainder() {
        h = (h ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    h
}

fn cached_blocks(src: &str, html: bool) -> std::rc::Rc<Vec<MdBlock>> {
    let key = (hash_str(src), src.len(), html);
    MD_PARSE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
            if entry.1.0.as_str() == src {
                let blocks = std::rc::Rc::clone(&entry.1.1);
                cache.push_back(entry); // 刷新到队尾（LRU）
                return blocks;
            }
        }
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TABLES);
        // c12: 任务列表 + GFM 自动链接；c14: YAML frontmatter 吞掉
        opts.insert(Options::ENABLE_TASKLISTS);
        opts.insert(Options::ENABLE_GFM);
        opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
        // v57-2: $…$ / $$…$$ 数学（pi-web remark-math parity）
        opts.insert(Options::ENABLE_MATH);
        let events: Vec<Event> = Parser::new_ext(src, opts).collect();
        let parsed = std::rc::Rc::new(parse_blocks(&events, html));
        cache.push_back((key, (src.to_string(), std::rc::Rc::clone(&parsed))));
        while cache.len() > MD_PARSE_CACHE_CAP
            || cache.iter().map(|(k, _)| k.1).sum::<usize>() > MD_PARSE_CACHE_BUDGET_BYTES
        {
            if cache.pop_front().is_none() {
                break;
            }
        }
        parsed
    })
}

/// 探针（automation snapshot）：（条数，源文字节）
pub(crate) fn md_parse_probe() -> (usize, usize) {
    MD_PARSE_CACHE.with(|cell| {
        let cache = cell.borrow();
        (cache.len(), cache.iter().map(|(k, _)| k.1).sum::<usize>())
    })
}

/// Html/InlineHtml 的字面显示（html=false 路径）：标签原文可见。
fn literal_runs(html_src: &str) -> Vec<Run> {
    vec![Run {
        text: html_src.to_string(),
        style: Style::Normal,
        url: None,
    }]
}

/// 超长消息（pi-web ⚠ Message content is very large）：提示行 + 纯文本
/// 滚动视图（pi-web 展开后的 pre maxHeight 420；省去一次点击）。
fn render_oversize(src: &str, t: &Theme) -> AnyElement {
    let kb = src.len() / 1024;
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_color(rgb(0xca8a04))
                .text_size(px(crate::ui::tokens::fixed::MD_WARN))
                .child(SharedString::from(format!(
                    "⚠ {} ({}KB)",
                    crate::i18n::tr("messages.too_large"),
                    kb
                ))),
        )
        .child(
            div()
                .id("md-oversize")
                .max_h(px(420.))
                .overflow_y_scroll()
                .restrict_scroll_to_axis()
                .font_family(MONO_FAMILY)
                .text_size(px(crate::ui::tokens::fixed::MD_OVERSIZE))
                .line_height(relative(1.5))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(src.to_string())),
        )
        .into_any_element()
}

/// 渲染列钳制的 flex 段（gpui 0.2.2 无带参 flex_grow，直写 style 精修）：
/// basis 0 + 指定 grow 占比 + shrink 0；min_w 用作窄面板下的保底钳制。
pub(crate) fn flex_span(grow: f32, min_w: Option<f32>) -> gpui::Div {
    let mut d = div().flex_basis(px(0.));
    d.style().flex_grow = Some(grow);
    d.style().flex_shrink = Some(0.);
    if let Some(w) = min_w {
        d = d.min_w(px(w));
    }
    d
}
