//! TermShared——paint 注册的监听器持有的共享视图（选择/滚轮交互）。
//! 自 element.rs 外移；字段/方法 pub(super) 限 element 子树。

use super::*;

#[derive(Clone)]
pub(super) struct TermShared {
    pub(super) tab_id: usize,
    pub(super) term: Arc<FairMutex<Term<Proxy>>>,
    pub(super) pty: EventLoopSender,
    pub(super) focus: gpui::FocusHandle,
    pub(super) weak: WeakEntity<Chat>,
    pub(super) cell_w: f32,
    pub(super) line_h: f32,
}

impl TermShared {
    /// Mouse down: 双击 = 选词，三击 = 选整（逻辑）行，单击 = 清选区+落锚点
    /// （xterm 语义：单击清掉上一次选中）。
    pub(super) fn start_selection(
        &self,
        pos: Point<Pixels>,
        bounds: gpui::Bounds<Pixels>,
        click_count: usize,
        _window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let cell = self.cell_at_shared(bounds, pos);
        // 选词/选行走 alacritty 自己的语义边界（zed/终端家族同口径）
        let ranged = match click_count {
            2 => self.semantic_range(cell, SelectionType::Semantic),
            3 => self.semantic_range(cell, SelectionType::Lines),
            _ => None,
        };
        let _ = self.weak.update(cx, |chat, cx| {
            if let Some(tab) = chat.ws.terminals.iter_mut().find(|t| t.id == self.tab_id) {
                match ranged {
                    Some((a, b)) => {
                        tab.selection = Some((a, b));
                        // 锚在词/行起点：双击后接着拖可继续扩选
                        tab.sel_anchor = Some(a);
                    }
                    None => {
                        tab.sel_anchor = Some(cell);
                        tab.selection = None;
                    }
                }
                cx.notify();
            }
        });
    }

    /// 拖动中：越界（拖到终端区上下之外）自动滚动，朝鼠标方向滚，越远滚
    /// 越多（每 1 行高 1 行，单次事件封顶 4 行）；鼠标事件驱动，拖出区外
    /// 后仍持续移鼠标就持续滚。alt screen 无回滚缓冲，不滚（否则等于给
    /// 应用狂发方向键），只把选点钳在首/末行。
    pub(super) fn drag_selection(
        &self,
        pos: Point<Pixels>,
        bounds: gpui::Bounds<Pixels>,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let top = bounds.origin.y;
        let bottom = bounds.origin.y + bounds.size.height;
        let out = if pos.y < top {
            Some(top - pos.y)
        } else if pos.y > bottom {
            Some(bottom - pos.y)
        } else {
            None
        };
        if let Some(d) = out {
            let alt = self.term.lock().mode().contains(TermMode::ALT_SCREEN);
            if !alt {
                let steps = (f32::from(d) / self.line_h).ceil().clamp(1., 4.);
                // 正 = 往历史滚（同 scroll() 的 delta 约定）
                let dir = if pos.y < top { 1. } else { -1. };
                self.scroll(ScrollDelta::Lines(Point::new(0., dir * steps)), window, cx);
            }
        }
        let head = self.cell_at_shared(bounds, pos);
        let _ = self.weak.update(cx, |chat, cx| {
            if let Some(tab) = chat.ws.terminals.iter_mut().find(|t| t.id == self.tab_id) {
                if let Some(anchor) = tab.sel_anchor {
                    let sel = if anchor == head {
                        None
                    } else {
                        Some((anchor, head))
                    };
                    if tab.selection != sel {
                        tab.selection = sel;
                        cx.notify();
                    }
                }
            }
        });
    }

    /// 松开左键 = 拖动结束：清锚点（选区保留）。
    pub(super) fn end_selection(&self, cx: &mut gpui::App) {
        let _ = self.weak.update(cx, |chat, cx| {
            if let Some(tab) = chat.ws.terminals.iter_mut().find(|t| t.id == self.tab_id)
                && tab.sel_anchor.take().is_some()
            {
                cx.notify();
            }
        });
    }

    /// 语义选区（双击选词 / 三击选逻辑行）：直接用 alacritty 的
    /// `semantic_search_left/right` 与 `line_search_left/right` 算边界，
    /// 口径与 alacritty/zed 一致（三击的「行」会跟随折行取整条逻辑行）。
    pub(super) fn semantic_range(&self, cell: SelPt, ty: SelectionType) -> Option<(SelPt, SelPt)> {
        semantic_range_of(&self.term.lock(), cell, ty)
    }

    pub(super) fn cell_at_shared(&self, bounds: gpui::Bounds<Pixels>, pos: Point<Pixels>) -> SelPt {
        let term = self.term.lock();
        let offset = term.grid().display_offset() as i32;
        let cols = term.grid().columns();
        let screen = term.grid().screen_lines() as i32;
        let history = term.grid().total_lines() as i32 - screen;
        drop(term);
        let x = f32::from(pos.x - bounds.origin.x - px(PAD_L));
        let y = f32::from(pos.y - bounds.origin.y - px(PAD_T));
        let col = ((x / self.cell_w).floor() as i32).clamp(0, cols as i32 - 1) as usize;
        let row = (y / self.line_h).floor() as i32;
        SelPt {
            line: (row - offset).clamp(-history, screen - 1),
            col,
        }
    }

    pub(super) fn scroll(&self, delta: ScrollDelta, _window: &mut Window, cx: &mut gpui::App) {
        let lines = match delta {
            ScrollDelta::Lines(d) => d.y,
            ScrollDelta::Pixels(d) => f32::from(d.y) / self.line_h,
        };
        if lines == 0. {
            return;
        }
        let mode = *self.term.lock().mode();
        let alt = mode.contains(TermMode::ALT_SCREEN);
        let mut count = lines.abs().round().max(1.) as i32;
        if alt {
            // alternate scroll: wheel -> arrow keys (zed alt_scroll parity)
            let seq: &[u8] = if lines > 0. { b"\x1b[A" } else { b"\x1b[B" };
            count = (count * 3).min(45);
            for _ in 0..count {
                let _ = self.pty.send(Msg::Input(seq.to_vec().into()));
            }
            return;
        }
        // positive delta = scroll up (towards history)
        let scroll = if lines > 0. { count } else { -count };
        self.term.lock().scroll_display(Scroll::Delta(scroll));
        let _ = self.weak.update(cx, |_, cx| cx.notify());
    }
}
