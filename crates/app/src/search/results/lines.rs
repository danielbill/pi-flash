//! 摘录行渲染——行号 gutter + 单色正文 + 命中底色（083 视觉口径）。

use gpui::prelude::*;
use gpui::{AnyElement, SharedString, StyledText, div, px, rgb};
use std::ops::Range;

use super::{Gutter, LINE_HEIGHT, OnOpen, line_h};
use crate::appearance::theme::{Theme, theme as T};
use crate::search::ResultLine;

/// 摘录行：行号 gutter + 单色正文 + 命中底色。
/// 长行**折行**（`StyledText` 原生换行），行高随内容增长，与 Zed 一致。
pub(super) fn render_line(
    block_ix: usize,
    line: &ResultLine,
    active: Option<usize>,
    on_open: &OnOpen,
    seg_ix: usize,
    line_ix: usize,
    gutter: Gutter,
) -> AnyElement {
    let t = T();
    let font = crate::appearance::file_font();
    let is_active = line.hit && active.is_some() && line.ordinal == active;
    let click = on_open.clone();
    let ordinal = line.ordinal;
    let line_no = line.line;
    let row_h = line_h();
    let highlights = match_highlights(&line.text, &line.spans, t);
    div()
        .id(SharedString::from(format!(
            "psb-l-{block_ix}-{seg_ix}-{line_ix}"
        )))
        .min_h(px(row_h))
        .w_full()
        .flex()
        // 折行后行会变高：gutter 与正文都从顶对齐
        .items_start()
        // 活跃命中：**只有左缘竖条**，不刷整行底色（Zed 同；此前刷 accent 洗底
        // 是用户评「颜色五花八门」的主因之一）
        .when(is_active, |d| d.border_l_2().border_color(rgb(t.accent)))
        .when(!is_active, |d| {
            d.border_l_2().border_color(gpui::rgba(0x00000000))
        })
        // 无 hover 变色：Zed 结果区鼠标划过不变色（用户 2026-10-21 定案）
        .cursor_pointer()
        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
            click(block_ix, line_no, ordinal, cx);
        })
        // 行号 gutter：与正文同族同号；左留 1ch、数字区右对齐、右留 1ch
        // （宽度随最宽行号位数动态算，见 [`Gutter`]）
        .child(
            div()
                .w(px(gutter.total()))
                .flex_shrink_0()
                .pl(px(gutter.left))
                .pr(px(gutter.right))
                .whitespace_nowrap()
                .text_align(gpui::TextAlign::Right)
                .font_family(font.family.clone())
                .text_size(px(font.size))
                .line_height(gpui::relative(LINE_HEIGHT))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(line.line.to_string())),
        )
        // 正文：单色 + 命中底色（StyledText 原生折行）
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pr(px(8.))
                .font_family(font.family.clone())
                .text_size(px(font.size))
                .line_height(gpui::relative(LINE_HEIGHT))
                .text_color(rgb(t.text))
                .child(StyledText::new(line.text.clone()).with_highlights(highlights)),
        )
        .into_any_element()
}

/// 命中区间 → `StyledText` 的 highlight 列表（不重叠、按字节序）。
///
/// 只产出「命中底色」，不掺任何语法色——结果区是单色正文（见 [`render_line`]
/// 的视觉口径）。底色用主题中性 selection 色 `bg_selected`（Zed 的命中底色
/// 同样是中性色，不是 accent 洗底）。
pub(in crate::search) fn match_highlights(
    line: &str,
    spans: &[Range<usize>],
    t: &'static Theme,
) -> Vec<(Range<usize>, gpui::HighlightStyle)> {
    if line.is_empty() || spans.is_empty() {
        return Vec::new();
    }
    let match_bg: gpui::Hsla = rgb(t.bg_selected).into();
    let mut bounds: Vec<usize> = vec![0, line.len()];
    for span in spans {
        bounds.push(span.start.min(line.len()));
        bounds.push(span.end.min(line.len()));
    }
    bounds.retain(|b| line.is_char_boundary(*b));
    bounds.sort_unstable();
    bounds.dedup();

    let mut out = Vec::new();
    for pair in bounds.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start >= end {
            continue;
        }
        let hit = spans.iter().any(|s| s.contains(&start));
        let mut style = gpui::HighlightStyle::default();
        if hit {
            style.background_color = Some(match_bg);
            style.color = Some(rgb(t.text).into());
        }
        out.push((start..end, style));
    }
    out
}
