use gpui::{px, Context, Point, Window};
use sum_tree::Bias;

use crate::input::{
    InputState, MoveDown, MoveEnd, MoveHome, MoveLeft, MovePageDown, MovePageUp, MoveRight,
    MoveToEnd, MoveToNextWord, MoveToPreviousWord, MoveToStart, MoveUp,
};

impl InputState {
    /// Called after moving the cursor. Updates preferred_column if we know where the cursor now is.
    ///
    /// pi-flash patch（dqjd S2）：列 = **显示列**（wrap 显示字节，续行含缩
    /// 进）——纵向移动的保列/兜底全在同一坐标系（zed 同语义）。x 仅在光标
    /// 行可视时可得（不可视 → None，同旧口径）。
    pub(super) fn update_preferred_column(&mut self) {
        let Some(last_layout) = &self.last_layout else {
            self.preferred_column = None;
            return;
        };

        let (disp_row, disp_col) =
            self.offset_to_display_point(last_layout, self.cursor(), Bias::Right);
        let Some(line) = last_layout.line(disp_row as usize) else {
            self.preferred_column = None;
            return;
        };

        let Some(pos) = line.position_for_index(disp_col, last_layout.line_height) else {
            self.preferred_column = None;
            return;
        };

        self.preferred_column = Some((pos.x, disp_col));
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
        // pi-flash patch（移植）：光标在显示空间渲染（折叠内 offset 经
        // 链换算贴边显示），buffer offset 不动（zed 同语义）
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
    ///
    /// pi-flash patch（dqjd S2）：统一按**显示行**移动（fold 隐藏行与软换
    /// 行段同为此空间的行——zed display 行移动同语义），列经显示换算收口。
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

        let was_preferred_column = self.preferred_column;

        let cur_offset = self.cursor();
        let (cur_row, _) = self.offset_to_display_point(last_layout, cur_offset, Bias::Right);
        let max_row = self.wrap_map.snapshot().max_point().row();
        let target = (cur_row as isize + move_lines).clamp(0, max_row as isize) as u32;

        let new_offset = if let Some((preferred_x, preferred_col)) = was_preferred_column {
            if let Some(line) = last_layout.line(target as usize) {
                if let Some(d) = line.closest_index_for_position(
                    Point {
                        x: preferred_x,
                        y: px(0.),
                    },
                    last_layout.line_height,
                ) {
                    // closest 返回显示字节（缩进区内 = 段边界，zed clip 同语义）
                    self.display_point_to_offset(last_layout, target, d, Bias::Right)
                } else {
                    self.display_point_to_offset(last_layout, target, 0, Bias::Right)
                }
            } else {
                // Not in visible range, use column directly（显示列兜底，钳行长）
                self.display_point_to_offset(last_layout, target, preferred_col, Bias::Right)
            }
        } else {
            self.display_point_to_offset(last_layout, target, 0, Bias::Right)
        };

        self.pause_blink_cursor(cx);
        self.move_to(new_offset, cx);
        // Set back the preferred_column
        self.preferred_column = was_preferred_column;
        cx.notify();
    }

    /// pi-flash patch（对齐 zed）：选区头纵向移动——Shift+↑/↓ 用。
    ///
    /// 与 [`Self::move_vertical`] 同一套换算（按**显示行**走、preferred
    /// column 锚定目标列），区别只在终点走 `select_to`（保留选区另一端，
    /// 越过锚点时的翻转由它处理）而非 `move_to`（塌缩成光标）。
    ///
    /// move_lines: 选区头移动的显示行数（正 = 向下，负 = 向上）。
    pub(super) fn select_vertical(&mut self, move_lines: isize, cx: &mut Context<Self>) {
        if self.mode.is_single_line() {
            return;
        }
        let Some(last_layout) = &self.last_layout else {
            return;
        };

        let was_preferred_column = self.preferred_column;

        let cur_offset = self.cursor();
        let (cur_row, _) = self.offset_to_display_point(last_layout, cur_offset, Bias::Right);
        let max_row = self.wrap_map.snapshot().max_point().row();
        let target = (cur_row as isize + move_lines).clamp(0, max_row as isize) as u32;

        let new_offset = if let Some((preferred_x, preferred_col)) = was_preferred_column {
            if let Some(line) = last_layout.line(target as usize) {
                if let Some(d) = line.closest_index_for_position(
                    Point {
                        x: preferred_x,
                        y: px(0.),
                    },
                    last_layout.line_height,
                ) {
                    self.display_point_to_offset(last_layout, target, d, Bias::Right)
                } else {
                    self.display_point_to_offset(last_layout, target, 0, Bias::Right)
                }
            } else {
                self.display_point_to_offset(last_layout, target, preferred_col, Bias::Right)
            }
        } else {
            self.display_point_to_offset(last_layout, target, 0, Bias::Right)
        };

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
