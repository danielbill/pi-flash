use gpui::{px, Context, Point, Window};
use sum_tree::Bias;

#[allow(unused_imports)]
use super::fold_map;

use crate::input::{
    InputState, MoveDown, MoveEnd, MoveHome, MoveLeft, MovePageDown, MovePageUp, MoveRight,
    MoveToEnd, MoveToNextWord, MoveToPreviousWord, MoveToStart, MoveUp, RopeExt as _,
};

impl InputState {
    /// Called after moving the cursor. Updates preferred_column if we know where the cursor now is.
    pub(super) fn update_preferred_column(&mut self) {
        let Some(last_layout) = &self.last_layout else {
            self.preferred_column = None;
            return;
        };

        let point = self.text.offset_to_point(self.cursor());
        // pi-flash patch（移植）：fold 态 preferred 列取 fold 空间列
        if self.fold_snapshot.has_folds() {
            let fp = self.fold_snapshot.to_fold_point(
                fold_map::InlayPoint(fold_map::Point::new(point.row as u32, point.column as u32)),
                Bias::Right,
            );
            let Some(line) = last_layout.lines.get(fp.row() as usize) else {
                self.preferred_column = None;
                return;
            };
            let Some(pos) = line.position_for_index(fp.column() as usize, last_layout.line_height)
            else {
                self.preferred_column = None;
                return;
            };
            self.preferred_column = Some((pos.x, fp.column() as usize));
            return;
        }
        let Some(line) = last_layout.line(point.row) else {
            self.preferred_column = None;
            return;
        };

        // PF-024: 折叠态下 point.column 是 doc 列，显示行按折叠列量取 x
        // （x 存进 preferred_column，column 仍存 doc 供非可见行兑底）。
        let mut col = point.column;
        if let Some(dec) = &last_layout.decorations {
            let line_start = self.text.line_start_offset(point.row);
            col = dec
                .doc_to_vis(line_start + col)
                .saturating_sub(dec.doc_to_vis(line_start));
        }

        let Some(pos) = line.position_for_index(col, last_layout.line_height) else {
            self.preferred_column = None;
            return;
        };

        self.preferred_column = Some((pos.x, point.column));
    }

    /// Move the cursor to the given offset.
    ///
    /// The offset is the UTF-8 offset.
    ///
    /// Ensure the offset use self.next_boundary or self.previous_boundary to get the correct offset.
    pub(crate) fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.clamp(0, self.text.len());
        // pi-flash patch（对齐 zed）：光标移出自动补入括号对即失效追踪——
        // 键盘移动与鼠标点击都汇经本方法（zed region 仅在选区处于对内时
        // 参与输入辅助）
        self.autoclose_invalidate_outside(offset);
        // pi-flash patch（移植）：光标在 fold 空间渲染（折叠内 offset 经
        // to_fold_point 贴边显示），buffer offset 不动（zed 同语义）
        self.selected_range = (offset..offset).into();
        self.scroll_to(offset, cx);
        self.pause_blink_cursor(cx);
        self.update_preferred_column();
        self.hide_context_menu(cx);
        cx.notify()
    }

    /// Move the cursor vertically by one line (up or down) while preserving the column if possible.
    ///
    /// move_lines: Number of lines to move vertically (positive for down, negative for up).
    pub(super) fn move_vertical(
        &mut self,
        move_lines: isize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode.is_single_line() {
            return;
        }
        let Some(last_layout) = &self.last_layout else {
            return;
        };

        let offset = self.cursor();
        let was_preferred_column = self.preferred_column;

        // pi-flash patch（移植）：fold 态按显示行移动（fold 空间无隐藏行，
        // zed display 行移动同语义）
        if self.fold_snapshot.has_folds() {
            let p = self.text.offset_to_point(offset);
            let fp = self.fold_snapshot.to_fold_point(
                fold_map::InlayPoint(fold_map::Point::new(p.row as u32, p.column as u32)),
                Bias::Right,
            );
            let target = (fp.row() as isize + move_lines).max(0) as u32;
            let target = target.min(self.fold_snapshot.max_point().row());
            let row_start = fold_map::FoldPoint::new(target, 0)
                .to_offset(&self.fold_snapshot)
                .0;
            let mut new_offset =
                self.fold_snapshot.fold_offset_to_buffer(fold_map::FoldOffset(row_start));
            if let Some((preferred_x, _)) = was_preferred_column {
                if let Some(line) = last_layout.line(target as usize) {
                    if let Some(x) = line.closest_index_for_position(
                        gpui::Point { x: preferred_x, y: px(0.) },
                        last_layout.line_height,
                    ) {
                        new_offset = self.fold_snapshot.fold_offset_to_buffer(
                            fold_map::FoldOffset(row_start + x),
                        );
                    }
                }
            }
            self.pause_blink_cursor(cx);
            self.move_to(new_offset, cx);
            self.preferred_column = was_preferred_column;
            cx.notify();
            return;
        }

        let mut display_point = self.text_wrapper.offset_to_display_point(offset);
        display_point.row = display_point.row.saturating_add_signed(move_lines);
        display_point.column = 0;
        let mut new_offset = self.text_wrapper.display_point_to_offset(display_point);

        if let Some((preferred_x, column)) = was_preferred_column {
            // Get display point again to update local_row.
            let mut next_display_point = self.text_wrapper.offset_to_display_point(new_offset);
            next_display_point.column = 0;
            let next_point = self.text_wrapper.display_point_to_point(next_display_point);
            let line_start_offset = self.text.line_start_offset(next_point.row);

            // If in visible range, prefer to use position to get column.
            if let Some(line) = last_layout.line(next_point.row) {
                if let Some(x) = line.closest_index_for_position(
                    Point {
                        x: preferred_x,
                        y: next_display_point.local_row * last_layout.line_height,
                    },
                    last_layout.line_height,
                ) {
                    // PF-024: closest 返回折叠列 → 经 FoldSet 换回 doc 偏移
                    // （直接 line_start + x 会把折叠坐标当 doc 用，光标漂移）
                    new_offset = if let Some(dec) = &last_layout.decorations {
                        dec.vis_to_doc(dec.doc_to_vis(line_start_offset) + x)
                    } else {
                        line_start_offset + x
                    };
                }
            } else {
                // Not in visible range, use column directly.
                let max_line_len = self.text.slice_line(next_point.row).len();
                new_offset = line_start_offset + column.min(max_line_len);
            }
        }

        self.pause_blink_cursor(cx);
        self.move_to(new_offset, cx);
        // Set back the preferred_column
        self.preferred_column = was_preferred_column;
        cx.notify();
    }

    /// pi-flash patch（对齐 zed）：选区头纵向移动——Shift+↑/↓ 用。
    ///
    /// 与 [`Self::move_vertical`] 同一套换算（按**显示行**走、preferred
    /// column 锚定目标列、PF-024 折叠列换回 doc 偏移），区别只在终点走
    /// `select_to`（保留选区另一端，越过锚点时的翻转由它处理）而非
    /// `move_to`（塌缩成光标）。
    ///
    /// move_lines: 选区头移动的显示行数（正 = 向下，负 = 向上）。
    pub(super) fn select_vertical(&mut self, move_lines: isize, cx: &mut Context<Self>) {
        if self.mode.is_single_line() {
            return;
        }
        let Some(last_layout) = &self.last_layout else {
            return;
        };

        let offset = self.cursor();
        let was_preferred_column = self.preferred_column;

        // pi-flash patch（移植）：fold 态按显示行移动（选区头）
        if self.fold_snapshot.has_folds() {
            let p = self.text.offset_to_point(offset);
            let fp = self.fold_snapshot.to_fold_point(
                fold_map::InlayPoint(fold_map::Point::new(p.row as u32, p.column as u32)),
                Bias::Right,
            );
            let target = (fp.row() as isize + move_lines).max(0) as u32;
            let target = target.min(self.fold_snapshot.max_point().row());
            let row_start = fold_map::FoldPoint::new(target, 0)
                .to_offset(&self.fold_snapshot)
                .0;
            let mut new_offset =
                self.fold_snapshot.fold_offset_to_buffer(fold_map::FoldOffset(row_start));
            if let Some((preferred_x, _)) = was_preferred_column {
                if let Some(line) = last_layout.line(target as usize) {
                    if let Some(x) = line.closest_index_for_position(
                        gpui::Point { x: preferred_x, y: px(0.) },
                        last_layout.line_height,
                    ) {
                        new_offset = self.fold_snapshot.fold_offset_to_buffer(
                            fold_map::FoldOffset(row_start + x),
                        );
                    }
                }
            }
            self.pause_blink_cursor(cx);
            self.select_to(new_offset, cx);
            self.preferred_column = was_preferred_column;
            cx.notify();
            return;
        }

        let mut display_point = self.text_wrapper.offset_to_display_point(offset);
        display_point.row = display_point.row.saturating_add_signed(move_lines);
        display_point.column = 0;
        let mut new_offset = self.text_wrapper.display_point_to_offset(display_point);

        if let Some((preferred_x, column)) = was_preferred_column {
            // Get display point again to update local_row.
            let mut next_display_point = self.text_wrapper.offset_to_display_point(new_offset);
            next_display_point.column = 0;
            let next_point = self.text_wrapper.display_point_to_point(next_display_point);
            let line_start_offset = self.text.line_start_offset(next_point.row);

            // If in visible range, prefer to use position to get column.
            if let Some(line) = last_layout.line(next_point.row) {
                if let Some(x) = line.closest_index_for_position(
                    Point {
                        x: preferred_x,
                        y: next_display_point.local_row * last_layout.line_height,
                    },
                    last_layout.line_height,
                ) {
                    new_offset = if let Some(dec) = &last_layout.decorations {
                        dec.vis_to_doc(dec.doc_to_vis(line_start_offset) + x)
                    } else {
                        line_start_offset + x
                    };
                }
            } else {
                // Not in visible range, use column directly.
                let max_line_len = self.text.slice_line(next_point.row).len();
                new_offset = line_start_offset + column.min(max_line_len);
            }
        }

        self.pause_blink_cursor(cx);
        self.select_to(new_offset, cx);
        // 纵向移动保持目标列粘性：select_to 会按新选区头重算
        // preferred_column，这里显式恢复——连续 Shift+↑/↓ 经过短行/空行
        // 后回到长行，列不漂（与 move_vertical 同语义）
        self.preferred_column = was_preferred_column;
        cx.notify();
    }

    pub(super) fn left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.pause_blink_cursor(cx);
        if self.selected_range.is_empty() {
            let mut off = self.previous_boundary(self.cursor());
            // PF-024 P2：原子跳过折叠段——光标永不停在隐藏语法字符之间
            if let Some(dec) = self
                .last_layout
                .as_ref()
                .and_then(|l| l.decorations.as_ref())
            {
                off = dec.next_atomic(off, -1);
            }
            self.move_to(off, cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    pub(super) fn right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.pause_blink_cursor(cx);
        if self.selected_range.is_empty() {
            let mut off = self.next_boundary(self.selected_range.end);
            // PF-024 P2：原子跳过折叠段
            if let Some(dec) = self
                .last_layout
                .as_ref()
                .and_then(|l| l.decorations.as_ref())
            {
                off = dec.next_atomic(off, 1);
            }
            self.move_to(off, cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    pub(super) fn up(&mut self, action: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.handle_action_for_context_menu(Box::new(action.clone()), window, cx) {
            return;
        }

        if self.mode.is_single_line() {
            return;
        }

        if !self.selected_range.is_empty() {
            self.move_to(
                self.previous_boundary(self.selected_range.start.saturating_sub(1)),
                cx,
            );
        }
        self.pause_blink_cursor(cx);
        self.move_vertical(-1, window, cx);
    }

    pub(super) fn down(&mut self, action: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        if self.handle_action_for_context_menu(Box::new(action.clone()), window, cx) {
            return;
        }

        if self.mode.is_single_line() {
            return;
        }

        if !self.selected_range.is_empty() {
            self.move_to(
                self.next_boundary(self.selected_range.end.saturating_sub(1)),
                cx,
            );
        }

        self.pause_blink_cursor(cx);
        self.move_vertical(1, window, cx);
    }

    pub(super) fn page_up(&mut self, _: &MovePageUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode.is_single_line() {
            return;
        }

        let Some(last_layout) = &self.last_layout else {
            return;
        };

        let display_lines = (self.input_bounds.size.height / last_layout.line_height) as isize;
        self.move_vertical(-display_lines, window, cx);
    }

    pub(super) fn page_down(
        &mut self,
        _: &MovePageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode.is_single_line() {
            return;
        }

        let Some(last_layout) = &self.last_layout else {
            return;
        };

        let display_lines = (self.input_bounds.size.height / last_layout.line_height) as isize;
        self.move_vertical(display_lines, window, cx);
    }

    pub(super) fn home(&mut self, _: &MoveHome, _: &mut Window, cx: &mut Context<Self>) {
        self.pause_blink_cursor(cx);
        let offset = self.start_of_line();
        self.move_to(offset, cx);
    }

    pub(super) fn end(&mut self, _: &MoveEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.pause_blink_cursor(cx);
        let offset = self.end_of_line();
        self.move_to(offset, cx);
    }

    pub(super) fn move_to_start(
        &mut self,
        _: &MoveToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to(0, cx);
    }

    pub(super) fn move_to_end(&mut self, _: &MoveToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.text.len(), cx);
    }

    pub(super) fn move_to_previous_word(
        &mut self,
        _: &MoveToPreviousWord,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.previous_start_of_word();
        self.move_to(offset, cx);
    }

    pub(super) fn move_to_next_word(
        &mut self,
        _: &MoveToNextWord,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.next_end_of_word();
        self.move_to(offset, cx);
    }
}
