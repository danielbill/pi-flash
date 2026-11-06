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
    // 流式块走 150ms 解析门（A3，2026-11-06 流式卡死修复）：门内复用上一
    // 棵解析树，非流式块照旧精确解析
    let blocks = if streaming {
        stream_gate_blocks(src, html)
    } else {
        cached_blocks(src, html)
    };
    if blocks.is_empty() {
        return div().into_any_element();
    }
    render_blocks(&blocks, 1, t, streaming, t.text).into_any_element()
}

mod cache;
mod runs;

use self::cache::{cached_blocks, flex_span, literal_runs, render_oversize, stream_gate_blocks};
pub(crate) use self::cache::{hash_bytes, hash_str, md_parse_probe};
pub(crate) use self::runs::highlight;
use self::runs::{
    block_margins, render_blocks, runs_element, runs_text, size_for_level, sized_text,
};
