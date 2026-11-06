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

mod element;
mod host;

pub(crate) use element::*;

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

#[cfg(test)]
mod tests;
