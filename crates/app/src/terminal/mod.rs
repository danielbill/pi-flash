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
