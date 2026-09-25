//! Built-in terminal — pi-web TerminalPanel parity on an alacritty_terminal
//! core (zed crates/terminal patterns adapted to the crates.io 0.26 API).
//!
//! pi-web surface: right-panel terminal tabs (TabBar), 38px header with status
//! dot + cwd + Restart, exit banner, fixed dark xterm surface (#111318) in
//! every theme, Consolas 13px / lineHeight 1.25, scrollback 8000, cursor #60a5fa.
//! pi-web spawns node-pty (ComSpec on Windows); here `tty::new` provides
//! ConPTY directly. Reconnect (SSE resume) has no in-process equivalent and is
//! intentionally dropped; mouse-reporting apps get wheel-as-arrows only.

use std::path::PathBuf;
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty::{self, Shell};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};
use futures::channel::mpsc::UnboundedSender;
use gpui::{
    px, Font, FontStyle, FontWeight, Hsla, Keystroke, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent, SharedString, UnderlineStyle,
    WeakEntity, Window,
};

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
/// xterm `fontSize: 13, lineHeight: 1.25` on --font-mono
pub const FONT_FAMILY: &str = "Consolas";
pub const FONT_SIZE: f32 = 13.;
pub const LINE_HEIGHT: f32 = FONT_SIZE * 1.25;
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
    Failed(String),
}

pub struct TerminalTab {
    pub id: usize,
    pub cwd: PathBuf,
    /// tab label: basename of cwd (pi-web `getFileName(tab.cwd)`)
    pub title: String,
    pub status: TermStatus,
    pub term: Arc<FairMutex<Term<Proxy>>>,
    pub pty: EventLoopSender,
    pub focus: gpui::FocusHandle,
    pub cols: usize,
    pub rows: usize,
    pub cell_w: f32,
    pub line_h: f32,
    /// grid-coordinate selection (Line includes display offset)
    pub selection: Option<(SelPt, SelPt)>,
    /// drag anchor while a selection is in progress
    pub sel_anchor: Option<SelPt>,
}

#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub struct SelPt {
    pub line: i32,
    pub col: usize,
}

/// Spawn the PTY + alacritty event loop for one tab.
/// `cell_w`/`line_h` come from measuring the mono font in the opening window.
#[allow(clippy::too_many_arguments)]
pub fn spawn_terminal(
    id: usize,
    cwd: PathBuf,
    cell_w: f32,
    line_h: f32,
    focus: gpui::FocusHandle,
    proxy: Proxy,
) -> Result<TerminalTab, String> {
    let dims = CellDims { cols: DEFAULT_COLS, rows: DEFAULT_ROWS };
    let config = Config { scrolling_history: SCROLLBACK, ..Config::default() };
    let term = Arc::new(FairMutex::new(Term::new(config, &dims, proxy.clone())));

    let mut options = tty::Options::default();
    // terminal-manager.ts: win32 -> ComSpec ?? cmd.exe with no args
    let shell = std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string());
    options.shell = Some(Shell::new(shell, Vec::new()));
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
    let event_loop = EventLoop::new(term.clone(), proxy, pty, true, false)
        .map_err(|e| e.to_string())?;
    let pty_tx = event_loop.channel();
    event_loop.spawn();

    Ok(TerminalTab {
        id,
        cwd: cwd.clone(),
        title: cwd
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| cwd.to_string_lossy().to_string()),
        status: TermStatus::Ready,
        term,
        pty: pty_tx,
        focus,
        cols: DEFAULT_COLS,
        rows: DEFAULT_ROWS,
        cell_w,
        line_h,
        selection: None,
        sel_anchor: None,
    })
}

/// terminal-manager.ts shellEnvironment: inherit everything, then pin TERM /
/// COLORTERM and a UTF-8 LANG when none is set (Windows codepage fix).
fn shell_env() -> std::collections::HashMap<String, String> {
    let mut env: std::collections::HashMap<String, String> = std::env::vars().collect();
    env.insert("TERM".into(), "xterm-256color".into());
    env.insert("COLORTERM".into(), "truecolor".into());
    let has_lang = ["LANG", "LC_ALL", "LC_CTYPE"].iter().any(|k| env.contains_key(*k));
    if !has_lang {
        env.insert("LANG".into(), "C.UTF-8".into());
    }
    env
}

/// 256-color palette (+256 fg / 257 bg / 258 cursor) from the xterm theme.
pub fn palette256(i: usize) -> u32 {
    match i {
        0..=15 => ANSI16[i],
        16..=231 => {
            let i = i - 16;
            let levels = [0x00u32, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
            let r = levels[i / 36];
            let g = levels[(i % 36) / 6];
            let b = levels[i % 6];
            (r << 16) | (g << 8) | b
        }
        232..=255 => {
            let v = (8 + (i - 232) * 10) as u32;
            (v << 16) | (v << 8) | v
        }
        256 => TERM_FG,
        257 => TERM_BG,
        258 => TERM_CURSOR,
        _ => TERM_FG,
    }
}

/// ColorRequest answer: palette entry as `Rgb`.
pub fn term_rgb(i: usize) -> Rgb {
    let v = palette256(i);
    Rgb { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8 }
}

fn named_index(n: NamedColor) -> usize {
    use NamedColor::*;
    match n {
        Black => 0,
        Red => 1,
        Green => 2,
        Yellow => 3,
        Blue => 4,
        Magenta => 5,
        Cyan => 6,
        White => 7,
        BrightBlack => 8,
        BrightRed => 9,
        BrightGreen => 10,
        BrightYellow => 11,
        BrightBlue => 12,
        BrightMagenta => 13,
        BrightCyan => 14,
        BrightWhite => 15,
        Foreground | BrightForeground | DimForeground => 256,
        Background => 257,
        Cursor => 258,
        DimBlack => 0,
        DimRed => 1,
        DimGreen => 2,
        DimYellow => 3,
        DimBlue => 4,
        DimMagenta => 5,
        DimCyan => 6,
        DimWhite => 7,
        _ => 256,
    }
}

fn color_to_rgb(c: &Color) -> u32 {
    match c {
        Color::Spec(rgb) => ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32,
        Color::Indexed(i) => palette256(*i as usize),
        Color::Named(n) => palette256(named_index(*n)),
    }
}

// ---------------------------------------------------------------------------
// grid snapshot: (cells, styles) per visible row
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
pub struct CellStyle {
    fg: Hsla,
    bg: Option<Hsla>,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
}

pub struct SnapRow {
    pub text: String,
    pub styles: Vec<CellStyle>,
}

/// Cell is part of the selection range (row-wise band between normalized pts).
fn cell_selected(sel: (SelPt, SelPt), line: i32, col: usize) -> bool {
    let (a, b) = if sel.0 <= sel.1 { sel } else { (sel.1, sel.0) };
    if line < a.line || line > b.line {
        return false;
    }
    if a.line == b.line {
        return col >= a.col && col <= b.col;
    }
    if line == a.line {
        col >= a.col
    } else if line == b.line {
        col <= b.col
    } else {
        true
    }
}

/// Snapshot the visible grid (rows tall) into styled rows.
/// The cursor is folded in as a cell style (block = bg swap, unfocused = dim).
pub fn snapshot(
    term: &Term<Proxy>,
    rows: usize,
    selection: Option<(SelPt, SelPt)>,
    focused: bool,
) -> Vec<SnapRow> {
    let grid = term.grid();
    let offset = grid.display_offset() as i32;
    let cols = grid.columns();
    let content = term.renderable_content();
    let cursor_line = content.cursor.point.line.0;
    let cursor_col = content.cursor.point.column.0;
    let cursor_shape = content.cursor.shape;

    let mut out = Vec::with_capacity(rows);
    for r in 0..rows {
        let line = r as i32 - offset;
        let row = &grid[Line(line)];
        let mut text = String::new();
        let mut styles: Vec<CellStyle> = Vec::new();
        for c in 0..cols {
            let cell = &row[Column(c)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue; // consumed by the wide char itself
            }
            let mut ch = if cell.flags.contains(Flags::HIDDEN) {
                ' '
            } else {
                cell.c
            };
            if ch == '\0' || ch == '\n' || ch == '\r' {
                ch = ' ';
            }

            // resolve fg (bold -> bright variant for indexed 0..8, xterm parity)
            let mut fg_rgb = color_to_rgb(&cell.fg);
            if cell.flags.contains(Flags::BOLD) {
                if let Color::Indexed(i) = cell.fg {
                    if i < 8 {
                        fg_rgb = palette256(i as usize + 8);
                    }
                }
            }
            let bg_rgb = color_to_rgb(&cell.bg);
            let (mut fg_rgb, mut bg_rgb) = if cell.flags.contains(Flags::INVERSE) {
                (bg_rgb, fg_rgb)
            } else {
                (fg_rgb, bg_rgb)
            };

            let selected = selection.is_some_and(|s| cell_selected(s, line, c));
            if selected {
                bg_rgb = TERM_SEL;
            }

            // cursor: block swaps fg/bg; hollow (unfocused) is a dim fill
            let on_cursor = line == cursor_line
                && c == cursor_col
                && cursor_shape != CursorShape::Hidden
                && !cell.flags.contains(Flags::WIDE_CHAR_SPACER);
            let mut cursor_underline = false;
            if on_cursor {
                if focused {
                    match cursor_shape {
                        CursorShape::Block => {
                            bg_rgb = TERM_CURSOR;
                            fg_rgb = TERM_BG;
                        }
                        _ => cursor_underline = true,
                    }
                } else {
                    bg_rgb = TERM_CURSOR;
                }
            }

            let mut fg = hsl(fg_rgb);
            if cell.flags.contains(Flags::DIM) && !on_cursor {
                fg = hsl_a(fg_rgb, 0.55);
            }
            let bg: Option<Hsla> = if on_cursor && !focused {
                Some(hsl_a(TERM_CURSOR, 0.35))
            } else if bg_rgb == TERM_BG && !selected && !on_cursor {
                None
            } else {
                Some(hsl(bg_rgb))
            };
            if on_cursor && focused && cursor_underline {
                fg = hsl(TERM_CURSOR);
            }

            let style = CellStyle {
                fg,
                bg,
                bold: cell.flags.contains(Flags::BOLD),
                italic: cell.flags.contains(Flags::ITALIC),
                underline: cell.flags.contains(Flags::ALL_UNDERLINES) || cursor_underline,
                strike: cell.flags.contains(Flags::STRIKEOUT),
            };
            text.push(ch);
            styles.push(style);
        }
        out.push(SnapRow { text, styles });
    }
    out
}

/// Extract the selected text from the grid (xterm `getString` parity: trailing
/// whitespace trimmed, lines joined with \n, wide spacers skipped).
pub fn selection_text(term: &Term<Proxy>, sel: (SelPt, SelPt)) -> String {
    let (a, b) = if sel.0 <= sel.1 { sel } else { (sel.1, sel.0) };
    let grid = term.grid();
    let screen = grid.screen_lines() as i32;
    let min_line = -(grid.total_lines() as i32 - screen);
    let a = SelPt { line: a.line.clamp(min_line, screen - 1), ..a };
    let b = SelPt { line: b.line.clamp(min_line, screen - 1), ..b };
    let mut lines = Vec::new();
    for line in a.line..=b.line {
        let row = &grid[Line(line)];
        let (c0, c1) = if a.line == b.line {
            (a.col, b.col)
        } else if line == a.line {
            (a.col, grid.columns() - 1)
        } else if line == b.line {
            (0, b.col)
        } else {
            (0, grid.columns() - 1)
        };
        let mut s = String::new();
        for c in c0..=c1.min(grid.columns() - 1) {
            let cell = &row[Column(c)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            s.push(cell.c);
        }
        lines.push(s.trim_end().to_string());
    }
    lines.join("\n")
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
            weak,
            interactivity: gpui::Interactivity::new(),
        }
    }

    /// Mouse position -> grid point (grid Line includes display offset).
    fn cell_at(&self, bounds: gpui::Bounds<Pixels>, pos: Point<Pixels>) -> SelPt {
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
        SelPt { line: (row - offset).clamp(-history, screen - 1), col }
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
        // fit: cols/rows from the laid-out area (FitAddon parity), resize the
        // term + PTY once per actual change
        let avail_w = f32::from(bounds.size.width - px(PAD_L + PAD_R)).max(self.cell_w);
        let avail_h = f32::from(bounds.size.height - px(PAD_T + PAD_B)).max(self.line_h);
        let cols = ((avail_w / self.cell_w) as usize).max(2);
        let rows = ((avail_h / self.line_h) as usize).max(2);
        if cols != self.cols || rows != self.rows {
            let tab_id = self.tab_id;
            let pty = self.pty.clone();
            let (cell_w, line_h) = (self.cell_w, self.line_h);
            let _ = self.weak.update(cx, |chat, cx| {
                if let Some(tab) = chat.terminals.iter_mut().find(|t| t.id == tab_id) {
                    tab.cols = cols;
                    tab.rows = rows;
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
        let shared = self.clone_shared();
        self.interactivity.paint(
            global_id,
            inspector_id,
            bounds,
            hitbox.as_ref(),
            window,
            cx,
            move |_, window, cx| {
                // snapshot the grid while locked, then paint styled runs
                let rows = snapshot(&term.lock(), grid_rows, selection, focused);
                let font_size = px(FONT_SIZE);
                let line_h = px(line_h);
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
                    let _ = line.paint(origin, line_h, window, cx);
                }

                // mouse: selection drag + wheel scroll (hit-tested manually)
                let this = shared.clone();
                window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
                    if phase != gpui::DispatchPhase::Bubble
                        || ev.button != MouseButton::Left
                        || !bounds.contains(&ev.position)
                    {
                        return;
                    }
                    this.start_selection(ev.position, bounds, window, cx);
                });
                let this = shared.clone();
                window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
                    if phase != gpui::DispatchPhase::Bubble
                        || ev.pressed_button != Some(MouseButton::Left)
                        || !bounds.contains(&ev.position)
                    {
                        return;
                    }
                    this.drag_selection(ev.position, bounds, window, cx);
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
    weak: WeakEntity<Chat>,
    cell_w: f32,
    line_h: f32,
}

impl TermShared {
    /// Mouse down: record the drag anchor and clear any previous selection
    /// (xterm clears on a plain click).
    fn start_selection(
        &self,
        pos: Point<Pixels>,
        bounds: gpui::Bounds<Pixels>,
        _window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let anchor = self.cell_at_shared(bounds, pos);
        let _ = self.weak.update(cx, |chat, cx| {
            if let Some(tab) = chat.terminals.iter_mut().find(|t| t.id == self.tab_id) {
                tab.sel_anchor = Some(anchor);
                tab.selection = None;
                cx.notify();
            }
        });
    }

    fn drag_selection(
        &self,
        pos: Point<Pixels>,
        bounds: gpui::Bounds<Pixels>,
        _window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let head = self.cell_at_shared(bounds, pos);
        let _ = self.weak.update(cx, |chat, cx| {
            if let Some(tab) = chat.terminals.iter_mut().find(|t| t.id == self.tab_id) {
                if let Some(anchor) = tab.sel_anchor {
                    let sel = if anchor == head { None } else { Some((anchor, head)) };
                    if tab.selection != sel {
                        tab.selection = sel;
                        cx.notify();
                    }
                }
            }
        });
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
        SelPt { line: (row - offset).clamp(-history, screen - 1), col }
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
        features: gpui::FontFeatures::default(),
        fallbacks: None,
        weight: font_weight(style.bold),
        style: font_style(style.italic),
    }
}

/// Measure the mono cell metrics (advance width of `M`, line height) in px.
pub fn measure_cell(window: &Window) -> (f32, f32) {
    let font = Font {
        family: FONT_FAMILY.into(),
        features: gpui::FontFeatures::default(),
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
        px(FONT_SIZE),
        &[run],
        None,
    );
    let cw = (f32::from(line.width) / 8.).max(1.);
    (cw, LINE_HEIGHT)
}

// ---------------------------------------------------------------------------
// key translation (zed mappings/keys.rs subset)
// ---------------------------------------------------------------------------

/// Translate a gpui keystroke into PTY bytes. Returns None when the keystroke
/// is not terminal input (e.g. plain modifier presses are filtered upstream).
pub fn keystroke_to_pty(k: &Keystroke, mode: &TermMode) -> Option<Vec<u8>> {
    let ctrl = k.modifiers.control;
    let alt = k.modifiers.alt;
    let shift = k.modifiers.shift;
    let key = k.key.as_str();
    let alt_prefix: &[u8] = if alt { b"\x1b" } else { b"" };

    // ctrl combos -> control characters
    if ctrl {
        if let Some(b) = ctrl_byte(key) {
            let mut v = alt_prefix.to_vec();
            v.push(b);
            return Some(v);
        }
        // ctrl+arrows etc. fall through to the modified CSI form below
    }

    // modifier mask for CSI 1;m{A} / CSI n;{m}~  (1 + shift + alt*2 + ctrl*4)
    let m = 1 + shift as i32 + 2 * alt as i32 + 4 * ctrl as i32;

    match key {
        "enter" => return Some(between(alt_prefix, b"\r")),
        "tab" => {
            return Some(if shift { b"\x1b[Z".to_vec() } else { b"\t".to_vec() })
        }
        "backspace" => return Some(between(alt_prefix, b"\x7f")),
        "escape" => return Some(between(alt_prefix, b"\x1b")),
        "space" => {
            if ctrl {
                return Some(b"\x00".to_vec());
            }
            return Some(between(alt_prefix, b" "));
        }
        _ => {}
    }

    // arrows / home / end / page / insert / delete / F-keys
    if let Some(seq) = special_key(key, m, mode) {
        return Some(seq);
    }

    // printable input: key_char carries layout/shift-correct characters
    if !ctrl {
        if let Some(s) = &k.key_char {
            if !s.is_empty() {
                let mut v = alt_prefix.to_vec();
                v.extend_from_slice(s.as_bytes());
                return Some(v);
            }
        }
        if key.len() == 1 && key.as_bytes()[0].is_ascii_graphic() {
            let mut v = alt_prefix.to_vec();
            v.push(key.as_bytes()[0]);
            return Some(v);
        }
    }
    None
}

fn between(prefix: &[u8], rest: &[u8]) -> Vec<u8> {
    let mut v = prefix.to_vec();
    v.extend_from_slice(rest);
    v
}

fn ctrl_byte(key: &str) -> Option<u8> {
    let b = match key {
        "space" | "@" => 0x00,
        "a" => 0x01,
        "b" => 0x02,
        "c" => 0x03,
        "d" => 0x04,
        "e" => 0x05,
        "f" => 0x06,
        "g" => 0x07,
        "h" => 0x08,
        "i" => 0x09,
        "j" => 0x0a,
        "k" => 0x0b,
        "l" => 0x0c,
        "m" => 0x0d,
        "n" => 0x0e,
        "o" => 0x0f,
        "p" => 0x10,
        "q" => 0x11,
        "r" => 0x12,
        "s" => 0x13,
        "t" => 0x14,
        "u" => 0x15,
        "v" => 0x16,
        "w" => 0x17,
        "x" => 0x18,
        "y" => 0x19,
        "z" => 0x1a,
        "[" => 0x1b,
        "\\" => 0x1c,
        "]" => 0x1d,
        "^" => 0x1e,
        "_" | "-" => 0x1f,
        _ => return None,
    };
    Some(b)
}

fn special_key(key: &str, m: i32, mode: &TermMode) -> Option<Vec<u8>> {
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    let seq: Vec<u8> = match key {
        "left" | "right" | "up" | "down" => {
            let c = match key {
                "left" => 'D',
                "right" => 'C',
                "up" => 'A',
                _ => 'B',
            };
            if m > 1 {
                format!("\x1b[1;{m}{c}").into_bytes()
            } else if app_cursor {
                format!("\x1bO{c}").into_bytes()
            } else {
                format!("\x1b[{c}").into_bytes()
            }
        }
        "home" | "end" => {
            let c = if key == "home" { 'H' } else { 'F' };
            if m > 1 {
                format!("\x1b[1;{m}{c}").into_bytes()
            } else if app_cursor {
                format!("\x1bO{c}").into_bytes()
            } else {
                format!("\x1b[{c}").into_bytes()
            }
        }
        "insert" => tilde(2, m),
        "delete" => tilde(3, m),
        "pageup" => tilde(5, m),
        "pagedown" => tilde(6, m),
        "f1" => b"\x1bOP".to_vec(),
        "f2" => b"\x1bOQ".to_vec(),
        "f3" => b"\x1bOR".to_vec(),
        "f4" => b"\x1bOS".to_vec(),
        "f5" => tilde(15, m),
        "f6" => tilde(17, m),
        "f7" => tilde(18, m),
        "f8" => tilde(19, m),
        "f9" => tilde(20, m),
        "f10" => tilde(21, m),
        "f11" => tilde(23, m),
        "f12" => tilde(24, m),
        _ => return None,
    };
    Some(seq)
}

fn tilde(n: u32, m: i32) -> Vec<u8> {
    if m > 1 {
        format!("\x1b[{n};{m}~").into_bytes()
    } else {
        format!("\x1b[{n}~").into_bytes()
    }
}

/// Paste text: bracketed when the app enabled it, CR-normalized otherwise
/// (zed `Terminal::paste` parity).
pub fn paste_bytes(text: &str, mode: &TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        let mut v = Vec::with_capacity(text.len() + 12);
        v.extend_from_slice(b"\x1b[200~");
        v.extend_from_slice(text.replace('\x1b', "").as_bytes());
        v.extend_from_slice(b"\x1b[201~");
        v
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::Processor;

    fn test_term(cols: usize, rows: usize, text: &[u8]) -> Term<Proxy> {
        let (tx, _rx) = futures::channel::mpsc::unbounded::<(usize, Event)>();
        let proxy = Proxy { tab: 0, tx };
        let mut term = Term::new(Config::default(), &CellDims { cols, rows }, proxy);
        Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::new()
            .advance(&mut term, text);
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
        let rows = snapshot(&term, 2, None, true);
        assert_eq!(rows[0].text, "hello     ");
        assert_eq!(rows[1].text, " ".repeat(10));
        // default fg/bg: no background run needed
        assert!(rows[0].styles[0].bg.is_none());
        assert_eq!(rows[0].styles[0].fg, hsl(TERM_FG));
    }

    #[test]
    fn snapshot_sgr_colors_and_inverse() {
        // red text, then inverse space
        let term = test_term(10, 2, b"\x1b[31mR\x1b[7m \x1b[0m ");
        let rows = snapshot(&term, 2, None, true);
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
        let rows = snapshot(&term, 2, Some(sel), true);
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
            modifiers: gpui::Modifiers { control: ctrl, alt, shift, ..Default::default() },
            key: key.into(),
            key_char: None,
        };
        assert_eq!(keystroke_to_pty(&k("enter", false, false, false), &plain), Some(b"\r".to_vec()));
        assert_eq!(keystroke_to_pty(&k("backspace", false, false, false), &plain), Some(vec![0x7f]));
        assert_eq!(keystroke_to_pty(&k("escape", false, false, false), &plain), Some(vec![0x1b]));
        assert_eq!(keystroke_to_pty(&k("tab", false, false, true), &plain), Some(b"\x1b[Z".to_vec()));
        assert_eq!(keystroke_to_pty(&k("up", false, false, false), &plain), Some(b"\x1b[A".to_vec()));
        assert_eq!(keystroke_to_pty(&k("up", false, false, false), &app), Some(b"\x1bOA".to_vec()));
        assert_eq!(keystroke_to_pty(&k("left", true, false, false), &plain), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(keystroke_to_pty(&k("pagedown", false, false, false), &plain), Some(b"\x1b[6~".to_vec()));
        assert_eq!(keystroke_to_pty(&k("f5", false, false, false), &plain), Some(b"\x1b[15~".to_vec()));
        assert_eq!(keystroke_to_pty(&k("d", true, false, false), &plain), Some(vec![0x04]));
        assert_eq!(keystroke_to_pty(&k("a", true, false, false), &plain), Some(vec![0x01]));
        // alt prefixing
        assert_eq!(keystroke_to_pty(&k("x", false, true, false), &plain), Some(b"\x1bx".to_vec()));
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
            Config { scrolling_history: 100, ..Config::default() },
            &CellDims { cols: 80, rows: 24 },
            Chan(tx.clone()),
        )));
        let mut options = tty::Options::default();
        options.shell = Some(Shell::new("cmd.exe".into(), Vec::new()));
        options.drain_on_exit = false;
        let pty = tty::new(
            &options,
            WindowSize { num_cols: 80, num_lines: 24, cell_width: 7, cell_height: 16 },
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
                        let text: String =
                            (0..80).map(|c| grid[Line(r)][Column(c)].c).collect();
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
