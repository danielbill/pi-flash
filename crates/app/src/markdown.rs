//! Markdown renderer for chat messages and md file preview (pi-web
//! MarkdownBody parity: globals.css `.markdown-body` spec + CodeBlock
//! structure). Streaming-friendly: whole-message re-render.
//!
//! pi-web 规格（app/globals.css + MermaidBlock.tsx CodeBlock）：
//! - 正文 14px / line-height 1.7；段落间距 8px
//! - 标题 600 字重 margin 10/5、h1 1.16em / h2 1.08em / h3 0.98em 混色
//! - 列表 marker = accent 72% 混 muted、600 字重
//! - 行内 code = bg-subtle 底；代码块 = 外框圆角 + 头部（语言名/复制）+ 12.5px/1.62
//! - 语法高亮：浅色 InspiredGitHub / 深色 base16-ocean.dark（pi-web 用
//!   Prism vs / vscDarkPlus 随明暗切换——固定单主题在另一半主题下不可读）

use gpui::{
    AnyElement, FontStyle, FontWeight, HighlightStyle, SharedString, StyledText, TextStyle,
    div, prelude::*, px, relative, rgb, rgba,
};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color, ThemeSet};
use syntect::parsing::SyntaxSet;

/// Syntax highlighting state (loaded once; ~100ms cold, cached for process life).
struct Syn {
    ps: SyntaxSet,
    ts: ThemeSet,
}

fn syn() -> &'static Syn {
    static SYN: std::sync::OnceLock<Syn> = std::sync::OnceLock::new();
    SYN.get_or_init(|| {
        // start from the built-in defaults (includes plain text + common langs),
        // then allow extra syntaxes from an optional assets folder
        let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
        builder.add_from_folder("assets/syntaxes", false).ok();
        let ps = builder.build();
        let ts = ThemeSet::load_defaults();
        Syn { ps, ts }
    })
}

/// 语法高亮主题按 UI 明暗切换（pi-web: isDark ? vscDarkPlus : vs）。
const DARK_THEME: &str = "base16-ocean.dark";
const LIGHT_THEME: &str = "InspiredGitHub";

/// Highlight `code` and return colored text segments (never spans across
/// lines — callers rely on per-line boundaries for the gutter).
fn highlight_segments(code: &str, lang: &str, dark: bool) -> Vec<(String, [u8; 3])> {
    let syn = syn();
    let syntax = lang
        .split(',')
        .next()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .and_then(|l| syn.ps.find_syntax_by_token(l))
        .unwrap_or_else(|| syn.ps.find_syntax_plain_text());
    let name = if dark { DARK_THEME } else { LIGHT_THEME };
    let Some(theme) = syn.ts.themes.get(name) else {
        return vec![(code.to_string(), [0xd7, 0xda, 0xdd])];
    };
    let mut hl = HighlightLines::new(syntax, theme);
    let mut out: Vec<(String, [u8; 3])> = Vec::new();
    for line in syntect::util::LinesWithEndings::from(code) {
        let Ok(ranges) = hl.highlight_line(line, &syn.ps) else { continue };
        // 每行起一段新 run（行号 gutter 需要按行插入）
        let mut line_open = false;
        for (style, text) in ranges {
            let Color { r, g, b, a: _ } = style.foreground;
            if let Some(last) = out.last_mut() {
                if line_open && last.1 == [r, g, b] {
                    last.0.push_str(text);
                    continue;
                }
            }
            out.push((text.to_string(), [r, g, b]));
            line_open = true;
        }
    }
    out
}

const MONO_FAMILY: &str = "Consolas";

// ---------------------------------------------------------------------------
// inline runs
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Style {
    Normal,
    Bold,
    Italic,
    BoldItalic,
    Code,
    Link,
    Strike,
}

#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub(crate) text: String,
    pub(crate) style: Style,
}

#[derive(Clone, Debug)]
pub(crate) enum MdBlock {
    Heading { level: u8, runs: Vec<Run> },
    Paragraph { runs: Vec<Run> },
    Code { lang: String, code: String },
    Quote { blocks: Vec<MdBlock> },
    ListItem { depth: usize, marker: String, runs: Vec<Run>, task: Option<bool> },
    Table { head: Vec<Vec<Run>>, rows: Vec<Vec<Vec<Run>>> },
    Image { url: String, alt: Vec<Run> },
    Rule,
}

fn style_push(styles: &mut Vec<Style>, tag: &Tag) {
    let base = styles.last().copied().unwrap_or(Style::Normal);
    let next = match tag {
        Tag::Strong => match base {
            Style::Italic | Style::BoldItalic => Style::BoldItalic,
            _ => Style::Bold,
        },
        Tag::Emphasis => match base {
            Style::Bold | Style::BoldItalic => Style::BoldItalic,
            _ => Style::Italic,
        },
        Tag::Link { .. } => Style::Link,
        Tag::Strikethrough => Style::Strike,
        _ => base,
    };
    styles.push(next);
}

fn style_pop(styles: &mut Vec<Style>, end: &TagEnd) {
    match end {
        TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link | TagEnd::Strikethrough => {
            styles.pop();
        }
        _ => {}
    }
}

/// GFM autolink-literal parity (pi-web remark-gfm；pulldown 0.13 无此扩展)：
/// 把普通文本按裸 http(s) URL 切成 Link run。尾部标点剥到 URL 外。
fn split_links(text: &str) -> Vec<(String, bool)> {
    const HTTPS: &str = "https://";
    const HTTP: &str = "http://";
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut rest = text;
    loop {
        let start = [HTTPS, HTTP]
            .iter()
            .filter_map(|sc| rest.find(sc))
            .min();
        let Some(start) = start else { break };
        // 前缀须是词边界（避免 "foohttps://" 误切）
        let boundary_ok = start == 0
            || !rest[..start]
                .chars()
                .next_back()
                .map(|c| c.is_alphanumeric())
                .unwrap_or(false);
        if !boundary_ok {
            // 跳过这个伪起点，从下一个字符继续找
            let (head, tail) = rest.split_at(start + 1);
            out.push((head.to_string(), false));
            rest = tail;
            continue;
        }
        // URL 止于空白或尖括号；尾部标点剥出
        let candidate = &rest[start..];
        let end = candidate
            .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
            .unwrap_or(candidate.len());
        let mut url = &candidate[..end];
        while url
            .chars()
            .next_back()
            .map(|c| "!.,;:?\")'".contains(c))
            .unwrap_or(false)
        {
            url = &url[..url.len() - 1];
        }
        if url.len() > HTTPS.len() {
            if start > 0 {
                out.push((rest[..start].to_string(), false));
            }
            out.push((url.to_string(), true));
            rest = &rest[start + url.len()..];
        } else {
            // 裸 scheme 无主体——原样保留
            let (head, tail) = rest.split_at(start + 1);
            out.push((head.to_string(), false));
            rest = tail;
        }
    }
    if out.is_empty() {
        vec![(text.to_string(), false)]
    } else {
        out.push((rest.to_string(), false));
        out
    }
}

/// Collect inline runs until `is_end` matches (consuming the end event).
fn collect_inline(events: &[Event], i: &mut usize, is_end: &dyn Fn(&Event) -> bool) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut styles: Vec<Style> = Vec::new();
    let mut text = String::new();
    let mut cur = Style::Normal;

    fn flush(text: &mut String, cur: Style, runs: &mut Vec<Run>) {
        if text.is_empty() {
            return;
        }
        let taken = std::mem::take(text);
        if cur == Style::Normal {
            // GFM 自动链接：普通文本里的裸 URL 提为 Link run
            for (seg, is_link) in split_links(&taken) {
                if !seg.is_empty() {
                    runs.push(Run {
                        text: seg,
                        style: if is_link { Style::Link } else { Style::Normal },
                    });
                }
            }
        } else {
            runs.push(Run { text: taken, style: cur });
        }
    }

    while *i < events.len() {
        let style_now = *styles.last().unwrap_or(&Style::Normal);
        match &events[*i] {
            e if is_end(e) => {
                *i += 1;
                flush(&mut text, cur, &mut runs);
                return runs;
            }
            Event::Text(t) => {
                if style_now != cur {
                    flush(&mut text, cur, &mut runs);
                    cur = style_now;
                }
                text.push_str(t);
            }
            Event::InlineHtml(h) => {
                // v57-1: 行内 HTML 片段 → 安全子集 runs（此前直接丢弃）
                flush(&mut text, cur, &mut runs);
                runs.extend(crate::render::html::inline_runs(h));
                cur = Style::Normal;
                styles.clear();
            }
            Event::Code(c) => {
                flush(&mut text, cur, &mut runs);
                runs.push(Run { text: c.to_string(), style: Style::Code });
            }
            Event::SoftBreak => text.push(' '),
            Event::HardBreak => text.push('\n'),
            Event::Start(tag) => style_push(&mut styles, tag),
            Event::End(end) => style_pop(&mut styles, end),
            _ => {}
        }
        *i += 1;
    }
    flush(&mut text, cur, &mut runs);
    runs
}

// ---------------------------------------------------------------------------
// block parsing
// ---------------------------------------------------------------------------

fn parse_blocks(events: &[Event]) -> Vec<MdBlock> {
    let mut out: Vec<MdBlock> = Vec::new();
    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Start(tag) => {
                let tag = tag.clone();
                i += 1;
                parse_block(events, &mut i, tag, &mut out, 0);
            }
            Event::Rule => {
                out.push(MdBlock::Rule);
                i += 1;
            }
            Event::Text(t) => {
                out.push(MdBlock::Paragraph {
                    runs: vec![Run { text: t.to_string(), style: Style::Normal }],
                });
                i += 1;
            }
            Event::Html(h) => {
                // v57-1: 块级 HTML → 安全子集块（此前直接丢弃）
                out.extend(crate::render::html::blocks(h));
                i += 1;
            }
            Event::InlineHtml(h) => {
                let runs = crate::render::html::inline_runs(h);
                if !runs.is_empty() {
                    out.push(MdBlock::Paragraph { runs });
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// True when the paragraph from `start` holds nothing but whitespace and a
/// single image — those render as a real image block (pi-web img: block).
fn solo_image(events: &[Event], start: usize) -> Option<String> {
    let mut url: Option<String> = None;
    let mut depth = 0usize;
    let mut i = start;
    while i < events.len() {
        match &events[i] {
            Event::End(TagEnd::Paragraph) => break,
            Event::Start(Tag::Image { dest_url, .. }) => {
                if depth == 0 {
                    if url.is_some() {
                        return None;
                    }
                    url = Some(dest_url.to_string());
                }
                depth += 1;
            }
            Event::End(TagEnd::Image) => depth -= 1,
            _ if depth == 0 => {
                if let Event::Text(t) = &events[i] {
                    if t.trim().is_empty() {
                        i += 1;
                        continue;
                    }
                }
                return None;
            }
            _ => {}
        }
        i += 1;
    }
    url
}

fn parse_block(
    events: &[Event],
    i: &mut usize,
    tag: Tag,
    out: &mut Vec<MdBlock>,
    depth: usize,
) {
    match tag {
        Tag::Paragraph => {
            if let Some(url) = solo_image(events, *i) {
                // 独立图片段落：定位 Image，收 alt，越过 End(Paragraph)
                while !matches!(events[*i], Event::Start(Tag::Image { .. })) {
                    *i += 1;
                }
                if let Event::Start(Tag::Image { .. }) = &events[*i] {
                    *i += 1;
                    let alt = collect_inline(events, i, &|e| {
                        matches!(e, Event::End(TagEnd::Image))
                    });
                    out.push(MdBlock::Image { url, alt });
                }
                while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::Paragraph)) {
                    *i += 1;
                }
                *i += 1;
            } else {
                let runs = collect_inline(events, i, &|e| {
                    matches!(e, Event::End(TagEnd::Paragraph))
                });
                out.push(MdBlock::Paragraph { runs });
            }
        }
        Tag::Heading { level, .. } => {
            let runs = collect_inline(events, i, &|e| {
                matches!(e, Event::End(TagEnd::Heading(_)))
            });
            out.push(MdBlock::Heading { level: level as u8, runs });
        }
        Tag::BlockQuote(_) => {
            let mut inner = Vec::new();
            while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::BlockQuote(_))) {
                if let Event::Start(t) = &events[*i] {
                    let t = t.clone();
                    *i += 1;
                    parse_block(events, i, t, &mut inner, depth + 1);
                } else {
                    *i += 1;
                }
            }
            *i += 1; // End(BlockQuote)
            out.push(MdBlock::Quote { blocks: inner });
        }
        Tag::CodeBlock(kind) => {
            let lang = match kind {
                CodeBlockKind::Fenced(l) => l.to_string(),
                CodeBlockKind::Indented => String::new(),
            };
            let mut code = String::new();
            while *i < events.len() {
                match &events[*i] {
                    Event::Text(t) => code.push_str(t),
                    Event::End(TagEnd::CodeBlock) => {
                        *i += 1;
                        break;
                    }
                    _ => {}
                }
                *i += 1;
            }
            out.push(MdBlock::Code { lang, code });
        }
        Tag::Table(_) => {
            let mut head: Vec<Vec<Run>> = Vec::new();
            let mut rows: Vec<Vec<Vec<Run>>> = Vec::new();
            while *i < events.len() {
                match &events[*i] {
                    Event::End(TagEnd::Table) => {
                        *i += 1;
                        break;
                    }
                    Event::Start(Tag::TableHead) => {
                        *i += 1;
                        head = parse_table_cells(events, i, &|e| {
                            matches!(e, Event::End(TagEnd::TableHead))
                        });
                    }
                    Event::Start(Tag::TableRow) => {
                        *i += 1;
                        let row = parse_table_cells(events, i, &|e| {
                            matches!(e, Event::End(TagEnd::TableRow))
                        });
                        rows.push(row);
                    }
                    _ => *i += 1,
                }
            }
            out.push(MdBlock::Table { head, rows });
        }
        Tag::Image { dest_url, .. } => {
            let alt = collect_inline(events, i, &|e| matches!(e, Event::End(TagEnd::Image)));
            out.push(MdBlock::Image { url: dest_url.to_string(), alt });
        }
        Tag::List(start) => {
            let ordered = start.is_some();
            let mut n = start.unwrap_or(1);
            while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::List(_))) {
                if let Event::Start(Tag::Item) = &events[*i] {
                    *i += 1;
                    let marker = if ordered {
                        format!("{n}.")
                    } else {
                        "•".to_string()
                    };
                    n += 1;
                    let mut runs: Vec<Run> = Vec::new();
                    let mut nested = Vec::new();
                    let mut task: Option<bool> = None;
                    while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::Item)) {
                        match &events[*i] {
                            // c12: 任务列表复选框（pi-web 自绘 14px accent 对勾）
                            Event::TaskListMarker(checked) => {
                                task = Some(*checked);
                                *i += 1;
                            }
                            Event::Start(inner_tag) => {
                                let inner_tag = inner_tag.clone();
                                *i += 1;
                                match &inner_tag {
                                    Tag::Paragraph => {
                                        let r = collect_inline(events, i, &|e| {
                                            matches!(e, Event::End(TagEnd::Paragraph))
                                        });
                                        if !runs.is_empty() {
                                            if let Some(last) = runs.last_mut() {
                                                last.text.push(' ');
                                            }
                                        }
                                        runs.extend(r);
                                    }
                                    Tag::List(_) => {
                                        parse_block(events, i, inner_tag, &mut nested, depth + 1);
                                    }
                                    Tag::CodeBlock(_) | Tag::BlockQuote(_) | Tag::Table(_) => {
                                        parse_block(events, i, inner_tag, &mut nested, depth + 1);
                                    }
                                    Tag::Image { .. } => {
                                        parse_block(events, i, inner_tag, &mut nested, depth + 1);
                                    }
                                    _ => {}
                                }
                            }
                            Event::Text(t) => {
                                runs.push(Run { text: t.to_string(), style: Style::Normal });
                                *i += 1;
                            }
                            _ => *i += 1,
                        }
                    }
                    *i += 1; // End(Item)
                    out.push(MdBlock::ListItem { depth, marker, runs, task });
                    out.extend(nested);
                } else {
                    *i += 1;
                }
            }
            *i += 1; // End(List)
        }
        _ => {
            // unknown/unsupported container: skip to its end (depth-naive but fine
            // for the subset markdown chat needs)
            let mut d = 1usize;
            while *i < events.len() && d > 0 {
                match &events[*i] {
                    Event::Start(_) => d += 1,
                    Event::End(_) => d -= 1,
                    _ => {}
                }
                *i += 1;
            }
        }
    }
}

/// Parse `Start(TableCell) … End(TableCell)` sequences until `is_end` fires
/// (the row/head End event), one `Vec<Run>` per cell.
fn parse_table_cells(
    events: &[Event],
    i: &mut usize,
    is_end: &dyn Fn(&Event) -> bool,
) -> Vec<Vec<Run>> {
    let mut cells: Vec<Vec<Run>> = Vec::new();
    while *i < events.len() {
        if is_end(&events[*i]) {
            *i += 1;
            break;
        }
        if matches!(events[*i], Event::Start(Tag::TableCell)) {
            *i += 1;
            let runs = collect_inline(events, i, &|e| {
                matches!(e, Event::End(TagEnd::TableCell))
            });
            cells.push(runs);
        } else {
            *i += 1;
        }
    }
    cells
}

// ---------------------------------------------------------------------------
// rendering — pi-web globals.css `.markdown-body` spec
// ---------------------------------------------------------------------------

use crate::theme::Theme;

/// markdown 基准字号（pi-web: 14px + chat-font-size-offset；本项目的 slot 缩放）
const BASE: f32 = 14.;

fn base_style(t: &Theme, size: f32, line_h: f32) -> TextStyle {
    // 006 markdown preview font slot (family; size scaled from the slot)
    let spec = crate::appearance::markdown_font();
    TextStyle {
        color: rgb(t.text).into(),
        font_family: spec.family.clone().into(),
        font_size: px(size / BASE * spec.size).into(),
        line_height: relative(line_h),
        ..Default::default()
    }
}

fn highlight(style: Style, t: &Theme) -> Option<HighlightStyle> {
    let h = match style {
        Style::Normal => return None,
        // pi-web strong: color-mix(text 88%, accent)
        Style::Bold => HighlightStyle {
            font_weight: Some(FontWeight::SEMIBOLD),
            color: Some(rgb(crate::theme::mix_rgb(t.text, t.accent, 0.88)).into()),
            ..Default::default()
        },
        // pi-web em: var(--text-muted)
        Style::Italic => HighlightStyle {
            font_style: Some(FontStyle::Italic),
            color: Some(rgb(t.text_muted).into()),
            ..Default::default()
        },
        Style::BoldItalic => HighlightStyle {
            font_weight: Some(FontWeight::SEMIBOLD),
            font_style: Some(FontStyle::Italic),
            color: Some(rgb(crate::theme::mix_rgb(t.text, t.accent, 0.88)).into()),
            ..Default::default()
        },
        // note: gpui 0.2.2 highlights cannot change font family; code gets bg only
        Style::Code => HighlightStyle { background_color: Some(rgb(t.tool_bg).into()), ..Default::default() },
        Style::Link => HighlightStyle {
            color: Some(rgb(t.accent).into()),
            underline: Some(gpui::UnderlineStyle { thickness: px(1.), ..Default::default() }),
            ..Default::default()
        },
        Style::Strike => HighlightStyle {
            strikethrough: Some(gpui::StrikethroughStyle { thickness: px(1.), ..Default::default() }),
            ..Default::default()
        },
    };
    Some(h)
}

fn styled_text(runs: &[Run], t: &Theme, size: f32, line_h: f32) -> StyledText {
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
    StyledText::new(s).with_default_highlights(&base_style(t, size, line_h), highlights)
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

fn render_blocks(blocks: &[MdBlock], depth: usize, t: &Theme, streaming: bool) -> gpui::Div {
    let mut col = div().flex().flex_col();
    for b in blocks {
        col = col.child(render_block(b, depth, t, streaming));
    }
    col
}

/// 代码块：外框圆角 7px + 头部（语言名 / 复制）+ 行号 + 12.5px/1.62 高亮体
/// （pi-web .markdown-code-block / .markdown-code-header / Prism 行号）。
fn render_code_block(lang: &str, code: &str, t: &Theme, streaming: bool) -> gpui::Div {
    let code = code.trim_end_matches('\n');
    let body_bg = crate::theme::mix_rgb(t.bg, t.bg_panel, 0.92);

    // header
    let copy_code = code.to_string();
    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .px(px(10.))
        .py(px(5.))
        .bg(rgb(t.bg_panel))
        .border_b_1()
        .border_color(rgb(t.border))
        .text_size(px(11.))
        .child(
            div()
                .font_family(MONO_FAMILY)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(if lang.is_empty() {
                    "text".to_string()
                } else {
                    lang.to_string()
                })),
        )
        .child(
            div()
                .id("md-copy")
                .cursor_pointer()
                .rounded(px(5.))
                .border_1()
                .border_color(rgb(t.border))
                .px(px(7.))
                .py(px(2.))
                .text_color(rgb(t.text_muted))
                .hover(|s| s.text_color(rgb(t.text)))
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_code.clone()));
                })
                .child(SharedString::from(crate::i18n::tr("复制"))),
        );

    // 代码体：行号 gutter 前缀 + 逐行高亮段
    let lines: Vec<&str> = if code.is_empty() {
        vec![""]
    } else {
        code.split('\n').collect()
    };
    // c15: 流式期间纯文本（pi-web 渲染裸 <pre>，不做 Prism 高亮）
    let mut text = String::new();
    let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    if streaming {
        // 纯文本，无行号无高亮
        text.push_str(code);
    } else {
        let n_digits = lines.len().to_string().len();
        let dim = rgb(t.text_dim);
        let mut segs = highlight_segments(code, lang, t.dark);
        segs.push((String::new(), [0, 0, 0])); // sentinel：保证末行 flush
        let mut seg_ix = 0usize;
        for (i, _) in lines.iter().enumerate() {
            // 行号 gutter
            let start = text.len();
            text.push_str(&format!("{:>w$}  ", i + 1, w = n_digits));
            highlights.push((
                start..text.len(),
                HighlightStyle { color: Some(dim.into()), ..Default::default() },
            ));
            // 该行的高亮段
            while seg_ix < segs.len() {
                let (seg, c) = &segs[seg_ix];
                match seg.find('\n') {
                    Some(pos) => {
                        let (head, _) = seg.split_at(pos);
                        if !head.is_empty() {
                            let start = text.len();
                            text.push_str(head);
                            push_color(&mut highlights, start, text.len(), *c);
                        }
                        seg_ix += 1;
                        break; // 该行结束
                    }
                    None => {
                        if !seg.is_empty() {
                            let start = text.len();
                            text.push_str(seg);
                            push_color(&mut highlights, start, text.len(), *c);
                        }
                        seg_ix += 1;
                    }
                }
            }
            if i + 1 < lines.len() {
                text.push('\n');
            }
        }
    }

    let base = TextStyle {
        color: rgb(t.text).into(),
        font_family: MONO_FAMILY.into(),
        font_size: px(12.5).into(),
        line_height: relative(1.62),
        ..Default::default()
    };

    div()
        .w_full()
        .mt(px(6.))
        .mb(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(7.))
        .overflow_hidden()
        .bg(rgb(body_bg))
        .child(header)
        .child(
            div()
                .id("md-code-body")
                .w_full()
                .px(px(13.))
                .py(px(11.))
                // c15：长行不再裁剪——nowrap + 横向滚动（pi-web <pre> 语义）
                .whitespace_nowrap()
                .overflow_x_scroll()
                .child(StyledText::new(text).with_default_highlights(&base, highlights)),
        )
}

fn push_color(
    highlights: &mut Vec<(std::ops::Range<usize>, HighlightStyle)>,
    start: usize,
    end: usize,
    c: [u8; 3],
) {
    let color = rgb(((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32);
    highlights.push((
        start..end,
        HighlightStyle { color: Some(color.into()), ..Default::default() },
    ));
}

/// 表格：外框圆角 7px、th bg_panel 650 字重、行分隔线、偶数行斑马纹
/// （pi-web .markdown-table-wrap）。
/// 单元格显示宽度权重：ASCII 记 1、CJK/全角记 2（HTML 表 auto layout 的
/// 近似度量）。
fn cell_weight(runs: &[Run]) -> usize {
    runs.iter().map(|r| r.text.chars().map(|c| if (c as u32) > 0x2e80 { 2 } else { 1 }).sum::<usize>()).sum()
}

fn render_table(head: &[Vec<Run>], rows: &[Vec<Vec<Run>>], t: &Theme) -> gpui::Div {
    // 列宽 = 列内最长单元格的显示宽度（钳制 [6,44]），归一化后全表共用
    // 同一组相对宽度——跨行对齐成网格，长单元格换行（pi-web HTML 表
    // auto layout 的近似；flex_1 按内容分配导致每行列位漂移，已废）
    let n_cols = head
        .len()
        .max(rows.iter().map(|r| r.len()).max().unwrap_or(0))
        .max(1);
    const MIN_W: usize = 6;
    const MAX_W: usize = 44;
    let mut weights = vec![MIN_W; n_cols];
    for (ci, cell) in head.iter().enumerate() {
        weights[ci] = weights[ci].max(cell_weight(cell).clamp(MIN_W, MAX_W));
    }
    for row in rows {
        for (ci, cell) in row.iter().enumerate().take(n_cols) {
            weights[ci] = weights[ci].max(cell_weight(cell).clamp(MIN_W, MAX_W));
        }
    }
    let total: f32 = weights.iter().sum::<usize>() as f32;
    let fracs: Vec<f32> = weights.iter().map(|w| *w as f32 / total).collect();

    let cell_div = |frac: f32| {
        div()
            .w(relative(frac))
            .min_w_0()
            .px(px(10.))
            .py(px(6.))
    };

    let head_cells: Vec<gpui::AnyElement> = head
        .iter()
        .enumerate()
        .map(|(ci, cell)| {
            cell_div(fracs[ci])
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(crate::theme::mix_rgb(t.text, t.text_muted, 0.88)))
                .child(styled_text(cell, t, BASE, 1.6))
                .into_any_element()
        })
        .collect();
    let head_row = div()
        .flex()
        .bg(rgb(t.bg_panel))
        .border_b_1()
        .border_color(rgb(t.border))
        .children(head_cells);

    let body_rows: Vec<gpui::AnyElement> = rows
        .iter()
        .enumerate()
        .map(|(ri, row)| {
            let mut line = div()
                .flex()
                .border_b_1()
                .border_color(rgb(t.border));
            if ri % 2 == 1 {
                line = line.bg(rgba(t.bg_subtle));
            }
            for (ci, cell) in row.iter().enumerate().take(n_cols) {
                line = line.child(cell_div(fracs[ci]).child(styled_text(cell, t, BASE, 1.6)));
            }
            line.into_any_element()
        })
        .collect();

    div()
        .w_full()
        .mt(px(8.))
        .mb(px(8.))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(7.))
        .overflow_hidden()
        .child(head_row)
        .children(body_rows)
}

/// 图片：本地文件 gpui img() 直渲染；http/不存在 → alt 文本占位。
fn render_image(url: &str, alt: &[Run], t: &Theme) -> gpui::AnyElement {
    let placeholder = || {
        div()
            .w_full()
            .my(px(8.))
            .text_color(rgb(t.text_dim))
            .italic()
            .child(SharedString::from(format!("🖼 {}", runs_text(alt))))
            .into_any_element()
    };
    if url.starts_with("http://") || url.starts_with("https://") || url.is_empty() {
        return placeholder();
    }
    let ext = std::path::Path::new(url)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let format = match ext.as_str() {
        "png" => Some(gpui::ImageFormat::Png),
        "jpg" | "jpeg" => Some(gpui::ImageFormat::Jpeg),
        "gif" => Some(gpui::ImageFormat::Gif),
        "bmp" => Some(gpui::ImageFormat::Bmp),
        "svg" => Some(gpui::ImageFormat::Svg),
        "webp" => Some(gpui::ImageFormat::Webp),
        _ => None,
    };
    let Some((bytes, format)) = std::fs::read(url).ok().zip(format) else {
        return placeholder();
    };
    div()
        .w_full()
        .my(px(8.))
        .child(
            gpui::img(std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)))
                .max_w_full()
                .rounded(px(6.)),
        )
        .into_any_element()
}

fn render_block(b: &MdBlock, depth: usize, t: &Theme, streaming: bool) -> AnyElement {
    match b {
        MdBlock::Heading { level, runs } => {
            let size = size_for_level(*level);
            // h3 color-mix(text 88%, muted)（pi-web h3 规则）
            let color = if *level == 3 {
                crate::theme::mix_rgb(t.text, t.text_muted, 0.88)
            } else {
                t.text
            };
            div()
                .w_full()
                .mt(px(10.))
                .mb(px(5.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(size))
                .line_height(relative(1.35))
                .text_color(rgb(color))
                .child(styled_text(runs, t, size, 1.35))
                .into_any_element()
        }
        MdBlock::Paragraph { runs } => div()
            .w_full()
            .mb(px(8.))
            .text_color(rgb(t.text))
            .child(styled_text(runs, t, BASE, 1.7))
            .into_any_element(),
        MdBlock::Code { code, lang, .. } => {
            render_code_block(lang, code, t, streaming).into_any_element()
        }
        MdBlock::Quote { blocks } => div()
            .w_full()
            .mt(px(6.))
            .mb(px(6.))
            .border_l_3()
            .border_color(rgb(crate::theme::mix_rgb(t.border, t.text_muted, 0.75)))
            .rounded_r(px(6.))
            .bg(rgba(t.bg_subtle))
            .px(px(11.))
            .py(px(6.))
            .text_color(rgb(t.text_muted))
            .child(render_blocks(blocks, depth + 1, t, streaming))
            .into_any_element(),
        MdBlock::ListItem { depth: d, marker, runs, task } => {
            let task = *task;
            // c12: 任务项 marker = 14px 复选框（选中 accent 对勾），否则原 marker
            let marker_el: AnyElement = match task {
                Some(checked) => div()
                    .w(px(14.))
                    .h(px(14.))
                    .mt(px(4.))
                    .flex_shrink_0()
                    .rounded(px(3.))
                    .border_1()
                    .border_color(rgb(if checked { t.accent } else { t.border }))
                    .bg(rgb(if checked { t.accent } else { t.bg }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(checked, |b| {
                        b.child(
                            div()
                                .text_size(px(10.))
                                .text_color(rgb(0xffffff))
                                .child(SharedString::from("✓")),
                        )
                    })
                    .into_any_element(),
                None => div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(crate::theme::mix_rgb(t.accent, t.text_muted, 0.72)))
                    .child(SharedString::from(marker.clone()))
                    .into_any_element(),
            };
            div()
                .flex()
                .mb(px(3.))
                .pl(px((d * 16) as f32))
                .child(marker_el)
                .child(div().flex_1().min_w_0().text_color(rgb(t.text)).child(styled_text(runs, t, BASE, 1.7)))
                .into_any_element()
        }
        MdBlock::Table { head, rows } => render_table(head, rows, t).into_any_element(),
        MdBlock::Image { url, alt } => render_image(url, alt, t),
        MdBlock::Rule => div()
            .w_full()
            .h(px(1.))
            .mt(px(12.))
            .mb(px(12.))
            .bg(rgb(t.border))
            .into_any_element(),
    }
}

/// pi-web MAX_MARKDOWN_CHARS（v56-3 c16）：超限跳过管线，退纯文本。
const MAX_MARKDOWN_CHARS: usize = 100_000;

/// Render a markdown string as a vertical stack of styled GPUI elements.
/// `streaming`（v56-3 c15）：流式中的消息跳过 syntect 高亮与行号
/// （pi-web CodeBlock：流式期间 Prism 逐 chunk 重分词是最贵开销）。
pub fn render(src: &str, t: &Theme, streaming: bool) -> AnyElement {
    if src.chars().count() > MAX_MARKDOWN_CHARS {
        return render_oversize(src, t);
    }
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    // c12: 任务列表 + GFM 自动链接；c14: YAML frontmatter 吞掉
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_GFM);
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    let events: Vec<Event> = Parser::new_ext(src, opts).collect();
    let blocks = parse_blocks(&events);
    if blocks.is_empty() {
        return div().into_any_element();
    }
    render_blocks(&blocks, 1, t, streaming).into_any_element()
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
                .text_size(px(12.))
                .child(SharedString::from(format!(
                    "⚠ {} ({}KB)",
                    crate::i18n::tr("消息内容过大，已按纯文本显示"),
                    kb
                ))),
        )
        .child(
            div()
                .id("md-oversize")
                .max_h(px(420.))
                .overflow_y_scroll()
                .font_family(MONO_FAMILY)
                .text_size(px(12.))
                .line_height(relative(1.5))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(src.to_string())),
        )
        .into_any_element()
}

/// Render with the active global theme.
pub fn render_themed(src: &str) -> AnyElement {
    render(src, crate::theme::theme(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(runs: &[Run]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    fn parse(src: &str) -> Vec<MdBlock> {
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TABLES);
        opts.insert(Options::ENABLE_TASKLISTS);
        opts.insert(Options::ENABLE_GFM);
        opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
        parse_blocks(&Parser::new_ext(src, opts).collect::<Vec<_>>())
    }

    #[test]
    fn headings_and_paragraphs() {
        let blocks = parse("# Title\n\nhello world");
        assert_eq!(blocks.len(), 2);
        match &blocks[0] {
            MdBlock::Heading { level, runs } => {
                assert_eq!(*level, 1);
                assert_eq!(text_of(runs), "Title");
            }
            other => panic!("{other:?}"),
        }
        match &blocks[1] {
            MdBlock::Paragraph { runs } => assert_eq!(text_of(runs), "hello world"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bold_and_code_runs() {
        let blocks = parse("a **b** `c` d");
        match &blocks[0] {
            MdBlock::Paragraph { runs } => {
                let styles: Vec<Style> = runs.iter().map(|r| r.style).collect();
                assert_eq!(styles, vec![Style::Normal, Style::Bold, Style::Normal, Style::Code, Style::Normal]);
                assert_eq!(text_of(runs), "a b c d");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn code_block_keeps_newlines_and_lang() {
        let blocks = parse("```rust\nfn a() {}\n```");
        match &blocks[0] {
            MdBlock::Code { lang, code } => {
                assert_eq!(lang, "rust");
                assert_eq!(code, "fn a() {}\n");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn list_items_with_markers() {
        let blocks = parse("- one\n- two\n\n1. x");
        let items: Vec<&MdBlock> =
            blocks.iter().filter(|b| matches!(b, MdBlock::ListItem { .. })).collect();
        assert_eq!(items.len(), 3);
        match items[0] {
            MdBlock::ListItem { marker, runs, depth, .. } => {
                assert_eq!(marker, "\u{2022}");
                assert_eq!(*depth, 0);
                assert_eq!(text_of(runs), "one");
            }
            other => panic!("{other:?}"),
        }
        match items[2] {
            MdBlock::ListItem { marker, .. } => assert_eq!(marker, "1."),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn code_highlight_produces_colored_runs() {
        for dark in [false, true] {
            let segs = highlight_segments("fn main() {}\n", "rust", dark);
            assert!(!segs.is_empty(), "dark={dark}");
            // keyword "fn" should be styled differently from plain text
            assert!(segs.iter().any(|(t, _)| t.contains("fn")));
            assert!(segs.len() > 1, "expected multiple colored segments, dark={dark}");
        }
    }

    #[test]
    fn table_parses_head_and_rows() {
        let blocks = parse("| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |");
        match &blocks[0] {
            MdBlock::Table { head, rows } => {
                assert_eq!(head.len(), 2);
                assert_eq!(text_of(&head[0]), "a");
                assert_eq!(rows.len(), 2);
                assert_eq!(text_of(&rows[1][0]), "3");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn image_parses_url_and_alt() {
        let blocks = parse("![alt text](img.png)");
        match &blocks[0] {
            MdBlock::Image { url, alt } => {
                assert_eq!(url, "img.png");
                assert_eq!(text_of(alt), "alt text");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn strikethrough_run() {
        let blocks = parse("~~gone~~ kept");
        match &blocks[0] {
            MdBlock::Paragraph { runs } => {
                assert!(runs.iter().any(|r| r.style == Style::Strike && r.text == "gone"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn task_list_marker_parsed() {
        let blocks = parse("- [x] done
- [ ] todo");
        let items: Vec<&MdBlock> =
            blocks.iter().filter(|b| matches!(b, MdBlock::ListItem { .. })).collect();
        assert_eq!(items.len(), 2);
        match items[0] {
            MdBlock::ListItem { task, .. } => assert_eq!(*task, Some(true)),
            other => panic!("{other:?}"),
        }
        match items[1] {
            MdBlock::ListItem { task, .. } => assert_eq!(*task, Some(false)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn gfm_autolink_becomes_link_run() {
        let blocks = parse("see https://example.com/x now");
        match &blocks[0] {
            MdBlock::Paragraph { runs } => {
                assert!(runs.iter().any(|r| r.style == Style::Link && r.text.contains("example.com")));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn yaml_frontmatter_swallowed() {
        let blocks = parse("---
title: x
---

hello");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::Paragraph { runs } => assert_eq!(text_of(runs), "hello"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn single_tilde_is_not_strikethrough() {
        // pi-web remark-gfm {singleTilde:false}: CJK 范围 "5~7" 不误删
        let blocks = parse("range 5~7 days");
        match &blocks[0] {
            MdBlock::Paragraph { runs } => {
                assert!(!runs.iter().any(|r| r.style == Style::Strike));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn chinese_text_roundtrip() {
        let blocks = parse("\u{4e2d}\u{6587}**\u{52a0}\u{7c97}**\u{6d4b}\u{8bd5}");
        match &blocks[0] {
            MdBlock::Paragraph { runs } => {
                assert_eq!(text_of(runs), "\u{4e2d}\u{6587}\u{52a0}\u{7c97}\u{6d4b}\u{8bd5}");
                assert_eq!(runs[1].style, Style::Bold);
            }
            other => panic!("{other:?}"),
        }
    }
}
