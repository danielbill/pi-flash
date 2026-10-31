//! TerminalElement（gpui 自绘 Element）：alacritty 网格绘制（layout/prepaint/
//! paint）、TermShared 选择/滚轮交互、文本 run/字体测量助手、语义选区。
//! 自 mod.rs 拆出（090 重构）；主题常量与 spawn 在 mod.rs。

use super::*;

pub struct TerminalElement {
    pub tab_id: usize,
    pub term: Arc<FairMutex<Term<Proxy>>>,
    pub pty: EventLoopSender,
    pub focus: gpui::FocusHandle,
    pub selection: Option<(SelPt, SelPt)>,
    pub cols: usize,
    pub rows: usize,
    pub cell_w: f32,
    pub line_h: f32,
    pub font_size: f32,
    pub preedit: Option<String>,
    /// blink phase for the caret bar (Chat `term_cursor_on`, 1Hz heartbeat)
    pub blink_on: bool,
    pub weak: WeakEntity<Chat>,
    interactivity: gpui::Interactivity,
}

impl TerminalElement {
    pub fn new(tab: &TerminalTab, weak: WeakEntity<Chat>) -> Self {
        Self {
            tab_id: tab.id,
            term: tab.term.clone(),
            pty: tab.pty.clone(),
            focus: tab.focus.clone(),
            selection: tab.selection,
            cols: tab.cols,
            rows: tab.rows,
            cell_w: tab.cell_w,
            line_h: tab.line_h,
            font_size: tab.font_size,
            preedit: tab.preedit.clone(),
            blink_on: false,
            weak,
            interactivity: gpui::Interactivity::new(),
        }
    }

    /// Blink phase for the caret bar — Chat's 1Hz `term_cursor_on`.
    pub fn blink_on(mut self, on: bool) -> Self {
        self.blink_on = on;
        self
    }
}

impl gpui::IntoElement for TerminalElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::Styled for TerminalElement {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl gpui::InteractiveElement for TerminalElement {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        &mut self.interactivity
    }
}

impl gpui::Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<gpui::Hitbox>;

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let layout_id = self.interactivity.request_layout(
            global_id,
            inspector_id,
            window,
            cx,
            |style, window, cx| window.request_layout(style, None, cx),
        );
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        global_id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> Self::PrepaintState {
        // 面板字号变化 → 戳不符时重测 cell（平时零测量成本），下方 fit 用新值
        let font_changed = font_size() != self.font_size;
        if font_changed {
            let (cw, lh) = measure_cell(window);
            self.cell_w = cw;
            self.line_h = lh;
            self.font_size = font_size();
        }
        // fit: cols/rows from the laid-out area (FitAddon parity), resize the
        // term + PTY once per actual change（字号变化也触发：PTY 的 cell 像素
        // 随 WindowSize 下发）
        let avail_w = f32::from(bounds.size.width - px(PAD_L + PAD_R)).max(self.cell_w);
        let avail_h = f32::from(bounds.size.height - px(PAD_T + PAD_B)).max(self.line_h);
        let cols = ((avail_w / self.cell_w) as usize).max(2);
        let rows = ((avail_h / self.line_h) as usize).max(2);
        if cols != self.cols || rows != self.rows || font_changed {
            let tab_id = self.tab_id;
            let pty = self.pty.clone();
            let (cell_w, line_h, font_size) = (self.cell_w, self.line_h, self.font_size);
            let _ = self.weak.update(cx, |chat, cx| {
                if let Some(tab) = chat.ws.terminals.iter_mut().find(|t| t.id == tab_id) {
                    tab.cols = cols;
                    tab.rows = rows;
                    tab.cell_w = cell_w;
                    tab.line_h = line_h;
                    tab.font_size = font_size;
                    tab.term.lock().resize(CellDims { cols, rows });
                    let _ = pty.send(Msg::Resize(WindowSize {
                        num_cols: cols as u16,
                        num_lines: rows as u16,
                        cell_width: cell_w as u16,
                        cell_height: line_h as u16,
                    }));
                    cx.notify();
                }
            });
        }
        // 同帧一致性（crash-1792490920）：prepaint 已把网格 resize 到新行列，
        // 同帧 paint 的 snapshot 必须用同一份行列数。上面只写 tab.* 的话，
        // 元素手里的 rows 还是构建帧时的旧值——终端区变矮的那一帧（shell
        // 退出出横幅/拖窗缩小/字号调大）paint 仍按旧 rows 读网格，越过
        // screen_lines 直接踩 alacritty 的越界断言。
        self.cols = cols;
        self.rows = rows;
        self.interactivity.prepaint(
            global_id,
            inspector_id,
            bounds,
            bounds.size,
            window,
            cx,
            |_, _, hitbox, _, _| hitbox,
        )
    }

    fn paint(
        &mut self,
        global_id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Option<gpui::Hitbox>,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let focused = self.focus.is_focused(window);
        // capture everything the paint closure needs up front: the closure
        // must own its data because self.interactivity is borrowed mutably
        let term = self.term.clone();
        let grid_rows = self.rows;
        let selection = self.selection;
        let line_h = self.line_h;
        let font_size = px(self.font_size);
        let self_font_size = self.font_size;
        let preedit = self.preedit.clone();
        let blink_on = self.blink_on;
        let shared = self.clone_shared();
        self.interactivity.paint(
            global_id,
            inspector_id,
            bounds,
            hitbox.as_ref(),
            window,
            cx,
            move |_, window, cx| {
                // snapshot the grid while locked, then paint styled runs;
                // caret viewport position rides along for the caret bar and
                // the preedit overlay
                let (rows, caret) = {
                    let locked = term.lock();
                    let rows = snapshot(&locked, grid_rows, selection);
                    let offset = locked.grid().display_offset() as i32;
                    let content = locked.renderable_content();
                    let row = content.cursor.point.line.0 + offset;
                    let col = content.cursor.point.column.0;
                    // Hidden = app hid the cursor (DECTCEM); bar stays off.
                    // chars_before = snapshot-text chars left of the cursor
                    // (wide spacers are skipped there), resolved against the
                    // shaped row below: CJK fallback glyphs don't advance
                    // exactly 2 cells, so col*cell_w drifts off the painted
                    // text the more wide chars sit before the caret
                    let chars_before = (0..col)
                        .filter(|c| {
                            !locked.grid()[Line(content.cursor.point.line.0)][Column(*c)]
                                .flags
                                .contains(Flags::WIDE_CHAR_SPACER)
                        })
                        .count();
                    let caret = (row >= 0
                        && (row as usize) < grid_rows
                        && content.cursor.shape != CursorShape::Hidden)
                        .then(|| (row as usize, chars_before));
                    (rows, caret)
                };
                let line_h = px(line_h);
                let mut caret_x: Option<Pixels> = None;
                for (r, row) in rows.iter().enumerate() {
                    if row.text.is_empty() {
                        continue;
                    }
                    let runs = merge_runs(row);
                    let line = window.text_system().shape_line(
                        SharedString::from(row.text.clone()),
                        font_size,
                        &runs,
                        None,
                    );
                    let origin = gpui::point(
                        bounds.origin.x + px(PAD_L),
                        bounds.origin.y + px(PAD_T) + line_h * r as f32,
                    );
                    // ShapedLine::paint only draws glyphs + decorations; run
                    // backgrounds (cursor block / selection / ANSI bg) need
                    // the explicit paint_background pass first
                    let _ = line.paint_background(origin, line_h, window, cx);
                    let _ = line.paint(origin, line_h, window, cx);
                    if let Some((ri, chars_before)) = &caret {
                        if *ri == r {
                            let byte_ix = row
                                .text
                                .chars()
                                .take(*chars_before)
                                .map(|c| c.len_utf8())
                                .sum::<usize>();
                            caret_x = Some(line.x_for_index(byte_ix));
                        }
                    }
                }

                // caret bar: 2px vertical line in the fg color at the shaped
                // pixel x of the cursor, ~1Hz blink while focused (heartbeat
                // from the term_cursor task); dim static bar when unfocused
                let _ = shared
                    .weak
                    .update(cx, |chat, _| chat.ws.term_cursor_focused = focused);
                if let (Some((row_ix, _)), Some(x)) = (caret, caret_x) {
                    let (alpha, visible) = if focused {
                        (1.0, blink_on)
                    } else {
                        (0.35, true)
                    };
                    if visible {
                        let bar_origin = gpui::point(
                            bounds.origin.x + px(PAD_L) + x,
                            bounds.origin.y + px(PAD_T) + line_h * row_ix as f32 + px(1.),
                        );
                        window.paint_quad(gpui::fill(
                            gpui::Bounds {
                                origin: bar_origin,
                                size: gpui::size(px(2.), line_h - px(2.)),
                            },
                            hsl_a(TERM_FG, alpha),
                        ));
                    }
                }

                // IME composition: underline the preedit string at the caret
                // pixel x (xterm draws composition inline; same affordance)
                if focused
                    && let (Some(pre), Some((row_ix, _)), Some(x)) = (&preedit, caret, caret_x)
                {
                    let run = gpui::TextRun {
                        len: pre.len(),
                        font: cell_font(&CellStyle {
                            fg: hsl(TERM_FG),
                            bg: None,
                            bold: false,
                            italic: false,
                            underline: true,
                            strike: false,
                        }),
                        color: hsl(TERM_FG),
                        background_color: None,
                        underline: Some(UnderlineStyle {
                            thickness: px(1.),
                            ..Default::default()
                        }),
                        strikethrough: None,
                    };
                    let line = window.text_system().shape_line(
                        SharedString::from(pre.clone()),
                        font_size,
                        &[run],
                        None,
                    );
                    let origin = gpui::point(
                        bounds.origin.x + px(PAD_L) + x,
                        bounds.origin.y + px(PAD_T) + line_h * row_ix as f32,
                    );
                    window.paint_quad(gpui::fill(
                        gpui::Bounds {
                            origin,
                            size: gpui::size(line.width, line_h),
                        },
                        hsl(0x1d222b),
                    ));
                    let _ = line.paint(origin, line_h, window, cx);
                }

                // IME target for this frame: composition → preedit overlay,
                // commit → PTY bytes. Without a registered handler the
                // Windows IME path drops composition/commit silently, so
                // 输入法 can never type into the terminal.
                if focused {
                    window.handle_input(
                        &shared.focus,
                        TermInput {
                            tab_id: shared.tab_id,
                            term: shared.term.clone(),
                            pty: shared.pty.clone(),
                            weak: shared.weak.clone(),
                            element_bounds: bounds,
                            cell_w: shared.cell_w,
                            line_h: shared.line_h,
                            font_size: self_font_size,
                        },
                        cx,
                    );
                }

                // mouse: selection drag + wheel scroll (hit-tested manually)
                let this = shared.clone();
                window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
                    if phase != gpui::DispatchPhase::Bubble || ev.button != MouseButton::Left {
                        return;
                    }
                    if !bounds.contains(&ev.position) {
                        // 点别处 = 上一轮拖选收场（鼠标在窗口外松开时收不到
                        // MouseUp，锚点会残留）
                        this.end_selection(cx);
                        return;
                    }
                    this.start_selection(ev.position, bounds, ev.click_count, window, cx);
                });
                // drag 不要求命中 bounds：拖出终端区（顶/底）正是「越界自动
                // 滚动」的场景；「是不是在拖本次选区」由 sel_anchor 判定
                let this = shared.clone();
                window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
                    if phase != gpui::DispatchPhase::Bubble
                        || ev.pressed_button != Some(MouseButton::Left)
                    {
                        return;
                    }
                    this.drag_selection(ev.position, bounds, window, cx);
                });
                // 松开左键 = 拖动结束：清锚点、保留选区。不清的话锚点会残留，
                // 之后在别处拖动会莫名改到终端选区
                let this = shared.clone();
                window.on_mouse_event(move |ev: &MouseUpEvent, phase, _window, cx| {
                    if phase != gpui::DispatchPhase::Bubble || ev.button != MouseButton::Left {
                        return;
                    }
                    this.end_selection(cx);
                });
                // right-click: terminal context menu (copy / paste / select
                // all) — 终端没有右键菜单的话，鼠标用户无从复制粘贴
                let this = shared.clone();
                window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
                    if phase != gpui::DispatchPhase::Bubble
                        || ev.button != MouseButton::Right
                        || !bounds.contains(&ev.position)
                    {
                        return;
                    }
                    cx.stop_propagation();
                    // 先聚焦终端：菜单动作作用于它，收起后键盘也继续可用
                    window.focus(&this.focus);
                    let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
                    let tab_id = this.tab_id;
                    let _ = this.weak.update(cx, |chat, cx| {
                        chat.dlg.term_menu = Some((tab_id, x, y));
                        cx.notify();
                    });
                });
                let this = shared;
                window.on_mouse_event(move |ev: &ScrollWheelEvent, phase, window, cx| {
                    if phase != gpui::DispatchPhase::Bubble || !bounds.contains(&ev.position) {
                        return;
                    }
                    this.scroll(ev.delta, window, cx);
                    cx.stop_propagation();
                });
            },
        );
    }
}

impl TerminalElement {
    /// Shared view of the element data for the paint-registered listeners.
    fn clone_shared(&self) -> TermShared {
        TermShared {
            tab_id: self.tab_id,
            term: self.term.clone(),
            pty: self.pty.clone(),
            focus: self.focus.clone(),
            weak: self.weak.clone(),
            cell_w: self.cell_w,
            line_h: self.line_h,
        }
    }
}

#[derive(Clone)]
struct TermShared {
    tab_id: usize,
    term: Arc<FairMutex<Term<Proxy>>>,
    pty: EventLoopSender,
    focus: gpui::FocusHandle,
    weak: WeakEntity<Chat>,
    cell_w: f32,
    line_h: f32,
}

impl TermShared {
    /// Mouse down: 双击 = 选词，三击 = 选整（逻辑）行，单击 = 清选区+落锚点
    /// （xterm 语义：单击清掉上一次选中）。
    fn start_selection(
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
    fn drag_selection(
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
    fn end_selection(&self, cx: &mut gpui::App) {
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
    fn semantic_range(&self, cell: SelPt, ty: SelectionType) -> Option<(SelPt, SelPt)> {
        semantic_range_of(&self.term.lock(), cell, ty)
    }

    fn cell_at_shared(&self, bounds: gpui::Bounds<Pixels>, pos: Point<Pixels>) -> SelPt {
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

    fn scroll(&self, delta: ScrollDelta, _window: &mut Window, cx: &mut gpui::App) {
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

/// 语义选区（双击选词 / 三击选整逻辑行）：边界全交给 alacritty 的
/// `Selection`（Semantic = `semantic_search_left/right`，Lines =
/// `line_search_left/right`，后者会跟随折行取整条逻辑行）。返回的
/// 端点即 grid 坐标（与 [`SelPt`] 口径一致，含历史行负数）。
pub fn semantic_range_of(
    term: &Term<Proxy>,
    cell: SelPt,
    ty: SelectionType,
) -> Option<(SelPt, SelPt)> {
    let point = alacritty_terminal::index::Point::new(Line(cell.line), Column(cell.col));
    let range = Selection::new(ty, point, Side::Left).to_range(term)?;
    Some((
        SelPt {
            line: range.start.line.0,
            col: range.start.column.0,
        },
        SelPt {
            line: range.end.line.0,
            col: range.end.column.0,
        },
    ))
}

/// Merge per-cell styles into gpui TextRuns (bytes length = utf8 len).
fn merge_runs(row: &SnapRow) -> Vec<gpui::TextRun> {
    let mut runs: Vec<gpui::TextRun> = Vec::new();
    for (ch, style) in row.text.chars().zip(&row.styles) {
        let len = ch.len_utf8();
        if let Some(last) = runs.last_mut() {
            let same = last.color == style.fg
                && last.background_color == style.bg
                && last.font.weight == font_weight(style.bold)
                && last.font.style == font_style(style.italic)
                && last.underline.is_some() == style.underline
                && last.strikethrough.is_some() == style.strike;
            if same {
                last.len += len;
                continue;
            }
        }
        runs.push(gpui::TextRun {
            len,
            font: cell_font(style),
            color: style.fg,
            background_color: style.bg,
            underline: style.underline.then(|| UnderlineStyle {
                thickness: px(1.),
                ..Default::default()
            }),
            strikethrough: style.strike.then(|| gpui::StrikethroughStyle {
                thickness: px(1.),
                ..Default::default()
            }),
        });
    }
    runs
}

fn font_weight(bold: bool) -> FontWeight {
    if bold {
        FontWeight::BOLD
    } else {
        FontWeight::NORMAL
    }
}

fn font_style(italic: bool) -> FontStyle {
    if italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    }
}

pub(crate) fn cell_font(style: &CellStyle) -> Font {
    Font {
        family: FONT_FAMILY.into(),
        features: gpui::FontFeatures::disable_ligatures(),
        fallbacks: None,
        weight: font_weight(style.bold),
        style: font_style(style.italic),
    }
}

/// Measure the mono cell metrics (advance width of `M`, line height) in px.
pub fn measure_cell(window: &Window) -> (f32, f32) {
    let font = Font {
        family: FONT_FAMILY.into(),
        features: gpui::FontFeatures::disable_ligatures(),
        fallbacks: None,
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    };
    let run = gpui::TextRun {
        len: 8,
        font,
        color: hsl(TERM_FG),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        SharedString::from("M".repeat(8)),
        px(font_size()),
        &[run],
        None,
    );
    let cw = (f32::from(line.width) / 8.).max(1.);
    (cw, line_height())
}

// ---------------------------------------------------------------------------
// key translation (zed mappings/keys.rs subset)
// ---------------------------------------------------------------------------
