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
                paint_grid(
                    focused,
                    term,
                    grid_rows,
                    selection,
                    line_h,
                    font_size,
                    self_font_size,
                    preedit,
                    blink_on,
                    shared,
                    bounds,
                    window,
                    cx,
                )
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

mod paint;
mod shared;

use self::paint::paint_grid;
use self::shared::TermShared;
pub(crate) use paint::cell_font;
