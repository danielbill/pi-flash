//! Built-in terminal — pi-web TerminalPanel parity on an alacritty_terminal
//! core (zed crates/terminal patterns adapted to the crates.io 0.26 API).
//!
//! pi-web surface: right-panel terminal tabs (TabBar), 38px header with status
//! dot + cwd + Restart, exit banner, fixed dark xterm surface (#111318) in
//! every theme, Consolas 13px / lineHeight 1.25, scrollback 8000, cursor #60a5fa.
//! pi-web spawns node-pty (ComSpec on Windows); here `tty::new` provides
//! ConPTY directly. Reconnect (SSE resume) has no in-process equivalent and is
//! intentionally dropped; mouse-reporting apps get wheel-as-arrows only.
use crate::*;

use std::path::PathBuf;
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty::{self, Shell};
use alacritty_terminal::vte::ansi::CursorShape;
use futures::channel::mpsc::UnboundedSender;
use gpui::{
    Font, FontStyle, FontWeight, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, ScrollDelta, ScrollWheelEvent, SharedString, UnderlineStyle, WeakEntity, Window,
    px,
};

mod input_encode;
mod palette;
mod snapshot;

use input_encode::*;
use palette::*;
pub(crate) use snapshot::*;

use crate::Chat;

// ---------------------------------------------------------------------------
// pi-web TerminalPanel.tsx xterm theme (fixed dark surface in every theme)
// ---------------------------------------------------------------------------

const TERM_BG: u32 = 0x111318;
const TERM_FG: u32 = 0xd7dce5;
const TERM_CURSOR: u32 = 0x60a5fa;
const TERM_SEL: u32 = 0x365b8a;
const ANSI16: [u32; 16] = [
    0x1d222b, 0xf87171, 0x4ade80, 0xfacc15, 0x60a5fa, 0xc084fc, 0x22d3ee, 0xe5e7eb, 0x6b7280,
    0xfca5a5, 0x86efac, 0xfde047, 0x93c5fd, 0xd8b4fe, 0x67e8f9, 0xffffff,
];

/// xterm `scrollback: 8000`
const SCROLLBACK: usize = 8000;
/// 终端等宽字体 = **JetBrains Mono**（pi-web `--font-mono` 次选、随二进制
/// 打包的唯一 mono，含 Regular/SemiBold/Bold/Italic 四字重；斜体为真字重，
/// 优于 Noto Sans Mono 的系统合成）。终端显式关 calt 连字（xterm 语义）。
pub const FONT_FAMILY: &str = "JetBrains Mono";

/// 终端字号 = 面板字号（档位 14/15/16/17 即终端值，2026-10-10 定夺去掉 -1）。
pub fn font_size() -> f32 {
    crate::appearance::panel_font().size
}

/// xterm `lineHeight: 1.25` 比例不变，随字号走。
fn line_height() -> f32 {
    font_size() * 1.25
}
/// `.terminal-xterm` padding: 10px 8px 22px 12px
pub const PAD_L: f32 = 12.;
pub const PAD_T: f32 = 10.;
pub const PAD_R: f32 = 8.;
pub const PAD_B: f32 = 22.;
const DEFAULT_COLS: usize = 80;
const DEFAULT_ROWS: usize = 24;

fn hsl(c: u32) -> Hsla {
    gpui::rgb(c).into()
}
fn hsl_a(c: u32, a: f32) -> Hsla {
    let mut h = hsl(c);
    h.a = a;
    h
}

// ---------------------------------------------------------------------------
// event proxy: alacritty EventListener -> Chat pump task
// ---------------------------------------------------------------------------

/// Forwards alacritty events to the Chat pump channel tagged with the tab id.
#[derive(Clone)]
pub struct Proxy {
    pub tab: usize,
    pub tx: UnboundedSender<(usize, Event)>,
}

impl EventListener for Proxy {
    fn send_event(&self, event: Event) {
        let _ = self.tx.unbounded_send((self.tab, event));
    }
}

/// Initial grid dimensions handed to `Term::new` (no history yet).
pub struct CellDims {
    pub cols: usize,
    pub rows: usize,
}

impl Dimensions for CellDims {
    fn columns(&self) -> usize {
        self.cols
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn total_lines(&self) -> usize {
        self.rows
    }
}

// ---------------------------------------------------------------------------
// terminal tab state (owned by Chat)
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
pub enum TermStatus {
    Ready,
    Exited(Option<i32>),
}

pub struct TerminalTab {
    pub id: usize,
    pub cwd: PathBuf,
    /// tab label: basename of cwd (pi-web `getFileName(tab.cwd)`)
    pub status: TermStatus,
    pub term: Arc<FairMutex<Term<Proxy>>>,
    pub pty: EventLoopSender,
    pub focus: gpui::FocusHandle,
    pub cols: usize,
    pub rows: usize,
    pub cell_w: f32,
    pub line_h: f32,
    /// cell 尺寸所按的字号戳；prepaint 发现与当前 [`font_size`] 不符即重测重排
    pub font_size: f32,
    /// grid-coordinate selection (Line includes display offset)
    pub selection: Option<(SelPt, SelPt)>,
    /// drag anchor while a selection is in progress
    pub sel_anchor: Option<SelPt>,
    /// IME composition string shown as an overlay at the caret (written by
    /// TermInput during WM_IME_COMPOSITION, cleared on commit/unmark)
    pub preedit: Option<String>,
}

#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub struct SelPt {
    pub line: i32,
    pub col: usize,
}

/// Spawn the PTY + alacritty event loop for one tab.
/// `cell_w`/`line_h` come from measuring the mono font in the opening window;
/// `font_size` 是它们所按的字号戳（restart 沿用旧 tab 时一并继承）。
/// `shell_override` = 021 右键【执行】（`cmd /k <脚本命令>` 进 tab）；
/// None = 默认 shell（ComSpec ?? cmd.exe，terminal-manager.ts 同款）。
#[allow(clippy::too_many_arguments)]
pub fn spawn_terminal(
    id: usize,
    cwd: PathBuf,
    cell_w: f32,
    line_h: f32,
    font_size: f32,
    focus: gpui::FocusHandle,
    proxy: Proxy,
    shell_override: Option<Shell>,
) -> Result<TerminalTab, String> {
    let dims = CellDims {
        cols: DEFAULT_COLS,
        rows: DEFAULT_ROWS,
    };
    let config = Config {
        scrolling_history: SCROLLBACK,
        ..Config::default()
    };
    let term = Arc::new(FairMutex::new(Term::new(config, &dims, proxy.clone())));

    let mut options = tty::Options::default();
    // terminal-manager.ts: win32 -> ComSpec ?? cmd.exe with no args
    let shell = shell_override.unwrap_or_else(|| {
        Shell::new(
            std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string()),
            Vec::new(),
        )
    });
    options.shell = Some(shell);
    options.working_directory = Some(cwd.clone());
    options.env = shell_env();
    options.drain_on_exit = true;

    let window_size = WindowSize {
        num_cols: DEFAULT_COLS as u16,
        num_lines: DEFAULT_ROWS as u16,
        cell_width: cell_w as u16,
        cell_height: line_h as u16,
    };
    let pty = tty::new(&options, window_size, 0).map_err(|e| e.to_string())?;
    let event_loop =
        EventLoop::new(term.clone(), proxy, pty, true, false).map_err(|e| e.to_string())?;
    let pty_tx = event_loop.channel();
    event_loop.spawn();

    Ok(TerminalTab {
        id,
        cwd: cwd.clone(),
        status: TermStatus::Ready,
        term,
        pty: pty_tx,
        focus,
        cols: DEFAULT_COLS,
        rows: DEFAULT_ROWS,
        cell_w,
        line_h,
        font_size,
        selection: None,
        sel_anchor: None,
        preedit: None,
    })
}

/// terminal-manager.ts shellEnvironment: inherit everything, then pin TERM /
/// COLORTERM and a UTF-8 LANG when none is set (Windows codepage fix).
fn shell_env() -> std::collections::HashMap<String, String> {
    let mut env: std::collections::HashMap<String, String> = std::env::vars().collect();
    env.insert("TERM".into(), "xterm-256color".into());
    env.insert("COLORTERM".into(), "truecolor".into());
    let has_lang = ["LANG", "LC_ALL", "LC_CTYPE"]
        .iter()
        .any(|k| env.contains_key(*k));
    if !has_lang {
        env.insert("LANG".into(), "C.UTF-8".into());
    }
    env
}

// ---------------------------------------------------------------------------
// TerminalElement: custom element painting the alacritty grid
// ---------------------------------------------------------------------------

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

fn cell_font(style: &CellStyle) -> Font {
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

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::Processor;
    use gpui::Keystroke;

    fn test_term(cols: usize, rows: usize, text: &[u8]) -> Term<Proxy> {
        let (tx, _rx) = futures::channel::mpsc::unbounded::<(usize, Event)>();
        let proxy = Proxy { tab: 0, tx };
        let mut term = Term::new(Config::default(), &CellDims { cols, rows }, proxy);
        Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::new().advance(&mut term, text);
        term
    }

    #[test]
    fn palette_matches_pi_web_theme() {
        assert_eq!(palette256(0), 0x1d222b);
        assert_eq!(palette256(1), 0xf87171);
        assert_eq!(palette256(8), 0x6b7280);
        assert_eq!(palette256(256), 0xd7dce5);
        assert_eq!(palette256(257), 0x111318);
        // 16..231 cube: index 17 is r=0 g=0 b=1 (16 is pure black)
        assert_eq!(palette256(16), 0x000000);
        assert_eq!(palette256(17), 0x00005f);
        // grayscale ramp starts at #080808
        assert_eq!(palette256(232), 0x080808);
    }

    #[test]
    fn snapshot_plain_text_rows() {
        let term = test_term(10, 2, b"hello");
        let rows = snapshot(&term, 2, None);
        assert_eq!(rows[0].text, "hello     ");
        assert_eq!(rows[1].text, " ".repeat(10));
        // default fg/bg: no background run needed
        assert!(rows[0].styles[0].bg.is_none());
        assert_eq!(rows[0].styles[0].fg, hsl(TERM_FG));
    }

    /// 裸 `exit` 拦截判定：各种提示符风格下的命中/不命中。
    #[test]
    fn exit_swallow_detection() {
        // cmd 风格：提示符后紧跟命令
        let cmd = test_term(40, 3, b"d:\\proj>exit");
        assert!(is_bare_exit(&cmd));
        // PowerShell 风格 + 命令前后空白
        let ps = test_term(40, 3, b"PS D:\\proj> exit ");
        assert!(is_bare_exit(&ps));
        // bash 风格（$ 提示符）
        let sh = test_term(40, 3, b"bi@host:/d/x$ exit");
        assert!(is_bare_exit(&sh));
        // 大小写不敏感
        let upper = test_term(40, 3, b"d:\\proj>EXIT");
        assert!(is_bare_exit(&upper));
        // 无提示符风格的裸行
        let bare = test_term(40, 3, b"exit");
        assert!(is_bare_exit(&bare));
        // 带参数/别的命令/空行：不拦
        let with_arg = test_term(40, 3, b"d:\\proj>exit 0");
        assert!(!is_bare_exit(&with_arg));
        let other = test_term(40, 3, b"d:\\proj>dir");
        assert!(!is_bare_exit(&other));
        let empty = test_term(40, 3, b"d:\\proj>");
        assert!(!is_bare_exit(&empty));
        // python REPL：exit() 是合法调用，不吞
        let py = test_term(40, 3, b">>> exit()");
        assert!(!is_bare_exit(&py));
    }

    #[test]
    fn snapshot_sgr_colors_and_inverse() {
        // red text, then inverse space
        let term = test_term(10, 2, b"\x1b[31mR\x1b[7m \x1b[0m ");
        let rows = snapshot(&term, 2, None);
        assert_eq!(rows[0].text, "R         ");
        assert_eq!(rows[0].styles[0].fg, hsl(0xf87171));
        // inverse: fg/bg swap — fg becomes default bg, bg becomes the
        // current (red) fg
        assert_eq!(rows[0].styles[1].fg, hsl(TERM_BG));
        assert_eq!(rows[0].styles[1].bg, Some(hsl(0xf87171)));
    }

    #[test]
    fn snapshot_selection_band() {
        let term = test_term(10, 2, b"abcdef");
        let sel = (SelPt { line: 0, col: 1 }, SelPt { line: 0, col: 3 });
        let rows = snapshot(&term, 2, Some(sel));
        assert!(!rows[0].styles[0].bg.is_some());
        assert!(rows[0].styles[1].bg.is_some());
        assert!(rows[0].styles[3].bg.is_some());
        assert!(!rows[0].styles[4].bg.is_some());
    }

    #[test]
    fn selection_text_extracts_range() {
        let term = test_term(10, 3, b"abcdef\r\nghijkl\r\nmnopqr");
        let sel = (SelPt { line: 0, col: 2 }, SelPt { line: 1, col: 3 });
        assert_eq!(selection_text(&term, sel), "cdef\nghij");
        // reversed endpoints normalize to the same range
        let rev = (SelPt { line: 1, col: 3 }, SelPt { line: 0, col: 2 });
        assert_eq!(selection_text(&term, rev), "cdef\nghij");
    }

    #[test]
    fn key_translation_table() {
        let plain = TermMode::empty();
        let app = TermMode::APP_CURSOR;
        let k = |key: &str, ctrl: bool, alt: bool, shift: bool| Keystroke {
            modifiers: gpui::Modifiers {
                control: ctrl,
                alt,
                shift,
                ..Default::default()
            },
            key: key.into(),
            key_char: None,
        };
        assert_eq!(
            keystroke_to_pty(&k("enter", false, false, false), &plain),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            keystroke_to_pty(&k("backspace", false, false, false), &plain),
            Some(vec![0x7f])
        );
        assert_eq!(
            keystroke_to_pty(&k("escape", false, false, false), &plain),
            Some(vec![0x1b])
        );
        assert_eq!(
            keystroke_to_pty(&k("tab", false, false, true), &plain),
            Some(b"\x1b[Z".to_vec())
        );
        assert_eq!(
            keystroke_to_pty(&k("up", false, false, false), &plain),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            keystroke_to_pty(&k("up", false, false, false), &app),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            keystroke_to_pty(&k("left", true, false, false), &plain),
            Some(b"\x1b[1;5D".to_vec())
        );
        assert_eq!(
            keystroke_to_pty(&k("pagedown", false, false, false), &plain),
            Some(b"\x1b[6~".to_vec())
        );
        assert_eq!(
            keystroke_to_pty(&k("f5", false, false, false), &plain),
            Some(b"\x1b[15~".to_vec())
        );
        assert_eq!(
            keystroke_to_pty(&k("d", true, false, false), &plain),
            Some(vec![0x04])
        );
        assert_eq!(
            keystroke_to_pty(&k("a", true, false, false), &plain),
            Some(vec![0x01])
        );
        // alt prefixing
        assert_eq!(
            keystroke_to_pty(&k("x", false, true, false), &plain),
            Some(b"\x1bx".to_vec())
        );
        // printable via key_char
        let mut ch = k("a", false, false, false);
        ch.key_char = Some("A".into());
        assert_eq!(keystroke_to_pty(&ch, &plain), Some(b"A".to_vec()));
    }

    #[test]
    fn paste_normalizes_newlines() {
        let plain = TermMode::empty();
        assert_eq!(paste_bytes("a\nb\r\nc", &plain), b"a\rb\rc".to_vec());
        let bracketed = TermMode::BRACKETED_PASTE;
        assert_eq!(
            paste_bytes("x", &bracketed),
            b"\x1b[200~x\x1b[201~".to_vec()
        );
    }

    /// IME commit: CR-normalized UTF-8, ESC stripped, no bracket markers.
    #[test]
    fn ime_commit_bytes_path() {
        assert_eq!(ime_commit_bytes("你好"), "你好".as_bytes().to_vec());
        assert_eq!(ime_commit_bytes("a\nb\r\nc"), b"a\rb\rc".to_vec());
        assert_eq!(ime_commit_bytes("x\x1by"), b"xy".to_vec());
    }

    /// 双击选词 / 三击选行：alacritty semantic / line 边界，含标点归组
    /// 与折行逻辑行两种情形。
    #[test]
    fn semantic_selection_word_and_line() {
        // 30 列、不折行：断言列边界。这些数值就是 alacritty
        // `semantic_search_left/right` 与 `line_search_left/right` 的原样
        // 输出（zed 双击/三击走的就是这两个函数）——包括两个“看上去偏
        // 一位”的既有语义：① 点的若是分隔符（空格），会把两侧的词一起
        // 含进来；② 右边界在词末之后多含一格（span 语义）。多出的那格
        // （空白）在拷贝时由 selection_text 去尾，不影响取出的文本。
        let t30 = test_term(30, 3, b"ls -l /tmp/foo_bar.txt");
        let sel = |term: &Term<Proxy>, col: usize, ty| {
            semantic_range_of(term, SelPt { line: 0, col }, ty)
        };
        let word = |col: usize| sel(&t30, col, SelectionType::Semantic);
        let line = |col: usize| sel(&t30, col, SelectionType::Lines);
        // `ls`：词首/词尾都归同一个词
        assert_eq!(
            word(0),
            Some((SelPt { line: 0, col: 0 }, SelPt { line: 0, col: 1 }))
        );
        assert_eq!(
            word(1),
            Some((SelPt { line: 0, col: 0 }, SelPt { line: 0, col: 1 }))
        );
        // 空格：分隔符扩到相邻词（alacritty 原样语义）
        assert_eq!(
            word(2),
            Some((SelPt { line: 0, col: 0 }, SelPt { line: 0, col: 4 }))
        );
        // 路径整串（`/` `_` `.` 都是词字符）
        assert_eq!(
            word(10),
            Some((SelPt { line: 0, col: 6 }, SelPt { line: 0, col: 21 }))
        );
        // 三击 = 整条屏幕行（到末列；尾部空白拷贝时去尾）
        assert_eq!(
            line(5),
            Some((SelPt { line: 0, col: 0 }, SelPt { line: 0, col: 29 }))
        );

        // 折行：逻辑行跨屏幕行（20 列下 21 字符必然折行）
        let t20 = test_term(20, 3, b"ls -l /tmp/foo_bar.txt");
        assert_eq!(
            sel(&t20, 10, SelectionType::Semantic),
            Some((SelPt { line: 0, col: 6 }, SelPt { line: 1, col: 1 }))
        );
        assert_eq!(
            sel(&t20, 5, SelectionType::Lines),
            Some((SelPt { line: 0, col: 0 }, SelPt { line: 1, col: 19 }))
        );

        // 宽字符（CJK）：整串汉字 + 尾部 spacer 都在区内
        let cjk = test_term(20, 3, "中文 测试".as_bytes());
        assert_eq!(
            sel(&cjk, 0, SelectionType::Semantic),
            Some((SelPt { line: 0, col: 0 }, SelPt { line: 0, col: 3 }))
        );
        // 选取范围拷出的文本（宽字符 spacer 不重复、不丢字）
        if let Some(range) = sel(&cjk, 0, SelectionType::Semantic) {
            assert_eq!(selection_text(&cjk, range), "中文");
        }
    }

    /// 选区删除计划：中段选区（含 alacritty 多给的一格空白）、行尾选区、
    /// 不可删的几种情形（只选空白/跨行/选区在光标右侧/alt screen）。
    #[test]
    fn deletion_plan_cases() {
        // 光标停在行尾（col 11）："abc def ghi"
        let term = test_term(30, 3, b"abc def ghi");
        let p = |a: usize, b: usize| {
            deletion_plan(
                &term,
                (SelPt { line: 0, col: a }, SelPt { line: 0, col: b }),
            )
        };
        // 中段 "def"：左移 3（选区字符）+ 4（"> ghi" 四个字符），删 3
        assert_eq!(p(4, 6), Some((7, 3)));
        // 中段 "def "（尾部那格空白是词边界，不当字符删）——同上
        assert_eq!(p(4, 7), Some((7, 3)));
        // 行尾 "ghi"：左移就是 3，不用右移
        assert_eq!(p(8, 10), Some((3, 3)));
        // 端点顺序反过来同样成立
        assert_eq!(p(6, 4), Some((7, 3)));
        // 只选空白 → 没有可删字符
        assert_eq!(p(3, 3), None);
        // 跨屏幕行 → 行编辑做不了
        assert_eq!(
            deletion_plan(
                &term,
                (SelPt { line: 0, col: 4 }, SelPt { line: 1, col: 2 }),
            ),
            None
        );

        // alt screen：全屏程序的行模型由应用自己管，不动
        let alt = test_term(30, 3, b"\x1b[?1049habc def ghi");
        assert_eq!(
            deletion_plan(&alt, (SelPt { line: 0, col: 4 }, SelPt { line: 0, col: 6 })),
            None
        );
    }

    /// Real ConPTY smoke test (no gpui): spawn cmd.exe through the same
    /// tty::new + EventLoop bridge the terminal tabs use, echo a marker, and
    /// read it back off the grid.
    #[test]
    #[cfg(windows)]
    fn conpty_echo_smoke() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        struct Chan(mpsc::Sender<Event>);
        impl EventListener for Chan {
            fn send_event(&self, e: Event) {
                let _ = self.0.send(e);
            }
        }

        let (tx, rx) = mpsc::channel();
        let term = Arc::new(FairMutex::new(Term::new(
            Config {
                scrolling_history: 100,
                ..Config::default()
            },
            &CellDims { cols: 80, rows: 24 },
            Chan(tx.clone()),
        )));
        let mut options = tty::Options::default();
        options.shell = Some(Shell::new("cmd.exe".into(), Vec::new()));
        options.drain_on_exit = false;
        let pty = tty::new(
            &options,
            WindowSize {
                num_cols: 80,
                num_lines: 24,
                cell_width: 7,
                cell_height: 16,
            },
            0,
        )
        .expect("conpty spawn");
        let event_loop =
            EventLoop::new(term.clone(), Chan(tx), pty, false, false).expect("event loop");
        let notifier = event_loop.channel();
        event_loop.spawn();

        notifier
            .send(Msg::Input(b"echo PIFLASH_OK\r\n".to_vec().into()))
            .expect("write to pty");
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut saw_output = false;
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Event::Wakeup) => {
                    let locked = term.lock();
                    let grid = locked.grid();
                    for r in 0..24 {
                        let text: String = (0..80).map(|c| grid[Line(r)][Column(c)].c).collect();
                        if text.contains("PIFLASH_OK") {
                            saw_output = true;
                        }
                    }
                    if saw_output {
                        break;
                    }
                }
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
        }
        let _ = notifier.send(Msg::Shutdown);
        assert!(saw_output, "conpty round-trip never produced echo output");
    }
}

// ---------------------------------------------------------------------------
// 终端创建/聚焦/按键（P2 归并自 actions_terminal.rs）
// ---------------------------------------------------------------------------

impl Chat {
    /// 打开一个终端；`cwd_override` 为 psp 项目菜单定向打开时
    /// 的项目目录，None = 当前工作区。v54: 终端在内容区 tab 显示。
    ///
    /// 不按 cwd 复用：每次调用都新开一个终端 tab（同一项目可开任意
    /// 多个，tab 标题按目录名显示、允许重名），不跳回已有终端。
    pub(crate) fn open_terminal(
        &mut self,
        cwd_override: Option<PathBuf>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        // 无项目态且无显式目录：没有可落终端的工作区，不开（防进程 cwd 泄入）
        let term_cwd = match cwd_override {
            Some(c) => c,
            None => match self.ws.cwd.clone() {
                Some(c) => c,
                None => return,
            },
        };
        // 生 tab（PTY + alacritty 事件泵），不激活不聚焦——启动 tab 恢复
        //（pi-flash-weia）共用这条路，回放后按存档统一复位激活位
        if !self.spawn_terminal_tab(term_cwd, window, cx) {
            cx.notify();
            return;
        }
        let tix = self.ws.panel_tabs.len() - 1;
        self.activate_panel_tab(tix, cx);
        self.set_content_view(ContentView::Term);
        let focus = self.ws.terminals[self.ws.terminals.len() - 1].focus.clone();
        window.focus(&focus);
        cx.notify();
    }

    /// 终端 tab 装载核心：spawn PTY + 入 terminals + 带尾挂 `PanelTab::Term`
    ///（open_terminal 与启动恢复共用；**不激活、不切视图、不聚焦**，调用
    /// 方自行接续）。失败落状态条返回 false。
    pub(crate) fn spawn_terminal_tab(
        &mut self,
        term_cwd: PathBuf,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tx) = self.ws.term_events.clone() else {
            return false;
        };
        let (cell_w, line_h) = terminal::measure_cell(window);
        let font_size = terminal::font_size();
        self.ws.term_seq += 1;
        let id = self.ws.term_seq;
        let focus = cx.focus_handle();
        let proxy = terminal::Proxy { tab: id, tx };
        match terminal::spawn_terminal(id, term_cwd, cell_w, line_h, font_size, focus, proxy, None)
        {
            Ok(tab) => {
                self.ws.terminals.push(tab);
                self.ws.active_terminal = Some(self.ws.terminals.len() - 1);
                self.ws.panel_tabs.push(PanelTab::Term(id));
                true
            }
            Err(e) => {
                self.set_status(format!("terminal spawn failed: {e}"), cx);
                false
            }
        }
    }

    /// 021 文件树右键【执行】：脚本进新终端 tab——`cmd /k cd /d <目录> &&
    /// <命令>`，脚本退出留提示符，交互/续敲都行。PTY working_directory 落
    /// 文件目录；命令里的 cd 另一半是 cmd 引号规则护盾（script_exec 模块头）。
    pub(crate) fn open_script_terminal(
        &mut self,
        file: PathBuf,
        plan: crate::services::script_exec::ExecPlan,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dir) = file.parent().map(|p| p.to_path_buf()) else {
            return;
        };
        let Some(tx) = self.ws.term_events.clone() else {
            return;
        };
        let (cell_w, line_h) = terminal::measure_cell(window);
        let font_size = terminal::font_size();
        self.ws.term_seq += 1;
        let id = self.ws.term_seq;
        let focus = cx.focus_handle();
        let proxy = terminal::Proxy { tab: id, tx };
        let shell = Shell::new(
            std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string()),
            vec!["/k".into(), plan.command],
        );
        match terminal::spawn_terminal(
            id,
            dir,
            cell_w,
            line_h,
            font_size,
            focus,
            proxy,
            Some(shell),
        ) {
            Ok(tab) => {
                self.ws.terminals.push(tab);
                self.ws.active_terminal = Some(self.ws.terminals.len() - 1);
                self.ws.panel_tabs.push(PanelTab::Term(id));
                let tix = self.ws.panel_tabs.len() - 1;
                self.activate_panel_tab(tix, cx);
                self.set_content_view(ContentView::Term);
                let focus = self.ws.terminals[self.ws.terminals.len() - 1].focus.clone();
                window.focus(&focus);
                cx.notify();
            }
            Err(e) => {
                self.set_status(format!("terminal spawn failed: {e}"), cx);
                cx.notify();
            }
        }
    }

    /// 内容区**当前显示**的终端在 `terminals` 里的下标（None = 激活的
    /// panel tab 不是终端）。`term_view` 就是按 `active_panel_tab` 渲染的，
    /// 所以这条是「渲染面」的唯一口径。
    pub(crate) fn shown_terminal_index(&self) -> Option<usize> {
        let id = match self
            .ws
            .active_panel_tab
            .and_then(|ix| self.ws.panel_tabs.get(ix))?
        {
            PanelTab::Term(id) => *id,
            _ => return None,
        };
        self.ws.terminals.iter().position(|t| t.id == id)
    }

    /// 当前显示终端的焦点句柄（渲染帧焦点仲裁用）。
    pub(crate) fn shown_terminal_focus(&self) -> Option<gpui::FocusHandle> {
        self.shown_terminal_index()
            .and_then(|ix| self.ws.terminals.get(ix))
            .map(|t| t.focus.clone())
    }

    /// 终端 tab 的选区删除（Backspace/Delete，以及「粘贴替换」先删选区）：
    /// 按 [`deletion_plan`] 算出的键数给 shell 发「左移 + 前向删除」。返回
    /// 是否真的删了（false = 这个选区删不了，调用方按原语义处理）。
    pub(crate) fn term_delete_selection(&mut self, tab_id: usize, cx: &mut Context<Self>) -> bool {
        use alacritty_terminal::event_loop::Msg;
        let Some(tab) = self.ws.terminals.iter_mut().find(|t| t.id == tab_id) else {
            return false;
        };
        let Some(sel) = tab.selection else {
            return false;
        };
        let Some((left, del)) = deletion_plan(&tab.term.lock(), sel) else {
            return false;
        };
        let mode = *tab.term.lock().mode();
        let left_seq = special_key("left", 1, &mode).unwrap_or_else(|| b"[D".to_vec());
        let del_seq = special_key("delete", 1, &mode).unwrap_or_else(|| b"[3~".to_vec());
        let mut bytes = Vec::with_capacity((left + del) * 4);
        for _ in 0..left {
            bytes.extend_from_slice(&left_seq);
        }
        for _ in 0..del {
            bytes.extend_from_slice(&del_seq);
        }
        let _ = tab.pty.send(Msg::Input(bytes.into()));
        tab.selection = None;
        tab.sel_anchor = None;
        cx.notify();
        true
    }

    /// 该终端 tab 的选区文本 → 系统剪贴板。返回是否真的复制了东西
    /// （无选区 = false，调用方据此决定 Ctrl+C 是否回落为 ^C）。
    /// 选区**不**清掉（xterm/浏览器语义：复制后还能看到选中的东西）。
    pub(crate) fn term_copy(&mut self, tab_id: usize, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self.ws.terminals.iter_mut().find(|t| t.id == tab_id) else {
            return false;
        };
        let Some(sel) = tab.selection else {
            return false;
        };
        let text = terminal::selection_text(&tab.term.lock(), sel);
        if text.is_empty() {
            return false;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        true
    }

    /// 系统剪贴板文本 → 该终端 PTY（换行归一 + bracketed paste 交给
    /// [`terminal::paste_bytes`]）。返回是否读到了剪贴板文本。
    pub(crate) fn term_paste(&mut self, tab_id: usize, cx: &mut Context<Self>) -> bool {
        use alacritty_terminal::event_loop::Msg;
        let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) else {
            return false;
        };
        // 有选区 = 粘贴替换：先删选区（删不掉就只清选区），再写入
        let had_sel = self
            .ws
            .terminals
            .iter()
            .find(|t| t.id == tab_id)
            .is_some_and(|t| t.selection.is_some());
        if had_sel && !self.term_delete_selection(tab_id, cx) {
            if let Some(tab) = self.ws.terminals.iter_mut().find(|t| t.id == tab_id) {
                tab.selection = None;
                tab.sel_anchor = None;
                cx.notify();
            }
        }
        let Some(tab) = self.ws.terminals.iter().find(|t| t.id == tab_id) else {
            return false;
        };
        let mode = *tab.term.lock().mode();
        let bytes = terminal::paste_bytes(&text, &mode);
        let _ = tab.pty.send(Msg::Input(bytes.into()));
        true
    }

    /// 全选：可见屏 + 回滚缓冲（选区坐标与 [`terminal::selection_text`]
    /// 的钳位口径一致，-history..screen）。
    pub(crate) fn term_select_all(&mut self, tab_id: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.ws.terminals.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let (screen, total, cols) = {
            let term = tab.term.lock();
            (
                term.grid().screen_lines() as i32,
                term.grid().total_lines() as i32,
                term.grid().columns(),
            )
        };
        let min_line = -(total - screen).max(0);
        tab.sel_anchor = None;
        tab.selection = Some((
            SelPt {
                line: min_line,
                col: 0,
            },
            SelPt {
                line: screen - 1,
                col: cols.saturating_sub(1),
            },
        ));
        cx.notify();
    }

    /// Terminal keyboard input: clipboard shortcuts first (pi-web
    /// attachCustomKeyEventHandler parity: Ctrl+C with selection copies and
    /// never sends ^C; Ctrl+V goes to the PTY, never the browser), plus the
    /// Windows 习惯键（Ctrl+Insert / Shift+Insert / Ctrl+Shift+C / Ctrl+Shift+A），
    /// then the keystroke → escape-sequence table. 目标恒为**当前显示**的终端。
    pub(crate) fn terminal_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        use alacritty_terminal::event_loop::Msg;
        let Some(tix) = self.shown_terminal_index().or(self.ws.active_terminal) else {
            return;
        };
        let Some(tab) = self.ws.terminals.get(tix) else {
            return;
        };
        let tab_id = tab.id;
        let k = &ev.keystroke;
        let ctrl = k.modifiers.control;
        let shift = k.modifiers.shift;
        // 右键菜单开着时 ESC 先收菜单（否则这一下会变成 PTY 的 ^[）
        if k.key == "escape" && self.dlg.term_menu.is_some() {
            self.dlg.term_menu = None;
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let mode = *tab.term.lock().mode();

        // 有选区时 Backspace/Delete = 删掉选中的内容（GUI 直觉）。删不了
        // （跨屏幕行/在滚回区/alt screen）就只把选区清掉，键照旧交给 shell。
        if k.key == "backspace" || k.key == "delete" {
            if self.term_delete_selection(tab_id, cx) {
                cx.stop_propagation();
                return;
            }
            if let Some(tab) = self.ws.terminals.iter_mut().find(|t| t.id == tab_id)
                && tab.selection.take().is_some()
            {
                tab.sel_anchor = None;
                cx.notify();
            }
        }

        // 粘贴：Ctrl+V（pi-web parity）/ Shift+Insert（Windows 习惯）
        if (ctrl && k.key == "v") || (shift && k.key == "insert") {
            self.term_paste(tab_id, cx);
            cx.stop_propagation();
            return;
        }
        // 复制：Ctrl+C 有选区才算复制（无选区落到下面的 ^C 中断）；
        //       Ctrl+Insert / Ctrl+Shift+C 即使无选区也吞掉（不把 ^C 送进 PTY）
        if ctrl && (k.key == "c" || k.key == "insert") {
            let copied = self.term_copy(tab_id, cx);
            if copied || shift || k.key == "insert" {
                cx.stop_propagation();
                return;
            }
        }
        // 全选（Windows Terminal 同键：Ctrl+Shift+A）
        if ctrl && shift && k.key == "a" {
            self.term_select_all(tab_id, cx);
            cx.stop_propagation();
            return;
        }
        let Some(tab) = self.ws.terminals.get(tix) else {
            return;
        };
        // 裸 `exit` 拦截：内嵌终端里 shell 退出只会留下一块「Process
        // exited」死面，没有再进去的路。Enter 时看一眼光标行，命令就是
        // `exit` 就当作用户要关这个终端——直接关掉 tab（同点 tab 的 ×，
        // PTY Shutdown 一并走）。`exit 0` 等带参数的不算，照常执行（出
        // 横幅不崩）；全屏应用不掺和。
        if k.key == "enter" && !mode.contains(TermMode::ALT_SCREEN) {
            let bare_exit = is_bare_exit(&tab.term.lock());
            let tab_id = tab.id;
            if bare_exit
                && let Some(ix) = self
                    .ws
                    .panel_tabs
                    .iter()
                    .position(|t| matches!(t, crate::PanelTab::Term(id) if *id == tab_id))
            {
                self.close_tab_at(ix, cx);
                cx.stop_propagation();
                return;
            }
        }
        if let Some(bytes) = terminal::keystroke_to_pty(k, &mode) {
            let _ = tab.pty.send(Msg::Input(bytes.into()));
            cx.stop_propagation();
        }
    }
}

impl Chat {
    pub(crate) fn on_term_event(
        &mut self,
        tab_id: usize,
        event: alacritty_terminal::event::Event,
        cx: &mut Context<Self>,
    ) {
        use alacritty_terminal::event_loop::Msg;
        let Some(tab) = self.ws.terminals.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        match event {
            alacritty_terminal::event::Event::Wakeup => {
                cx.notify();
            }
            alacritty_terminal::event::Event::PtyWrite(s) => {
                let _ = tab.pty.send(Msg::Input(s.into_bytes().into()));
            }
            alacritty_terminal::event::Event::ClipboardStore(_, text) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
            alacritty_terminal::event::Event::ClipboardLoad(_, fmt) => {
                let text = cx
                    .read_from_clipboard()
                    .and_then(|i| i.text())
                    .unwrap_or_default();
                let _ = tab.pty.send(Msg::Input(fmt(&text).into_bytes().into()));
            }
            alacritty_terminal::event::Event::ColorRequest(i, fmt) => {
                let _ = tab
                    .pty
                    .send(Msg::Input(fmt(terminal::term_rgb(i)).into_bytes().into()));
            }
            alacritty_terminal::event::Event::TextAreaSizeRequest(fmt) => {
                let ws = alacritty_terminal::event::WindowSize {
                    num_cols: tab.cols as u16,
                    num_lines: tab.rows as u16,
                    cell_width: tab.cell_w as u16,
                    cell_height: tab.line_h as u16,
                };
                let _ = tab.pty.send(Msg::Input(fmt(ws).into_bytes().into()));
            }
            alacritty_terminal::event::Event::ChildExit(status) => {
                tab.status = TermStatus::Exited(status.code());
                cx.notify();
            }
            alacritty_terminal::event::Event::Exit => {
                if tab.status == TermStatus::Ready {
                    tab.status = TermStatus::Exited(None);
                }
                cx.notify();
            }
            _ => {}
        }
    }
}
