use std::{ops::Range, rc::Rc};

use gpui::{
    fill, point, px, relative, size, App, Bounds, Corners, Element, ElementId, ElementInputHandler,
    Entity, GlobalElementId, Half, HighlightStyle, Hitbox, Hsla, IntoElement, LayoutId,
    MouseButton, MouseMoveEvent, Pixels, Point, ShapedLine, SharedString, Size, Style, TextRun,
    TextStyle, UnderlineStyle, Window,
};
use ropey::Rope;
use smallvec::SmallVec;

use crate::{
    input::{
        blink_cursor::CURSOR_WIDTH, line_layout::LineLayout, tab_map::TabExpansion, RopeExt as _,
    },
    ActiveTheme as _, Colorize, Root,
};

use super::fold_map::{FoldBufferExt, FoldPoint, Point as FoldSpacePoint};
use super::{mode::InputMode, Decorations, DisplayRowData, InputState, LastLayout};
use sum_tree::Bias;

const BOTTOM_MARGIN_ROWS: usize = 3;
pub(super) const RIGHT_MARGIN: Pixels = px(10.);
pub(super) const LINE_NUMBER_RIGHT_MARGIN: Pixels = px(10.);
/// 视口上下额外 shaping/高亮的软换行行数：方向键/选区端点会落在视口外
/// 1-2 行，这些行要保持真实 shaped（preferred_column 取 x 用）。
const EXTRA_SHAPE_ROWS: u32 = 2;

/// PF 2026-10-16（pi-flash 滚屏优化）：行高吸附到**整设备像素**。行高默认
/// 是 1.5 × 字号，非整倍显示缩放下常见小数（如 1.5× 缩放、14px 字号 →
/// 逻辑 21px = 设备 31.5px）；滚轮步进 = N × 行高，行落在小数设备像素上
/// 滚动时整屏亚像素抖动/发虚（Zed editor 的 pixel_snap 同款问题域）。
/// 吸附后所有行的 y 都落在整设备像素。纯函数——request_layout/prepaint/
/// paint 三处必须用同一份，否则同帧内位置不一致。
fn snap_line_height(line_height: Pixels, scale_factor: f32) -> Pixels {
    if scale_factor > 0. {
        px((f32::from(line_height) * scale_factor).round() / scale_factor)
    } else {
        line_height
    }
}

/// pi-flash patch（移植）：zed Disclosure 折叠符号——assets/icons/
/// chevron_right.svg / chevron_down.svg 原样路径（16 视箱、stroke-width
/// 1.2、round 端点），Disclosure 用 IconSize::Small = 14px 渲染（缩放
/// px_size/16），PathBuilder 描边。
fn paint_chevron(
    window: &mut Window,
    origin: Point<Pixels>,
    px_size: f32,
    down: bool,
    color: Hsla,
) {
    let s = px_size / 16.0;
    let pts: [(f32, f32); 3] = if down {
        // chevron_down.svg: M4.15186 6.47321L7.99258 10.1696L11.8483 6.47321
        [(4.15186, 6.47321), (7.99258, 10.1696), (11.8483, 6.47321)]
    } else {
        // chevron_right.svg: M6.44653 4.16071L10.1608 8.00143L6.44653 11.8571
        [(6.44653, 4.16071), (10.1608, 8.00143), (6.44653, 11.8571)]
    };
    let mut b = gpui::PathBuilder::stroke(px(1.2 * s));
    b.move_to(point(origin.x + px(pts[0].0 * s), origin.y + px(pts[0].1 * s)));
    b.line_to(point(origin.x + px(pts[1].0 * s), origin.y + px(pts[1].1 * s)));
    b.line_to(point(origin.x + px(pts[2].0 * s), origin.y + px(pts[2].1 * s)));
    if let Ok(path) = b.build() {
        window.paint_path(path, color);
    }
}

/// pi-flash patch（移植）：zed indent guides 绘制（multi_buffer.rs
/// indent_guides_in_range 走栈算法 + indent_guides.rs fold 过滤 +
/// element.rs column_pixels / paint_indent_guides；calculate_indent_guide_
/// bounds 的 multibuffer 块调整不适用单 buffer，略）。1px 竖线，色取主题
/// border（zed editor_indent_guide 语义位）。
#[allow(clippy::too_many_arguments)]
fn paint_indent_guides(
    state: &InputState,
    last_layout: &LastLayout,
    line_height: Pixels,
    origin: Point<Pixels>,
    top_padding: Pixels,
    window: &mut Window,
    cx: &App,
) {
    if state.decorations.is_some() {
        return;
    }
    // zed LanguageSettings 默认 tab_size = 4；PF TabSize 配置了则取之
    let tab_size = state.mode.tab_size().map_or(4, |t| t.tab_size as u32);
    let max_row = state.text.lines_len().saturating_sub(1) as u32;
    if max_row == 0 {
        return;
    }

    // zed visible_buffer_range：可见显示行 → buffer 行范围
    let first_buf = last_layout.rows.first().map(|r| r.buffer_row).unwrap_or(0);
    let last_buf = last_layout.rows.last().map(|r| r.buffer_row).unwrap_or(0);
    let guides = super::fold_map::indent_guides_in_range(
        &state.text,
        first_buf..(last_buf + 1).min(max_row + 1),
        tab_size,
    );
    let guides = state.indent_guides_filter_folds(guides);
    if guides.is_empty() {
        return;
    }

    // zed column_pixels（editor.rs:12373）：shape " ".repeat(column)
    let font_size = window.text_style().font_size.to_pixels(window.rem_size());
    let single_indent = window
        .text_system()
        .shape_line(
            " ".repeat(tab_size as usize).into(),
            font_size,
            &[TextRun {
                len: tab_size as usize,
                font: window.text_style().font(),
                color: gpui::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        )
        .width;

    // buffer 行 → 显示行（dqjd S2：经显示换算收口——fold 态 = 显示行空间，
    // 软换行 = wrap 行；zed to_display_point 同语义）
    let to_display = |buf_row: u32| -> u32 {
        state
            .wrap_map
            .snapshot()
            .make_wrap_point(FoldSpacePoint::new(buf_row, 0), Bias::Right)
            .row()
    };

    let vis_start = last_layout.visible_range.start as u32;
    let vis_end = last_layout.visible_range.end as u32;
    for g in guides {
        // zed：guide x = 文本起点 + depth × 单级缩进宽（depth 0 贴文本左缘）
        let x = origin.x + last_layout.line_number_width + single_indent * g.depth as f32;
        let start_display = to_display(g.start_row);
        let end_display = to_display(g.end_row);
        // zed calculate_indent_guide_bounds：cons = end_row+1 的显示行（zed
        // to_display_point 把超尾点钳到末行），length = (cons - start) × 行高；
        // cons == end（buffer 尾）时再延一行补末行全长
        let cons_display = to_display((g.end_row + 1).min(max_row));
        let seg_start = start_display.max(vis_start);
        let seg_end = cons_display.max(vis_start).min(vis_end);
        if seg_end <= seg_start {
            continue;
        }
        let length = (seg_end - seg_start) as f32 * line_height
            + if cons_display <= end_display {
                line_height
            } else {
                px(0.)
            };
        let y = origin.y + top_padding + (seg_start - vis_start) as f32 * line_height;
        // zed default_colors.rs:128/282：editor_indent_guide = 前景色极淡
        // 透明版（暗色 dark_alpha step_4 ≈ 8% 白、亮色 ≈ 9% 黑）——PF 主题
        // 无该令牌，border 令牌暗色下与编辑器背景亮度差过小（1px 线不可见，
        // 用户实机暗色主题复现），用 foreground 同 hue 压 alpha 等价换算。
        let mut guide = cx.theme().foreground;
        guide.a = 0.14;
        window.paint_quad(fill(
            Bounds {
                origin: point(x, y),
                size: size(px(1.), length),
            },
            guide,
        ));
    }
}

/// zed element.rs:281 `point_selection.to_display_point(map)`：高亮区间先
/// 换算成**显示空间点（行,列）**再逐行画——zed 按行号范围取行，绝不用字节
/// 坐标逐行试探。dqjd S2：行列 = 显示点（wrap 行 / deco vis 行，经显示换
/// 算收口产出），字节窗夹取改为**行区间与可视行求交**。
pub(crate) struct QuadSpan {
    pub start_row: u32,
    pub start_col: usize,
    pub end_row: u32,
    pub end_col: usize,
}

pub(super) struct TextElement {
    pub(crate) state: Entity<InputState>,
    placeholder: SharedString,
    /// pi-flash 追加：光标色覆盖（None = 主题 `caret`）
    caret_color: Option<Hsla>,
    /// pi-flash 追加：占位符色覆盖（None = 主题 `muted_foreground`）
    placeholder_color: Option<Hsla>,
    /// pi-flash 追加：正文文字色覆盖（None = 主题 `foreground`）
    text_color: Option<Hsla>,
}

impl TextElement {
    pub(super) fn new(state: Entity<InputState>) -> Self {
        Self {
            state,
            placeholder: SharedString::default(),
            caret_color: None,
            placeholder_color: None,
            text_color: None,
        }
    }

    /// Set the placeholder text of the input field.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// pi-flash 追加：光标色覆盖（None = 主题 `caret`）。
    pub fn caret_color(mut self, color: Option<Hsla>) -> Self {
        self.caret_color = color;
        self
    }

    /// pi-flash 追加：占位符色覆盖（None = 主题 `muted_foreground`）。
    /// pi-flash 追加：正文文字色覆盖。
    pub fn text_color(mut self, color: Option<Hsla>) -> Self {
        self.text_color = color;
        self
    }

    pub fn placeholder_color(mut self, color: Option<Hsla>) -> Self {
        self.placeholder_color = color;
        self
    }

    fn paint_mouse_listeners(&mut self, window: &mut Window, _: &mut App) {
        window.on_mouse_event({
            let state = self.state.clone();

            move |event: &MouseMoveEvent, _, window, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    state.update(cx, |state, cx| {
                        state.on_drag_move(event, window, cx);
                    });
                }
            }
        });
    }

    /// Returns the:
    ///
    /// - cursor bounds
    /// - scroll offset
    /// - current row index (No only the visible lines, but all lines)
    ///
    /// This method also will update for track scroll to cursor.
    ///
    /// pi-flash patch（dqjd S2）：显示行恒等高——光标/选区端点经显示换算
    /// 收口，y 闭式解，x 由本行 ShapedLine 查询；旧三分支（fold/等高/软换）
    /// 并为一条路径。
    fn layout_cursor(
        &self,
        last_layout: &LastLayout,
        bounds: &mut Bounds<Pixels>,
        _: &mut Window,
        cx: &mut App,
    ) -> (Option<Bounds<Pixels>>, Point<Pixels>, Option<usize>) {
        let state = self.state.read(cx);

        let line_height = last_layout.line_height;
        let line_number_width = last_layout.line_number_width;

        let mut selected_range = state.selected_range;
        if let Some(ime_marked_range) = &state.ime_marked_range {
            selected_range = (ime_marked_range.end..ime_marked_range.end).into();
        }
        let is_selected_all = selected_range.len() == state.text.len();

        let cursor = state.cursor();
        let mut scroll_offset = state.scroll_handle.offset();
        let mut cursor_bounds = None;

        // If the input has a fixed height (Otherwise is auto-grow), we need to add a bottom margin to the input.
        let top_bottom_margin = if state.mode.is_auto_grow() {
            line_height
        } else if last_layout.visible_range.len() < BOTTOM_MARGIN_ROWS * 8 {
            line_height
        } else {
            BOTTOM_MARGIN_ROWS * line_height
        };

        // The cursor corresponds to the current cursor position in the display space.
        let (cursor_row, cursor_col) =
            state.offset_to_display_point(last_layout, cursor, Bias::Right);
        let (start_row, start_col) =
            state.offset_to_display_point(last_layout, selected_range.start, Bias::Left);
        let (end_row, end_col) =
            state.offset_to_display_point(last_layout, selected_range.end, Bias::Right);
        let current_row = Some(cursor_row as usize);

        let pos_for = |row: u32, col: usize| -> Point<Pixels> {
            let y = row as f32 * line_height;
            let mut x = px(0.);
            if row >= last_layout.visible_range.start as u32 {
                if let Some(line) = last_layout.line(row as usize) {
                    if let Some(p) = line.position_for_index(col, line_height) {
                        x = p.x;
                    }
                }
            }
            point(x, y)
        };

        let cursor_pos = pos_for(cursor_row, cursor_col);
        let cursor_start = pos_for(start_row, start_col);
        let cursor_end = pos_for(end_row, end_col);

        {
            let selection_changed = state.last_selected_range != Some(selected_range);
            if selection_changed && !is_selected_all {
                scroll_offset.x = if scroll_offset.x + cursor_pos.x
                    > (bounds.size.width - line_number_width - RIGHT_MARGIN)
                {
                    // cursor is out of right
                    bounds.size.width - line_number_width - RIGHT_MARGIN - cursor_pos.x
                } else if scroll_offset.x + cursor_pos.x < px(0.) {
                    // cursor is out of left
                    scroll_offset.x - cursor_pos.x
                } else {
                    scroll_offset.x
                };

                // If we change the scroll_offset.y, GPUI will render and trigger the next run loop.
                // So, here we just adjust offset by `line_height` for move smooth.
                scroll_offset.y =
                    if scroll_offset.y + cursor_pos.y > bounds.size.height - top_bottom_margin {
                        // cursor is out of bottom
                        scroll_offset.y - line_height
                    } else if scroll_offset.y + cursor_pos.y < top_bottom_margin {
                        // cursor is out of top
                        (scroll_offset.y + line_height).min(px(0.))
                    } else {
                        scroll_offset.y
                    };

                if state.selection_reversed {
                    if scroll_offset.x + cursor_start.x < px(0.) {
                        // selection start is out of left
                        scroll_offset.x = -cursor_start.x;
                    }
                    if scroll_offset.y + cursor_start.y < px(0.) {
                        // selection start is out of top
                        scroll_offset.y = -cursor_start.y;
                    }
                } else {
                    if scroll_offset.x + cursor_end.x <= px(0.) {
                        // selection end is out of left
                        scroll_offset.x = -cursor_end.x;
                    }
                    if scroll_offset.y + cursor_end.y <= px(0.) {
                        // selection end is out of top
                        scroll_offset.y = -cursor_end.y;
                    }
                }
            }

            // cursor bounds
            let cursor_height = line_height;
            cursor_bounds = Some(Bounds::new(
                point(
                    bounds.left() + cursor_pos.x + line_number_width + scroll_offset.x,
                    bounds.top() + cursor_pos.y + ((line_height - cursor_height) / 2.),
                ),
                size(CURSOR_WIDTH, cursor_height),
            ));
        }

        if let Some(deferred_scroll_offset) = state.deferred_scroll_offset {
            scroll_offset = deferred_scroll_offset;
        }

        bounds.origin = bounds.origin + scroll_offset;

        (cursor_bounds, scroll_offset, current_row)
    }

    /// Layout the match range to per-row rect quads (Zed-style).
    ///
    /// pi-flash patch（dqjd S2）：入参 = 显示空间 QuadSpan；行区间与可视行
    /// 求交后逐行画矩形（显示行等高、单段行——不再有行内软换行拆分）。
    pub(crate) fn layout_match_quads(
        span: &QuadSpan,
        last_layout: &LastLayout,
        bounds: &Bounds<Pixels>,
    ) -> Option<Vec<Bounds<Pixels>>> {
        let line_height = last_layout.line_height;
        let line_number_width = last_layout.line_number_width;
        let vis_start = last_layout.visible_range.start as u32;
        let vis_end = last_layout.visible_range.end as u32;

        if span.end_row < vis_start || span.start_row >= vis_end {
            return None;
        }
        let first = span.start_row.max(vis_start);
        let last = span.end_row.min(vis_end.saturating_sub(1)).max(first);

        let mut line_corners = vec![];
        for wrap_row in first..=last {
            let ix = (wrap_row - vis_start) as usize;
            let Some(line) = last_layout.lines.get(ix) else {
                continue;
            };
            let Some(row_data) = last_layout.rows.get(ix) else {
                continue;
            };
            let line_origin = point(
                px(0.),
                last_layout.visible_top + (wrap_row - vis_start) as f32 * line_height,
            );

            let col_start = if wrap_row == span.start_row {
                span.start_col
            } else {
                0
            };
            let col_end = if wrap_row == span.end_row {
                span.end_col
            } else {
                row_data.text.len()
            };
            let col_start = col_start.min(row_data.text.len());
            let col_end = col_end.min(row_data.text.len());

            let start_x = line
                .position_for_index(col_start, line_height)
                .map(|p| p.x)
                .unwrap_or(px(0.));
            let end_x = line
                .position_for_index(col_end, line_height)
                .map(|p| p.x)
                .unwrap_or_else(|| {
                    line.position_for_index(row_data.text.len(), line_height)
                        .map(|p| p.x)
                        .unwrap_or(px(0.))
                });
            // Ensure at least 6px width for the selection for empty lines.
            let end_x = end_x.max(start_x + px(6.));

            line_corners.push(Corners {
                top_left: line_origin + point(start_x, px(0.)),
                top_right: line_origin + point(end_x, px(0.)),
                bottom_left: line_origin + point(start_x, line_height),
                bottom_right: line_origin + point(end_x, line_height),
            });
        }

        if line_corners.is_empty() {
            return None;
        }

        // Fix corners to make sure the left to right direction
        for corners in &mut line_corners {
            if corners.top_left.x > corners.top_right.x {
                std::mem::swap(&mut corners.top_left, &mut corners.top_right);
                std::mem::swap(&mut corners.bottom_left, &mut corners.bottom_right);
            }
        }

        // v62: Zed 同款逐行矩形（每显示行一个 quad，paint_quad 直绘）——
        // 同步 lyon tessellation 已在 v62 移除，保持逐行 quad。
        let path_origin = bounds.origin + point(line_number_width, px(0.));
        let quads = line_corners
            .iter()
            .map(|corners| {
                Bounds::from_corners(
                    path_origin + corners.top_left,
                    path_origin + corners.bottom_right,
                )
            })
            .collect();

        Some(quads)
    }

    fn layout_search_matches(
        &self,
        last_layout: &LastLayout,
        bounds: &Bounds<Pixels>,
        cx: &mut App,
    ) -> Vec<(Vec<Bounds<Pixels>>, bool)> {
        let search_panel = self.state.read(cx).search_panel.clone();
        let Some((ranges, current_match_ix)) = search_panel.and_then(|panel| {
            if let Some(matcher) = panel.read(cx).matcher() {
                Some((matcher.matched_ranges.clone(), matcher.current_match_ix))
            } else {
                None
            }
        }) else {
            return vec![];
        };

        let mut matches = Vec::new();
        let state = self.state.read(cx);
        for (index, range) in ranges.as_ref().iter().enumerate() {
            let span = Self::quad_span(state, last_layout, range.clone());
            if let Some(quads) = Self::layout_match_quads(&span, last_layout, bounds) {
                matches.push((quads, current_match_ix == index));
            }
        }

        matches
    }

    fn layout_hover_highlight(
        &self,
        last_layout: &LastLayout,
        bounds: &Bounds<Pixels>,
        cx: &mut App,
    ) -> Option<Vec<Bounds<Pixels>>> {
        let hover_popover = self.state.read(cx).hover_popover.clone();
        let Some(symbol_range) = hover_popover.map(|popover| popover.read(cx).symbol_range.clone())
        else {
            return None;
        };

        let span = Self::quad_span(&self.state.read(cx), last_layout, symbol_range);
        Self::layout_match_quads(&span, last_layout, bounds)
    }

    fn layout_document_colors(
        &self,
        document_colors: &[(Range<usize>, Hsla)],
        last_layout: &LastLayout,
        bounds: &Bounds<Pixels>,
        cx: &mut App,
    ) -> Vec<(Vec<Bounds<Pixels>>, Hsla)> {
        let mut colors = vec![];
        let state = self.state.read(cx);
        for (range, color) in document_colors.iter() {
            let span = Self::quad_span(state, last_layout, range.clone());
            if let Some(quads) = Self::layout_match_quads(&span, last_layout, bounds) {
                colors.push((quads, *color));
            }
        }

        colors
    }

    /// buffer 字节区间 → 显示空间 QuadSpan（dqjd S2：经显示换算收口，
    /// Bias::Left/Right = zed anchor before/after 偏向）。
    fn quad_span(state: &InputState, layout: &LastLayout, range: Range<usize>) -> QuadSpan {
        let (start_row, start_col) =
            state.offset_to_display_point(layout, range.start, Bias::Left);
        let (end_row, end_col) = state.offset_to_display_point(layout, range.end, Bias::Right);
        QuadSpan {
            start_row,
            start_col,
            end_row,
            end_col,
        }
    }

    fn layout_selections(
        &self,
        last_layout: &LastLayout,
        bounds: &Bounds<Pixels>,
        cx: &mut App,
    ) -> Vec<Bounds<Pixels>> {
        let state = self.state.read(cx);
        let mut selected_range = state.selected_range;
        if let Some(ime_marked_range) = &state.ime_marked_range {
            if !ime_marked_range.is_empty() {
                selected_range = (ime_marked_range.end..ime_marked_range.end).into();
            }
        }
        if selected_range.is_empty() {
            return vec![];
        }

        let (start_ix, end_ix) = if selected_range.start < selected_range.end {
            (selected_range.start, selected_range.end)
        } else {
            (selected_range.end, selected_range.start)
        };

        let span = Self::quad_span(state, last_layout, start_ix..end_ix);
        Self::layout_match_quads(&span, last_layout, bounds).unwrap_or_default()
    }

    /// Calculate the width of the line numbers
    fn layout_line_numbers(
        state: &InputState,
        text: &Rope,
        font_size: Pixels,
        style: &TextStyle,
        window: &mut Window,
    ) -> (Pixels, usize, Pixels, Pixels) {
        let total_lines = text.lines_len();
        let line_number_len = match total_lines {
            0..=9999 => 5,
            10000..=99999 => 6,
            100000..=999999 => 7,
            _ => 8,
        };

        let (left_padding, right_padding) = if state.mode.line_number() {
            let ch_width = window
                .text_system()
                .shape_line(
                    "0".into(),
                    font_size,
                    &[TextRun {
                        len: 1,
                        font: style.font(),
                        color: gpui::black(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                )
                .width;
            (ch_width, ch_width * 4.)
        } else {
            (px(0.), px(0.))
        };

        let line_number_width = if state.mode.line_number() {
            let digit_width = window
                .text_system()
                .shape_line(
                    "0".repeat(line_number_len).into(),
                    font_size,
                    &[TextRun {
                        len: line_number_len,
                        font: style.font(),
                        color: gpui::black(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                )
                .width;
            digit_width + left_padding + right_padding
        } else {
            px(0.)
        };

        (
            line_number_width,
            line_number_len,
            left_padding,
            right_padding,
        )
    }
}

pub(super) struct PrepaintState {
    /// The lines of entire lines.
    last_layout: LastLayout,
    /// The lines only contains the visible lines in the viewport, based on `visible_range`.
    line_numbers: Option<Vec<SmallVec<[ShapedLine; 1]>>>,
    /// Size of the scrollable area by entire lines.
    scroll_size: Size<Pixels>,
    cursor_bounds: Option<Bounds<Pixels>>,
    cursor_scroll_offset: Point<Pixels>,
    /// row index (zero based), no wrap, same line as the cursor.
    current_row: Option<usize>,
    selection_quads: Vec<Bounds<Pixels>>,
    hover_highlight_quads: Option<Vec<Bounds<Pixels>>>,
    search_match_quads: Vec<(Vec<Bounds<Pixels>>, bool)>,
    document_color_quads: Vec<(Vec<Bounds<Pixels>>, Hsla)>,
    hover_definition_hitbox: Option<Hitbox>,
    bounds: Bounds<Pixels>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let state = self.state.read(cx);
        let line_height = snap_line_height(window.line_height(), window.scale_factor());

        let mut style = Style::default();
        style.size.width = relative(1.).into();
        if state.mode.is_multi_line() {
            style.flex_grow = 1.0;
            style.size.height = relative(1.).into();
            if state.mode.is_auto_grow() {
                // Auto grow to let height match to rows, but not exceed max rows.
                let rows = state.mode.max_rows().min(state.mode.rows());
                style.min_size.height = (rows * line_height).into();
            } else {
                style.min_size.height = line_height.into();
            }
        } else {
            // For single-line inputs, the minimum height should be the line height
            style.size.height = line_height.into();
        };

        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let line_height = snap_line_height(window.line_height(), window.scale_factor());
        let mut bounds = bounds;

        // ---------- 相位一（&state）：行渲染数据 / shaping / LastLayout ----------
        let phase1 = {
        let state = self.state.read(cx);
        let is_empty = state.text.len() == 0;
        let multi_line = state.mode.is_multi_line();

        // ---------- PF-024 装饰 provider（dqjd S3 收编链） ----------
        // provider 每帧纯函数现算（折叠集随选区 reveal）；折叠集与链快照
        // 不一致时重建 fold→tab→wrap 链（deco 折叠 = 空占位符 Transform，
        // 与 tab/软换行自由组合）。styles = **绝对 vis 坐标**（契约 v2）。
        // 摘装饰后残留 deco 折叠也在此清空（快照回无折叠态）。
        let decorations: Option<Rc<Decorations>> = if is_empty {
            None
        } else {
            match state.decorations.as_ref() {
                Some(p) => p
                    .decorate(
                        &state.text,
                        0..state.text.lines_len(),
                        state.selected_range.start..state.selected_range.end,
                    )
                    .map(Rc::new),
                None => None,
            }
        };
        let deco_stale = match decorations.as_ref() {
            Some(dec) => dec.folds != state.fold_snapshot.fold_ranges(),
            // pi-flash patch（dqjd 修复）：无 provider ≠ 可清。code editor
            // 的手动 gutter 折叠是用户态，任何一帧都不许清（原 has_folds()
            // 判定把点箭头折上的块下一帧就抹掉——折叠永远挂不住）；只有
            // **全 deco 空占位符**的残留折叠（md 摘装饰/provider 本帧空手
            // 而归）才回无折叠态。
            None => state.fold_snapshot.has_folds() && state.fold_snapshot.is_deco_folded(),
        };
        drop(state);
        if deco_stale {
            let folds = decorations
                .as_ref()
                .map(|d| d.folds.clone())
                .unwrap_or_default();
            self.state
                .update(cx, |st, scx| st.rebuild_fold_snapshot_from_ranges(folds, scx));
        }
        let state = self.state.read(cx);

        // ---------- 可视区间（显示行恒等高，闭式解；Zed PositionMap 同款） ----------
        let wrap = state.wrap_map.snapshot();
        let total_rows = if is_empty {
            self.placeholder.split('\n').count() as u32
        } else if multi_line {
            wrap.max_point().row() + 1
        } else {
            1
        };
        let scroll_top = if let Some(deferred_scroll_offset) = state.deferred_scroll_offset {
            deferred_scroll_offset.y
        } else {
            state.scroll_handle.offset().y
        };
        let rows_above = ((-scroll_top) / line_height).max(0.) as u32;
        let visible_rows = (bounds.size.height / line_height).ceil().max(0.) as u32;
        let start = rows_above.min(total_rows.saturating_sub(1));
        let end = (start + visible_rows + EXTRA_SHAPE_ROWS + 2).min(total_rows);
        let visible_range = start as usize..end as usize;
        let visible_top = start as f32 * line_height;

        let text_style = window.text_style();
        let base_color = self.text_color.unwrap_or(cx.theme().foreground);
        let document_colors = if is_empty {
            Vec::new()
        } else {
            state
                .lsp
                .document_colors_for_range(&state.text, &visible_range)
        };

        // ---------- 行渲染数据 ----------
        let mut rows: Vec<DisplayRowData> = Vec::with_capacity((end - start) as usize);
        if state.masked && !is_empty {
            // 掩码行（单行密码框）：显示 "*"*chars，换算仍走链（真实文本
            // offset——非 ASCII 掩码的命中近似与旧版一致）
            let mask_text: String = "*".repeat(state.text.chars().count());
            let mask_len = mask_text.len();
            for ix in start as usize..(end as usize).min(1) {
                rows.push(DisplayRowData {
                    wrap_row: ix as u32,
                    tab_row: 0,
                    buffer_row: 0,
                    is_row_start: true,
                    indent: 0,
                    seg_start: 0,
                    seg_end: mask_len as u32,
                    row_start_fold: 0,
                    exp: None,
                    text: mask_text.clone().into(),
                    runs: vec![TextRun {
                        len: mask_len,
                        font: text_style.font(),
                        color: base_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                });
            }
        } else if is_empty {
            // 占位符行（无展开映射、无样式）
            let placeholder_lines: Vec<&str> = self.placeholder.split('\n').collect();
            for (ix, line) in placeholder_lines
                .iter()
                .enumerate()
                .skip(start as usize)
                .take((end - start) as usize)
            {
                rows.push(DisplayRowData {
                    wrap_row: ix as u32,
                    tab_row: ix as u32,
                    buffer_row: ix as u32,
                    is_row_start: true,
                    indent: 0,
                    seg_start: 0,
                    seg_end: 0,
                    row_start_fold: 0,
                    exp: None,
                    text: (*line).to_string().into(),
                    runs: vec![TextRun {
                        len: line.len(),
                        font: text_style.font(),
                        color: self
                            .placeholder_color
                            .unwrap_or(cx.theme().muted_foreground),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                });
            }
        } else {
            // 链行（fold/plain/deco/single-line 统一——deco 折叠 = 链上的空
            // 占位符 Transform，样式源分歧收进 row_source_runs）：fold 行渲染
            // 数据缓存构建（tab 展开 + 样式 runs 每 fold 行一次，段切片共享）
            let deco = decorations.as_ref().map(|d| &**d);
            let mut fold_cache: Option<(u32, Rc<TabExpansion>, Rc<str>, Vec<TextRun>)> = None;
            for wrap_row in start..end {
                let info = wrap.row_info(wrap_row);
                let cache_hit = fold_cache
                    .as_ref()
                    .map(|(r, _, _, _)| *r == info.tab_row)
                    .unwrap_or(false);
                if !cache_hit {
                    let exp = Rc::new(state.tab_snapshot.expand_row(info.tab_row));
                    let expanded_text: Rc<str> = Rc::from(exp.text.as_str());
                    let runs = self.row_source_runs(
                        state,
                        info.tab_row,
                        &document_colors,
                        &text_style,
                        base_color,
                        deco,
                        cx,
                    );
                    fold_cache = Some((info.tab_row, exp, expanded_text, runs));
                }
                let (_, exp, expanded_text, expanded_runs) = fold_cache.as_ref().unwrap();

                let seg_start = (info.seg_start as usize).min(expanded_text.len());
                let seg_end = (info.seg_end as usize).min(expanded_text.len());
                let seg_text: &str = &expanded_text[seg_start..seg_end];
                let indent = info.indent as usize;
                let mut text = String::with_capacity(indent + seg_text.len());
                if indent > 0 {
                    text.push_str(&" ".repeat(indent));
                }
                text.push_str(seg_text);

                // 段 runs：展开行 runs 切片 + 缩进前缀 run（缩进样式取段首）
                let mut runs = Vec::new();
                if indent > 0 {
                    let (font, color) = expanded_runs
                        .first()
                        .map(|r| (r.font.clone(), r.color))
                        .unwrap_or_else(|| (text_style.font(), base_color));
                    runs.push(TextRun {
                        len: indent,
                        font,
                        color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    });
                }
                runs.extend(slice_runs(expanded_runs, seg_start, seg_end));

                let buffer_row = state.fold_snapshot.row_to_buffer_row(info.tab_row);
                let row_start_fold = FoldPoint::new(info.tab_row, 0)
                    .to_offset(&state.fold_snapshot)
                    .0;
                rows.push(DisplayRowData {
                    wrap_row,
                    tab_row: info.tab_row,
                    buffer_row,
                    is_row_start: info.is_row_start,
                    indent: info.indent,
                    seg_start: info.seg_start,
                    seg_end: info.seg_end,
                    row_start_fold,
                    exp: Some(exp.clone()),
                    text: text.into(),
                    runs,
                });
            }
        }

        // ---------- shaping（每显示行一条 ShapedLine） ----------
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let lines: Vec<LineLayout> = rows
            .iter()
            .map(|row| {
                // PF-024 P1：行级字号上浮（deco 标题行）——倍数来自装饰，
                // cap 在行高内。
                let row_font_size = match decorations.as_ref() {
                    Some(dec) => dec
                        .line_scale
                        .iter()
                        .find(|(l, _)| *l == row.buffer_row as usize)
                        .map(|(_, s)| font_size * s.min(1.7))
                        .unwrap_or(font_size),
                    None => font_size,
                };
                let shaped_line = window.text_system().shape_line(
                    row.text.clone(),
                    row_font_size,
                    &row.runs,
                    None,
                );
                LineLayout::new().lines(smallvec::smallvec![shaped_line])
            })
            .collect();

        // ---------- 行号宽 ----------
        let display_text_for_gutter = if is_empty {
            Rc::new(Rope::from(self.placeholder.as_str()))
        } else {
            Rc::new(state.text.clone())
        };
        let (line_number_width, line_number_len, gutter_left_padding, gutter_right_padding) =
            Self::layout_line_numbers(
                &state,
                &display_text_for_gutter,
                font_size,
                &text_style,
                window,
            );

        let wrap_width = if multi_line && state.soft_wrap {
            Some(bounds.size.width - line_number_width - RIGHT_MARGIN)
        } else {
            None
        };

        // ---------- 横向内容宽（无软换行 = 最长行实宽） ----------
        let mut longest_line_width = wrap_width.unwrap_or(px(0.));
        if wrap_width.is_none() && multi_line && !is_empty && decorations.is_none() && !rows.is_empty()
        {
            // dqjd S2：最长行 = wrap 摘要 longest_row（字符口径；zed
            // TransformSummary 同源），展开文本实 shape。
            let longest_row = wrap.longest_row();
            let longest_text = state.tab_snapshot.expand_row(longest_row).text;
            let longest_len = longest_text.len();
            longest_line_width = window
                .text_system()
                .shape_line(
                    longest_text.into(),
                    font_size,
                    &[TextRun {
                        len: longest_len,
                        font: text_style.font(),
                        color: gpui::black(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                )
                .width;
        }
        if wrap_width.is_none() && multi_line && !is_empty {
            if let Some(dec) = decorations.as_ref() {
                // PF-024：横向滚动宽度按折叠后最长行（doc 最长行可能已折叠）
                let longest_row = wrap.longest_row().min(dec.display.lines_len() as u32);
                let longest_line: SharedString =
                    dec.display.slice_line(longest_row as usize).to_string().into();
                longest_line_width = window
                    .text_system()
                    .shape_line(
                        longest_line.clone(),
                        font_size,
                        &[TextRun {
                            len: longest_line.len(),
                            font: text_style.font(),
                            color: gpui::black(),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        None,
                    )
                    .width;
            }
        }

        // ---------- 可见源字节窗（观测/兼容字段） ----------
        let visible_range_offset = if is_empty || rows.is_empty() {
            0..0
        } else if decorations.is_some() {
            let first = rows.first().unwrap().buffer_row as usize;
            let last = rows.last().unwrap().buffer_row as usize;
            state.text.line_start_offset(first)..state.text.line_end_offset(last)
        } else {
            let first = rows.first().unwrap();
            let last = rows.last().unwrap();
            let last_len = state.fold_snapshot.row_text(last.tab_row).len();
            first.row_start_fold..last.row_start_fold + last_len
        };

        let mut last_layout = LastLayout {
            visible_range: visible_range.clone(),
            visible_top,
            visible_range_offset,
            line_height,
            wrap_width,
            line_number_width,
            gutter_left_padding,
            gutter_right_padding,
            tab_size: state.mode.tab_size().map_or(2, |t| t.tab_size as u32),
            rows: Rc::new(rows),
            fold_mode: decorations.is_none(),
            lines: Rc::new(lines),
            cursor_bounds: None,
            decorations: decorations.clone(),
        };

        // 相位一返回（行号在相位二构建——需要 current_row）
        (
            last_layout,
            total_rows,
            font_size,
            text_style,
            line_number_len,
            line_number_width,
            longest_line_width,
            document_colors,
        )
        };

        let (mut last_layout, total_rows, font_size, text_style, line_number_len, line_number_width, longest_line_width, document_colors) = phase1;

        // ---------- 相位二（&mut cx）：光标 / 命中 / 行号 ----------
        let (cursor_bounds, cursor_scroll_offset, current_row) =
            self.layout_cursor(&last_layout, &mut bounds, window, cx);
        last_layout.cursor_bounds = cursor_bounds;

        let search_match_quads = self.layout_search_matches(&last_layout, &bounds, cx);
        let selection_quads = self.layout_selections(&last_layout, &bounds, cx);
        let hover_highlight_quads = self.layout_hover_highlight(&last_layout, &bounds, cx);
        let document_color_quads =
            self.layout_document_colors(&document_colors, &last_layout, &bounds, cx);

        // ---------- 行号 ----------
        let line_numbers = if self.state.read(cx).mode.line_number() {
            let mut line_numbers = vec![];
            let other_line_runs = vec![TextRun {
                len: line_number_len,
                font: text_style.font(),
                color: cx.theme().muted_foreground,
                background_color: None,
                underline: None,
                strikethrough: None,
            }];
            let current_line_runs = vec![TextRun {
                len: line_number_len,
                font: text_style.font(),
                color: cx.theme().foreground,
                background_color: None,
                underline: None,
                strikethrough: None,
            }];

            // build line numbers（dqjd S2：行号 = 缓冲行号，行首段显示、
            // 续行空位）
            for (ix, row) in last_layout.rows.iter().enumerate() {
                let line_no: SharedString = if row.is_row_start {
                    format!("{:>width$}", row.buffer_row + 1, width = line_number_len).into()
                } else {
                    " ".repeat(line_number_len).into()
                };

                let runs = if current_row == Some(ix + last_layout.visible_range.start) {
                    &current_line_runs
                } else {
                    &other_line_runs
                };

                let mut sub_lines: SmallVec<[ShapedLine; 1]> = SmallVec::new();
                sub_lines.push(
                    window
                        .text_system()
                        .shape_line(line_no, font_size, &runs, None),
                );
                line_numbers.push(sub_lines);
            }
            Some(line_numbers)
        } else {
            None
        };

        let hover_definition_hitbox =
            self.layout_hover_definition_hitbox(&self.state.read(cx), window, cx);

        // ---------- 滚动内容尺寸 ----------
        let empty_bottom_height = bounds
            .size
            .height
            .half()
            .max(BOTTOM_MARGIN_ROWS * line_height);
        let scroll_size = size(
            if longest_line_width + line_number_width + RIGHT_MARGIN > bounds.size.width {
                longest_line_width + line_number_width + RIGHT_MARGIN
            } else {
                longest_line_width
            },
            (total_rows as f32 * line_height + empty_bottom_height).max(bounds.size.height),
        );

        PrepaintState {
            bounds,
            last_layout,
            scroll_size,
            line_numbers,
            cursor_bounds,
            cursor_scroll_offset,
            current_row,
            selection_quads,
            search_match_quads,
            hover_highlight_quads,
            hover_definition_hitbox,
            document_color_quads,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        input_bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.state.read(cx).focus_handle.clone();
        let show_cursor = self.state.read(cx).show_cursor(window, cx);
        let focused = focus_handle.is_focused(window);
        let bounds = prepaint.bounds;
        let selected_range = self.state.read(cx).selected_range;
        let visible_range = &prepaint.last_layout.visible_range;

        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.state.clone()),
            cx,
        );

        // Set Root focused_input when self is focused
        if focused {
            let state = self.state.clone();
            if Root::read(window, cx).focused_input.as_ref() != Some(&state) {
                Root::update(window, cx, |root, _, cx| {
                    root.focused_input = Some(state);
                    cx.notify();
                });
            }
        }

        // And reset focused_input when next_frame start
        window.on_next_frame({
            let state = self.state.clone();
            move |window, cx| {
                if !focused && Root::read(window, cx).focused_input.as_ref() == Some(&state) {
                    Root::update(window, cx, |root, _, cx| {
                        root.focused_input = None;
                        cx.notify();
                    });
                }
            }
        });

        // Paint multi line text
        let line_height = snap_line_height(window.line_height(), window.scale_factor());
        let origin = bounds.origin;

        let invisible_top_padding = prepaint.last_layout.visible_top;

        let mut mask_offset_y = px(0.);
        if self.state.read(cx).masked {
            // Move down offset for vertical centering the *****
            if cfg!(target_os = "macos") {
                mask_offset_y = px(3.);
            } else {
                mask_offset_y = px(2.5);
            }
        }

        let active_line_color = cx.theme().highlight_theme.style.editor_active_line;

        // pi-flash patch（移植）：indent guides（zed paint_indent_guides，
        // 顺序同 zed：背景之后、行号/文本之前——最底层）
        {
            let st = self.state.read(cx);
            paint_indent_guides(
                &st,
                &prepaint.last_layout,
                line_height,
                origin,
                invisible_top_padding,
                window,
                cx,
            );
        }

        // Paint active line
        let mut offset_y = px(0.);
        if let Some(line_numbers) = prepaint.line_numbers.as_ref() {
            offset_y += invisible_top_padding;

            // Each item is the normal lines.
            for (ix, _) in line_numbers.iter().enumerate() {
                let row = visible_range.start + ix;
                let is_active = prepaint.current_row == Some(row);
                let p = point(input_bounds.origin.x, origin.y + offset_y);
                let height = line_height;
                // Paint the current line background
                if is_active {
                    if let Some(bg_color) = active_line_color {
                        window.paint_quad(fill(
                            Bounds::new(p, size(bounds.size.width, height)),
                            bg_color,
                        ));
                    }
                }
                offset_y += height;
            }
        }

        // Paint selections
        if window.is_window_active() {
            let secondary_selection = cx.theme().selection.saturation(0.1);
            for (quads, is_active) in prepaint.search_match_quads.iter() {
                let color = if *is_active {
                    cx.theme().selection
                } else {
                    secondary_selection
                };
                for quad in quads {
                    window.paint_quad(fill(*quad, color));
                }
            }

            for quad in &prepaint.selection_quads {
                window.paint_quad(fill(*quad, cx.theme().selection));
            }

            // Paint hover highlight
            for quad in prepaint.hover_highlight_quads.iter().flatten() {
                window.paint_quad(fill(*quad, secondary_selection));
            }
        }

        // Paint document colors
        for (quads, color) in prepaint.document_color_quads.iter() {
            for quad in quads {
                window.paint_quad(fill(*quad, *color));
            }
        }

        // Paint text
        let mut offset_y = mask_offset_y + invisible_top_padding;
        for line in prepaint.last_layout.lines.iter() {
            let p = point(
                origin.x + prepaint.last_layout.line_number_width,
                origin.y + offset_y,
            );
            _ = line.paint(p, line_height, window, cx);
            offset_y += line.size(line_height).height;
        }

        // Paint blinking cursor
        if focused && show_cursor {
            if let Some(mut cursor_bounds) = prepaint.cursor_bounds.take() {
                cursor_bounds.origin.y += prepaint.cursor_scroll_offset.y;
                window.paint_quad(fill(
                    cursor_bounds,
                    self.caret_color.unwrap_or(cx.theme().caret),
                ));
            }
        }

        // Paint line numbers
        let mut offset_y = px(0.);
        if let Some(line_numbers) = prepaint.line_numbers.as_ref() {
            offset_y += invisible_top_padding;

            // Paint line number background（pi-flash patch（移植）：zed
            // gutter 背景 = editor 背景全宽，含折叠符号带）
            window.paint_quad(fill(
                Bounds {
                    origin: input_bounds.origin,
                    size: size(prepaint.last_layout.line_number_width, input_bounds.size.height),
                },
                cx.theme().editor_background(),
            ));

            // Each item is the normal lines.
            for (ix, lines) in line_numbers.iter().enumerate() {

                // pi-flash patch（移植）：行号右对齐于折叠符号带左侧
                // （zed 行号 justify_end；统一位数时 = origin + left_padding）
                let p = point(
                    input_bounds.origin.x + prepaint.last_layout.gutter_left_padding,
                    origin.y + offset_y,
                );
                let is_active = prepaint.current_row == Some(ix + visible_range.start);

                // paint active line number background
                if is_active {
                    if let Some(bg_color) = active_line_color {
                        window.paint_quad(fill(
                            Bounds::new(
                                p,
                                size(prepaint.last_layout.line_number_width, line_height),
                            ),
                            bg_color,
                        ));
                    }
                }

                for line in lines {
                    _ = line.paint(p, line_height, window, cx);
                    offset_y += line_height;
                }
            }

            // pi-flash patch（移植）：gutter 折叠符号（zed render_crease_
            // toggle / prepaint_crease_toggles）——符号带起点 = width -
            // right_padding，符号在带内水平居中、行内垂直居中；显示条件 =
            // 折叠块首恒显，可折叠行悬停 gutter 或光标行显（render_crease_
            // toggle 同式）；悬停行带 secondary 药丸底（IconButton hover
            // 态）；色 = 折叠态 Color::Selected(text_accent) / 展开态
            // Color::Muted（IconButton render 分支）。点击命中带见 state 侧
            // gutter_arrow_hit（同符号带）。
            if self.state.read(cx).mode.is_code_editor() && prepaint.last_layout.fold_mode {
                let muted = cx.theme().muted_foreground;
                let accent = cx.theme().accent;
                let pill_bg = cx.theme().secondary;
                let icon_px = 14.0_f32;
                let gutter_w = f32::from(prepaint.last_layout.line_number_width);
                let right_padding = f32::from(prepaint.last_layout.gutter_right_padding);
                let fold_area_x = input_bounds.origin.x + px(gutter_w - right_padding);
                let fold_area_w = px(right_padding);
                let rows_info: Vec<(bool, bool, bool)> = {
                    let fs = self.state.read(cx);
                    let gutter_hovered = fs.gutter_hover_row.is_some();
                    let mut v = Vec::new();
                    for row in prepaint.last_layout.rows.iter() {
                        if !row.is_row_start {
                            // 续行无符号（行首段才挂折叠符号）
                            v.push((false, false, false));
                            continue;
                        }
                        let buf_row = row.buffer_row as u32;
                        let folded = fs.fold_starts_at_row(buf_row);
                        let foldable = folded || fs.text.starts_indent(buf_row);
                        let show = folded
                            || (foldable
                                && (gutter_hovered
                                    || prepaint.current_row == Some(row.wrap_row as usize)));
                        let hovered = fs.gutter_hover_row == Some(buf_row as usize);
                        v.push((folded, show, hovered));
                    }
                    v
                };
                let mut arrow_y = invisible_top_padding;
                for (folded, show, hovered) in rows_info {
                    if show {
                        // zed prepaint_crease_toggles：centering = (fold_area
                        // - toggle) / 2, (line_height - toggle) / 2
                        let icon_origin = point(
                            fold_area_x + (fold_area_w - px(icon_px)) / 2.,
                            origin.y + arrow_y + ((line_height - px(icon_px)) / 2.).max(px(0.)),
                        );
                        // zed Disclosure = IconButton（style 默认 Subtle）：hover
                        // 底是**图标外一圈的紧凑方钮**（icon + 各向 ~4px 内边距，
                        // rounded sm），不是整个 fold 带宽——整带宽 = right_
                        // padding（4ch），字号一大就是贴住行号/正文的长条（用
                        // 户实机截图）；折叠态 chevron = Color::Selected、展开
                        // = Muted（IconButton render 分支）。
                        if hovered {
                            let btn_pad = px(4.0);
                            let btn_w = px(icon_px) + btn_pad * 2.;
                            let btn_h = (px(icon_px) + btn_pad * 2.).min(line_height);
                            window.paint_quad(
                                fill(
                                    Bounds::new(
                                        point(
                                            fold_area_x + (fold_area_w - btn_w) / 2.,
                                            origin.y
                                                + arrow_y
                                                + ((line_height - btn_h) / 2.).max(px(0.)),
                                        ),
                                        size(btn_w, btn_h),
                                    ),
                                    pill_bg,
                                )
                                .corner_radii(px(4.0)),
                            );
                        }
                        // zed Disclosure：is_open = !folded（折叠态显 ChevronRight ˃）
                        let icon_color = if folded { accent } else { muted };
                        paint_chevron(window, icon_origin, icon_px, !folded, icon_color);
                    }
                    arrow_y += line_height;
                }
            }
        }

        self.state.update(cx, |state, cx| {
            let next_cursor = state.cursor();
            // v62: paint 尾原先无条件 notify——draw 期间的 notify 不调度新帧
            // （window.rs invalidate_view 的 DrawPhase::None 分支才置 dirty），
            // 但会把本视图写进 dirty 集合，于是「任何原因触发的帧」（光标
            // 闪烁/流式动画/终端输出）都要重建编辑器并沿祖先链连坐整个宿主
            // 视图（pi-flash 的 Chat 全量重渲染）。稳态（布局 bounds/光标/
            // 选区/滚动/内容尺寸均未变）跳过；真实变化路径（replace_text_
            // in_range、move_to、set_font、set_input_bounds 折行宽变化、
            // blink 定时器）都自带 notify，不依赖此处。
            let changed = state.last_bounds != Some(bounds)
                || state.last_cursor != Some(next_cursor)
                || state.last_selected_range != Some(selected_range)
                || state.scroll_size != prepaint.scroll_size
                || state.input_bounds != input_bounds
                || state.scroll_handle.offset() != prepaint.cursor_scroll_offset;
            state.last_layout = Some(prepaint.last_layout.clone());
            state.last_bounds = Some(bounds);
            state.last_cursor = Some(next_cursor);
            state.set_input_bounds(input_bounds, cx);
            state.last_selected_range = Some(selected_range);
            state.scroll_size = prepaint.scroll_size;
            state
                .scroll_handle
                .set_offset(prepaint.cursor_scroll_offset);
            state.deferred_scroll_offset = None;

            if changed {
                cx.notify();
            }
        });

        if let Some(hitbox) = prepaint.hover_definition_hitbox.as_ref() {
            window.set_cursor_style(gpui::CursorStyle::PointingHand, &hitbox);
        }

        self.paint_mouse_listeners(window, cx);
    }
}

impl TextElement {
    /// 一条结构行（tab/fold 行）的**展开空间**样式 runs：fold 行走
    /// row_segments（buffer 段 ts 高亮 + 占位符灰显），普通行走 ts 高亮 +
    /// diagnostics + document colors + IME（dqjd S2 自旧全局 runs / 折行
    /// runs 两路收编为每行直查——「从 doc 0 连续平铺」filler 不变式随之消灭）。
    #[allow(clippy::too_many_arguments)]
    fn row_source_runs(
        &self,
        state: &InputState,
        tab_row: u32,
        document_colors: &[(Range<usize>, Hsla)],
        text_style: &TextStyle,
        base_color: Hsla,
        deco: Option<&Decorations>,
        cx: &App,
    ) -> Vec<TextRun> {
        let has_folds = state.fold_snapshot.has_folds();
        let row_text = state.fold_snapshot.row_text(tab_row);
        let row_len = row_text.len();
        let base_font = text_style.font();
        let mut src_runs: Vec<TextRun> = Vec::new();

        if let Some(dec) = deco {
            // dqjd S3：deco 行——dec.styles = **绝对 vis 坐标**（契约 v2），
            // 按本行 vis 区间切片（vis = fold 空间：行首 fold offset + 行长）。
            // 空占位符段零宽，无需灰显；IME 区间 doc→vis 后同法相交。
            let row_start_vis = FoldPoint::new(tab_row, 0)
                .to_offset(&state.fold_snapshot)
                .0;
            let styles: Vec<(Range<usize>, HighlightStyle)> = dec
                .styles
                .iter()
                .filter_map(|(r, st)| {
                    let a = r.start.saturating_sub(row_start_vis).min(row_len);
                    let b = r.end.saturating_sub(row_start_vis).min(row_len);
                    (b > a).then_some((a..b, *st))
                })
                .collect();
            let partitioned = style_partition(styles, row_len);
            let mut line_runs: Vec<TextRun> = Vec::new();
            let mut cursor = 0usize;
            for (r, st) in &partitioned {
                if r.start > cursor {
                    line_runs.push(TextRun {
                        len: r.start - cursor,
                        font: base_font.clone(),
                        color: base_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    });
                }
                line_runs.push(text_style.clone().highlight(*st).to_run(r.len()));
                cursor = r.end;
            }
            if cursor < row_len {
                line_runs.push(TextRun {
                    len: row_len - cursor,
                    font: base_font.clone(),
                    color: base_color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                });
            }
            // IME 组串下划线（doc→vis 映射后按行 vis 区间相交）
            let ime_vis = state.ime_marked_range.as_ref().map(|ime| {
                dec.doc_to_vis(ime.start)..dec.doc_to_vis(ime.end)
            });
            src_runs = apply_ime_marking(line_runs, row_start_vis..row_start_vis + row_len, ime_vis);
        } else if has_folds {
            // fold 行：buffer 段走 ts 高亮，占位符段灰显（zed fold chunk
            // placeholder 语义；行号 = 绝对 fold 行）
            let segs = state.fold_snapshot.row_segments(tab_row);
            let mut styled: Vec<(Range<usize>, HighlightStyle)> = vec![];
            let highlighter = match &state.mode {
                InputMode::CodeEditor {
                    highlighter,
                    diagnostics: _,
                    ..
                } => Some(highlighter.borrow()),
                _ => None,
            };
            for (local, buf) in &segs {
                match buf {
                    Some(buf) => {
                        let seg_styles = highlighter
                            .as_ref()
                            .and_then(|hl| hl.as_ref())
                            .map(|hl| {
                                hl.styles(buf, &cx.theme().highlight_theme)
                                    .into_iter()
                                    .filter_map(|(r, st)| {
                                        let a = local.start + r.start.saturating_sub(buf.start);
                                        let b = local.start + r.end.saturating_sub(buf.start);
                                        (b > a).then_some((a..b.min(local.end), st))
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_else(|| vec![(local.clone(), HighlightStyle::default())]);
                        let mut seg_styles = seg_styles;
                        seg_styles.sort_by_key(|(r, _)| r.start);
                        let mut cursor = local.start;
                        for (r, st) in seg_styles {
                            if r.start > cursor {
                                styled.push((cursor..r.start, HighlightStyle::default()));
                            }
                            styled.push((r.clone(), st));
                            cursor = cursor.max(r.end);
                        }
                        if cursor < local.end {
                            styled.push((cursor..local.end, HighlightStyle::default()));
                        }
                    }
                    None => {
                        styled.push((local.clone(), HighlightStyle::default()));
                    }
                }
            }
            styled.sort_by_key(|(r, _)| r.start);
            let mut merged: Vec<(Range<usize>, HighlightStyle)> = vec![];
            for (r, st) in styled {
                let r = r.start..r.end.min(row_len);
                if r.end <= r.start {
                    continue;
                }
                if let Some(last) = merged.last_mut() {
                    if last.0.end == r.start && last.1 == st {
                        last.0.end = r.end;
                        continue;
                    }
                }
                merged.push((r, st));
            }
            let muted = cx.theme().muted_foreground;
            let mut cursor = 0usize;
            for (r, st) in &merged {
                if r.start > cursor {
                    src_runs.push(TextRun {
                        len: r.start - cursor,
                        font: base_font.clone(),
                        color: base_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    });
                }
                let mut run = text_style.clone().highlight(*st).to_run(r.len());
                // 占位符段（buffer=None 的 local 区间）灰显
                if segs
                    .iter()
                    .any(|(l, b)| b.is_none() && l.start <= r.start && l.end >= r.end)
                {
                    run.color = muted;
                }
                src_runs.push(run);
                cursor = r.end;
            }
            if cursor < row_len {
                src_runs.push(TextRun {
                    len: row_len - cursor,
                    font: base_font.clone(),
                    color: base_color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                });
            }
        } else {
            // 普通行：ts 高亮 + diagnostics + document colors（bg）+ IME
            let row_start = state.text.line_start_offset(tab_row as usize);
            let row_end = row_start + state.text.line_len(tab_row as usize) + 1;
            let mut styles: Vec<(Range<usize>, HighlightStyle)> = vec![];
            if let InputMode::CodeEditor { highlighter, .. } = &state.mode {
                if let Some(hl) = highlighter.borrow().as_ref() {
                    styles.extend(hl.styles(&(row_start..row_end), &cx.theme().highlight_theme));
                }
            }
            if let Some(set) = state.mode.diagnostics() {
                let ds = set.styles_for_range(&(row_start..row_end), cx);
                styles = gpui::combine_highlights(ds, styles).collect();
            }
            // hover definition style（lsp/definitions.rs 的 TextElement 方法）
            if let Some((hr, hover_style)) = self.layout_hover_definition(cx) {
                if hr.start < row_end && hr.end > row_start {
                    let a = hr.start.max(row_start) - row_start;
                    let b = hr.end.min(row_end) - row_start;
                    styles.push((a..b, hover_style));
                }
            }
            styles.sort_by_key(|(r, _)| r.start);
            // 行内 local 坐标 + 全覆盖分区
            let local_styles: Vec<(Range<usize>, HighlightStyle)> = styles
                .into_iter()
                .filter_map(|(r, st)| {
                    let a = r.start.saturating_sub(row_start);
                    let b = r.end.saturating_sub(row_start).min(row_len);
                    (b > a).then_some((a..b, st))
                })
                .collect();
            let partitioned = style_partition(local_styles, row_len);
            let mut line_runs: Vec<TextRun> = Vec::new();
            let mut cursor = 0usize;
            for (r, st) in &partitioned {
                if r.start > cursor {
                    line_runs.push(TextRun {
                        len: r.start - cursor,
                        font: base_font.clone(),
                        color: base_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    });
                }
                line_runs.push(text_style.clone().highlight(*st).to_run(r.len()));
                cursor = r.end;
            }
            if cursor < row_len {
                line_runs.push(TextRun {
                    len: row_len - cursor,
                    font: base_font.clone(),
                    color: base_color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                });
            }
            // document colors：bg 段拆分（绝对字节游标语义保留）
            let bg: Vec<(Range<usize>, Hsla)> = document_colors
                .iter()
                .filter(|(r, _)| r.start < row_end && r.end > row_start)
                .cloned()
                .collect();
            let line_runs = if bg.is_empty() {
                line_runs
            } else {
                split_runs_by_bg_segments(row_start, &line_runs, &bg)
            };
            // IME 组串下划线（行内相交区间）
            let ime = state.ime_marked_range.clone().map(|r| r.start..r.end);
            src_runs = apply_ime_marking(line_runs, row_start..row_end, ime);
        }

        // tab 展开层：runs 重排到展开坐标（shape 输入同坐标系）
        let exp = state.tab_snapshot.expand_row(tab_row);
        let (fb_font, fb_color) = src_runs
            .first()
            .map(|r| (r.font.clone(), r.color))
            .unwrap_or_else(|| (base_font.clone(), base_color));
        super::tab_map::rerun_expanded(&src_runs, &exp, fb_font, fb_color)
    }
}

/// Get the runs for the given range.
///
/// The range is the byte range of the wrapped line.
pub(super) fn runs_for_range(
    runs: &[TextRun],
    line_offset: usize,
    range: &Range<usize>,
) -> Vec<TextRun> {
    let mut result = vec![];
    let range = (line_offset + range.start)..(line_offset + range.end);
    let mut cursor = 0;

    for run in runs {
        let run_start = cursor;
        let run_end = cursor + run.len;

        if run_end <= range.start {
            cursor = run_end;
            continue;
        }

        if run_start >= range.end {
            break;
        }

        let start = range.start.max(run_start) - run_start;
        let end = range.end.min(run_end) - run_start;
        let len = end - start;

        if len > 0 {
            result.push(TextRun { len, ..run.clone() });
        }

        cursor = run_end;
    }

    result
}

/// dqjd S2：runs 字节区间切片（展开空间行内段切分，重基到 0）。
fn slice_runs(runs: &[TextRun], start: usize, end: usize) -> Vec<TextRun> {
    runs_for_range(runs, 0, &(start..end))
}

/// dqjd S2：全覆盖分区（样式区间 → 按序全覆盖，间隙填默认）。
fn style_partition(
    styles: Vec<(Range<usize>, HighlightStyle)>,
    total: usize,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for (r, st) in styles {
        let r = r.start..r.end.min(total);
        if r.end <= r.start {
            continue;
        }
        if r.start > cursor {
            out.push((cursor..r.start, HighlightStyle::default()));
        }
        let extend = match out.last_mut() {
            Some(last) if last.0.end == r.start && last.1 == st => {
                last.0.end = r.end;
                false
            }
            _ => true,
        };
        let end = r.end;
        if extend {
            out.push((r, st));
        }
        cursor = cursor.max(end);
    }
    if cursor < total {
        out.push((cursor..total, HighlightStyle::default()));
    }
    out
}

/// dqjd S2：IME 组选区下划线——行 runs 与 ime 区间相交段拆分加标记。
/// `ime` 与 `row_range` 同坐标系（普通行 = doc；deco 行 = vis）。
fn apply_ime_marking(
    mut runs: Vec<TextRun>,
    row_range: Range<usize>,
    ime: Option<Range<usize>>,
) -> Vec<TextRun> {
    let Some(ime) = ime else {
        return runs;
    };
    if ime.end <= row_range.start || ime.start >= row_range.end {
        return runs;
    }
    let a = ime.start.max(row_range.start) - row_range.start;
    let b = ime.end.min(row_range.end) - row_range.start;
    let mut out = Vec::with_capacity(runs.len() + 2);
    let mut cursor = 0usize;
    for run in runs.drain(..) {
        let run_start = cursor;
        let run_end = cursor + run.len;
        cursor = run_end;
        if run_end <= a || run_start >= b {
            out.push(run);
            continue;
        }
        // 拆 [run_start..run_end] 与 [a..b] 的差/交
        if a > run_start {
            out.push(TextRun {
                len: a - run_start,
                ..run.clone()
            });
        }
        let mark_start = a.max(run_start);
        let mark_end = b.min(run_end);
        if mark_end > mark_start {
            out.push(TextRun {
                len: mark_end - mark_start,
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(run.color),
                    wavy: false,
                }),
                ..run.clone()
            });
        }
        if run_end > b {
            out.push(TextRun {
                len: run_end - b,
                ..run
            });
        }
    }
    out
}

fn split_runs_by_bg_segments(
    start_offset: usize,
    runs: &[TextRun],
    bg_segments: &[(Range<usize>, Hsla)],
) -> Vec<TextRun> {
    let mut result = vec![];

    let mut cursor = start_offset;
    for run in runs {
        let mut run_start = cursor;
        let run_end = cursor + run.len;

        for (bg_range, bg_color) in bg_segments {
            if run_end <= bg_range.start || run_start >= bg_range.end {
                continue;
            }

            // Overlap exists
            if run_start < bg_range.start {
                // Add the part before the background range
                result.push(TextRun {
                    len: bg_range.start - run_start,
                    ..run.clone()
                });
            }

            // Add the overlapping part with background color
            let overlap_start = run_start.max(bg_range.start);
            let overlap_end = run_end.min(bg_range.end);
            let text_color = if bg_color.l >= 0.5 {
                gpui::black()
            } else {
                gpui::white()
            };

            let run_len = overlap_end.saturating_sub(overlap_start);
            if run_len > 0 {
                result.push(TextRun {
                    len: run_len,
                    color: text_color,
                    ..run.clone()
                });

                cursor = bg_range.end;
                run_start = cursor;
            }
        }

        if run_end > cursor {
            // Add the part after the background range
            result.push(TextRun {
                len: run_end - cursor,
                ..run.clone()
            });
        }

        cursor = run_end;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runs_for_range() {
        let run = TextRun {
            len: 0,
            font: gpui::font(".SystemUIFont"),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        // use hello this-is-test
        let runs = vec![
            // use
            TextRun {
                len: 3,
                ..run.clone()
            },
            // \s
            TextRun {
                len: 1,
                ..run.clone()
            },
            // hello
            TextRun {
                len: 5,
                ..run.clone()
            },
            // \s
            TextRun {
                len: 1,
                ..run.clone()
            },
            // this-is-test
            TextRun {
                len: 12,
                ..run.clone()
            },
        ];

        #[track_caller]
        fn assert_runs(actual: Vec<TextRun>, expected: &[usize]) {
            let left = actual.iter().map(|run| run.len).collect::<Vec<_>>();
            assert_eq!(left, expected);
        }

        assert_runs(runs_for_range(&runs, 0, &(0..0)), &[]);
        assert_runs(runs_for_range(&runs, 0, &(0..100)), &[3, 1, 5, 1, 12]);

        assert_runs(runs_for_range(&runs, 0, &(0..6)), &[3, 1, 2]);
        assert_runs(runs_for_range(&runs, 0, &(1..6)), &[2, 1, 2]);
        assert_runs(runs_for_range(&runs, 0, &(3..10)), &[1, 5, 1]);
        assert_runs(runs_for_range(&runs, 0, &(5..8)), &[3]);
        assert_runs(runs_for_range(&runs, 3, &(0..3)), &[1, 2]);
        assert_runs(runs_for_range(&runs, 3, &(2..10)), &[4, 1, 3]);
        assert_runs(runs_for_range(&runs, 9, &(0..8)), &[1, 7]);
    }

    #[test]
    fn test_slice_runs() {
        let run = TextRun {
            len: 0,
            font: gpui::font(".SystemUIFont"),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = vec![
            TextRun {
                len: 5,
                ..run.clone()
            },
            TextRun {
                len: 7,
                ..run.clone()
            },
        ];
        let sliced = slice_runs(&runs, 3, 9);
        assert_eq!(sliced.iter().map(|r| r.len).collect::<Vec<_>>(), vec![2, 4]);
    }

    #[test]
    fn test_split_runs_by_bg_segments() {
        let run = TextRun {
            len: 0,
            font: gpui::font(".SystemUIFont"),
            color: gpui::blue(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let runs = vec![
            TextRun {
                len: 5,
                ..run.clone()
            },
            TextRun {
                len: 7,
                ..run.clone()
            },
            TextRun {
                len: 24,
                ..run.clone()
            },
        ];

        let bg_segments = vec![(8..12, gpui::red()), (12..18, gpui::blue())];
        let result = split_runs_by_bg_segments(5, &runs, &bg_segments);
        assert_eq!(
            result.iter().map(|run| run.len).collect::<Vec<_>>(),
            vec![3, 2, 2, 5, 1, 23]
        );
        assert_eq!(result[0].color, gpui::blue());
        assert_eq!(result[1].color, gpui::black());
        assert_eq!(result[2].color, gpui::black());
        assert_eq!(result[3].color, gpui::black());
        assert_eq!(result[4].color, gpui::black());
        assert_eq!(result[5].color, gpui::blue());
    }
}
