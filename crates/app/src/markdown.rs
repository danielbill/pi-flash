//! Minimal Markdown renderer for chat messages (pi-web MarkdownBody parity,
//! visual polish deferred). Streaming-friendly: whole-message re-render.

use gpui::{
    AnyElement, FontStyle, FontWeight, HighlightStyle, SharedString, StyledText, TextStyle,
    div, prelude::*, px, rgb,
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

const THEME: &str = "base16-ocean.dark";

/// Highlight `code` and return colored text segments.
fn highlight_segments(code: &str, lang: &str) -> Vec<(String, [u8; 3])> {
    let syn = syn();
    let syntax = lang
        .split(',')
        .next()
        .map(str::trim)
        .and_then(|l| syn.ps.find_syntax_by_token(l))
        .unwrap_or_else(|| syn.ps.find_syntax_plain_text());
    let Some(theme) = syn.ts.themes.get(THEME) else {
        return vec![(code.to_string(), [0xd7, 0xda, 0xdd])];
    };
    let mut hl = HighlightLines::new(syntax, theme);
    let mut out: Vec<(String, [u8; 3])> = Vec::new();
    for line in syntect::util::LinesWithEndings::from(code) {
        let Ok(ranges) = hl.highlight_line(line, &syn.ps) else { continue };
        for (style, text) in ranges {
            let Color { r, g, b, a: _ } = style.foreground;
            // merge consecutive segments with identical colors
            if let Some(last) = out.last_mut() {
                if last.1 == [r, g, b] {
                    last.0.push_str(text);
                    continue;
                }
            }
            out.push((text.to_string(), [r, g, b]));
        }
    }
    out
}

const MONO_FAMILY: &str = "Consolas";
const COL_TEXT: u32 = 0xe8e8e8; // --text
const COL_DIM: u32 = 0xa4a4a4; // --text-dim
const COL_CODE_BG: u32 = 0x222222; // --tool-bg
const COL_RULE: u32 = 0x454545; // --border
const COL_LINK: u32 = 0xa4c2f4; // --accent

// ---------------------------------------------------------------------------
// inline runs
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Style {
    Normal,
    Bold,
    Italic,
    BoldItalic,
    Code,
    Link,
}

#[derive(Clone, Debug)]
struct Run {
    text: String,
    style: Style,
}

#[derive(Clone, Debug)]
enum MdBlock {
    Heading { level: u8, runs: Vec<Run> },
    Paragraph { runs: Vec<Run> },
    Code { lang: String, code: String },
    Quote { blocks: Vec<MdBlock> },
    ListItem { depth: usize, marker: String, runs: Vec<Run> },
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
        _ => base,
    };
    styles.push(next);
}

fn style_pop(styles: &mut Vec<Style>, end: &TagEnd) {
    match end {
        TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link => {
            styles.pop();
        }
        _ => {}
    }
}

/// Collect inline runs until `is_end` matches (consuming the end event).
fn collect_inline(events: &[Event], i: &mut usize, is_end: &dyn Fn(&Event) -> bool) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut styles: Vec<Style> = Vec::new();
    let mut text = String::new();
    let mut cur = Style::Normal;

    fn flush(text: &mut String, cur: Style, runs: &mut Vec<Run>) {
        if !text.is_empty() {
            runs.push(Run { text: std::mem::take(text), style: cur });
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
            _ => i += 1,
        }
    }
    out
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
            let runs = collect_inline(events, i, &|e| matches!(e, Event::End(TagEnd::Paragraph)));
            out.push(MdBlock::Paragraph { runs });
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
                    while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::Item)) {
                        match &events[*i] {
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
                                    Tag::CodeBlock(_) | Tag::BlockQuote(_) => {
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
                    out.push(MdBlock::ListItem { depth, marker, runs });
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

// ---------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------

fn base_style(color: u32, size: f32) -> TextStyle {
    TextStyle {
        color: rgb(color).into(),
        font_family: "Segoe UI".into(),
        font_size: px(size).into(),
        ..Default::default()
    }
}

fn highlight(style: Style) -> Option<HighlightStyle> {
    let h = match style {
        Style::Normal => return None,
        Style::Bold => HighlightStyle { font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() },
        Style::Italic => HighlightStyle { font_style: Some(FontStyle::Italic), ..Default::default() },
        Style::BoldItalic => HighlightStyle {
            font_weight: Some(FontWeight::SEMIBOLD),
            font_style: Some(FontStyle::Italic),
            ..Default::default()
        },
        // note: gpui 0.2.2 highlights cannot change font family; code gets bg only
        Style::Code => HighlightStyle { background_color: Some(rgb(COL_CODE_BG).into()), ..Default::default() },
        Style::Link => HighlightStyle {
            color: Some(rgb(COL_LINK).into()),
            underline: Some(gpui::UnderlineStyle { thickness: px(1.), ..Default::default() }),
            ..Default::default()
        },
    };
    Some(h)
}

fn styled_text(runs: &[Run], color: u32, size: f32) -> StyledText {
    let mut s = String::new();
    let mut highlights = Vec::new();
    for r in runs {
        let start = s.len();
        s.push_str(&r.text);
        let end = s.len();
        if let Some(h) = highlight(r.style) {
            highlights.push((start..end, h));
        }
    }
    StyledText::new(s).with_default_highlights(&base_style(color, size), highlights)
}

fn size_for_level(level: u8) -> f32 {
    match level {
        1 => 22.,
        2 => 18.,
        3 => 16.,
        4 => 15.,
        _ => 14.,
    }
}

fn render_blocks(blocks: &[MdBlock], depth: usize) -> gpui::Div {
    let mut col = div().flex().flex_col().gap_2();
    for b in blocks {
        col = col.child(render_block(b, depth));
    }
    col
}

fn render_block(b: &MdBlock, depth: usize) -> AnyElement {
    match b {
        MdBlock::Heading { level, runs } => div()
            .w_full()
            .mt_2()
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(size_for_level(*level)))
            .text_color(rgb(COL_TEXT))
            .child(styled_text(runs, COL_TEXT, size_for_level(*level)))
            .into_any_element(),
        MdBlock::Paragraph { runs } => div()
            .w_full()
            .text_color(rgb(COL_TEXT))
            .child(styled_text(runs, COL_TEXT, 14.))
            .into_any_element(),
        MdBlock::Code { code, lang, .. } => {
            let code = code.trim_end();
            let base = TextStyle {
                color: rgb(COL_TEXT).into(),
                font_family: MONO_FAMILY.into(),
                font_size: px(12.).into(),
                ..Default::default()
            };
            let mut text = String::new();
            let mut highlights = Vec::new();
            for (seg, [r, g, b]) in highlight_segments(code, lang) {
                let start = text.len();
                text.push_str(&seg);
                let end = text.len();
                highlights.push((
                    start..end,
                    HighlightStyle { color: Some(rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32).into()), ..Default::default() },
                ));
            }
            div()
                .w_full()
                .my_1()
                .p_2()
                .rounded_md()
                .bg(rgb(COL_CODE_BG))
                .font_family(MONO_FAMILY)
                .text_xs()
                .text_color(rgb(COL_TEXT))
                .child(StyledText::new(text).with_default_highlights(&base, highlights))
                .into_any_element()
        }
        MdBlock::Quote { blocks } => div()
            .w_full()
            .border_l_2()
            .border_color(rgb(COL_RULE))
            .pl_3()
            .child(render_blocks(blocks, depth + 1))
            .into_any_element(),
        MdBlock::ListItem { depth: d, marker, runs } => div()
            .flex()
            .gap_2()
            .pl(px((d.saturating_sub(1) * 16) as f32))
            .child(div().text_color(rgb(COL_DIM)).child(SharedString::from(marker.clone())))
            .child(div().flex_1().text_color(rgb(COL_TEXT)).child(styled_text(runs, COL_TEXT, 14.)))
            .into_any_element(),
        MdBlock::Rule => div()
            .w_full()
            .h(px(1.))
            .my_1()
            .bg(rgb(COL_RULE))
            .into_any_element(),
    }
}

/// Render a markdown string as a vertical stack of styled GPUI elements.
pub fn render(src: &str) -> AnyElement {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    let events: Vec<Event> = Parser::new_ext(src, opts).collect();
    let blocks = parse_blocks(&events);
    if blocks.is_empty() {
        return div().into_any_element();
    }
    render_blocks(&blocks, 1).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(runs: &[Run]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    fn parse(src: &str) -> Vec<MdBlock> {
        parse_blocks(&Parser::new(src).collect::<Vec<_>>())
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
            MdBlock::ListItem { marker, runs, depth } => {
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
        let segs = highlight_segments("fn main() {}
", "rust");
        assert!(!segs.is_empty());
        // keyword "fn" should be styled differently from plain text
        assert!(segs.iter().any(|(t, _)| t.contains("fn")));
        assert!(segs.len() > 1, "expected multiple colored segments");
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
