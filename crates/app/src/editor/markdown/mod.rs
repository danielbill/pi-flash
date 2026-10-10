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
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::ops::Range;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color, ThemeSet};
use syntect::parsing::SyntaxSet;

use crate::ui::ScrollAxisExt;

/// Syntax highlighting state (loaded once; ~100ms cold, cached for process life).
struct Syn {
    ps: SyntaxSet,
    ts: ThemeSet,
    /// 深色主题（VS Code Dark+ 近似，代码构建——syntect 默认主题集无
    /// vscDarkPlus，base16-ocean.dark 色系偏蓝灰差距大）
    dark: syntect::highlighting::Theme,
}

/// 启动期预热（startup::spawn_boot_tasks 的后台线程调用）：syntect 语法集
/// + 主题集冷加载 ~百 ms，此前落在首次 md 渲染帧里（第一次渲染卡顿）。
/// OnceLock 线程安全——预热未完成时 UI 线程首渲仍会等待，但启动后用户
/// 手速开文件必然晚于后台装载完成。
pub fn warm_up() {
    let syn = syn();
    let _ = syn.ps.syntaxes().len();
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
        let dark = vs_dark_plus();
        Syn { ps, ts, dark }
    })
}

/// 语法高亮主题按 UI 明暗切换（pi-web: isDark ? vscDarkPlus : vs）。
const LIGHT_THEME: &str = "InspiredGitHub";

/// VS Code Dark+ 调色板（dark_plus.json 的 token 色），按选择器前缀匹配。
/// 空选择器 = 全局默认前景 #D4D4D4。
fn vs_dark_plus() -> syntect::highlighting::Theme {
    use syntect::highlighting::{Color, StyleModifier, ThemeItem};
    let fg = |rgb24: u32| Color { r: (rgb24 >> 16) as u8, g: (rgb24 >> 8) as u8, b: rgb24 as u8, a: 0xff };
    let item = |selector: &str, rgb24: u32| ThemeItem {
        scope: selector.parse().expect("valid scope selector"),
        style: StyleModifier { foreground: Some(fg(rgb24)), background: None, font_style: None },
    };
    let table: &[(&str, u32)] = &[
        ("", 0xd4d4d4),
        ("comment", 0x6a9955),
        ("string", 0xce9178),
        ("string.regexp", 0xd16969),
        ("constant.numeric", 0xb5cea8),
        ("constant.character.escape", 0xd7ba7d),
        ("constant.language", 0x569cd6),
        ("keyword.control", 0xc586c0),
        ("keyword", 0x569cd6),
        ("storage", 0x569cd6),
        ("entity.name.function", 0xdcdcaa),
        ("support.function", 0xdcdcaa),
        ("entity.name.type", 0x4ec9b0),
        ("entity.name.class", 0x4ec9b0),
        ("entity.name.struct", 0x4ec9b0),
        ("entity.name.enum", 0x4ec9b0),
        ("support.type", 0x4ec9b0),
        ("support.class", 0x4ec9b0),
        ("entity.name.tag", 0x569cd6),
        ("entity.other.attribute-name", 0x9cdcfe),
        ("variable", 0x9cdcfe),
        ("support.variable", 0x9cdcfe),
    ];
    syntect::highlighting::Theme {
        name: Some("vs-dark-plus".into()),
        author: Some("pi-flash".into()),
        settings: Default::default(),
        scopes: table.iter().map(|(s, c)| item(s, *c)).collect(),
    }
}

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
    let theme = if dark {
        &syn.dark
    } else {
        syn.ts.themes.get(LIGHT_THEME).unwrap_or_else(|| syn.ts.themes.values().next().unwrap())
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

// 高亮结果缓存：gpui List 每帧对可见条目重建元素树，syntect 正则状态机
// 对大代码块是 ms 级纯 CPU，输入不变时每帧白打（打字/拖选/流式动画/光标
// 闪烁任一原因的重绘都会连坐）。key = 代码哈希 + 长度 + 语言哈希 + 明暗，
// 命中后源文全等校验防碰撞；线程局部 VecDeque LRU（与 MD_PARSE_CACHE
// 同款，元素构建只在主线程）。流式期间代码块逐 delta 变化，最多占满队头
// 被挤出，不影响命中态。
thread_local! {
    static HL_CACHE: std::cell::RefCell<
        std::collections::VecDeque<
            ((u64, usize, u64, bool), (String, String, std::rc::Rc<Vec<(String, [u8; 3])>>)),
        >,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
const HL_CACHE_CAP: usize = 128;

/// [`highlight_segments`] 的 LRU 包装（渲染热路径一律走这里）。
fn cached_highlight_segments(
    code: &str,
    lang: &str,
    dark: bool,
) -> std::rc::Rc<Vec<(String, [u8; 3])>> {
    let key = (hash_str(code), code.len(), hash_str(lang), dark);
    HL_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
            if entry.1 .0.as_str() == code && entry.1 .1.as_str() == lang {
                let segs = std::rc::Rc::clone(&entry.1 .2);
                cache.push_back(entry); // 刷新到队尾（LRU）
                return segs;
            }
        }
        let segs = std::rc::Rc::new(highlight_segments(code, lang, dark));
        cache.push_back((key, (code.to_string(), lang.to_string(), std::rc::Rc::clone(&segs))));
        if cache.len() > HL_CACHE_CAP {
            cache.pop_front();
        }
        segs
    })
}

/// 等宽字体（pi-web --font-mono 首选 JetBrains Mono；三档字重随二进制打包，
/// main.rs 注册。gpui 单 family 参数，无回退链——打包保证可解析）
pub(crate) const MONO_FAMILY: &str = "JetBrains Mono";

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
    Heading { level: u8, runs: Vec<Run> },
    Paragraph { runs: Vec<Run> },
    Code { lang: String, code: String },
    Quote { blocks: Vec<MdBlock> },
    ListItem { depth: usize, marker: String, runs: Vec<Run>, task: Option<bool> },
    Table { head: Vec<Vec<Run>>, rows: Vec<Vec<Vec<Run>>> },
    Image { url: String, alt: Vec<Run>, width: Option<f32> },

    /// 块级公式（$$…$$，v57-2）
    Math { latex: String },
    /// mermaid 图代码块（v57-3）
    Mermaid { source: String },
    Rule,
}

/// 样式栈元素：(样式, 链接目标)。URL 随样式进栈——链接内嵌粗体等嵌套
/// 场景不丢跳转目标，渲染层按 run.url 接点击。
pub(crate) type StyleEntry = (Style, Option<String>);

fn style_push(styles: &mut Vec<StyleEntry>, tag: &Tag) {
    let (base, base_url) = styles
        .last()
        .map(|(s, u)| (*s, u.clone()))
        .unwrap_or((Style::Normal, None));
    let next = match tag {
        Tag::Strong => (
            match base {
                Style::Italic | Style::BoldItalic => Style::BoldItalic,
                _ => Style::Bold,
            },
            base_url,
        ),
        Tag::Emphasis => (
            match base {
                Style::Bold | Style::BoldItalic => Style::BoldItalic,
                _ => Style::Italic,
            },
            base_url,
        ),
        Tag::Link { dest_url, .. } => (Style::Link, Some(dest_url.to_string())),
        Tag::Strikethrough => (Style::Strike, base_url),
        _ => (base, base_url),
    };
    styles.push(next);
}

fn style_pop(styles: &mut Vec<StyleEntry>, end: &TagEnd) {
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
fn collect_inline(
    events: &[Event],
    i: &mut usize,
    is_end: &dyn Fn(&Event) -> bool,
    html: bool,
) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut styles: Vec<StyleEntry> = Vec::new();
    let mut text = String::new();
    let mut cur = Style::Normal;
    let mut cur_url: Option<String> = None;

    fn flush(text: &mut String, cur: Style, cur_url: &Option<String>, runs: &mut Vec<Run>) {
        if text.is_empty() {
            return;
        }
        let taken = std::mem::take(text);
        if cur == Style::Normal {
            // GFM 自动链接：普通文本里的裸 URL 提为 Link run（目标 = 文本本身）
            for (seg, is_link) in split_links(&taken) {
                if !seg.is_empty() {
                    let url = is_link.then(|| seg.clone());
                    runs.push(Run {
                        text: seg,
                        style: if is_link { Style::Link } else { Style::Normal },
                        url,
                    });
                }
            }
        } else {
            runs.push(Run { text: taken, style: cur, url: cur_url.clone() });
        }
    }

    while *i < events.len() {
        match &events[*i] {
            e if is_end(e) => {
                *i += 1;
                flush(&mut text, cur, &cur_url, &mut runs);
                return runs;
            }
            Event::Text(t) => {
                let (s, u) = styles
                    .last()
                    .map(|(s, u)| (*s, u.clone()))
                    .unwrap_or((Style::Normal, None));
                if s != cur || u != cur_url {
                    flush(&mut text, cur, &cur_url, &mut runs);
                    cur = s;
                    cur_url = u;
                }
                text.push_str(t);
            }
            Event::InlineHtml(h) => {
                // v57-1: 行内 HTML；html=false（用户气泡）时标签原文可见。
                // 开/闭标签跨事件维持样式栈（配对标签样式不丢）
                flush(&mut text, cur, &cur_url, &mut runs);
                if html {
                    use crate::editor::markdown::render::html::InlineHtmlEffect as E;
                    match crate::editor::markdown::render::html::fragment_effect(h) {
                        E::StylePush(st, url) => styles.push((st, url)),
                        E::StylePop => {
                            styles.pop();
                        }
                        E::Runs(rs) => runs.extend(rs),
                    }
                    let (s, u) = styles
                        .last()
                        .map(|(s, u)| (*s, u.clone()))
                        .unwrap_or((Style::Normal, None));
                    cur = s;
                    cur_url = u;
                } else {
                    runs.extend(literal_runs(h));
                    cur = Style::Normal;
                    cur_url = None;
                    styles.clear();
                }
            }
            Event::InlineMath(tex) => {
                // v57-2: 行内公式标记 run（渲染期拆段成图）
                flush(&mut text, cur, &cur_url, &mut runs);
                runs.push(Run { text: tex.to_string(), style: Style::Math, url: None });
            }
            Event::DisplayMath(tex) => {
                // 多行 $$…$$ 的 DisplayMath 事件发在段落流内（实测），同用标记
                flush(&mut text, cur, &cur_url, &mut runs);
                runs.push(Run { text: tex.to_string(), style: Style::DisplayMath, url: None });
            }
            Event::Code(c) => {
                flush(&mut text, cur, &cur_url, &mut runs);
                runs.push(Run { text: c.to_string(), style: Style::Code, url: None });
            }
            // html=false 即用户气泡路径（render_user 唯一调用方）：段内软
            // 换行保留 \n（pi-web parity：.markdown-user-message p 的
            // white-space:pre-wrap——用户分次回车的多行按原文折行，而非
            // CommonMark 默认折叠成空格）
            Event::SoftBreak => text.push(if html { ' ' } else { '\n' }),
            Event::HardBreak => text.push('\n'),
            Event::Start(tag) => style_push(&mut styles, tag),
            Event::End(end) => style_pop(&mut styles, end),
            _ => {}
        }
        *i += 1;
    }
    flush(&mut text, cur, &cur_url, &mut runs);
    runs
}

// ---------------------------------------------------------------------------
// block parsing
// ---------------------------------------------------------------------------

/// HTML 块内容三路分发：doc 模式 img 提取（html_block_doc）/ 安全子集映射
/// （html::blocks）/ 用户气泡标签原文。裸 `Event::Html` 与
/// `Start(Tag::HtmlBlock)` 包裹层共用。
fn html_block_dispatch(raw: &str, html: bool) -> Vec<MdBlock> {
    if html && doc_mode() {
        html_block_doc(raw)
    } else if html {
        crate::editor::markdown::render::html::blocks(raw)
    } else {
        vec![MdBlock::Paragraph { runs: literal_runs(raw) }]
    }
}

fn parse_blocks(events: &[Event], html: bool) -> Vec<MdBlock> {
    let mut out: Vec<MdBlock> = Vec::new();
    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Start(Tag::HtmlBlock) => {
                // pulldown 0.10+ 把块级 HTML 包进 Start/End(HtmlBlock)。此前
                // 落进 parse_block 的兜底「跳到容器尾」，内部 Event::Html 整段
                // 被吞——README 头部 <p><img></p>+tagline 就这样消失（图片
                // 不显示的真根因，v57-1/doc 模式两条 HTML 路径都没被走到）。
                i += 1;
                let mut raw = String::new();
                while i < events.len() && !matches!(events[i], Event::End(TagEnd::HtmlBlock)) {
                    if let Event::Html(h) | Event::InlineHtml(h) = &events[i] {
                        raw.push_str(h);
                    }
                    i += 1;
                }
                i += 1; // End(HtmlBlock)
                out.extend(html_block_dispatch(&raw, html));
            }
            Event::Start(tag) => {
                let tag = tag.clone();
                i += 1;
                parse_block(events, &mut i, tag, &mut out, 0, html);
            }
            Event::Rule => {
                out.push(MdBlock::Rule);
                i += 1;
            }
            Event::Text(t) => {
                out.push(MdBlock::Paragraph {
                    runs: vec![Run { text: t.to_string(), style: Style::Normal, url: None }],
                });
                i += 1;
            }
            Event::Html(h) => {
                out.extend(html_block_dispatch(h, html));
                i += 1;
            }
            Event::DisplayMath(tex) => {
                // v57-2: 块级公式（$$…$$）
                out.push(MdBlock::Math { latex: tex.to_string() });
                i += 1;
            }
            Event::InlineHtml(h) => {
                if html {
                    let runs = crate::editor::markdown::render::html::inline_runs(h);
                    if !runs.is_empty() {
                        out.push(MdBlock::Paragraph { runs });
                    }
                } else {
                    out.push(MdBlock::Paragraph { runs: literal_runs(h) });
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
    html: bool,
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
}, html);
                    out.push(MdBlock::Image { url, alt, width: None });
                }
                while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::Paragraph)) {
                    *i += 1;
                }
                *i += 1;
            } else {
                let runs = collect_inline(events, i, &|e| {
                    matches!(e, Event::End(TagEnd::Paragraph))
}, html);
                out.push(MdBlock::Paragraph { runs });
            }
        }
        Tag::Heading { level, .. } => {
            let runs = collect_inline(events, i, &|e| {
                matches!(e, Event::End(TagEnd::Heading(_)))
}, html);
            out.push(MdBlock::Heading { level: level as u8, runs });
        }
        Tag::BlockQuote(_) => {
            let mut inner = Vec::new();
            while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::BlockQuote(_))) {
                if let Event::Start(t) = &events[*i] {
                    let t = t.clone();
                    *i += 1;
                    parse_block(events, i, t, &mut inner, depth + 1, html);
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
            if lang.trim() == "mermaid" {
                // v57-3: mermaid 图（渲染期流式回退源码）
                out.push(MdBlock::Mermaid { source: code });
            } else {
                out.push(MdBlock::Code { lang, code });
            }
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
                        head = parse_table_cells(
                            events,
                            i,
                            &|e| matches!(e, Event::End(TagEnd::TableHead)),
                            html,
                        );
                    }
                    Event::Start(Tag::TableRow) => {
                        *i += 1;
                        let row = parse_table_cells(
                            events,
                            i,
                            &|e| matches!(e, Event::End(TagEnd::TableRow)),
                            html,
                        );
                        rows.push(row);
                    }
                    _ => *i += 1,
                }
            }
            out.push(MdBlock::Table { head, rows });
        }
        Tag::Image { dest_url, .. } => {
            let alt = collect_inline(events, i, &|e| matches!(e, Event::End(TagEnd::Image)), html);
            out.push(MdBlock::Image { url: dest_url.to_string(), alt, width: None });
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
}, html);
                                        if !runs.is_empty() {
                                            if let Some(last) = runs.last_mut() {
                                                last.text.push(' ');
                                            }
                                        }
                                        runs.extend(r);
                                    }
                                    Tag::List(_) => {
                                        parse_block(events, i, inner_tag, &mut nested, depth + 1, html);
                                    }
                                    Tag::CodeBlock(_) | Tag::BlockQuote(_) | Tag::Table(_) => {
                                        parse_block(events, i, inner_tag, &mut nested, depth + 1, html);
                                    }
                                    Tag::Image { .. } => {
                                        parse_block(events, i, inner_tag, &mut nested, depth + 1, html);
                                    }
                                    Tag::HtmlBlock => {
                                        // 列表项内包裹式 HTML 块：收拢 chunks 走
                                        // 统一分发（i 已过 Start，此处到 End 为止）
                                        let mut raw = String::new();
                                        while *i < events.len()
                                            && !matches!(events[*i], Event::End(TagEnd::HtmlBlock))
                                        {
                                            if let Event::Html(h) | Event::InlineHtml(h) =
                                                &events[*i]
                                            {
                                                raw.push_str(h);
                                            }
                                            *i += 1;
                                        }
                                        *i += 1; // End(HtmlBlock)
                                        nested.extend(html_block_dispatch(&raw, html));
                                    }
                                    _ => {}
                                }
                            }
                            Event::Text(t) => {
                                runs.push(Run { text: t.to_string(), style: Style::Normal, url: None });
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
    html: bool,
) -> Vec<Vec<Run>> {
    let mut cells: Vec<Vec<Run>> = Vec::new();
    while *i < events.len() {
        if is_end(&events[*i]) {
            *i += 1;
            break;
        }
        if matches!(events[*i], Event::Start(Tag::TableCell)) {
            *i += 1;
            let runs =
                collect_inline(events, i, &|e| matches!(e, Event::End(TagEnd::TableCell)), html);
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
    MD_SPEC.with(|c| c.borrow().clone()).unwrap_or_else(crate::appearance::file_font)
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
            font_weight: Some(FontWeight::BOLD),
            font_style: Some(FontStyle::Italic),
            color: Some(rgb(crate::theme::mix_rgb(t.text, t.accent, 0.88)).into()),
            ..Default::default()
        },
        // note: gpui 0.2.2 highlights cannot change font family; 正文的行内 code
        // 走 paragraph_element 拆段成等宽盒，此高亮只是表格/标题内的兜底
        Style::Code => HighlightStyle { background_color: Some(rgb(t.tool_bg).into()), ..Default::default() },
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
        Style::Strike => HighlightStyle {
            strikethrough: Some(gpui::StrikethroughStyle { thickness: px(1.), ..Default::default() }),
            ..Default::default()
        },
        // 公式在渲染前已拆段为图片；防御性落到代码风
        Style::Math | Style::DisplayMath => {
            HighlightStyle { background_color: Some(rgb(t.tool_bg).into()), ..Default::default() }
        }
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

// ---------------------------------------------------------------------------
// 文件路径点击（pi-web lib/file-links.ts resolveLocalFileHref 的桌面简化版）
// ---------------------------------------------------------------------------

/// 点击打开文件标签页的目标：渲染入口由调用方 set（消息区 / 文件 md 预览），
/// 元素**构建期** clone 进事件闭包——事件期不读 thread_local，无跨帧陈旧。
#[derive(Clone)]
struct MdTarget {
    chat: gpui::WeakEntity<crate::Chat>,
    /// md 预览场景的相对路径基准（预览文件所在目录）；聊天消息 None =
    /// 点击时按 chat.cwd 解析
    base: Option<std::path::PathBuf>,
}

thread_local! {
    static MD_TARGET: std::cell::RefCell<Option<MdTarget>> =
        const { std::cell::RefCell::new(None) };
}

/// markdown 渲染入口处由调用方设置；消息区传 `None` base（相对路径按
/// chat.cwd），文件 md 预览传预览文件所在目录（pi-web baseDir 同型）。
pub(crate) fn set_link_target(
    chat: gpui::WeakEntity<crate::Chat>,
    base: Option<std::path::PathBuf>,
) {
    MD_TARGET.with(|c| *c.borrow_mut() = Some(MdTarget { chat, base }));
}

/// 构建期取当前目标（clone 进闭包）。
fn md_target() -> Option<MdTarget> {
    MD_TARGET.with(|c| c.borrow().clone())
}

/// 点击打开路径：有目标走 `Chat::open_file_tab`（已开复用置顶 / html→浏览器
/// / 二进制·超限·缺失兜底都在其中）；无目标退回系统关联打开（cx.open_url
/// 对本地路径 = ShellExecute）。
fn open_path_with(target: Option<MdTarget>, path: String, cx: &mut gpui::App) {
    if let Some(MdTarget { chat, base }) = target {
        let _ = chat.update(cx, |c, cx| {
            let p = std::path::Path::new(&path);
            let p = if p.is_absolute() {
                p.to_path_buf()
            } else {
                base.as_deref().unwrap_or(&c.cwd).join(p)
            };
            c.open_file_tab(p, cx);
        });
    } else {
        cx.open_url(&path);
    }
}

/// 行内 code 文本 / 链接 href 形似本地文件路径 → 可打开的路径串（已剥
/// `:行[:列]` 后缀）。判定锚定 code span / href 边界，路径内允许空格与
/// CJK（「已写入 `D:\...\1 每日博文\x.md`」场景）；不做盘上存在性检查
/// （渲染期 fs 调用不可接受，缺失由 open_file_tab 状态栏兜底）。
pub(crate) fn as_file_path(input: &str) -> Option<String> {
    let mut s = input.trim();
    // 成对引号包裹（agent 常见 `'D:\x y.md'` / `"D:\x.md"`）
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"'))
            || (s.starts_with('\'') && s.ends_with('\'')))
    {
        s = s[1..s.len() - 1].trim();
    }
    if s.is_empty() {
        return None;
    }
    // file:// URL 解码后按本地路径规则重走
    if let Some(rest) = s.strip_prefix("file://") {
        let decoded = percent_decode(rest);
        if let Some(body) = decoded.strip_prefix('/') {
            let body = body.trim_start_matches('/'); // 容错多余斜杠
            // file:///C:/x → C:/x（盘符）；file:///home/u → /home/u（POSIX 根）
            if looks_like_win_drive(body) {
                return Some(body.to_string());
            }
            return is_local_path(&decoded).then(|| decoded);
        }
        // file://server/share/x → UNC \\server\share\x（Windows 惯用反斜杠）
        let unc = format!("\\\\{}", decoded.replace('/', "\\"));
        return is_local_path(&unc).then_some(unc);
    }
    // 其他 scheme 一律拒绝（http:/https:/data:…；单字母 `X:` 是盘符不算）
    if let Some(colon) = s.find(':') {
        let head = &s[..colon];
        if head.len() >= 2 && head.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
    }
    // 剥尾部行列号（:12 / :12:3，纯数字才算；盘符 `C:\x` 的冒号后非数字不受影响）
    let mut s = s;
    for _ in 0..2 {
        let Some(colon) = s.rfind(':') else { break };
        let tail = &s[colon + 1..];
        if colon > 0 && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
            s = &s[..colon];
        } else {
            break;
        }
    }
    is_local_path(s).then(|| s.to_string())
}

/// 路径形态判定：绝对（盘符/UNC/POSIX 根）或含分隔符的相对路径；纯文件名
/// 不判路径（防 `TODO`/`README.md` 误判——写过的文件已有轮末 chips 链路）。
fn is_local_path(s: &str) -> bool {
    if s.is_empty() || s.starts_with('#') || s.starts_with('?') {
        return false;
    }
    if looks_like_win_drive(s) || s.starts_with("\\\\") || s.starts_with('/') {
        return true;
    }
    s.contains('/') || s.contains('\\')
}

fn looks_like_win_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(hi) = (b[i + 1] as char).to_digit(16)
            && let Some(lo) = (b[i + 2] as char).to_digit(16)
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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

fn render_blocks(blocks: &[MdBlock], depth: usize, t: &Theme, streaming: bool, color: u32) -> gpui::Div {
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

// ---------------------------------------------------------------------------
// 代码块横向滚动句柄表——Zed Markdown::code_block_scroll_handles 的无状态
// 等价物：渲染函数没有实体可挂句柄，仿 ui::vlist SCROLL_HANDLES 建线程
// 局部表，按代码内容哈希键控（同内容块共享句柄=镜像滚动，罕见且无害）。
// Scrollbar 拖拽写句柄偏移，track_scroll 的滚动容器每帧读同一偏移。
// 哈希键无法枚举回收，超容量整表清空。
// ---------------------------------------------------------------------------
struct CodeScroll {
    handle: gpui::ScrollHandle,
    bar_state: gpui_component::scroll::ScrollbarState,
}

thread_local! {
    static CODE_SCROLLS: std::cell::RefCell<
        std::collections::HashMap<u64, std::rc::Rc<CodeScroll>>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

fn code_scroll(hash: u64) -> std::rc::Rc<CodeScroll> {
    CODE_SCROLLS.with(|cell| {
        let mut map = cell.borrow_mut();
        if map.len() > 256 {
            map.clear();
        }
        map.entry(hash)
            .or_insert_with(|| {
                std::rc::Rc::new(CodeScroll {
                    handle: gpui::ScrollHandle::new(),
                    bar_state: gpui_component::scroll::ScrollbarState::default(),
                })
            })
            .clone()
    })
}

/// 代码块：外框圆角 7px + 头部（语言名 / 复制）+ 行号 + 高亮体（字号 =
/// 槽位字号 -1，字体大小设置.md「代码块内容」；pi-web 行高 1.62）。
/// 文档模式代码块（Zed/GitHub 预览形制）：无语言标签/复制/行号 chrome，
/// 圆角框 + 语法高亮 + 横向滚动。
fn render_code_block_doc(lang: &str, code: &str, t: &Theme) -> gpui::AnyElement {
    let code = code.trim_end_matches('\n');
    let body_bg = crate::theme::mix_rgb(t.bg, t.bg_panel, 0.92);
    let mut text = String::new();
    let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    for (seg, c) in cached_highlight_segments(code, lang, t.dark).iter() {
        if seg.is_empty() {
            continue;
        }
        let start = text.len();
        text.push_str(seg);
        push_color(&mut highlights, start, text.len(), *c);
    }
    let spec = active_md_spec();
    let base = TextStyle {
        color: rgb(t.text).into(),
        font_family: MONO_FAMILY.into(),
        font_size: px(spec.size - 1.).into(),
        line_height: relative(1.6),
        ..Default::default()
    };
    let hash = hash_str(&format!("doc\0{lang}\0{code}"));
    let scroll = code_scroll(hash);
    div()
        .relative()
        .w_full()
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(8.))
        .bg(rgb(body_bg))
        .overflow_hidden()
        .child(
            div()
                .id(SharedString::from(format!("md-code-doc-{hash:016x}")))
                // Zed markdown 代码块同构：滚动容器必须 display:flex——gpui
                // block 子元素被拉伸到容器宽，永远无横向溢出可滚；flex_none
                // 内层让 nowrap 文本按自然宽度溢出成可滚内容
                .flex()
                .w_full()
                .px(px(14.))
                .py(px(12.))
                .text_size(px(spec.size - 1.))
                .line_height(relative(1.6))
                .whitespace_nowrap()
                .overflow_x_scroll()
                // 锁轴：纵向滚轮穿透给外层页面滚动，横向滚轮只滚本块
                .restrict_scroll_to_axis()
                .track_scroll(&scroll.handle)
                .child(
                    div().flex_none().child(
                        StyledText::new(text).with_default_highlights(&base, highlights),
                    ),
                ),
        )
        // 横向滑块：Scrollbar 必须套显式 absolute 细条（用户气泡/导航浮层
        // 的既有配方）——直接挂容器用其自带 Absolute+100% 布局在 gpui 0.2.2
        // 下测不出可见 thumb。内容不超宽时 Scrollbar 自隐藏，无需手动 gate。
        .child(
            div()
                .absolute()
                .left(px(3.))
                .right(px(3.))
                .bottom(px(3.))
                .h(px(8.))
                .child(gpui_component::scroll::Scrollbar::horizontal(
                    &scroll.bar_state,
                    &scroll.handle,
                )),
        )
        .into_any_element()
}

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
        .text_size(px(crate::ui::tokens::fixed::MD_LANG_BAR))
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
        // LRU 命中返回 Rc 借用，不再每帧 clone 全部高亮段
        let segs = cached_highlight_segments(code, lang, t.dark);
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

    let spec = active_md_spec();
    let base = TextStyle {
        color: rgb(t.text).into(),
        font_family: MONO_FAMILY.into(),
        font_size: px(spec.size - 1.).into(),
        line_height: relative(1.62),
        ..Default::default()
    };
    let hash = hash_str(&format!("chat\0{lang}\0{code}"));
    let scroll = code_scroll(hash);

    div()
        .w_full()
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(7.))
        .overflow_hidden()
        .bg(rgb(body_bg))
        // pi-web box-shadow: 0 1px 0 border 42%（块底一条更深的细线）
        .shadow(vec![gpui::BoxShadow {
            color: gpui::rgba(crate::theme::border_alpha(t, 0x6b)).into(),
            offset: gpui::point(px(0.), px(1.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
        }])
        .child(header)
        .child(
            div()
                .relative()
                .w_full()
                .child(
                    div()
                        .id(SharedString::from(format!("md-code-body-{hash:016x}")))
                        // Zed 同构：flex 滚动容器 + flex_none 内层（见
                        // render_code_block_doc 注释），块级横向滚动才有
                        // 溢出内容可滚
                        .flex()
                        .w_full()
                        .px(px(13.))
                        .py(px(11.))
                        // 字号挂容器（见 sized_text 注释）：代码块 = 槽位字号 -1
                        .text_size(px(spec.size - 1.))
                        .line_height(relative(1.62))
                        // c15：长行不再裁剪——nowrap + 横向滚动（pi-web <pre> 语义）
                        .whitespace_nowrap()
                        .overflow_x_scroll()
                        // 锁轴：纵向滚轮穿透给外层页面滚动，横向滚轮只滚本块
                        .restrict_scroll_to_axis()
                        .track_scroll(&scroll.handle)
                        .child(
                            div().flex_none().child(
                                StyledText::new(text).with_default_highlights(&base, highlights),
                            ),
                        ),
                )
                // 横向滑块：显式 absolute 细条（同 render_code_block_doc 注释）
                .child(
                    div()
                        .absolute()
                        .left(px(3.))
                        .right(px(3.))
                        .bottom(px(3.))
                        .h(px(8.))
                        .child(gpui_component::scroll::Scrollbar::horizontal(
                            &scroll.bar_state,
                            &scroll.handle,
                        )),
                ),
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

    // 字体大小设置.md：表格头 = 槽位字号，表格内容 = 槽位字号 -1，行高 1.7
    // （字号经 sized_text 挂容器，见其注释）
    let head_cells: Vec<gpui::AnyElement> = head
        .iter()
        .enumerate()
        .map(|(ci, cell)| {
            cell_div(fracs[ci])
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(crate::theme::mix_rgb(t.text, t.text_muted, 0.88)))
                .child(sized_text(cell, t, BASE, 1.7, crate::theme::mix_rgb(t.text, t.text_muted, 0.88), FontWeight::SEMIBOLD))
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
                line = line.child(cell_div(fracs[ci]).child(sized_text(cell, t, 13., 1.7, t.text, FontWeight::NORMAL)));
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
/// 相对路径按 md 文件目录解析（IMG_BASE，render_themed 设置）。
fn render_image(url: &str, width: Option<f32>, alt: &[Run], t: &Theme) -> gpui::AnyElement {    let placeholder = || {
        div()
            .w_full()
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
    let path = std::path::Path::new(url);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        resolve_relative_image(path)
    };
    let Some((bytes, format)) = std::fs::read(&path).ok().zip(format) else {
        return placeholder();
    };
    div()
        .w_full()
        .child(
            gpui::img(std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)))
                .max_w_full()
                .when_some(width, |d, w| d.w(px(w.max(24.))))
                .rounded(px(6.)),
        )
        .into_any_element()
}

/// 行内 code 盒（pi-web .markdown-inline-code）：mono（槽位字号 -1.12 =
/// 0.92em 的绝对差换算）+ bg-subtle + 圆角 5 + padding 1px 5px + 70% 边框
/// 描边；颜色恒 --text（引用块内不继承）。
/// gpui 行内 highlight 换不了字体家族，拆盒才能落 mono。
fn inline_code_box(text: &str, t: &Theme) -> AnyElement {
    let spec = active_md_spec();
    let style = TextStyle {
        color: rgb(t.text).into(),
        font_family: MONO_FAMILY.into(),
        font_size: px(spec.size - 1.12).into(),
        line_height: relative(1.5),
        ..Default::default()
    };
    let mut box_div = div()
        .max_w_full()
        .rounded(px(5.))
        .bg(rgba(t.bg_subtle))
        .border_1()
        .border_color(gpui::rgba((t.border << 8) | 0xb3))
        .px(px(5.))
        .py(px(1.))
        // 字号挂容器（见 sized_text 注释）：行内 code = 槽位 -1.12（0.92em）
        .text_size(px(spec.size - 1.12))
        .line_height(relative(1.5));
    // 路径形 code 可点开文件标签页（pi-web 消息链接的行内 code 变体）：
    // on_mouse_down 无需 stateful id，流式增长中也即时可点；非路径 code 盒零改动
    if let Some(path) = as_file_path(text) {
        let target = md_target();
        let accent = t.accent;
        box_div = box_div
            .cursor_pointer()
            .hover(move |s| s.text_color(rgb(accent)))
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                open_path_with(target.clone(), path.clone(), cx);
            });
    }
    box_div
        .child(StyledText::new(text.to_string()).with_default_highlights(&style, Vec::new()))
        .into_any_element()
}

/// 行内富段：含 Code/Math 的段落拆 flex-wrap 段（文本段内部自然换行，
/// code = 等宽盒、公式 = 图片）；纯文本段落保持单一 StyledText。
fn paragraph_element(runs: &[Run], t: &Theme, color: u32) -> AnyElement {
    let rich = runs
        .iter()
        .any(|r| matches!(r.style, Style::Code | Style::Math | Style::DisplayMath));
    if !rich {
        return sized_text(runs, t, BASE, 1.7, color, FontWeight::NORMAL);
    }
    let flush =
        |row: gpui::Div, tr: &mut Vec<Run>, t: &Theme, color: u32| -> gpui::Div {
            if !tr.is_empty() {
                let taken = std::mem::take(tr);
                return row.child(div().max_w_full().child(sized_text(&taken, t, BASE, 1.7, color, FontWeight::NORMAL)));
            }
            row
        };
    let mut row = div().w_full().flex().flex_wrap().items_end();
    let mut text_run: Vec<Run> = Vec::new();
    for r in runs {
        match r.style {
            Style::Code => {
                row = flush(row, &mut text_run, t, color);
                row = row.child(inline_code_box(&r.text, t));
            }
            Style::Math => {
                row = flush(row, &mut text_run, t, color);
                row = row.child(crate::editor::markdown::render::math::inline_element(&r.text, t));
            }
            Style::DisplayMath => {
                row = flush(row, &mut text_run, t, color);
                // 块级公式：flex_wrap 下 w_full 独占一行
                row = row.child(div().w_full().child(crate::editor::markdown::render::math::block_element(&r.text, t)));
            }
            _ => text_run.push(r.clone()),
        }
    }
    row = flush(row, &mut text_run, t, color);
    row.into_any_element()
}

fn render_block(b: &MdBlock, depth: usize, t: &Theme, streaming: bool, color: u32) -> AnyElement {
    match b {
        MdBlock::Heading { level, runs } => {
            let doc = doc_mode();
            let spec = active_md_spec();
            // 文档模式 = Zed/GitHub 尺度（h1/h2 放大 + 底部分隔线）；聊天区
            // 维持 pi-web 标题规则（h1 1.16em …）
            let size = if doc {
                doc_size_for_level(*level, spec.size)
            } else {
                size_for_level(*level)
            };
            // pi-web 标题规则：h1/h2/h4-h6 = var(--text)（引用块内也是），
            // h3 = color-mix(text 88%, muted)
            let color = if *level == 3 {
                crate::theme::mix_rgb(t.text, t.text_muted, 0.88)
            } else {
                t.text
            };
            let text_size = if doc { size } else { spec.size + (size - BASE) };
            let underlined = doc && (*level <= 2);
            div()
                .w_full()
                .font_weight(if doc && *level == 1 { FontWeight::BOLD } else { FontWeight::SEMIBOLD })
                .text_size(px(text_size))
                .line_height(relative(1.35))
                .when(underlined, |d| {
                    d.pb(px(if *level == 1 { 7. } else { 5. }))
                        .border_b_1()
                        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x55)))
                })
                .child(runs_element(runs, t, size, 1.35, color, FontWeight::SEMIBOLD))
                .into_any_element()
        }
        MdBlock::Paragraph { runs } => paragraph_element(runs, t, color),
        MdBlock::Code { code, lang, .. } => {
            if doc_mode() {
                render_code_block_doc(lang, code, t)
            } else {
                render_code_block(lang, code, t, streaming).into_any_element()
            }
        }
        MdBlock::Quote { blocks } => div()
            .w_full()
            .border_l_3()
            .border_color(rgb(crate::theme::mix_rgb(t.border, t.text_muted, 0.75)))
            .rounded_r(px(6.))
            .bg(rgba(t.bg_subtle))
            .px(px(11.))
            .py(px(6.))
            // pi-web blockquote color: var(--text-muted)——经 color 参数落进
            // 内部段落的 StyledText（此前容器色被子元素硬编码 text 覆盖）
            .child(render_blocks(blocks, depth + 1, t, streaming, t.text_muted))
            .into_any_element(),
        MdBlock::ListItem { depth: d, marker, runs, task } => {
            let task = *task;
            // 悬挂缩进（pi-web ul{padding-left:22px} + li{padding-left:2px}
            // + list-style-position:outside）：marker 落在文字左侧的固定
            // 槽里，文字统一从 24px 起排，嵌套每层 +22px。
            // items_start（非 center）：pi-web outside marker 与内容第一行
            // 对齐——多行 item 时圆点/勾选框必须钉在首行，垂直居中会漂到
            // 整条 item 中间；行盒 = 1.7×字号，文本 marker（"1."）行盒同高
            // 自然落首行
            let slot = div()
                .w(px(24.))
                .flex_shrink_0()
                .flex()
                .items_start()
                .justify_end() // marker 靠槽右缘 = 悬挂在文字左侧
                .pr(px(4.)) // marker 右边到文字的视觉间隙（浏览器 outside marker）
                // 字体大小设置.md「list mark = 设置值」；绝对差换算下
                // 不随外层标题字号复利放大
                .text_size(px(active_md_spec().size))
                .line_height(relative(1.7))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(crate::theme::mix_rgb(t.accent, t.text_muted, 0.72)));
            let marker_el: AnyElement = match task {
                // c12: 任务项 marker = 14px 复选框（选中 = accent 10% 淡底 +
                // 55% 边框 + accent 对勾，pi-web :checked 规则）；top:0.35em
                // 钉在首行（pi-web input 绝对定位 top 0.35em parity）
                Some(checked) => slot
                    .child(
                        div()
                            .mt(px(5.))
                            .w(px(14.))
                            .h(px(14.))
                            .flex_shrink_0()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(rgb(if checked {
                                crate::theme::mix_rgb(t.accent, t.border, 0.55)
                            } else {
                                t.border
                            }))
                            .bg(rgb(if checked {
                                crate::theme::mix_rgb(t.accent, t.bg, 0.10)
                            } else {
                                t.bg
                            }))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(checked, |b| {
                                b.child(
                                    div()
                                        .text_size(px(crate::ui::tokens::fixed::MD_CHECK_GLYPH))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(t.accent))
                                        .child(SharedString::from("✓")),
                                )
                            }),
                    )
                    .into_any_element(),
                // 无序列表：画 0.45em 实心圆（Chrome list-style disc 尺寸），
                // 首行行盒内垂直居中——"•" 字形在 14px 下只有 ~4px 且偏细
                None if marker == "•" => {
                    let spec = active_md_spec();
                    let dot = (0.45 * spec.size).round();
                    let first_line = 1.7 * spec.size;
                    slot.child(
                        div()
                            .mt(px(((first_line - dot) / 2.).max(0.)))
                            .size(px(dot))
                            .rounded(px(dot / 2.))
                            .bg(rgb(crate::theme::mix_rgb(t.accent, t.text_muted, 0.72))),
                    )
                    .into_any_element()
                }
                None => slot.child(SharedString::from(marker.clone())).into_any_element(),
            };
            div()
                .flex()
                .pl(px((d * 22) as f32))
                .child(marker_el)
                .child(div().flex_1().min_w_0().child(paragraph_element(runs, t, t.text)))
                .into_any_element()
        }
        MdBlock::Table { head, rows } => render_table(head, rows, t).into_any_element(),
        MdBlock::Image { url, alt, width } => render_image(url, *width, alt, t),
        MdBlock::Math { latex } => crate::editor::markdown::render::math::block_element(latex, t),
        MdBlock::Mermaid { source } => {
            // pi-web MermaidBlock parity：流式期间只显源码；失败回退源码块
            if !streaming {
                if let Some(el) = crate::editor::markdown::render::mermaid::diagram_element(source, t.dark) {
                    return el;
                }
            }
            render_code_block("mermaid", source, t, true).into_any_element()
        }
        MdBlock::Rule => {
            // pi-web hr: 渐变淡出线（linear-gradient 90deg transparent-border-
            // transparent）。gpui 渐变仅两停靠点，用底线 + 两端 18% 遮罩复现
            let base: gpui::Hsla = rgb(t.border).into();
            let bg: gpui::Hsla = rgb(t.bg).into();
            let bg_a0 = gpui::Hsla { a: 0., ..bg };
            div()
                .w_full()
                .h(px(1.))
                .relative()
                .child(div().size_full().bg(base))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w(relative(0.18))
                        .h_full()
                        .bg(gpui::linear_gradient(
                            90.,
                            gpui::linear_color_stop(bg, 0.),
                            gpui::linear_color_stop(bg_a0, 1.),
                        )),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .w(relative(0.18))
                        .h_full()
                        .bg(gpui::linear_gradient(
                            90.,
                            gpui::linear_color_stop(bg_a0, 0.),
                            gpui::linear_color_stop(bg, 1.),
                        )),
                )
                .into_any_element()
        }
    }
}

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

/// FNV-1a 按 8 字节块处理（进程内去重键，非加密；命中另有全等校验兜底）
pub(crate) fn hash_str(s: &str) -> u64 {
    let mut h = 0xcbf29ce484222325;
    let mut chunks = s.as_bytes().chunks_exact(8);
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
            if entry.1 .0.as_str() == src {
                let blocks = std::rc::Rc::clone(&entry.1 .1);
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
        if cache.len() > MD_PARSE_CACHE_CAP {
            cache.pop_front();
        }
        parsed
    })
}

/// Html/InlineHtml 的字面显示（html=false 路径）：标签原文可见。
fn literal_runs(html_src: &str) -> Vec<Run> {
    vec![Run { text: html_src.to_string(), style: Style::Normal, url: None }]
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
                    crate::i18n::tr("消息内容过大，已按纯文本显示"),
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

// ---------------------------------------------------------------------------
// 023 文档模式（fileView md 预览专用）：Zed/GitHub 式文档排版——标题放大
// 带分隔线、代码块素装（无标签/复制/行号）、HTML 块提取 <img>、相对路径
// 图片按 md 文件目录解析。聊天区渲染完全不受影响（doc 标志渲染前置位/
// 后复位，元素树构建是同步的）。
// ---------------------------------------------------------------------------

static DOC_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn doc_mode() -> bool {
    DOC_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

fn set_doc_mode(on: bool) {
    DOC_MODE.store(on, std::sync::atomic::Ordering::Relaxed);
}

static IMG_BASE: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

fn img_base() -> Option<std::path::PathBuf> {
    IMG_BASE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// 相对图片路径解析（025 P1）：md 目录直查 → md 同级 `assets/` 直层 →
/// `assets/<子目录>/`同名匹配（Obsidian 归档结构 assets/文章名/file.png，
/// wiki 引用多为裸文件名）→ 都找不到回退原路径（占位）。
fn resolve_relative_image(url: &std::path::Path) -> std::path::PathBuf {
    let Some(base) = img_base() else {
        return url.to_path_buf();
    };
    let direct = base.join(url);
    if direct.exists() {
        return direct;
    }
    let assets = base.join("assets");
    if let Ok(rd) = std::fs::read_dir(&assets) {
        let flat = assets.join(url);
        if flat.exists() {
            return flat;
        }
        let Some(name) = url.file_name() else {
            return url.to_path_buf();
        };
        for e in rd.flatten() {
            let cand = e.path().join(name);
            if cand.is_file() {
                return cand;
            }
        }
    }
    url.to_path_buf()
}

/// 文档模式标题字号（Zed/GitHub 尺度，相对预览正文字号）。
fn doc_size_for_level(level: u8, base: f32) -> f32 {
    match level {
        1 => base * 2.05,
        2 => base * 1.5,
        3 => base * 1.26,
        4 => base * 1.1,
        _ => base,
    }
}

/// HTML 块的文档模式兜底（html.rs 块级解析对「<p><img></p>+裸文本」这类
/// 连排块会整段丢失——README 头部即此形态）：提取全部 <img> 为图片块，
/// 剩余标签剥掉、内文保留为段落。
fn html_block_doc(h: &str) -> Vec<MdBlock> {
    fn attr(tag: &str, name: &str) -> Option<String> {
        let lower = tag.to_ascii_lowercase();
        let key = format!("{name}=");
        let i = lower.find(&key)?;
        let rest = &tag[i + key.len()..];
        let quote = rest.chars().next()?;
        let val = if quote == '"' || quote == '\'' {
            let end = rest[1..].find(quote)? + 1;
            &rest[1..end]
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            &rest[..end]
        };
        Some(val.to_string())
    }
    let mut out = Vec::new();
    let mut rest = h;
    while let Some(pos) = rest.find("<img") {
        let Some(end) = rest[pos..].find('>') else { break };
        let tag = &rest[pos..=pos + end];
        if let Some(url) = attr(tag, "src") {
            let width = attr(tag, "width")
                .map(|w| w.trim().trim_end_matches("px").trim().to_string())
                .and_then(|w| w.parse::<f32>().ok());
            out.push(MdBlock::Image { url, width, alt: Vec::new() });
        }
        rest = &rest[pos + end + 1..];
    }
    // 剥标签留内文
    let mut text = String::new();
    let mut in_tag = false;
    for ch in h.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
    if !text.is_empty() {
        out.push(MdBlock::Paragraph {
            runs: vec![Run { text, style: Style::Normal, url: None }],
        });
    }
    out
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

/// 文档预览虚拟化（抄 zed thread_view 的 list 架构，v60）：文件预览不再
/// 一次性构建整棵元素树（progress.md 784 块全量构建 ≈ 30ms/帧，滚动必
/// 卡），改由 ListState 每帧只建可视条目。条目 = 单个 md 块（zed 的
/// MarkdownElement 同样按块出元素、块自带 margins）。两步交给调用方接
/// gpui::list：
/// - [`doc_blocks`]：解析（LRU 缓存）+ 取块数当 item_count；
/// - [`render_doc_item`]：list 条目闭包，懒渲染第 ix 块。
pub fn doc_blocks(src: &str) -> std::rc::Rc<Vec<MdBlock>> {
    // 025 P1：Obsidian wiki 图片 `![[file.png]]` → 标准 `![](file.png)`。
    // pulldown-cmark 不认 wiki 语法（Obsidian 迁移库大量存在）；在解析
    // 前统一展开（fence 代码块内不碰，纯 `[[wiki链接]]` 不碰）。
    let expanded = expand_wiki_images(src);
    set_doc_mode(true);
    let blocks = cached_blocks(&expanded, true);
    set_doc_mode(false);
    blocks
}

/// 把源码里的 wiki 图片引用展开为标准 markdown（逐行，fence 感知）。
/// `![[a.png]]` → `![](a.png)`；`![[a.png|300]]` 取尺寸前的文件名。
/// 另：独占一行的图片引用（wiki 或标准式）前后补空行——Obsidian 习惯
/// 回车即图（图与文字间常无空行），GFM 同段落不产图片块，不补分割
/// 预览就不出图（025 P1 实翻车）。
fn expand_wiki_images(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 32);
    let mut in_fence = false;
    for line in src.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        let expanded = expand_wiki_line(line);
        let trimmed = expanded.trim();
        if is_standalone_image_ref(trimmed) {
            let blank_before = out.is_empty() || out.ends_with("\n\n");
            if !blank_before {
                out.push('\n');
            }
            out.push_str(&expanded);
            if !expanded.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        } else {
            out.push_str(&expanded);
        }
    }
    out
}

/// 整行恰为一条图片引用：`![[…]]` 或 `![](…)`（闭合到行尾）。
fn is_standalone_image_ref(line: &str) -> bool {
    if let Some(rest) = line.strip_prefix("![[") {
        return rest.ends_with("]]" ) && !rest.contains('\n');
    }
    if let Some(rest) = line.strip_prefix("![](") {
        return rest.ends_with(')')
            && !rest.contains("![](")
            && !rest.contains('\n');
    }
    false
}

fn expand_wiki_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    loop {
        let Some(s) = rest.find("![[") else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..s]);
        let after = &rest[s + 3..];
        let Some(e) = after.find("]]" ) else {
            // 无闭合：原样保留剩余部分
            out.push_str(&rest[s..]);
            break;
        };
        let name = &after[..e];
        if name.is_empty() || name.contains('[') || name.contains(']') || name.contains('\n') {
            out.push_str(&rest[s..s + 3 + e + 2]);
        } else {
            // Obsidian 尺寸语法 `![[a.png|300]]`：取管道前文件名
            let file = name.split('|').next().unwrap_or(name).trim();
            out.push_str("![](");
            out.push_str(file);
            out.push(')');
        }
        rest = &after[e + 2..];
    }
    out
}

/// list 条目渲染。doc_mode/字体规格/img 基准在条目内自设——list 条目
/// 懒渲染发生在调用方 render 函数返回之后，不能依赖外层的 set/reset 窗口。
pub fn render_doc_item(
    blocks: &std::rc::Rc<Vec<MdBlock>>,
    ix: usize,
    base_dir: Option<&std::path::Path>,
) -> AnyElement {
    let mut spec = crate::appearance::file_font();
    spec.size = (spec.size * 1.15).round();
    set_md_spec(spec);
    set_doc_mode(true);
    *IMG_BASE.lock().unwrap_or_else(|e| e.into_inner()) = base_dir.map(|p| p.to_path_buf());
    let el = render_doc_item_inner(blocks, ix, crate::theme::theme());
    set_doc_mode(false);
    el
}

fn render_doc_item_inner(blocks: &[MdBlock], ix: usize, t: &Theme) -> AnyElement {
    let Some(b) = blocks.get(ix) else {
        return div().into_any_element();
    };
    // 本块的 mb 不在这里落：非末块由下一条目的 top 取用，末块弃用
    // （文档 pb(40) 兜底）——与整树版"末块 mb 无人消费"一致
    let mt = block_margins(b).0;
    let first = ix == 0;
    let last = ix + 1 == blocks.len();
    // 块间距 = max(前块 mb, 本块 mt)——整树版 render_blocks 的合并规则，
    // 但虚拟化后没有"上一条目的已渲染 margin"可折算，改按块数据自洽：
    // 每条目自己算 pt（前块 margins 可查），mb 只喂给下一条目、自身不落。
    // li→li 之间是列表容器 gap 3px；首条目额外吃文档 pt(26)；末条目弃
    // mb、文档 pb(40) 兜底（全部与整树版逐像素同规）
    let top = if first {
        26. + mt
    } else if matches!(blocks[ix - 1], MdBlock::ListItem { .. })
        && matches!(b, MdBlock::ListItem { .. })
    {
        3.
    } else {
        block_margins(&blocks[ix - 1]).1.max(mt)
    };
    div()
        .w_full()
        .flex()
        .pt(px(top))
        .when(last, |d| d.pb(px(40.)))
        .child(flex_span(4., Some(20.)))
        // 内容段必须显式 min_w(0)：flex 项的自动最小尺寸 = min-content，而
        // gpui 的文本在 MinContent 下不折行（wrap_width = None，单行宽度），
        // 于是 92% 段被撑到整段单行宽、正文永远不回行（右缘溢出）。min_w(0)
        // 后段宽 = 字面 92/100 容器宽，StyledText 拿到 definite 宽自然折行
        .child(
            flex_span(92., None)
                .min_w(px(0.))
                .child(render_block(b, 1, t, false, t.text)),
        )
        .child(flex_span(4., Some(20.)))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    /// 025 P1：wiki 图片展开——fence 外展开、fence 内不碰、尺寸语法取文件名。
    #[test]
    fn wiki_images_expanded_outside_fence_only() {
        let src = "看图\n\n![[file-a.png]]\n\n```\n![[code.png]]\n```\n\n![[b.png|300]]\n";
        let blocks = super::doc_blocks(src);
        let imgs: Vec<&str> = blocks
            .iter()
            .filter_map(|b| match b {
                MdBlock::Image { url, .. } => Some(url.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(imgs, vec!["file-a.png", "b.png"]);
    }

    /// 025 P1：含空格路径用 <> 包裹后 pulldown 能正常解析为图片。
    #[test]
    fn spaced_path_in_angle_brackets_parses_as_image() {
        let blocks = super::doc_blocks("![](<assets/文章 一/file-1.png>)");
        assert!(matches!(
            blocks.first(),
            Some(MdBlock::Image { url, .. }) if url == "assets/文章 一/file-1.png"
        ));
    }

    /// 裸空格路径（不包 <>）解析失败作为对照——证明包裹必要性。
    #[test]
    fn bare_spaced_path_fails_to_parse_as_image() {
        let blocks = super::doc_blocks("![](assets/文章 一/file-1.png)");
        assert!(!matches!(blocks.first(), Some(MdBlock::Image { .. })));
    }
    use super::*;

    fn text_of(runs: &[Run]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    fn parse(src: &str) -> Vec<MdBlock> {
        parse_with(src, true)
    }

    fn parse_with(src: &str, html: bool) -> Vec<MdBlock> {
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TABLES);
        opts.insert(Options::ENABLE_TASKLISTS);
        opts.insert(Options::ENABLE_GFM);
        opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
        // v57-2: $…$ / $$…$$ 数学（pi-web remark-math parity）
        opts.insert(Options::ENABLE_MATH);
        parse_blocks(&Parser::new_ext(src, opts).collect::<Vec<_>>(), html)
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

    /// README 头部形（`<p><img></p>` HTML 块）：doc 模式兜底必须提取出
    /// 图片块（src/width/alt），纯文本剥标签后保留（回归锁 pi-flash 图片
    /// 不显示——html.rs 行内路径对块内 img 只产 🖼 占位文本）。
    #[test]
    fn html_block_doc_extracts_img_with_width() {
        let blocks = html_block_doc(
            "<p align=\"left\"><img src=\"crates/app/assets/icon/pi-flash-256.png\" width=\"88\" alt=\"pi-flash logo\"></p>",
        );
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::Image { url, alt, width } => {
                assert_eq!(url, "crates/app/assets/icon/pi-flash-256.png");
                assert_eq!(*width, Some(88.));
                assert_eq!(text_of(alt), "");
            }
            other => panic!("{other:?}"),
        }
    }

    /// 同上兜底的剥标签支路：img 之外的文本内容不能丢。
    #[test]
    fn html_block_doc_keeps_text_after_img() {
        let blocks = html_block_doc(
            "<p><img src=\"a.png\">尾随文本</p>",
        );
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], MdBlock::Image { url, .. } if url == "a.png"));
        match &blocks[1] {
            MdBlock::Paragraph { runs } => assert_eq!(text_of(runs), "尾随文本"),
            other => panic!("{other:?}"),
        }
    }

    /// 链接可点击的前提：`[text](url)` 的 dest_url 必须随 run 保留
    /// （此前 Tag::Link{..} 直接丢弃，链接只有样式没有目标）。
    #[test]
    fn link_keeps_dest_url() {
        let blocks = parse("see [网易转载原文](https://example.com/a?b=1) here");
        match &blocks[0] {
            MdBlock::Paragraph { runs } => {
                assert_eq!(runs.len(), 3);
                assert_eq!(runs[0].url, None);
                assert_eq!(runs[1].style, Style::Link);
                assert_eq!(runs[1].text, "网易转载原文");
                assert_eq!(runs[1].url.as_deref(), Some("https://example.com/a?b=1"));
                assert_eq!(runs[2].url, None);
            }
            other => panic!("{other:?}"),
        }
    }

    /// GFM autolink：裸 URL 提为 Link run，目标 = 文本本身。
    #[test]
    fn autolink_url_is_text() {
        let blocks = parse("visit https://example.com/x. ok");
        match &blocks[0] {
            MdBlock::Paragraph { runs } => {
                assert_eq!(runs.len(), 3);
                assert_eq!(runs[1].style, Style::Link);
                assert_eq!(runs[1].text, "https://example.com/x");
                assert_eq!(runs[1].url.as_deref(), Some("https://example.com/x"));
            }
            other => panic!("{other:?}"),
        }
    }

    /// 链接内嵌粗体：样式合成 Bold，但 URL 随样式栈保留（点击不丢）。
    #[test]
    fn nested_bold_in_link_keeps_url() {
        let blocks = parse("[**bold link**](https://example.com)");
        match &blocks[0] {
            MdBlock::Paragraph { runs } => {
                assert_eq!(runs.len(), 1);
                assert_eq!(runs[0].style, Style::Bold);
                assert_eq!(runs[0].url.as_deref(), Some("https://example.com"));
            }
            other => panic!("{other:?}"),
        }
    }

    /// 行内 `<a href>`：href 经 InlineHtml 分支随 run 保留。
    #[test]
    fn inline_html_link_keeps_href() {
        let blocks = parse_with("<a href=\"https://x.example\">y</a>", true);
        match &blocks[0] {
            MdBlock::Paragraph { runs } => {
                assert_eq!(runs.len(), 1);
                assert_eq!(runs[0].style, Style::Link);
                assert_eq!(runs[0].url.as_deref(), Some("https://x.example"));
            }
            other => panic!("{other:?}"),
        }
    }

    // ---- as_file_path：消息内文件路径点击的识别层 ----

    #[test]
    fn file_path_accepts_absolute_and_relative() {
        // Windows 盘符（含空格 + CJK——截图场景）
        assert_eq!(
            as_file_path(r"D:\my_obsidian\文章\1 每日博文\x.md").as_deref(),
            Some(r"D:\my_obsidian\文章\1 每日博文\x.md")
        );
        assert_eq!(as_file_path("D:/a/b.md").as_deref(), Some("D:/a/b.md"));
        // UNC / POSIX 根
        assert_eq!(as_file_path(r"\\server\share\x.txt").as_deref(), Some(r"\\server\share\x.txt"));
        assert_eq!(as_file_path("/usr/local/bin/zsh").as_deref(), Some("/usr/local/bin/zsh"));
        // 含分隔符的相对路径
        assert_eq!(as_file_path("src/foo.rs").as_deref(), Some("src/foo.rs"));
        assert_eq!(as_file_path(r"..\x.py").as_deref(), Some(r"..\x.py"));
    }

    #[test]
    fn file_path_strips_line_col_suffix() {
        assert_eq!(as_file_path("src/app.rs:120").as_deref(), Some("src/app.rs"));
        assert_eq!(as_file_path("src/app.rs:120:5").as_deref(), Some("src/app.rs"));
        // 盘符冒号不受影响
        assert_eq!(as_file_path(r"C:\x.md").as_deref(), Some(r"C:\x.md"));
        // 剥完失去分隔符 = 纯文件名，不认
        assert_eq!(as_file_path("foo.md:12"), None);
    }

    #[test]
    fn file_path_rejects_non_paths() {
        // 纯文件名/普通词：写过的文件已有轮末 chips，防误判
        assert_eq!(as_file_path("README.md"), None);
        assert_eq!(as_file_path("TODO"), None);
        assert_eq!(as_file_path("npm install"), None);
        // 其他 scheme
        assert_eq!(as_file_path("https://example.com/a"), None);
        assert_eq!(as_file_path("mailto:a@b.c"), None);
        assert_eq!(as_file_path("#anchor"), None);
        assert_eq!(as_file_path(""), None);
    }

    #[test]
    fn file_path_quotes_and_file_url() {
        // 成对引号包裹
        assert_eq!(as_file_path("'D:\\x y.md'").as_deref(), Some("D:\\x y.md"));
        assert_eq!(as_file_path("\"src/a b.rs\"").as_deref(), Some("src/a b.rs"));
        // file:// URL：盘符 / POSIX 根 / UNC
        assert_eq!(as_file_path("file:///C:/Users/x.md").as_deref(), Some("C:/Users/x.md"));
        assert_eq!(as_file_path("file:///home/u/x").as_deref(), Some("/home/u/x"));
        assert_eq!(
            as_file_path("file://server/share/x.txt").as_deref(),
            Some(r"\\server\share\x.txt")
        );
        // 百分号解码
        assert_eq!(as_file_path("file:///D:/%E6%96%87/x.md").as_deref(), Some("D:/文/x.md"));
    }

    /// DOC_MODE 恢复 guard：断言失败也要复位，免得毒化同进程其他测试。
    struct DocModeGuard;
    impl Drop for DocModeGuard {
        fn drop(&mut self) {
            set_doc_mode(false);
        }
    }

    /// pulldown 0.13 包裹式 HTML 块回归锁（README 头部形态）：块级 HTML 被
    /// Start(HtmlBlock) 包裹后，此前落进 parse_block 兜底整段被吞——图片
    /// 和 tagline 一起消失。此测锁住「包裹内容必须走到 doc 分发」；其余
    /// 测试均不含块级 HTML，不受并行下 DOC_MODE 翻转影响。
    #[test]
    fn html_block_wrapped_reaches_doc_dispatch() {
        let _g = DocModeGuard;
        set_doc_mode(true);
        let blocks = parse_with(
            "# t\n\n<p align=\"left\"><img src=\"a.png\" width=\"88\" alt=\"l\"></p>\ntagline text\n\n## h\n",
            true,
        );
        assert_eq!(blocks.len(), 4, "heading + Image + tagline Paragraph + heading");
        match &blocks[1] {
            MdBlock::Image { url, width, .. } => {
                assert_eq!(url, "a.png");
                assert_eq!(*width, Some(88.));
            }
            other => panic!("{other:?}"),
        }
        match &blocks[2] {
            // CommonMark type-6 HTML 块到空行为止：下一行 tagline 同属块内，
            // 剥标签后保留为段落（Zed 截图同款：链接语法按原文显示）
            MdBlock::Paragraph { runs } => assert_eq!(text_of(runs), "tagline text"),
            other => panic!("{other:?}"),
        }
    }

    /// 非 doc 模式（聊天 html=true 不存在，此测直打 html::blocks 分支）：
    /// 包裹式 HTML 块不能再整段消失。
    #[test]
    fn html_block_wrapped_reaches_html_subset_dispatch() {
        let blocks = parse_with("<div>hello</div>\n\nafter", true);
        assert!(!blocks.is_empty(), "块级 HTML 不得整段丢失");
        assert!(blocks.iter().any(|b| matches!(b, MdBlock::Paragraph { runs } if text_of(runs).contains("hello"))));
        match &blocks[blocks.len() - 1] {
            MdBlock::Paragraph { runs } => assert_eq!(text_of(runs), "after"),
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
    fn highlight_cache_hits_return_identical_segments() {
        let code = "fn main() {\n    let x = 1;\n}\nlet y = 2;\n";
        let direct = highlight_segments(code, "rust", true);
        let first = cached_highlight_segments(code, "rust", true);
        let second = cached_highlight_segments(code, "rust", true); // 应命中
        assert_eq!(&direct, &first[..]);
        assert_eq!(&direct, &second[..]);
        // 明暗/语言不串缓存
        let light = cached_highlight_segments(code, "rust", false);
        assert_eq!(&highlight_segments(code, "rust", false), &light[..]);
        let py = cached_highlight_segments(code, "python", true);
        assert_eq!(&highlight_segments(code, "python", true), &py[..]);
    }

    /// 开销留档（--nocapture 看）：大代码块 syntect 冷跑 vs 缓存命中。
    /// Chat 每帧重建可见元素树，此差值就是原先编辑/拖选时每帧白打的成本。
    #[test]
    fn highlight_cache_cost_probe() {
        let unit = "fn generated_item(n: usize) -> usize {\n    let mut acc = n ^ 0x9E37_79B9;\n    for i in 0..8 {\n        acc = acc.rotate_left(7).wrapping_mul(0x100_0000_001B3);\n    }\n    match acc % 5 { 0 => acc, 1 => acc + 1, 2 => !acc, 3 => acc >> 2, _ => acc << 2 }\n}\n";
        let code = unit.repeat(40); // ~320 行，AI 回复常见体量
        warm_up(); // syn() 一次性语法集装载不计入
        let t = std::time::Instant::now();
        let cold = highlight_segments(&code, "rust", true);
        let d_cold = t.elapsed();
        let _ = cached_highlight_segments(&code, "rust", true);
        let t = std::time::Instant::now();
        for _ in 0..100 {
            let _ = cached_highlight_segments(&code, "rust", true);
        }
        let d_hot = t.elapsed();
        eprintln!(
            "[perf] highlight {}B/{}lines: syntect {:.2} ms, cached hit {:.4} ms",
            code.len(),
            cold.iter().filter(|(s, _)| s.contains('\n')).count(),
            d_cold.as_secs_f64() * 1e3,
            d_hot.as_secs_f64() * 1e3 / 100.
        );
        assert!(d_cold > d_hot / 100, "缓存命中应显著快于 syntect 冷跑");
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
            MdBlock::Image { url, alt, width } => {
                assert_eq!(url, "img.png");
                assert_eq!(text_of(alt), "alt text");
                assert_eq!(*width, None);
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
    fn inline_math_becomes_marker_run() {
        let blocks = parse("能量公式 $E=mc^2$ 很有名");
        match &blocks[0] {
            MdBlock::Paragraph { runs } => {
                assert!(runs.iter().any(|r| r.style == Style::Math && r.text == "E=mc^2"));
                assert!(runs.iter().any(|r| r.style == Style::Normal && r.text.contains("能量公式")));
            }
            other => panic!("{other:?}"),
        }
    }

    /// 渲染冒烟：vs Dark+ 高亮 / 行内 code 盒 / 引用 muted / 任务框 / hr
    /// 渐变各路径在明暗两主题下都不 panic（vs_dark_plus 是代码构建主题）。
    #[test]
    fn render_smoke_all_paths_both_modes() {
        let src = "# 标题\n\n正文 **bold** `code` [link](https://x.y) $E=mc^2$\n\n```rust\nfn a() {}\n```\n\n> 引用 **内粗**\n\n- [x] done\n- [ ] todo\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---\n";
        for t in crate::theme::ALL {
            let el = render(src, &t.1, false);
            let _ = el.into_any_element();
        }
    }

    #[test]
    fn display_math_is_block() {
        let blocks = parse(r"前文

$$
\frac{1}{2}
$$");
        let has_display = blocks.iter().any(|b| matches!(b, MdBlock::Paragraph { runs }
            if runs.iter().any(|r| r.style == Style::DisplayMath && r.text.contains("frac"))));
        assert!(has_display, "multiline $$ must surface as DisplayMath marker run");
    }


    #[test]
    fn user_mode_shows_html_literals() {
        // render_user：标签不渲染、原文可见（用户消息凭证原则）
        let blocks = parse_with("<b>粗体</b> 保持", false);
        match &blocks[0] {
            MdBlock::Paragraph { runs } => {
                let all = runs.iter().map(|r| r.text.as_str()).collect::<String>();
                assert!(all.contains("<b>粗体</b>"), "tags must stay literal: {all}");
                assert!(runs.iter().all(|r| r.style == Style::Normal));
            }
            other => panic!("{other:?}"),
        }
        // html=true 路径依旧渲染（assistant）
        let blocks2 = parse_with("<b>粗体</b> 保持", true);
        match &blocks2[0] {
            MdBlock::Paragraph { runs } => {
                assert!(!runs.iter().any(|r| r.text.contains("<b>")), "tags consumed");
                assert!(runs.iter().any(|r| r.style == Style::Bold));
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

#[cfg(test)]
mod real_file_tests {
    /// 真实文章回归（025 P1）：12 个引用全展开 + 磁盘能找到图片文件。
    #[test]
    fn real_article_all_refs_expand_and_resolve() {
        let p = std::path::Path::new(
            r"D:\my_obsidian\gitee_vault\mynotes\20观宏知微\文章\1 每日博文\一块RTX5070跑Qwen3.8-next-flash 150B飚速90TS！算力自由降临了？.md",
        );
        let Ok(src) = std::fs::read_to_string(p) else {
            eprintln!("skip: 真实文章不在本机");
            return;
        };
        let refs = src.matches("![[").count() + src.matches("![](").count();
        let blocks = super::doc_blocks(&src);
        let imgs: Vec<&str> = blocks
            .iter()
            .filter_map(|b| match b {
                super::MdBlock::Image { url, .. } => Some(url.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(imgs.len(), refs, "引用应全展开: {imgs:?}");
        // 每个 url：md 目录直查 或 assets/<子目录>/同名命中（render 出图前提）
        let base = p.parent().unwrap();
        for u in &imgs {
            let name = std::path::Path::new(u).file_name().unwrap();
            let direct = base.join(u);
            let hit = direct.exists()
                || std::fs::read_dir(base.join("assets"))
                    .map(|rd| rd.flatten().any(|e| e.path().join(name).is_file()))
                    .unwrap_or(false);
            assert!(hit, "找不到图片文件: {u}");
        }
        eprintln!("全部 {refs} 个引用展开且磁盘命中");
    }
}
