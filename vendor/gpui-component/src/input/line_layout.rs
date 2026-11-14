//! LineLayout——一行（显示行）的塑形结果容器（多 ShapedLine 段）。
//!
//! pi-flash patch（dqjd S2）：自 text_wrapper.rs 抽出独立——TextWrapper
//! （doc 行 + 软换行段表）已被 wrap_map.rs 的 SumTree Transform 链取代，
//! 本模块只保留 shape 输出的行容器（每显示行一条）。

use gpui::{point, px, size, ShapedLine, Size, Window, App, Pixels, Point};
use smallvec::SmallVec;

/// The layout info of a line with soft wrapped lines.
pub(crate) struct LineLayout {
    /// Total bytes length of this line.
    len: usize,
    /// The soft wrapped lines of this line (Include the first line).
    pub(crate) wrapped_lines: SmallVec<[ShapedLine; 1]>,
    pub(crate) longest_width: Pixels,
}

impl LineLayout {
    pub(crate) fn new() -> Self {
        Self {
            len: 0,
            longest_width: px(0.),
            wrapped_lines: SmallVec::new(),
        }
    }

    pub(crate) fn lines(mut self, wrapped_lines: SmallVec<[ShapedLine; 1]>) -> Self {
        self.set_wrapped_lines(wrapped_lines);
        self
    }

    pub(crate) fn set_wrapped_lines(&mut self, wrapped_lines: SmallVec<[ShapedLine; 1]>) {
        self.len = wrapped_lines.iter().map(|l| l.len).sum();
        let width = wrapped_lines
            .iter()
            .map(|l| l.width)
            .max()
            .unwrap_or_default();
        self.longest_width = width;
        self.wrapped_lines = wrapped_lines;
    }

    /// Get the position (x, y) for the given index in this line layout.
    ///
    /// - The `offset` is a local byte index in this line layout.
    /// - The return value is relative to the top-left corner of this line layout, start from (0, 0)
    pub(crate) fn position_for_index(
        &self,
        offset: usize,
        line_height: Pixels,
    ) -> Option<Point<Pixels>> {
        let mut acc_len = 0;
        let mut offset_y = px(0.);

        for (i, line) in self.wrapped_lines.iter().enumerate() {
            let is_last = i + 1 == self.wrapped_lines.len();
            let line_len = if is_last { line.len + 1 } else { line.len };

            let range = acc_len..(acc_len + line_len);
            if range.contains(&offset) {
                let x = line.x_for_index(offset.saturating_sub(acc_len));
                return Some(point(x, offset_y));
            }
            acc_len += line_len;
            offset_y += line_height;
        }

        None
    }

    /// Get the closest index for the given x in this line layout.
    pub(super) fn closest_index_for_x(&self, x: Pixels) -> usize {
        let mut acc_len = 0;
        for (i, line) in self.wrapped_lines.iter().enumerate() {
            let is_last = i + 1 == self.wrapped_lines.len();
            if x <= line.width {
                let mut ix = line.closest_index_for_x(x);
                if !is_last && ix == line.text.len() {
                    // For soft wrap line, we can't put the cursor at the end of the line.
                    let c_len = line.text.chars().last().map(|c| c.len_utf8()).unwrap_or(0);
                    ix = ix.saturating_sub(c_len);
                }

                return acc_len + ix;
            }
            // PF 2026-10-14：按覆写后的 len 累加（视口窗外垫的占位段 text
            // 为空但 len 是真实字节数，text.len 会把段后所有定位推错）。
            acc_len += line.len();
        }

        acc_len
    }

    /// Get the index for the given position (x, y) in this line layout.
    ///
    /// The `pos` is relative to the top-left corner of this line layout, start from (0, 0)
    /// The return value is a local byte index in this line layout, start from 0.
    pub(super) fn closest_index_for_position(
        &self,
        pos: Point<Pixels>,
        line_height: Pixels,
    ) -> Option<usize> {
        let mut offset = 0;
        let mut line_top = px(0.);
        for (i, line) in self.wrapped_lines.iter().enumerate() {
            let is_last = i + 1 == self.wrapped_lines.len();
            let line_bottom = line_top + line_height;
            if pos.y >= line_top && pos.y < line_bottom {
                let mut ix = line.closest_index_for_x(pos.x);
                if !is_last && ix == line.text.len() {
                    // For soft wrap line, we can't put the cursor at the end of the line.
                    let c_len = line.text.chars().last().map(|c| c.len_utf8()).unwrap_or(0);
                    ix = ix.saturating_sub(c_len);
                }
                return Some(offset + ix);
            }

            // PF 2026-10-14：按覆写后的 len 累加（占位段 text 为空，len 是
            // 真实字节数；见 element.rs layout_lines 的视口窗外垫段）。
            offset += line.len();
            line_top = line_bottom;
        }

        None
    }

    pub(super) fn index_for_position(
        &self,
        pos: Point<Pixels>,
        line_height: Pixels,
    ) -> Option<usize> {
        let mut offset = 0;
        let mut line_top = px(0.);
        for line in self.wrapped_lines.iter() {
            let line_bottom = line_top + line_height;
            if pos.y >= line_top && pos.y < line_bottom {
                let ix = line.index_for_x(pos.x)?;
                return Some(offset + ix);
            }

            offset += line.len();
            line_top = line_bottom;
        }

        None
    }

    pub(super) fn size(&self, line_height: Pixels) -> Size<Pixels> {
        size(self.longest_width, self.wrapped_lines.len() * line_height)
    }

    pub(super) fn paint(
        &self,
        pos: Point<Pixels>,
        line_height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) {
        for (ix, line) in self.wrapped_lines.iter().enumerate() {
            // PF 2026-10-14：text 为空的段 = 视口窗外垫的占位段（等 len 空
            // ShapedLine，见 element.rs layout_lines）或真空行——paint 是
            // 一次 paint_layer，94 万字符单行 ~6000 个空段跳过后每帧省掉
            // 同量级的空层。
            if line.text.is_empty() {
                continue;
            }
            _ = line.paint(
                pos + point(px(0.), ix * line_height),
                line_height,
                window,
                cx,
            );
        }
    }
}
