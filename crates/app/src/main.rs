//! pi-flash — desktop shell for the pi coding agent.
//!
//! Component-by-component translation of pi-web (see PORT_PLAN.md). Layout
//! values (sizes, colors, spacing) come from pi-web sources: globals.css
//! theme tokens, panel-layout.ts, MessageView/ChatInput/AppShell structures.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use futures::StreamExt;
use gpui::{
    App, Application, Context, FocusHandle, Focusable, KeyDownEvent, ListAlignment, ListState,
    MouseButton, ParentElement,
    Render, SharedString, Styled, WindowOptions, div, prelude::*, px, rgb,
};
use pi_link::protocol::Command;
use pi_link::sessions::{SessionInfo, list_sessions, list_sessions_for_cwd, read_tail_messages};

mod agent_session;
mod actions_dialogs;
mod actions_menu;
mod actions_panels;
mod actions_rename;
mod actions_runtime;
mod actions_sessions;
mod actions_terminal;
mod appearance;
mod assets;
mod dialogs;
mod ext_ui;
mod function_panel;
mod pages;
mod i18n;
mod markdown;
mod models_config;
mod theme;
mod services;
mod session;
mod settings;
mod status_bar;
mod titlebar;
mod terminal;
mod ui;
pub(crate) use ui::editor_input::EditorInputElement;
use i18n::tr;
use models_config::EnabledState;
use theme::theme as T;
use services::format::*;
use services::git::*;
use services::workspace::*;
use session::messages::{Msg, msgs_from_tail};
use session::runtime::SessionRuntime;
use terminal::{TermStatus, TerminalTab};
use ui::TextInput;
use ui::icon;
use ext_ui::render_ext_dialog;

static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
static PERF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// ---------------------------------------------------------------------------
// state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Dialog {
    ModelSelect { input: gpui::Entity<TextInput> },
    BranchTree,
    ProjectSelect,
    GitDiff { path: PathBuf, patch: String },
    FilePreview { path: PathBuf },
    SessionSearch { input: gpui::Entity<TextInput> },
}

/// Full-page state (005/011/012): Welcome shows until the project/session
/// list has loaded; the session view carries the newSession hero (012)
/// whenever no session is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Welcome,
    Session,
}

/// functionPanel active view (015/018): mutually exclusive, switched from
/// the bottom control bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DockPanel {
    Sessions,
    Files,
    Git,
    Terminal,
}

impl DockPanel {
    fn as_str(&self) -> &'static str {
        match self {
            DockPanel::Sessions => "sessions",
            DockPanel::Files => "files",
            DockPanel::Git => "git",
            DockPanel::Terminal => "terminal",
        }
    }

    fn parse(s: &str) -> DockPanel {
        match s {
            "files" => DockPanel::Files,
            "git" => DockPanel::Git,
            "terminal" => DockPanel::Terminal,
            _ => DockPanel::Sessions,
        }
    }
}

#[derive(Debug, Clone)]
struct AttachedImage {
    name: String,
    data_b64: String,
    mime: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MenuKind {
    Slash,
    At,
}

#[derive(Debug, Clone)]
struct MenuItem {
    insert: String,
}

struct Chat {
    focus: FocusHandle,
    dialog_focus: FocusHandle,
    dialog: Option<Dialog>,
    page: Page,
    dock_panel: DockPanel,
    dock_right: bool,
    input: String,
    pending_images: Vec<AttachedImage>,
    history: Vec<String>,
    history_ix: Option<usize>,
    session_tail_cache: std::collections::HashMap<PathBuf, Vec<Msg>>,
    sessions: Vec<SessionInfo>,
    sessions_list: ListState,
    cwd: PathBuf,
    branch: String,
    available_models: Vec<pi_link::protocol::ModelInfo>,
    runtimes: std::collections::HashMap<String, gpui::Entity<session::runtime::SessionRuntime>>,
    active_key: String,
    draft_seq: usize,
    menu_ix: usize,
    sidebar_sessions_frac: f32,
    term_events: Option<futures::channel::mpsc::UnboundedSender<(usize, alacritty_terminal::event::Event)>>,
    op_tx: Option<futures::channel::mpsc::UnboundedSender<String>>,
    // editor view state
    ime_marked: Option<std::ops::Range<usize>>,
    caret_on: bool,
    input_focused: bool,
    // sidebar view state
    hovered_session: Option<usize>,
    renaming: Option<PathBuf>,
    rename_input: Option<gpui::Entity<TextInput>>,
    confirm_delete: Option<PathBuf>,
    search_open: bool,
    search_input: gpui::Entity<TextInput>,
    sessions_list_count: usize,
    // session content search (013): dialog query input + background results
    search_hits: Vec<pi_link::sessions::SearchHit>,
    search_truncated: bool,
    search_running: bool,
    search_needle: String,
    search_gen: u64,
    pending_locate: Option<(PathBuf, Option<i64>, String)>,
    // shell surfaces
    pill_menu: Option<PillMenu>,
    top_panel: Option<TopPanel>,
    // git panel
    git_files: Vec<GitFile>,
    git_add_del: (u64, u64),
    git_tab: function_panel::git_panel::GitTab,
    git_log: Vec<GitCommit>,
    git_error: Option<String>,
    git_commit_input: gpui::Entity<TextInput>,
    // workspace / project files
    project_files: Vec<String>,
    expanded_dirs: HashSet<PathBuf>,
    file_cache: std::collections::HashMap<PathBuf, FileTab>,
    // terminals (dock view)
    terminals: Vec<TerminalTab>,
    active_terminal: Option<usize>,
    term_seq: usize,
    panel_tabs: Vec<PanelTab>,
    active_panel_tab: Option<usize>,
    // settings panel data
    mc_patterns: Option<Vec<String>>,
    mc_state: EnabledState,
    mc_creds: Vec<(String, pi_link::config::CredentialKind)>,
    mc_project_scope: bool,
    mc_skills: Vec<pi_link::skills::SkillEntry>,
    mc_pkgs_global: Vec<serde_json::Value>,
    mc_pkgs_project: Vec<serde_json::Value>,
    mc_default_tools: Option<Vec<String>>,
    // extension UI surface (active session)
    ext_status: Vec<(String, String)>,
    ext_widgets: Vec<(String, Vec<String>, bool)>,
    ext_dialog: Option<pi_link::protocol::ExtensionUiRequest>,
    ext_input: gpui::Entity<TextInput>,
    ext_notice: Option<(String, u8)>,
    // subagent test runs (settings panel; G+ moves per-session)
    sa_profiles: Vec<pi_link::subagents::SubagentProfile>,
    sa_settings: pi_link::subagents::SubagentSettings,
    sa_runs: Vec<SubagentRun>,
    sa_run_seq: usize,
    sound_on: bool,
    settings: Option<gpui::Entity<settings::SettingsPanel>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TopPanel {
    System,
    Tools,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PillMenu {
    Thinking,
    Tools,
}

/// One right-panel tab: a file viewer or a terminal session.
#[derive(Debug, Clone, PartialEq)]
enum PanelTab {
    Term(usize),
}

/// Cached file content for a viewer tab (read once on open).
struct FileTab {
    content: String,
}

/// One live subagent run (child RPC session spawned with profile flags).
struct SubagentRun {
    id: usize,
    profile: String,
    status: u8,
    last_text: String,
    session: Option<pi_link::client::PiSession>,
}

impl Chat {
    fn new(cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let dialog_focus = cx.focus_handle();
        let cwd = std::env::var("PI_FLASH_CWD")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        // startup restore decides workspace + last session BEFORE the first
        // spawn — exactly one pi process (ARCHITECTURE.md §4, was two)
        let last_ws = get_last_workspace();
        let target_ws = last_ws.unwrap_or_else(|| cwd.to_string_lossy().to_string());
        let cwd = if !same_ws(&target_ws, &cwd.to_string_lossy()) {
            let ws_path = PathBuf::from(&target_ws);
            if ws_path.is_dir() { ws_path } else { cwd }
        } else {
            cwd
        };
        let branch = read_branch(&cwd);
        let last_open = get_last_open(&cwd.to_string_lossy())
            .map(PathBuf::from)
            .filter(|p| p.exists());
        let sessions_list = ListState::new(0, ListAlignment::Top, px(500.));
        let dock_state = get_dock_state();

        let mut chat = Self {
            focus,
            dialog_focus,
            dialog: None,
            input: String::new(),
            pending_images: Vec::new(),
            session_tail_cache: std::collections::HashMap::new(),
            // skeleton first (ARCHITECTURE.md §4): the list fills in the
            // background task below; the page flips Welcome -> Session then
            sessions: Vec::new(),
            search_hits: Vec::new(),
            search_truncated: false,
            search_running: false,
            search_needle: String::new(),
            search_gen: 0,
            pending_locate: None,
            page: Page::Welcome,
            dock_panel: dock_state
                .as_ref()
                .map(|d| DockPanel::parse(&d.panel))
                .unwrap_or(DockPanel::Sessions),
            dock_right: dock_state
                .as_ref()
                .map(|d| d.position == "right")
                .unwrap_or(false),
            sessions_list,
            cwd: cwd.clone(),
            branch,
            runtimes: std::collections::HashMap::new(),
            active_key: String::new(),
            draft_seq: 0,
            available_models: Vec::new(),
            expanded_dirs: HashSet::new(),
            git_files: Vec::new(),
            git_tab: function_panel::git_panel::GitTab::Changes,
            git_log: Vec::new(),
            git_error: None,
            git_commit_input: {
                let input = cx.new(|cx| TextInput::new(cx).placeholder(tr("提交信息（提交已暂存更改）")));
                input
            },
            git_add_del: (0, 0),
            project_files: Vec::new(),
            history: Vec::new(),
            history_ix: None,
            menu_ix: 0,
            hovered_session: None,
            terminals: Vec::new(),
            active_terminal: None,
            term_seq: 0,
            term_events: None,
            mc_patterns: None,
            mc_state: EnabledState::default(),
            mc_creds: Vec::new(),
            mc_project_scope: false,
            mc_skills: Vec::new(),
            mc_pkgs_global: Vec::new(),
            mc_pkgs_project: Vec::new(),
            mc_default_tools: None,
            op_tx: None,
            ext_status: Vec::new(),
            ext_widgets: Vec::new(),
            ext_dialog: None,
            ext_input: cx.new(|cx| TextInput::new(cx)),
            ext_notice: None,
            sa_profiles: Vec::new(),
            sa_settings: pi_link::subagents::SubagentSettings::default(),
            sa_runs: Vec::new(),
            sa_run_seq: 0,
            sidebar_sessions_frac: 0.5,
            panel_tabs: Vec::new(),
            active_panel_tab: None,
            file_cache: std::collections::HashMap::new(),
            caret_on: true,
            input_focused: false,
            pill_menu: None,
            sound_on: load_sound_pref(),
            ime_marked: None,
            top_panel: None,
            settings: None,
            renaming: None,
            rename_input: None,
            search_open: false,
            search_input: cx
                .new(|cx| TextInput::new(cx).placeholder(tr("搜索会话..."))),
            sessions_list_count: 0,
            confirm_delete: None,
        };
        // wire input callbacks that need the root entity handle
        let weak_self = cx.entity().downgrade();
        chat.search_input.update(cx, |ti, _| {
            ti.set_on_change(Box::new(move |_, cx| {
                // typing refilters the sessions list (owner repaint)
                let _ = weak_self.update(cx, |_, cx| cx.notify());
            }));
        });
        let weak_ext = cx.entity().downgrade();
        chat.ext_input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |v, cx| {
                let _ = weak_ext.update(cx, |c, cx| {
                    c.ext_respond(Some(v.to_string()), None, false, cx);
                });
            }));
        });
        chat.sessions_list.reset(chat.sessions.len());
        chat.load_project_files();

        // caret blink pump (2 Hz toggle; repaint only while the editor is
        // focused — input_focused is refreshed every render)
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(530))
                    .await;
                let ok = this
                    .update(cx, |c, cx| {
                        c.caret_on = !c.caret_on;
                        if c.input_focused {
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !ok {
                    break;
                }
            }
        })
        .detach();
        // settings panel CLI op pump (pi install/remove runs in background)
        let (op_tx, mut op_rx) = futures::channel::mpsc::unbounded::<String>();
        chat.op_tx = Some(op_tx);
        cx.spawn(async move |this, cx| {
            while let Some(msg) = op_rx.next().await {
                if this
                    .update(cx, |chat, cx| {
                        chat.set_status(msg, cx);
                        chat.reload_settings_panel();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        // terminal event pump: alacritty EventListener -> Chat (single channel)
        let (term_tx, term_rx) =
            futures::channel::mpsc::unbounded::<(usize, alacritty_terminal::event::Event)>();
        chat.term_events = Some(term_tx);
        cx.spawn(async move |this, cx| {
            let mut rx = term_rx;
            while let Some((tab_id, event)) = rx.next().await {
                if this
                    .update(cx, |chat, cx| chat.on_term_event(tab_id, event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        // Startup restore (pi-web last-open-by-workspace, plus a global
        // last-workspace pointer): reopen the app in the workspace that was
        // used last and reopen the session it had open.
        let mut chat = chat;
        chat.load_project_files();
        chat.refresh_git();
        // models panel state (enabledModels whitelist + credentials) for the
        // picker filter — loaded once at startup, refreshed when opened
        chat.reload_settings_panel();
        // initial runtime: restore the last session (disk-direct tail) or a
        // lazy draft. The process spawns AFTER first paint — the conversation
        // is already on screen by then (startup §4).
        let rt_key = last_open
            .as_deref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "draft-0".to_string());
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(
                rt_key.clone(),
                cwd.clone(),
                last_open.clone(),
            );
            if let Some(path) = &last_open {
                r.messages = msgs_from_tail(read_tail_messages(path, 256 * 1024, 100));
                r.list.reset(r.messages.len());
                r.status = "resuming".into();
            }
            r
        });
        chat.draft_seq = if last_open.is_some() { 1 } else { 0 };
        chat.runtimes.insert(rt_key.clone(), rt.clone());
        chat.active_key = rt_key.clone();
        chat.subscribe_runtime(&rt, cx);
        // background process attach (no process kill involved; spawn is a
        // one-time ~100ms process creation off the first frame)
        let rt_for_spawn = rt.clone();
        cx.spawn(async move |_this, cx| {
            let _ = rt_for_spawn.update(cx, |r, cx| {
                if r.agent.session.is_none() {
                    if let Some(rx) = r.spawn() {
                        let epoch = r.agent.epoch;
                        session::runtime::SessionRuntime::attach_pump(&rt_for_spawn, rx, epoch, cx);
                    }
                }
                if let Some(s) = &r.agent.session {
                    let _ = s.send(&Command::GetMessages);
                    let _ = s.send(&Command::GetTree);
                }
                r.refresh_state();
            });
        })
        .detach();
        if PERF.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(t0) = T0.get() {
                eprintln!("[perf] last-session tail rendered: {:?}", t0.elapsed());
            }
        }
        // git panel: Enter in the commit box commits staged changes
        let entity_for_git = cx.entity();
        chat.git_commit_input.update(cx, |ti, _| {
            let weak_git = entity_for_git.downgrade();
            ti.set_on_submit(Box::new(move |_, cx| {
                let _ = weak_git.update(cx, |c, cx| c.git_commit_staged(cx));
            }));
        });
        // idle recycle (pi-web idle-timeout parity): every 60s, kill the
        // process of any non-active session idle >10min. Messages stay —
        // reopening is instant; the next prompt re-pulls the process.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(60))
                .await;
            let ok = this
                .update(cx, |chat, cx| {
                    let idle_cap = std::time::Duration::from_secs(600);
                    for (key, rt) in &chat.runtimes {
                        if *key == chat.active_key {
                            continue;
                        }
                        let r = rt.read(cx);
                        if !r.agent_running
                            && r.last_activity.elapsed() > idle_cap
                            && r.agent.session.is_some()
                        {
                            rt.update(cx, |r2, _| r2.shutdown_process());
                        }
                    }
                })
                .is_ok();
            if !ok {
                break;
            }
        })
        .detach();
        // background fill: project session list (startup budget §4 — the
        // first frame renders the welcome page while this lands)
        let cwd_text = chat.cwd.to_string_lossy().to_string();
        cx.spawn(async move |this, cx| {
            let sessions = list_sessions_for_cwd(&cwd_text, 100)
                .into_iter()
                .filter(|s| same_ws(&s.cwd, &cwd_text))
                .collect::<Vec<_>>();
            let _ = this.update(cx, |chat, cx| {
                chat.sessions = sessions;
                chat.sessions_list.reset(chat.sessions.len());
                if chat.page == Page::Welcome {
                    chat.page = Page::Session;
                }
                cx.notify();
                if PERF.load(std::sync::atomic::Ordering::Relaxed) {
                    if let Some(t0) = T0.get() {
                        eprintln!("[perf] session list: {:?}", t0.elapsed());
                    }
                }
            });
            // cross-project tail preload (startup §4): the last N sessions'
            // conversations land in memory off the frame path, so switching
            // to a recent project paints its last session instantly
            let n = preload_sessions();
            if n > 0 {
                let active = last_open.clone();
                let preloaded = cx
                    .background_spawn(async move {
                        let mut map = std::collections::HashMap::new();
                        for s in list_sessions(n) {
                            if map.len() >= n {
                                break;
                            }
                            if Some(&s.path) == active.as_ref() {
                                continue;
                            }
                            let msgs =
                                msgs_from_tail(read_tail_messages(&s.path, 256 * 1024, 100));
                            map.insert(s.path, msgs);
                        }
                        map
                    })
                    .await;
                let _ = this.update(cx, |chat, _cx| {
                    chat.session_tail_cache = preloaded;
                });
            }
        })
        .detach();
        chat
    }

    fn persist_dock(&mut self) {
        save_dock_state(&DockState {
            position: if self.dock_right { "right" } else { "left" }.into(),
            panel: self.dock_panel.as_str().into(),
            width: 260.,
        });
    }

    fn refresh_state(&self, cx: &mut gpui::App) {
        self.rt().update(cx, |r, _| r.refresh_state());
    }


    fn refresh_sessions(&mut self) {
        let cwd = self.cwd.to_string_lossy().to_string();
        self.sessions = list_sessions_for_cwd(&cwd, 100)
            .into_iter()
            .filter(|s| same_ws(&s.cwd, &cwd))
            .collect();
        // ListState caches the row count — without this the list renders
        // stale (empty) after switching projects
        self.sessions_list.reset(self.sessions.len());
    }

    fn refresh_git(&mut self) {
        self.git_files = git_status_files(&self.cwd);
        self.git_add_del = git_numstat(&self.cwd);
    }

    // -----------------------------------------------------------------------
    // built-in terminal (pi-web TerminalPanel parity)
    // -----------------------------------------------------------------------



    fn on_term_event(
        &mut self,
        tab_id: usize,
        event: alacritty_terminal::event::Event,
        cx: &mut Context<Self>,
    ) {
        use alacritty_terminal::event_loop::Msg;
        let Some(tab) = self.terminals.iter_mut().find(|t| t.id == tab_id) else {
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

    fn switch_project(&mut self, cwd: PathBuf, cx: &mut Context<Self>) {
        self.cwd = cwd;
        // mark as the globally-last active workspace for startup restore
        set_last_workspace(&self.cwd.to_string_lossy());
        self.branch = read_branch(&self.cwd);
        self.refresh_sessions();
        self.dialog = None;
        self.expanded_dirs.clear();
        self.refresh_git();
        self.load_project_files();
        if let Some(p) = get_last_open(&self.cwd.to_string_lossy()) {
            let path = PathBuf::from(&p);
            if path.exists() {
                self.open_session(path, false, cx);
                cx.notify();
                return;
            }
        }
        self.new_session(cx);
        cx.notify();
    }



    fn with_active_editor<Act: FnOnce(&mut SessionRuntime, &mut Context<SessionRuntime>)>(
        &mut self,
        cx: &mut Context<Self>,
        act: Act,
    ) {
        let rt = self.rt();
        let (input, images, history) = (
            self.input.clone(),
            self.pending_images.clone(),
            self.history.clone(),
        );
        rt.update(cx, |r, cx| {
            r.input = input;
            r.pending_images = images;
            r.history = history;
            act(r, cx);
        });
        // pull post-action state back (history push, input clear)
        let (input, images, history) = {
            let r = rt.read(cx);
            (r.input.clone(), r.pending_images.clone(), r.history.clone())
        };
        self.input = input;
        self.pending_images = images;
        self.history = history;
        self.history_ix = None;
        cx.notify();
    }

    fn send_input(&mut self, cx: &mut Context<Self>) {
        self.with_active_editor(cx, |r, cx| r.send_input(cx));
    }

    fn steer_input(&mut self, cx: &mut Context<Self>) {
        self.with_active_editor(cx, |r, cx| r.steer_input(cx));
    }

    fn follow_up_input(&mut self, cx: &mut Context<Self>) {
        self.with_active_editor(cx, |r, cx| r.follow_up_input(cx));
    }

    fn abort_stream(&mut self, cx: &mut Context<Self>) {
        self.rt().update(cx, |r, cx| r.abort_stream(cx));
        cx.notify();
    }


    fn set_thinking_level(&mut self, key: &str, cx: &mut Context<Self>) {
        self.rt().update(cx, |r, cx| r.set_thinking_level(key, cx));
    }

    fn mc_set_tools_preset(&mut self, key: &str, cx: &mut Context<Self>) {
        let rt = self.rt();
        rt.update(cx, |r, cx| {
            r.tools_preset = key.to_string();
            if let Some(rx) = r.spawn() {
                let epoch = r.agent.epoch;
                session::runtime::SessionRuntime::attach_pump(&rt, rx, epoch, cx);
            }
            if let Some(s) = &r.agent.session {
                let _ = s.send(&Command::GetMessages);
                let _ = s.send(&Command::GetTree);
            }
            r.refresh_state();
            r.status = crate::i18n::tf("工具预设: {k} (会话进程已重绑)", &[("k", key.to_string())]);
            cx.emit(session::runtime::SessionEvent::Changed);
        });
        cx.notify();
    }

    fn rt(&self) -> gpui::Entity<session::runtime::SessionRuntime> {
        self.runtimes
            .get(&self.active_key)
            .expect("active runtime exists")
            .clone()
    }

    fn set_status(&mut self, msg: String, cx: &mut Context<Self>) {
        self.rt().update(cx, |r, _| r.status = msg);
    }




    fn load_project_files(&mut self) {
        self.project_files = walk_files(&self.cwd, 3, 400);
    }


























}

impl Focusable for Chat {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// git status/diff (lib/git-changes.ts parity)
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// LLM session title (pi-web lib/session-title.ts parity)
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// IME support: gpui routes Windows IME through the focused view's
// EntityInputHandler (replace_and_mark_text_in_range = composition,
// replace_text_in_range = commit). UTF-16 offsets per the trait contract.
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// branch tree helpers (BranchNavigator.tsx parity)
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// root render
// ---------------------------------------------------------------------------

impl Render for Chat {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if PERF.load(std::sync::atomic::Ordering::Relaxed)
            && !FIRST.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            if let Some(t0) = T0.get() {
                eprintln!("[perf] first frame: {:?}", t0.elapsed());
            }
        }
        // keep terminal focus alive across frames (render focuses chat input
        // otherwise, which would steal it back every redraw)
        //
        // dialog inputs own their focus handles; force-focus only when the
        // input isn't already focused so click-to-focus still works
        let dialog_input = match &self.dialog {
            Some(Dialog::ModelSelect { input }) | Some(Dialog::SessionSearch { input }) => {
                Some(input.clone())
            }
            _ => None,
        };
        // inline rename input keeps keyboard focus until committed/cancelled
        let rename_focus = self.rename_input.clone();
        // NOTE: settings inputs are click-to-focus only — frame-level focus
        // forcing on a not-yet-mounted entity recurses in gpui focus handling
        // (stack overflow); dialogs keep the force since they mount before
        // their first frame.
        if let Some(input) = rename_focus.or(dialog_input) {
            let handle = input.read(cx).focus_handle_in(cx);
            if !handle.is_focused(window) {
                window.focus(&handle);
            }
        } else if self.ext_dialog.is_some() {
            let ext_text = matches!(
                self.ext_dialog.as_ref().map(|r| &r.method),
                Some(
                    pi_link::protocol::ExtUiMethod::Input { .. }
                        | pi_link::protocol::ExtUiMethod::Editor { .. }
                )
            );
            let handle = if ext_text {
                self.ext_input.read(cx).focus_handle()
            } else {
                self.dialog_focus.clone()
            };
            if !handle.is_focused(window) {
                window.focus(&handle);
            }
        } else if self.dialog.is_some() {
            if !self.dialog_focus.is_focused(window) {
                window.focus(&self.dialog_focus);
            }
        } else if let Some(panel) = self.settings.as_ref() {
            // settings inputs stay click-to-focus (see NOTE above); claim
            // the modal escape target only while nothing inside the modal
            // holds focus, so Esc reaches the modal's close handler
            let p = panel.read(cx);
            let inner_focused = p.focus.is_focused(window)
                || p.key_input.read(cx).focus_handle_in(cx).is_focused(window)
                || p.install_input.read(cx).focus_handle_in(cx).is_focused(window)
                || p.sa_input.read(cx).focus_handle_in(cx).is_focused(window);
            if !inner_focused && !self.dialog_focus.is_focused(window) {
                window.focus(&self.dialog_focus);
            }
        } else if !self.terminals.iter().any(|t| t.focus.is_focused(window)) {
            window.focus(&self.focus);
        }
        let t = T();

        let entity = cx.entity();
        let weak = entity.downgrade();
        let weak_for_dialog = weak.clone();

        let right_px = 24.;
        // popup menu overlay anchored above the editor toolbar row (options
        // reflect the ACTIVE session's overrides)
        let (thinking_override, preset_key) = {
            let rt = self.rt();
            let r = rt.read(cx);
            (r.thinking_override.clone(), r.tools_preset.clone())
        };
        let pill_menu_el = self.pill_menu.map(|menu| {
            let weak_menu = weak.clone();
            let rows: Vec<(String, String, bool)> = match menu {
                PillMenu::Thinking => [
                    ("auto", tr("使用 pi 默认设置"), thinking_override.is_none()),
                    ("low", tr("低强度推理"), thinking_override.as_deref() == Some("low")),
                    ("high", tr("高强度推理"), thinking_override.as_deref() == Some("high")),
                    ("max", tr("最强推理"), thinking_override.as_deref() == Some("max")),
                ]
                .iter()
                .map(|(k, d, on)| (k.to_string(), d.to_string(), *on))
                .collect(),
                PillMenu::Tools => [
                    ("configured", tr("取自 settings.json 的 defaultTools"), preset_key == "configured"),
                    ("chat-only", tr("仅聊天"), preset_key == "chat-only"),
                    ("read-only", tr("4 个只读内置工具"), preset_key == "read-only"),
                    ("default", tr("4 个内置工具"), preset_key == "default"),
                    ("full", tr("全部内置工具"), preset_key == "full"),
                ]
                .iter()
                .map(|(k, d, on)| (k.to_string(), d.to_string(), *on))
                .collect(),
            };
            let is_thinking = menu == PillMenu::Thinking;
            let items = rows
                .into_iter()
                .map(|(key, desc, active)| {
                    let weak_row = weak_menu.clone();
                    let key_for_rpc = key.clone();
                    div()
                        .id(SharedString::from(format!(
                            "pm-{}-{}",
                            if is_thinking { "t" } else { "w" },
                            key
                        )))
                        .min_h(px(40.))
                        .px_3()
                        .py_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = weak_row.update(cx, |c, cx| {
                                c.pill_menu = None;
                                if is_thinking {
                                    c.set_thinking_level(&key_for_rpc, cx);
                                } else {
                                    c.mc_set_tools_preset(&key_for_rpc, cx);
                                }
                            });
                        })
                        .child(
                            div()
                                .w(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .children(active.then(|| icon("check", 13., t.accent))),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(if active {
                                    gpui::FontWeight::SEMIBOLD
                                } else {
                                    gpui::FontWeight::NORMAL
                                })
                                .text_color(rgb(t.text))
                                .child(SharedString::from(key)),
                        )
                        .child(
                            div()
                                .ml_auto()
                                .text_size(px(11.))
                                .text_color(rgb(t.text_dim))
                                .child(desc),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            div()
                .absolute()
                .inset_0()
                .child(
                    // transparent backdrop: click anywhere closes the menu
                    div()
                        .size_full()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, {
                            let weak = weak_menu.clone();
                            move |_, _, cx| {
                                let _ = weak.update(cx, |c, cx| {
                                    if c.pill_menu.is_some() {
                                        c.pill_menu = None;
                                        cx.notify();
                                    }
                                });
                            }
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .bottom(px(120.))
                        .right(px(right_px))
                        .min_w(px(320.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .bg(rgb(t.bg))
                        .shadow_lg()
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .children(items),
                )
                .into_any_element()
        });

        // ---- main column (session::main_column) --------------------------
        let main_col = session::main_column(self, entity.clone(), &weak, window, cx);
        // 005 layout: vertical shell — titlebar / body(dock + session) / control bar
        let body = if self.page == Page::Welcome {
            pages::welcome::welcome().into_any_element()
        } else {
            let dock_el =
                function_panel::dock(self, entity.clone(), &weak, window, cx);
            if self.dock_right {
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(main_col)
                    .child(dock_el)
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(dock_el)
                    .child(main_col)
                    .into_any_element()
            }
        };
        let mut root = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(t.bg))
            .text_color(rgb(t.text))
            .font_family(crate::appearance::panel_font().family.clone())
            .child(titlebar::title_bar(self, window, cx))
            .child(body)
            .child(status_bar::control_bar(self, cx));

        root = dialogs::render_dialogs(root, self, &weak_for_dialog, t, cx);

        if let Some(panel) = self.settings.clone() {
            let data = settings::SettingsFormData::snapshot(panel.read(cx), cx);
            root = root.child(settings::render_settings(self, &weak_for_dialog, &data));
        }
        // toolbar pill popup menus
        if let Some(el) = pill_menu_el {
            root = root.child(el);
        }
        // top-bar dropdown panels (系统提示词 / 工具定义)
        if let Some(tp) = self.top_panel {
            let weak_tp = weak.clone();
            let mut panel = div()
                .id("top-panel")
                .absolute()
                .top(px(44.))
                .left(px(276.))
                .w(px(680.))
                .max_h(px(520.))
                .bg(rgb(t.bg))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(8.))
                .shadow_lg()
                .overflow_y_scroll()
                .p(px(12.))
                .flex()
                .flex_col()
                .gap_2();
            match tp {
                TopPanel::System => {
                    panel = panel.child(
                        div()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(t.text))
                            .child(tr("系统提示词")),
                    );
                    let rt = self.rt();
                    let sys_prompt = rt.read(cx).sys_prompt.clone();
                    match &sys_prompt {
                        Some(text) => {
                            panel = panel.child(
                                div()
                                    .font_family("Consolas")
                                    .text_size(px(11.))
                                    .text_color(rgb(t.text_muted))
                                    .flex()
                                    .flex_col()
                                    .children(
                                        text.lines().map(|l| {
                                            div().child(SharedString::from(l.to_string()))
                                        }),
                                    ),
                            );
                        }
                        None => {
                            panel = panel.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(t.text_dim))
                                    .child(tr("正在获取（export 中）…")),
                            );
                        }
                    }
                }
                TopPanel::Tools => {
                    let session_tools = self.rt().read(cx).session_tools.clone();
                    panel = panel.child(
                        div()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(t.text))
                            .child(tr("工具定义")),
                    );
                    match &session_tools {
                        Some(tools) if !tools.is_empty() => {
                            for (name, desc) in tools {
                                panel = panel.child(
                                    div()
                                        .flex()
                                        .items_baseline()
                                        .gap_2()
                                        .px_2()
                                        .py(px(4.))
                                        .rounded(px(4.))
                                        .hover(|s| s.bg(rgb(t.bg_hover)))
                                        .child(
                                            div()
                                                .font_family("Consolas")
                                                .text_size(px(11.))
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(rgb(t.text))
                                                .child(SharedString::from(name.clone())),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_size(px(11.))
                                                .text_color(rgb(t.text_muted))
                                                .child(SharedString::from(desc.clone())),
                                        ),
                                );
                            }
                        }
                        _ => {
                            panel = panel.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(t.text_dim))
                                    .child(tr("正在获取（export 中）…")),
                            );
                        }
                    }
                }
            }
            root = root.child(
                div()
                    .absolute()
                    .inset_0()
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, {
                                let w = weak_tp.clone();
                                move |_, _, cx| {
                                    let _ = w.update(cx, |c, cx| {
                                        if c.top_panel.is_some() {
                                            c.top_panel = None;
                                            cx.notify();
                                        }
                                    });
                                }
                            }),
                    )
                    .child(panel),
            );
        }
        // extension notify toast (top-right)
        if let Some((message, ty)) = &self.ext_notice {
            let color = match ty {
                1 => 0xfacc15,
                2 => 0xf87171,
                _ => 0x4ade80,
            };
            let text: SharedString = message.clone().into();
            root = root.child(
                div()
                    .absolute()
                    .top(px(12.))
                    .right(px(12.))
                    .max_w(px(420.))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(color))
                    .bg(rgb(t.bg_panel))
                    .shadow_lg()
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(div().size(px(7.)).rounded_full().mt(px(4.)).bg(rgb(color)))
                    .child(div().text_size(px(12.)).text_color(rgb(t.text)).child(text)),
            );
        }
        // blocking extension dialog (select/confirm/input/editor)
        if let Some(req) = &self.ext_dialog {
            root = root.child(render_ext_dialog(self, req.clone(), &weak_for_dialog));
        }
        root
    }
}

/// Models panel dialog (pi-web ModelsConfig parity): 900px surface, 240px
/// provider sidebar, detail pane with API-key editor + per-provider
/// enabledModels list (36px rows, 32×18 ConfigSwitch, pi-web tokens).


fn main() {
    let _ = T0.set(std::time::Instant::now());
    PERF.store(true, std::sync::atomic::Ordering::Relaxed);
    // theme: PI_FLASH_THEME (dev override) > persisted settings.json > mist
    if std::env::var("PI_FLASH_THEME").ok().and_then(|n| theme::set_by_name(&n).then_some(())).is_none() {
        let app = app_settings().theme;
        if let Some(name) = app.or_else(|| pi_link::config::read_theme(&pi_link::config::settings_path())) {
            theme::set_by_name(&name);
        }
    }
    // language: persisted workspace-memory preference
    if let Some(ix) = load_lang_pref() {
        i18n::set_lang(ix);
    }
    Application::new()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            // gpui-component (widget library powering TextInput): global
            // init + token mapping from the active app theme (appearance
            // owns the remap so theme switches re-run it)
            gpui_component::init(cx);
            appearance::sync_gpui_tokens(cx);
            // restore last window bounds (startup restore layer §4)
            let restored = get_window_state();
            let bounds = gpui::Bounds::centered(
                None,
                gpui::size(
                    px(restored.as_ref().map(|w| w.w as f32).unwrap_or(1180.)),
                    px(restored.as_ref().map(|w| w.h as f32).unwrap_or(760.)),
                ),
                cx,
            );
            let window_bounds = if restored.map(|w| w.maximized).unwrap_or(false) {
                gpui::WindowBounds::Maximized(bounds)
            } else {
                gpui::WindowBounds::Windowed(bounds)
            };
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("pi-flash".into()),
                        // client-side title bar (005: app-drawn, window
                        // control hitboxes registered by titlebar.rs)
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    // gpui-component widgets require its Root as the window
                    // root view (renders their context-menu/popover layers)
                    let chat = cx.new(Chat::new);
                    let weak = chat.downgrade();
                    // persist window bounds + dock layout on close so the
                    // startup restore layer has data (§4)
                    window.on_window_should_close(cx, move |window, cx| {
                        let b = window.bounds();
                        save_window_state(&WindowState {
                            x: f64::from(b.origin.x),
                            y: f64::from(b.origin.y),
                            w: f64::from(b.size.width),
                            h: f64::from(b.size.height),
                            maximized: window.is_maximized(),
                        });
                        if let Some(chat) = weak.upgrade() {
                            chat.update(cx, |chat, _cx| chat.persist_dock());
                        }
                        true
                    });
                    cx.new(|cx| gpui_component::Root::new(chat.into(), window, cx))
                },
            )
            .unwrap();
            cx.activate(true);
        });
}