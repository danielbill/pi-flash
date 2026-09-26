//! pi-flash — desktop shell for the pi coding agent.
//!
//! Component-by-component translation of pi-web (see PORT_PLAN.md). Layout
//! values (sizes, colors, spacing) come from pi-web sources: globals.css
//! theme tokens, panel-layout.ts, MessageView/ChatInput/AppShell structures.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use futures::{StreamExt, channel::mpsc::UnboundedReceiver};
use gpui::{
    Animation, AnimationExt, App, Application, Context, FocusHandle, Focusable, KeyDownEvent,
    ListAlignment, ListState, MouseButton, ParentElement, Render, SharedString, Styled,
    WindowOptions, div, list, prelude::*, pulsating_between, px, rgb,
};
use agent_session::AgentSession;
use pi_link::protocol::{
    AssistantEvent, Block, Command, Event, SessionState, SessionStats, SlashCommand, TreeNode,
    Usage, content_blocks, parse_tree,
};
use pi_link::sessions::{SessionInfo, list_sessions_for_cwd, read_tail_messages};

mod agent_session;
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
use i18n::tr;
use models_config::EnabledState;
use theme::theme as T;
use services::branch::*;
use services::format::*;
use services::git::*;
use services::title::{TitleTurn, build_title_transcript, parse_export_html, sanitize_title};
use services::workspace::*;
use session::messages::{Msg, Role, UsageLine, render_msg};
use terminal::{TermStatus, TerminalTab};
use ui::TextInput;
use ui::{icon, pill};
use ext_ui::{render_ext_dialog, render_ext_widget};

// ---------------------------------------------------------------------------
// state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Dialog {
    ModelSelect { input: gpui::Entity<TextInput> },
    BranchTree,
    ProjectSelect,
    GitDiff { path: PathBuf, patch: String },
    /// read-only file preview (replaces the old right-panel viewer tab)
    FilePreview { path: PathBuf },
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
    title: String,
    desc: String,
}

struct Chat {
    focus: FocusHandle,
    dialog_focus: FocusHandle,
    dialog: Option<Dialog>,
    /// full-page state (welcome gate until the session list lands)
    page: Page,
    /// functionPanel active view + side (015/018, persisted)
    dock_panel: DockPanel,
    dock_right: bool,
    input: String,
    messages: Vec<Msg>,
    list: ListState,
    sessions: Vec<SessionInfo>,
    sessions_list: ListState,
    cwd: PathBuf,
    branch: String,
    /// pi RPC process ownership (spawn/epoch/events)
    agent: AgentSession,
    status: String,
    state: Option<SessionState>,
    stats: Option<SessionStats>,
    active_session_file: Option<PathBuf>,
    collapsed: HashSet<(usize, usize)>,
    expanded_dirs: HashSet<PathBuf>,
    /// working-tree changes for the current project
    git_files: Vec<GitFile>,
    git_add_del: (u64, u64),
    /// git panel (022 simplified) state
    git_tab: function_panel::git_panel::GitTab,
    git_log: Vec<GitCommit>,
    git_error: Option<String>,
    git_commit_input: gpui::Entity<TextInput>,
    commands: Vec<SlashCommand>,
    available_models: Vec<pi_link::protocol::ModelInfo>,
    project_files: Vec<String>,
    pending_images: Vec<AttachedImage>,
    history: Vec<String>,
    history_ix: Option<usize>,
    menu_ix: usize,
    hovered_session: Option<usize>,
    pending_rename: bool,
    /// get_tree snapshot: (roots, active leaf id)
    branch_tree: Option<(Vec<TreeNode>, Option<String>)>,
    /// user-message entry ids along the active root→leaf path (fork anchors)
    active_user_entry_ids: Vec<String>,
    /// built-in terminal tabs (pi-web TerminalPanel); cwd-keyed dedupe
    terminals: Vec<TerminalTab>,
    active_terminal: Option<usize>,
    term_seq: usize,
    /// alacritty event channel for all terminal tabs
    term_events: Option<futures::channel::mpsc::UnboundedSender<
        (usize, alacritty_terminal::event::Event),
    >>,
    /// settings panel state (loaded when the panel opens)
    mc_patterns: Option<Vec<String>>,
    mc_state: EnabledState,
    mc_creds: Vec<(String, pi_link::config::CredentialKind)>,
    mc_project_scope: bool,
    mc_skills: Vec<pi_link::skills::SkillEntry>,
    mc_pkgs_global: Vec<serde_json::Value>,
    mc_pkgs_project: Vec<serde_json::Value>,
    mc_default_tools: Option<Vec<String>>,
    /// background pi-CLI operation results (install/remove) -> status line
    op_tx: Option<futures::channel::mpsc::UnboundedSender<String>>,
    // extension UI protocol state (pi-web rpc-manager parity)
    /// ordered status items (setStatus)
    ext_status: Vec<(String, String)>,
    /// widgets (setWidget): key, lines, above-editor
    ext_widgets: Vec<(String, Vec<String>, bool)>,
    /// blocking extension dialog (select/confirm/input/editor)
    ext_dialog: Option<pi_link::protocol::ExtensionUiRequest>,
    /// its text field (Input/Editor variants)
    ext_input: gpui::Entity<TextInput>,
    /// transient notify toast (message, 0 info/1 warning/2 error)
    ext_notice: Option<(String, u8)>,
    // subagent profiles + runs (pi-web subagents.ts / AgentSessionPanel)
    sa_profiles: Vec<pi_link::subagents::SubagentProfile>,
    sa_settings: pi_link::subagents::SubagentSettings,
    sa_runs: Vec<SubagentRun>,
    sa_run_seq: usize,
    /// LLM title generation results (one-off pi --print run)
    title_tx: Option<futures::channel::mpsc::UnboundedSender<Result<String, String>>>,
    titling: bool,
    /// right panel width (px; drag handle + expand toggle, pi-web parity)
    /// right panel tabs: file viewers + terminals in one TabBar (pi-web
    /// AppShell panelTabs merge)
    panel_tabs: Vec<PanelTab>,
    active_panel_tab: Option<usize>,
    /// right-panel drag: (start pointer x, start width)
    /// markdown Source/Preview toggle for file tabs (per-path)
    /// cached content of open file tabs
    file_cache: std::collections::HashMap<PathBuf, FileTab>,
    /// sessions-pane height as a fraction of the sidebar (pi-web
    /// --sidebar-session-pane-height; default half)
    sidebar_sessions_frac: f32,
    /// active sidebar pane drag: (start pointer y, start fraction)
    /// editor caret blink state (toggled by the blink pump)
    caret_on: bool,
    /// whether the chat editor currently owns focus (updated in render)
    input_focused: bool,
    /// local thinking-level override ("auto" = pi default governs)
    thinking_override: Option<String>,
    /// toolbar pill popup (thinking / tools preset menus)
    pill_menu: Option<PillMenu>,
    /// notification sound preference (persisted "__sound")
    sound_on: bool,
    /// wall-clock start of the current agent run (for the t/s estimate)
    stream_started: Option<std::time::Instant>,
    /// IME composition marked range (utf16 offsets into self.input)
    ime_marked: Option<std::ops::Range<usize>>,
    /// session system prompt + tools parsed from export_html (top panels)
    sys_prompt: Option<String>,
    session_tools: Option<Vec<(String, String)>>,
    /// top-bar dropdown panel (系统提示词 / 工具定义)
    top_panel: Option<TopPanel>,
    /// settings modal (own entity; pi-web SettingsPanel)
    settings: Option<gpui::Entity<settings::SettingsPanel>>,
    /// inline session rename (pi-web SessionSidebar renaming): the row being
    /// edited + its input (value pre-filled, select-all on focus)
    renaming: Option<PathBuf>,
    rename_input: Option<gpui::Entity<TextInput>>,
    /// sidebar session text search (pi-web SessionSearch)
    search_open: bool,
    search_input: gpui::Entity<TextInput>,
    sessions_list_count: usize,
    /// send feedback: pulsing "waiting for model" row under the message list
    /// (pi-web agentPhase=waiting_model + animate-[pulse_1.5s_infinite])
    phase_waiting: bool,
    /// inline delete confirmation on a session row (pi-web confirmDelete)
    confirm_delete: Option<PathBuf>,
    /// true from AgentStart until AgentEnd/AgentSettled (pi-web
    /// runningSessionIds; drives the sidebar spinner)
    agent_running: bool,
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
    path: PathBuf,
    content: String,
    truncated: bool,
}

/// One live subagent run (child RPC session spawned with profile flags).
struct SubagentRun {
    id: usize,
    profile: String,
    /// 0 running · 1 completed · 2 failed · 3 aborted
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
        let mut agent = AgentSession::new(1);
        let events = agent.spawn(&cwd, last_open.as_deref());
        let connected = agent.session.is_some();

        let list = ListState::new(0, ListAlignment::Bottom, px(1000.));
        list.reset(0);
        let sessions_list = ListState::new(0, ListAlignment::Top, px(500.));
        let dock_state = get_dock_state();

        let mut chat = Self {
            focus,
            dialog_focus,
            dialog: None,
            input: String::new(),
            messages: Vec::new(),
            list,
            // skeleton first (ARCHITECTURE.md §4): the list fills in the
            // background task below; the page flips Welcome -> Session then
            sessions: Vec::new(),
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
            agent,
            status: status_line(connected, "idle"),
            state: None,
            stats: None,
            active_session_file: None,
            collapsed: HashSet::new(),
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
            commands: Vec::new(),
            available_models: Vec::new(),
            project_files: Vec::new(),
            pending_images: Vec::new(),
            history: Vec::new(),
            history_ix: None,
            menu_ix: 0,
            hovered_session: None,
            pending_rename: false,
            branch_tree: None,
            active_user_entry_ids: Vec::new(),
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
            title_tx: None,
            titling: false,
            sidebar_sessions_frac: 0.5,
            panel_tabs: Vec::new(),
            active_panel_tab: None,
            file_cache: std::collections::HashMap::new(),
            caret_on: true,
            input_focused: false,
            thinking_override: None,
            pill_menu: None,
            sound_on: load_sound_pref(),
            stream_started: None,
            ime_marked: None,
            sys_prompt: None,
            session_tools: None,
            top_panel: None,
            settings: None,
            renaming: None,
            rename_input: None,
            search_open: false,
            search_input: cx
                .new(|cx| TextInput::new(cx).placeholder(tr("搜索会话..."))),
            sessions_list_count: 0,
            phase_waiting: false,
            confirm_delete: None,
            agent_running: false,
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
        chat.refresh_state();

        if let Some(events) = events {
            cx.spawn(async move |this, cx| {
                consume_events(this, cx, events, 1).await;
            })
            .detach();
        }
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
        // LLM title pump: one-off `pi --print` result -> set_session_name
        let (title_tx, mut title_rx) =
            futures::channel::mpsc::unbounded::<Result<String, String>>();
        chat.title_tx = Some(title_tx);
        cx.spawn(async move |this, cx| {
            while let Some(result) = title_rx.next().await {
                if this
                    .update(cx, |chat, cx| chat.on_title_result(result, cx))
                    .is_err()
                {
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
                        chat.status = msg;
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
        if let Some(path) = last_open {
            // single-spawn startup: pi already resumed from the file — fill
            // UI state + disk-direct render, no second process, no waiting
            // for the RPC snapshot to show the last conversation
            chat.active_session_file = Some(path.clone());
            chat.status = status_line(chat.agent.session.is_some(), "resuming");
            if let Some(session) = &chat.agent.session {
                let _ = session.send(&Command::GetMessages);
                let _ = session.send(&Command::GetTree);
            }
            for msg in read_tail_messages(&path, 256 * 1024, 100) {
                let m = &msg["message"];
                let role = m["role"].as_str().unwrap_or("");
                let blocks = content_blocks(&m["content"]);
                let usage = Usage::parse(&m["usage"]);
                chat.ingest_message(role, blocks, usage, None, None, cx);
            }
            chat.notify_list(cx);
            chat.refresh_state();
        }
        // git panel: Enter in the commit box commits staged changes
        let entity_for_git = cx.entity();
        chat.git_commit_input.update(cx, |ti, _| {
            let weak_git = entity_for_git.downgrade();
            ti.set_on_submit(Box::new(move |_, cx| {
                let _ = weak_git.update(cx, |c, cx| c.git_commit_staged(cx));
            }));
        });
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
            });
        })
        .detach();
        chat
    }

    /// Persist the dock layout (015 side + 018 active view).
    fn persist_dock(&mut self) {
        save_dock_state(&DockState {
            position: if self.dock_right { "right" } else { "left" }.into(),
            panel: self.dock_panel.as_str().into(),
            width: 260.,
        });
    }

    fn refresh_state(&self) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::GetState);
            let _ = session.send(&Command::GetSessionStats);
            let _ = session.send(&Command::GetCommands);
            let _ = session.send(&Command::GetAvailableModels);
        }
    }


    /// Reload sessions filtered to the selected project (pi-web
    /// sessionsForProject: only the selected cwd's sessions are listed).
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

    /// Re-read working-tree changes (git status + numstat summary).
    fn refresh_git(&mut self) {
        self.git_files = git_status_files(&self.cwd);
        self.git_add_del = git_numstat(&self.cwd);
    }

    // -----------------------------------------------------------------------
    // built-in terminal (pi-web TerminalPanel parity)
    // -----------------------------------------------------------------------

    /// Open (or focus) a terminal for the selected workspace cwd. One PTY per
    /// cwd (terminal-manager idempotent-create parity).
    fn open_terminal(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.dock_panel = DockPanel::Terminal;
        if let Some(ix) = self.terminals.iter().position(|t| same_path(&t.cwd, &self.cwd)) {
            self.active_terminal = Some(ix);
            let focus = self.terminals[ix].focus.clone();
            window.focus(&focus);
            cx.notify();
            return;
        }
        let Some(tx) = self.term_events.clone() else { return };
        let (cell_w, line_h) = terminal::measure_cell(window);
        self.term_seq += 1;
        let id = self.term_seq;
        let focus = cx.focus_handle();
        let proxy = terminal::Proxy { tab: id, tx };
        match terminal::spawn_terminal(id, self.cwd.clone(), cell_w, line_h, focus, proxy) {
            Ok(tab) => {
                self.terminals.push(tab);
                self.active_terminal = Some(self.terminals.len() - 1);
                self.panel_tabs.push(PanelTab::Term(id));
                self.active_panel_tab = Some(self.panel_tabs.len() - 1);
                let focus = self.terminals[self.terminals.len() - 1].focus.clone();
                window.focus(&focus);
                cx.notify();
            }
            Err(e) => {
                self.status = format!("terminal spawn failed: {e}");
                cx.notify();
            }
        }
    }

    /// Close a tab: shutdown the PTY, drop state (DELETE /api/terminal/:id).
    fn close_terminal(&mut self, ix: usize, window: &mut gpui::Window, cx: &mut Context<Self>) {
        if ix >= self.terminals.len() {
            return;
        }
        let term_id = self.terminals[ix].id;
        if let Some(pix) = self.panel_tabs.iter().position(|t| matches!(t, PanelTab::Term(id) if *id == term_id)) {
            self.panel_tabs.remove(pix);
            self.active_panel_tab = match self.active_panel_tab {
                Some(a) if a >= self.panel_tabs.len() => {
                    if self.panel_tabs.is_empty() {
                        None
                    } else {
                        Some(a.saturating_sub(1))
                    }
                }
                other => other,
            };
        }
        let _ = self.terminals[ix].pty.send(alacritty_terminal::event_loop::Msg::Shutdown);
        self.terminals.remove(ix);
        self.active_terminal = match self.active_terminal {
            Some(a) if a >= self.terminals.len() => {
                if self.terminals.is_empty() {
                    None
                } else {
                    Some(a.saturating_sub(1))
                }
            }
            other => other,
        };
        if self.active_terminal.is_none() {
            window.focus(&self.focus);
        }
        cx.notify();
    }

    /// Restart: kill + respawn with the same cwd and current grid size.
    fn restart_terminal(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(tx) = self.term_events.clone() else { return };
        let Some(old) = self.terminals.get(ix) else { return };
        if old.status == TermStatus::Ready {
            let _ = old.pty.send(alacritty_terminal::event_loop::Msg::Shutdown);
        }
        let (cols, rows, cell_w, line_h) = (old.cols, old.rows, old.cell_w, old.line_h);
        let cwd = old.cwd.clone();
        self.term_seq += 1;
        let id = self.term_seq;
        let focus = cx.focus_handle();
        let proxy = terminal::Proxy { tab: id, tx };
        match terminal::spawn_terminal(id, cwd, cell_w, line_h, focus, proxy) {
            Ok(mut tab) => {
                tab.cols = cols;
                tab.rows = rows;
                self.terminals[ix] = tab;
                self.active_terminal = Some(ix);
            }
            Err(e) => {
                self.terminals[ix].status = TermStatus::Failed(e);
            }
        }
        cx.notify();
    }

    /// Active terminal index helper.
    fn active_term(&mut self) -> Option<&mut TerminalTab> {
        self.active_terminal
            .and_then(|ix| self.terminals.get_mut(ix))
    }

    /// Terminal keyboard input: copy/paste shortcuts first (pi-web
    /// attachCustomKeyEventHandler parity: Ctrl+C with selection copies and
    /// never sends ^C; Ctrl+V goes to the PTY, never the browser), then the
    /// keystroke → escape-sequence table.
    fn terminal_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        use alacritty_terminal::event_loop::Msg;
        let Some(tab) = self.active_term() else { return };
        let k = &ev.keystroke;
        let ctrl = k.modifiers.control;
        let shift = k.modifiers.shift;
        let mode = *tab.term.lock().mode();

        if ctrl && k.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
                let bytes = terminal::paste_bytes(&text, &mode);
                let _ = tab.pty.send(Msg::Input(bytes.into()));
            }
            cx.stop_propagation();
            return;
        }
        if ctrl && k.key == "c" && (shift || tab.selection.is_some()) {
            if let Some(sel) = tab.selection {
                let text = terminal::selection_text(&tab.term.lock(), sel);
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                tab.selection = None;
                tab.sel_anchor = None;
            }
            cx.stop_propagation();
            return;
        }
        if let Some(bytes) = terminal::keystroke_to_pty(k, &mode) {
            let _ = tab.pty.send(Msg::Input(bytes.into()));
            cx.stop_propagation();
        }
    }

    // -----------------------------------------------------------------------
    // models panel (pi-web ModelsConfig parity: enabledModels + API keys)
    // -----------------------------------------------------------------------

    /// `provider/modelId` refs of every available model, display order.
    fn mc_refs(&self) -> Vec<String> {
        self.available_models
            .iter()
            .map(|m| format!("{}/{}", m.provider, m.id))
            .collect()
    }


    /// Pump task target: route one alacritty event to its tab.
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

    /// Switch workspace: reset context, filter sessions, restore the last
    /// open session of that workspace (or land on a blank new session).
    fn switch_project(&mut self, cwd: PathBuf, cx: &mut Context<Self>) {
        self.cwd = cwd;
        // mark as the globally-last active workspace for startup restore
        set_last_workspace(&self.cwd.to_string_lossy());
        self.branch = read_branch(&self.cwd);
        self.refresh_sessions();
        self.messages.clear();
        self.state = None;
        self.stats = None;
        self.active_session_file = None;
        self.collapsed.clear();
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

    /// Open the unified diff for a changed file.
    fn open_git_diff(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let untracked = self
            .git_files
            .iter()
            .any(|f| f.path == path && f.status == GitStatus::Untracked);
        let patch = git_file_diff(&self.cwd, &path, untracked);
        self.dialog = Some(Dialog::GitDiff { path, patch });
        cx.notify();
    }

    fn open_project_select(&mut self, cx: &mut Context<Self>) {
        self.dialog = Some(Dialog::ProjectSelect);
        cx.notify();
    }

    /// Open the branch navigator: request a fresh tree, show the panel.
    fn open_branch_tree(&mut self, cx: &mut Context<Self>) {
        self.branch_tree = None;
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::GetTree);
        }
        self.dialog = Some(Dialog::BranchTree);
        cx.notify();
    }

    /// Fork a new session branching before the given user-message entry.
    /// pi rebinds this process to the branched session; the "fork" response
    /// handler reloads state/messages/tree.
    fn fork_from_entry(&mut self, entry_id: String, cx: &mut Context<Self>) {
        if self
            .state
            .as_ref()
            .is_some_and(|s| s.is_streaming)
        {
            self.status = "cannot fork while running".into();
            cx.notify();
            return;
        }
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::Fork { entry_id });
            self.dialog = None;
        }
        cx.notify();
    }

    fn load_project_files(&mut self) {
        self.project_files = walk_files(&self.cwd, 3, 400);
    }

    fn model_label_text(&self) -> String {
        self.state
            .as_ref()
            .and_then(|s| s.model_label())
            .unwrap_or_else(|| "pi".to_string())
    }

    /// pi-web ChatWindow: pulsing phase label renders while the agent is
    /// running but no assistant content has arrived yet
    fn phase_row_visible(&self) -> bool {
        self.phase_waiting
            && !matches!(self.messages.last(), Some(m) if m.role == Role::Assistant)
    }

    fn notify_list(&mut self, cx: &mut Context<Self>) {
        self.list
            .reset(self.messages.len() + usize::from(self.phase_row_visible()));
        cx.notify();
    }

    fn last_assistant(&mut self) -> &mut Msg {
        if !matches!(self.messages.last(), Some(m) if m.role == Role::Assistant) {
            self.messages
                .push(Msg { role: Role::Assistant, blocks: Vec::new(), usage: None, entry_id: None });
        }
        self.messages.last_mut().expect("just pushed")
    }

    fn assistant_slot(&mut self, content_index: usize, fresh: Block) -> &mut Block {
        let msg = self.last_assistant();
        while msg.blocks.len() <= content_index {
            let pad = msg.blocks.len();
            msg.blocks.push(Block::Text { content_index: pad, text: String::new() });
        }
        let same_kind = match (&msg.blocks[content_index], &fresh) {
            (Block::Text { .. }, Block::Text { .. })
            | (Block::Thinking { .. }, Block::Thinking { .. })
            | (Block::ToolCall { .. }, Block::ToolCall { .. }) => true,
            _ => false,
        };
        if !same_kind {
            msg.blocks[content_index] = fresh;
        }
        &mut msg.blocks[content_index]
    }

    fn ingest_message(
        &mut self,
        role: &str,
        blocks: Vec<Block>,
        usage: Option<Usage>,
        time: Option<String>,
        entry_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        match role {
            "user" => {
                self.messages.push(Msg { role: Role::User, blocks, usage: None, entry_id });
            }
            "assistant" => {
                self.messages.push(Msg {
                    role: Role::Assistant,
                    blocks,
                    usage: usage.map(|u| UsageLine {
                        input: u.input,
                        output: u.output,
                        cache_read: u.cache_read,
                        cost: u.cost,
                        time: time.clone().unwrap_or_default(),
                    }),
                    entry_id: None,
                });
            }
            "toolResult" => {
                let text: String = blocks
                    .iter()
                    .map(|b| match b {
                        Block::Text { text, .. } => text.as_str(),
                        _ => "",
                    })
                    .collect::<Vec<_>>()
                    .join("")
                    .trim_end()
                    .to_string();
                if let Some(m) = self.messages.last_mut() {
                    if m.role == Role::Assistant {
                        if let Some(Block::ToolCall { result, .. }) = m
                            .blocks
                            .iter_mut()
                            .rev()
                            .find(|b| matches!(b, Block::ToolCall { .. }))
                        {
                            result.push_str(&text);
                        }
                    }
                }
            }
            _ => {}
        }
        self.notify_list(cx);
    }

    /// 引导：中断当前运行并立即注入此消息（rpc steer）。
    fn steer_input(&mut self, cx: &mut Context<Self>) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        let images: Vec<serde_json::Value> = self
            .pending_images
            .iter()
            .map(|img| {
                serde_json::json!({
                    "type": "image", "data": img.data_b64, "mimeType": img.mime
                })
            })
            .collect();
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::Steer { message: text, images });
        }
        self.input.clear();
        self.pending_images.clear();
        cx.notify();
    }

    /// 后续消息：Agent 完成后排队此消息（rpc follow_up）。
    fn follow_up_input(&mut self, cx: &mut Context<Self>) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::FollowUp { message: text });
        }
        self.input.clear();
        self.pending_images.clear();
        cx.notify();
    }

    /// 停止（rpc abort）。
    fn abort_stream(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::Abort);
        }
        self.stream_started = None;
        cx.notify();
    }

    fn send_input(&mut self, cx: &mut Context<Self>) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("未连接").into();
            cx.notify();
            return;
        };
        let streaming = self.state.as_ref().is_some_and(|s| s.is_streaming);
        let images: Vec<serde_json::Value> = self
            .pending_images
            .iter()
            .map(|img| {
                serde_json::json!({
                    "type": "image",
                    "data": img.data_b64,
                    "mimeType": img.mime
                })
            })
            .collect();
        let cmd = if streaming {
            Command::Steer { message: text.clone(), images: images.clone() }
        } else {
            Command::Prompt { message: text.clone(), images }
        };
        match session.send(&cmd) {
            Ok(_) => {
                if self.history.last().map(|h| h != &text).unwrap_or(true) {
                    self.history.push(text.clone());
                }
                self.history_ix = None;
                self.input.clear();
                self.pending_images.clear();
                self.status = if streaming { "steering" } else { "running" }.into();
                if !streaming {
                    // pi-web optimistic append: the sent bubble shows up
                    // immediately, RPC echo later upgrades it in place
                    self.messages.push(Msg {
                        role: Role::User,
                        blocks: vec![Block::Text { content_index: 0, text }],
                        usage: None,
                        entry_id: None,
                    });
                    self.phase_waiting = true;
                    self.notify_list(cx);
                }
            }
            Err(e) => self.status = e,
        }
        cx.notify();
    }

    fn send_follow_up(&mut self, cx: &mut Context<Self>) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::FollowUp { message: text });
            self.input.clear();
            self.pending_images.clear();
            self.status = "queued".into();
        }
        cx.notify();
    }

    fn abort(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::Abort);
            self.status = "aborting".into();
            cx.notify();
        }
    }

    /// Begin an inline rename (pi-web SessionSidebar: the row becomes an
    /// input with the current title selected; typing replaces it).
    fn start_rename(&mut self, path: PathBuf, prefill: String, cx: &mut Context<Self>) {
        let weak_ok = cx.entity().downgrade();
        let weak_esc = cx.entity().downgrade();
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .select_all_on_focus()
                .placeholder(tr("session name"))
        });
        input.update(cx, |ti, cx| ti.set_value(prefill, cx));
        input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |v, cx| {
                let _ = weak_ok.update(cx, |c, cx| c.apply_rename(v.trim().to_string(), cx));
            }));
            ti.set_on_escape(Box::new(move |cx| {
                let _ = weak_esc.update(cx, |c, cx| {
                    c.renaming = None;
                    c.rename_input = None;
                    cx.notify();
                });
            }));
        });
        self.renaming = Some(path);
        self.rename_input = Some(input);
        cx.notify();
    }

    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.renaming = None;
        self.rename_input = None;
        cx.notify();
    }

    /// Model-select dialog with a live filter input.
    fn model_select_dialog(cx: &mut Context<Self>) -> Dialog {
        let weak_change = cx.entity().downgrade();
        let weak_esc = cx.entity().downgrade();
        let input = cx.new(|cx| TextInput::new(cx).placeholder("filter models..."));
        input.update(cx, |ti, _| {
            ti.set_on_change(Box::new(move |_, cx| {
                let _ = weak_change.update(cx, |_, cx| cx.notify());
            }));
            ti.set_on_escape(Box::new(move |cx| {
                let _ = weak_esc.update(cx, |c, cx| {
                    c.dialog = None;
                    cx.notify();
                });
            }));
        });
        Dialog::ModelSelect { input }
    }

    /// Rename commit path that never reads the input entity (called from
    /// the input's own submit callback where the entity is borrowed).
    fn apply_rename(&mut self, name: String, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::SetSessionName { name });
        }
        // sidebar label comes from the session file's `session_info` entry;
        // reload it when pi confirms the write (set_session_name response —
        // an immediate re-read here would race the file flush)
        self.refresh_state();
        self.renaming = None;
        self.rename_input = None;
        cx.notify();
    }

    fn select_model(&mut self, provider: String, id: String, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::SetModel { provider, model: id });
        }
        self.dialog = None;
        self.refresh_state();
        cx.notify();
    }

    /// Thinking level from the pill menu. "auto" clears the local override
    /// (pi default governs, pi-web parity — no RPC); other levels are sent.
    fn set_thinking_level(&mut self, level: &str, cx: &mut Context<Self>) {
        if level == "auto" {
            self.thinking_override = None;
            cx.notify();
            return;
        }
        self.thinking_override = Some(level.to_string());
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::SetThinkingLevel { level: level.to_string() });
        }
        cx.notify();
    }

    /// Current tools preset key from settings.json defaultTools
    /// (pi-web tool-presets.ts resolution; "" = custom list).
    fn tool_preset_key(&self) -> &'static str {
        match &self.mc_default_tools {
            None => "configured",
            Some(list) if list.is_empty() => "chat-only",
            Some(list) if list == &vec!["read".to_string(), "grep".to_string(), "find".to_string(), "ls".to_string()] => "read-only",
            Some(list) if list == &vec!["read".to_string(), "bash".to_string(), "edit".to_string(), "write".to_string()] => "default",
            Some(list) if list == &vec!["bash".to_string(), "read".to_string(), "edit".to_string(), "write".to_string(), "grep".to_string(), "find".to_string(), "ls".to_string()] => "full",
            _ => "",
        }
    }

    fn tool_preset_label(&self) -> &'static str {
        match self.tool_preset_key() {
            "" => "configured",
            other => other,
        }
    }

    /// Editor toolbar 压缩: rpc compact (summarize the context).
    fn compact_session(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::Compact);
            self.status = tr("压缩中…").to_string();
            cx.notify();
        }
    }

    /// LLM session title (pi-web lib/session-title.ts parity via a one-off
    /// `pi --no-session --print` run; the in-process SDK call pi-web uses is
    /// not reachable over RPC).
    fn auto_title(&mut self, cx: &mut Context<Self>) {
        if self.titling {
            return;
        }
        let transcript = build_title_transcript(
            &self
                .messages
                .iter()
                .map(|m| TitleTurn { user: m.role == Role::User, text: m.plain_text() })
                .collect::<Vec<_>>(),
        );
        if transcript.is_empty() {
            self.status = tr("nothing to title yet").to_string();
            cx.notify();
            return;
        }
        let Some(tx) = self.title_tx.clone() else { return };
        self.titling = true;
        self.status = tr("生成标题…").to_string();
        cx.notify();

        // cheap one-shot: no tools, thinking off, current session's model
        let mut args: Vec<String> = vec![
            "--no-session".into(),
            "--print".into(),
            "--no-tools".into(),
            "--thinking".into(),
            "off".into(),
        ];
        if let Some(model) = self.state.as_ref().and_then(|s| s.model.clone()) {
            args.push("--provider".into());
            args.push(model.provider.clone());
            args.push("--model".into());
            args.push(model.id.clone());
        }
        args.push("--system-prompt".into());
        let (title_sys_prompt, title_prompt) = crate::services::title::title_prompts();
        args.push(title_sys_prompt.into());
        args.push("--".into());
        args.push(format!("{transcript}\n\n{title_prompt}"));

        let cwd = self.cwd.clone();
        std::thread::spawn(move || {
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let result = pi_link::vendor::run_cli_stdout(&cwd, &arg_refs)
                .map(|out| sanitize_title(&out))
                .and_then(|t| if t.is_empty() { Err("empty title".into()) } else { Ok(t) });
            let _ = tx.unbounded_send(result);
        });
        cx.notify();
    }

    fn on_title_result(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        self.titling = false;
        match result {
            Ok(title) => {
                if let Some(session) = &self.agent.session {
                    let _ = session.send(&Command::SetSessionName { name: title.clone() });
                }
                self.status = tr("已生成标题: {title}").replace("{title}", &title);
                self.refresh_state();
            }
            Err(e) => {
                self.status = tr("标题生成失败: {e}").replace("{e}", &e);
            }
        }
        cx.notify();
    }

    /// Request an export (the exported HTML embeds the live session's
    /// systemPrompt + tool definitions, which the RPC surface does not
    /// expose directly). The Response handler parses + caches them.
    fn request_system_info(&mut self, cx: &mut Context<Self>) {
        if self.session_tools.is_some() && self.sys_prompt.is_some() {
            return;
        }
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::ExportHtml);
        }
        cx.notify();
    }

    fn export_html(&mut self, cx: &mut Context<Self>) {
        self.request_system_info(cx);
    }

    fn new_session(&mut self, cx: &mut Context<Self>) {
        let events = self.agent.spawn(&self.cwd, None);
        self.messages.clear();
        self.state = None;
        self.stats = None;
        self.active_session_file = None;
        self.collapsed.clear();
        self.phase_waiting = false;
        self.agent_running = false;
        self.renaming = None;
        self.rename_input = None;
        clear_last_open(&self.cwd.to_string_lossy());
        self.status = status_line(self.agent.session.is_some(), tr("新会话"));
        self.refresh_state();
        self.refresh_git();
        self.load_project_files();
        if let Some(events) = events {
            let epoch = self.agent.epoch;
            cx.spawn(async move |this, cx| {
                consume_events(this, cx, events, epoch).await;
            })
            .detach();
        }
        self.list.reset(0);
        cx.notify();
    }

    fn open_session(&mut self, path: PathBuf, rename: bool, cx: &mut Context<Self>) {
        let cwd = self
            .sessions
            .iter()
            .find(|s| s.path == path)
            .map(|s| PathBuf::from(s.cwd.clone()))
            .unwrap_or_else(|| self.cwd.clone());
        let events = self.agent.spawn(&cwd, Some(&path));
        self.cwd = cwd;
        set_last_open(&self.cwd.to_string_lossy(), &path.to_string_lossy());
        self.expanded_dirs.clear();
        self.branch = read_branch(&self.cwd);
        self.messages.clear();
        self.state = None;
        self.stats = None;
        self.active_session_file = None;
        self.collapsed.clear();
        self.pending_rename = rename;
        self.phase_waiting = false;
        self.agent_running = false;
        self.confirm_delete = None;
        self.renaming = None;
        self.rename_input = None;
        self.status = status_line(self.agent.session.is_some(), "resuming");
        // disk-direct: render the tail from the session file before the RPC
        // snapshot lands; the authoritative get_messages response then
        // replaces it (full history)
        for msg in read_tail_messages(&path, 256 * 1024, 100) {
            let m = &msg["message"];
            let role = m["role"].as_str().unwrap_or("");
            let blocks = content_blocks(&m["content"]);
            let usage = Usage::parse(&m["usage"]);
            self.ingest_message(role, blocks, usage, None, None, cx);
        }
        self.notify_list(cx);
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::GetMessages);
            // branch tree snapshot for the fork panel
            let _ = session.send(&Command::GetTree);
        }
        self.refresh_state();
        self.load_project_files();
        if let Some(events) = events {
            let epoch = self.agent.epoch;
            cx.spawn(async move |this, cx| {
                consume_events(this, cx, events, epoch).await;
            })
            .detach();
        }
        self.list.reset(0);
        cx.notify();
    }

    /// pi-web DELETE /api/sessions/{id}: unlink the session file; a live
    /// (active) session is aborted + shut down first and the shell resets
    /// to a fresh draft with the same cwd.
    fn delete_session(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.confirm_delete = None;
        let was_active = self.active_session_file.as_deref() == Some(path.as_path());
        if was_active {
            if let Some(session) = &self.agent.session {
                let _ = session.send(&Command::Abort);
            }
            // respawn without a session file = pi-web new-draft-with-same-cwd
            // (drops the old pi process, clearing the file handle on Windows)
            self.new_session(cx);
        }
        match std::fs::remove_file(&path) {
            Ok(_) => {
                self.sessions.retain(|s| s.path != path);
                self.sessions_list.reset(self.sessions.len());
                self.status = status_line(self.agent.session.is_some(), "session deleted");
            }
            Err(e) => {
                self.status = crate::i18n::tf("删除失败: {e}", &[("e", e.to_string())])
            }
        }
        self.refresh_sessions();
        cx.notify();
    }

    /// Open a file as a right-panel tab (pi-web file tabs; replaces the
    /// old preview dialog). Re-activates an existing tab for the path.
    fn open_file_tab(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        const MAX: u64 = 200 * 1024;
        let too_big = std::fs::metadata(&path).map(|m| m.len() > MAX).unwrap_or(false);
        let content = if too_big {
            "(file too large to preview)".to_string()
        } else {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    if bytes.contains(&0) {
                        "(binary file)".to_string()
                    } else {
                        String::from_utf8_lossy(&bytes).to_string()
                    }
                }
                Err(e) => format!("read failed: {e}"),
            }
        };
        self.file_cache.insert(path.clone(), FileTab { path: path.clone(), content, truncated: too_big });
        self.dialog = Some(Dialog::FilePreview { path });
        cx.notify();
    }

    fn close_panel_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.panel_tabs.len() {
            return;
        }
        let removed = self.panel_tabs.remove(ix);
        if let PanelTab::Term(id) = &removed {
            if let Some(tix) = self.terminals.iter().position(|t| t.id == *id) {
                let _ = self.terminals[tix].pty.send(alacritty_terminal::event_loop::Msg::Shutdown);
                self.terminals.remove(tix);
            }
        }
        self.active_panel_tab = match self.active_panel_tab {
            Some(a) if a >= self.panel_tabs.len() => {
                if self.panel_tabs.is_empty() {
                    None
                } else {
                    Some(a.saturating_sub(1))
                }
            }
            other => other,
        };
        cx.notify();
    }

    /// File viewer meta line: language · lines · size (pi-web FileViewer).
    fn file_meta(path: &Path, content: &str) -> String {
        let lang = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("txt")
            .to_string();
        let lines = content.lines().count();
        let bytes = content.len();
        let size = if bytes < 1024 {
            format!("{bytes} B")
        } else {
            format!("{:.1} KB", bytes as f64 / 1024.)
        };
        format!("{lang} · {lines} lines · {size}")
    }

    fn is_markdown(path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e == "md" || e == "markdown")
    }

    fn attach_images(&mut self, cx: &mut Context<Self>) {
        let opts = gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        };
        let rx = cx.prompt_for_paths(opts);
        cx.spawn(async move |this, cx| {
            let picked = rx.await.ok().and_then(|r| r.ok()).flatten();
            let Some(paths) = picked else {
                return;
            };
            let _ = this.update(cx, |chat, cx| {
                for path in paths {
                    let Ok(bytes) = std::fs::read(&path) else { continue };
                    use base64::Engine as _;
                    let data_b64 =
                        base64::engine::general_purpose::STANDARD.encode(&bytes);
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "image".into());
                    let mime = mime_from_ext(&path);
                    chat.pending_images
                        .push(AttachedImage { name, data_b64, mime });
                }
                if !chat.pending_images.is_empty() {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn load_project_files_pub(&mut self) {
        self.load_project_files();
    }

    fn active_menu(&self) -> Option<MenuKind> {
        let input = &self.input;
        if input.starts_with('/') && !input[1..].contains(char::is_whitespace) {
            return Some(MenuKind::Slash);
        }
        if let Some(at) = input.rfind('@') {
            if !input[at..].contains(char::is_whitespace) && input[at + 1..].len() < 64 {
                return Some(MenuKind::At);
            }
        }
        None
    }

    fn menu_items(&self) -> Vec<MenuItem> {
        match self.active_menu() {
            Some(MenuKind::Slash) => {
                let q = self.input[1..].to_lowercase();
                self.commands
                    .iter()
                    .filter(|c| q.is_empty() || c.name.to_lowercase().starts_with(&q))
                    .take(8)
                    .map(|c| MenuItem {
                        insert: c.name.clone(),
                        title: format!("/{}", c.name),
                        desc: c.description.clone(),
                    })
                    .collect()
            }
            Some(MenuKind::At) => {
                let at = self.input.rfind('@').unwrap_or(0);
                let q = self.input[at + 1..].to_lowercase();
                self.project_files
                    .iter()
                    .filter(|f| q.is_empty() || f.to_lowercase().contains(&q))
                    .take(8)
                    .map(|f| MenuItem {
                        insert: f.clone(),
                        title: f.clone(),
                        desc: String::new(),
                    })
                    .collect()
            }
            None => Vec::new(),
        }
    }

    fn accept_menu(&mut self, insert: String, cx: &mut Context<Self>) {
        match self.active_menu() {
            Some(MenuKind::Slash) => self.input = format!("/{insert} "),
            Some(MenuKind::At) => {
                if let Some(at) = self.input.rfind('@') {
                    self.input = format!("{}{} ", &self.input[..=at], insert);
                }
            }
            None => {}
        }
        self.menu_ix = 0;
        cx.notify();
    }

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Response { command, success, error, data, .. } => {
                if command == "get_state" && success {
                    if let Some(data) = &data {
                        self.state = Some(SessionState::parse(data));
                    }
                    if self.pending_rename {
                        self.pending_rename = false;
                        let prefill = self
                            .state
                            .as_ref()
                            .and_then(|s| s.session_name.clone())
                            .or_else(|| {
                                self.messages
                                    .iter()
                                    .find(|m| matches!(m.role, Role::User))
                                    .map(|m| m.plain_text())
                            })
                            .map(|v| v.chars().take(50).collect::<String>())
                            .unwrap_or_default();
                        if let Some(path) = self.active_session_file.clone() {
                            self.start_rename(path, prefill, cx);
                        }
                    }
                } else if command == "get_session_stats" && success {
                    if let Some(data) = &data {
                        self.stats = Some(SessionStats::parse(data));
                        self.active_session_file =
                            data["sessionFile"].as_str().map(PathBuf::from);
                    }
                } else if command == "set_session_name" && success {
                    // pi flushed the name to the session file — reload the
                    // sidebar labels (pi-web onRenamed → loadSessions), with
                    // one delayed pass to cover flush lag
                    self.refresh_sessions();
                    cx.notify();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(500))
                            .await;
                        let _ = this.update(cx, |c, cx| {
                            c.refresh_sessions();
                            cx.notify();
                        });
                    })
                    .detach();
                } else if command == "get_commands" && success {
                    if let Some(data) = &data {
                        self.commands = SlashCommand::parse_list(data);
                    }
                } else if command == "export_html" && success {
                    if let Some(path) = data
                        .as_ref()
                        .and_then(|d| d["path"].as_str())
                        .map(PathBuf::from)
                    {
                        if let Ok(html) = std::fs::read_to_string(&path) {
                            let (prompt, tools) = parse_export_html(&html);
                            if self.sys_prompt.is_none() {
                                self.sys_prompt = prompt;
                            }
                            if self.session_tools.is_none() && !tools.is_empty() {
                                self.session_tools = Some(tools);
                            }
                        }
                        let _ = std::fs::remove_file(&path);
                        cx.notify();
                    }
                } else if command == "get_available_models" && success {
                    if let Some(data) = &data {
                        self.available_models =
                            pi_link::protocol::parse_model_list(data);
                    }
                } else if command == "get_tree" && success {
                    if let Some(data) = &data {
                        let (tree, leaf) = parse_tree(data);
                        self.active_user_entry_ids =
                            collect_path_user_ids(&tree, leaf.as_deref());
                        let mut ids = self.active_user_entry_ids.iter();
                        for m in self.messages.iter_mut() {
                            if m.role == Role::User {
                                m.entry_id = ids.next().cloned();
                            }
                        }
                        self.branch_tree = Some((tree, leaf));
                    }
                } else if command == "fork" {
                    if success {
                        // pi rebound this process to the branched session.
                        self.branch_tree = None;
                        self.messages.clear();
                        self.notify_list(cx);
                        if let Some(s) = self.agent.session.as_ref() {
                            let _ = s.send(&Command::GetState);
                            let _ = s.send(&Command::GetMessages);
                            let _ = s.send(&Command::GetTree);
                        }
                        // branch_tree was refreshed by the GetTree request below;
                        // message entry ids re-map there as well.
                        self.refresh_sessions();
                        self.status = "forked".into();
                    } else {
                        self.status = format!(
                            "fork failed: {}",
                            error.unwrap_or_default()
                        );
                    }
                } else if command == "export_html" && success {
                    self.status = format!(
                        "exported: {}",
                        data.and_then(|d| d["path"].as_str().map(str::to_string))
                            .unwrap_or_default()
                    );
                } else if command == "get_messages" && success {
                    if let Some(data) = &data {
                        for msg in data["messages"].as_array().into_iter().flatten() {
                            let role = msg["role"].as_str().unwrap_or("");
                            let blocks = content_blocks(&msg["content"]);
                            let usage = Usage::parse(&msg["usage"]);
                            self.ingest_message(role, blocks, usage, None, None, cx);
                        }
                        // map user messages to active-path entry ids (fork anchors)
                        let mut ids = self.active_user_entry_ids.iter();
                        for m in self.messages.iter_mut() {
                            if m.role == Role::User {
                                m.entry_id = ids.next().cloned();
                            }
                        }
                        self.notify_list(cx);
                    }
                    self.status = status_line(true, "resumed");
                } else if success {
                    self.status = format!("{command} ok");
                } else {
                    self.status =
                        format!("{command} failed: {}", error.unwrap_or_default());
                }
            }
            Event::MessageStart { role, blocks, timestamp } => {
                match role.as_str() {
                    "user" => {
                        // upgrade the optimistic send bubble in place instead
                        // of pushing a duplicate echo (pi-web
                        // optimisticUserMessageKey parity); image blocks and
                        // entry ids ride in with the echo
                        let echo_text = blocks
                            .iter()
                            .map(|b| match b {
                                Block::Text { text, .. } => text.as_str(),
                                _ => "",
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        let optimistic = self.phase_waiting
                            && matches!(self.messages.last(), Some(m)
                                if m.role == Role::User && m.plain_text() == echo_text);
                        if optimistic {
                            if let Some(m) = self.messages.last_mut() {
                                m.blocks = blocks;
                            }
                            self.phase_waiting = false;
                        } else {
                            self.messages
                                .push(Msg { role: Role::User, blocks, usage: None, entry_id: None });
                        }
                    }
                    "assistant" => {
                        self.phase_waiting = false;
                        self.messages.push(Msg {
                            role: Role::Assistant,
                            blocks,
                            usage: None,
                            entry_id: None,
                        });
                    }
                    "toolResult" => {
                        let text: String = blocks
                            .iter()
                            .map(|b| match b {
                                Block::Text { text, .. } => text.as_str(),
                                _ => "",
                            })
                            .collect::<Vec<_>>()
                            .join("")
                            .trim_end()
                            .to_string();
                        if let Some(m) = self.messages.last_mut() {
                            if m.role == Role::Assistant {
                                if let Some(Block::ToolCall { result, .. }) = m
                                    .blocks
                                    .iter_mut()
                                    .rev()
                                    .find(|b| matches!(b, Block::ToolCall { .. }))
                                {
                                    result.push_str(&text);
                                }
                            }
                        }
                    }
                    _ => {}
                }
                let _ = timestamp;
            }
            Event::MessageUpdate(assistant_event) => {
                self.phase_waiting = false;
                match assistant_event {
                AssistantEvent::TextDelta { content_index, delta } => {
                    if let Block::Text { text, .. } = self.assistant_slot(
                        content_index,
                        Block::Text { content_index, text: String::new() },
                    ) {
                        text.push_str(&delta);
                    }
                }
                AssistantEvent::TextEnd { content_index, content } => {
                    if let Block::Text { text, .. } = self.assistant_slot(
                        content_index,
                        Block::Text { content_index, text: String::new() },
                    ) {
                        *text = content;
                    }
                }
                AssistantEvent::ThinkingStart { content_index } => {
                    self.assistant_slot(
                        content_index,
                        Block::Thinking { content_index, text: String::new() },
                    );
                }
                AssistantEvent::ThinkingDelta { content_index, delta } => {
                    if let Block::Thinking { text, .. } = self.assistant_slot(
                        content_index,
                        Block::Thinking { content_index, text: String::new() },
                    ) {
                        text.push_str(&delta);
                    }
                }
                AssistantEvent::ThinkingEnd { content_index, content } => {
                    if let Block::Thinking { text, .. } = self.assistant_slot(
                        content_index,
                        Block::Thinking { content_index, text: String::new() },
                    ) {
                        *text = content;
                        // pi-web parity: collapse thinking once it completes
                        let msg_ix = self.messages.len().saturating_sub(1);
                        self.collapsed.insert((msg_ix, content_index));
                    }
                }
                AssistantEvent::ToolCallStart { content_index, id, tool_name } => {
                    self.assistant_slot(
                        content_index,
                        Block::ToolCall {
                            content_index,
                            id,
                            name: tool_name,
                            args: String::new(),
                            result: String::new(),
                        },
                    );
                }
                AssistantEvent::ToolCallDelta { content_index, delta } => {
                    if let Block::ToolCall { args, .. } = self.assistant_slot(
                        content_index,
                        Block::ToolCall {
                            content_index,
                            id: String::new(),
                            name: String::new(),
                            args: String::new(),
                            result: String::new(),
                        },
                    ) {
                        args.push_str(&delta);
                    }
                }
                AssistantEvent::ToolCallEnd { content_index, tool_call } => {
                    let name = tool_call["toolName"]
                        .as_str()
                        .or_else(|| tool_call["name"].as_str())
                        .unwrap_or("")
                        .to_string();
                    let args = tool_call["arguments"]
                        .as_object()
                        .filter(|o| !o.is_empty())
                        .map(|o| serde_json::Value::Object(o.clone()).to_string())
                        .unwrap_or_default();
                    let name_c = name;
                    let args_c = args;
                    if let Block::ToolCall { name, args, .. } = self.assistant_slot(
                        content_index,
                        Block::ToolCall {
                            content_index,
                            id: String::new(),
                            name: name_c.clone(),
                            args: args_c.clone(),
                            result: String::new(),
                        },
                    ) {
                        *name = name_c;
                        *args = args_c;
                    }
                }
                AssistantEvent::Other(_) => {}
                }
            }
            Event::MessageEnd { role, blocks, usage, timestamp } => {
                if role == "assistant" {
                    if let Some(m) = self.messages.last_mut() {
                        if m.role == Role::Assistant {
                            m.blocks = blocks;
                            m.usage = usage.map(|u| UsageLine {
                                input: u.input,
                                output: u.output,
                                cache_read: u.cache_read,
                                cost: u.cost,
                                time: timestamp.map(fmt_hhmm).unwrap_or_default(),
                            });
                        }
                    }
                }
            }
            Event::AgentStart => {
                self.agent_running = true;
                self.status = "running".into();
                if self.stream_started.is_none() {
                    self.stream_started = Some(std::time::Instant::now());
                }
            }
            Event::AgentSettled => {
                self.agent_running = false;
                self.phase_waiting = false;
                self.status = status_line(true, "idle");
                self.stream_started = None;
                self.refresh_state();
            }
            Event::AgentEnd { .. } => {
                self.agent_running = false;
                self.phase_waiting = false;
                if self.sound_on {
                    play_notify_sound();
                }
                self.stream_started = None;
                // the session file exists now — make the new session show up
                // in the sidebar (pi-web refreshKey-on-agent_end parity)
                self.refresh_sessions();
                // refresh branch tree so newly-sent user messages gain entry ids
                if let Some(s) = self.agent.session.as_ref() {
                    let _ = s.send(&Command::GetTree);
                }
                // agent may have written files: refresh git status
                self.refresh_git();
            }
            Event::ExtensionUi(req) => self.on_ext_ui(req, cx),
            Event::Unparsed(_) => {}
        }
        self.notify_list(cx);
    }
}

async fn consume_events(
    this: gpui::WeakEntity<Chat>,
    cx: &mut gpui::AsyncApp,
    mut rx: UnboundedReceiver<Event>,
    epoch: u64,
) {
    while let Some(event) = rx.next().await {
        let stale = this
            .update(cx, |chat, cx| {
                if chat.agent.epoch != epoch {
                    return true;
                }
                chat.on_event(event, cx);
                false
            })
            .unwrap_or(true);
        if stale {
            return;
        }
    }
    let _ = this.update(cx, |chat, cx| {
        if chat.agent.epoch == epoch {
            chat.status = "pi exited".into();
            cx.notify();
        }
    });
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
impl gpui::EntityInputHandler for Chat {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        adjusted_range: &mut Option<std::ops::Range<usize>>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<String> {
        let text: Vec<u16> = self.input.encode_utf16().collect();
        let slice: String = text
            .get(range.start..range.end)?
            .iter()
            .map(|&u| char::from_u32(u as u32).unwrap_or('\u{fffd}'))
            .collect();
        adjusted_range.replace(range);
        Some(slice)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        // hand-rolled editor: caret at end, empty selection
        let end = self.input.encode_utf16().count();
        Some(gpui::UTF16Selection { range: end..end, reversed: false })
    }

    fn marked_text_range(
        &self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        self.ime_marked.clone()
    }

    fn unmark_text(&mut self, _window: &mut gpui::Window, _cx: &mut gpui::Context<Self>) {
        self.ime_marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) {
        match range.or_else(|| self.ime_marked.clone()) {
            Some(r) => {
                let start = self.utf16_to_char_offset(r.start);
                let end = self.utf16_to_char_offset(r.end);
                self.input.replace_range(start..end, text);
            }
            None => self.input.push_str(text),
        }
        self.ime_marked = None;
        self.menu_ix = 0;
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<std::ops::Range<usize>>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) {
        // composition update: swap the marked span for the new composition
        // string, then re-mark it
        let range = range.or_else(|| self.ime_marked.clone());
        let start = match &range {
            Some(r) => self.utf16_to_char_offset(r.start),
            None => self.input.chars().count(),
        };
        let end = range
            .as_ref()
            .map(|r| self.utf16_to_char_offset(r.end))
            .unwrap_or(start);
        self.input.replace_range(start..end, new_text);
        let start_u16 = self.input.chars().take(start).map(char::len_utf16).sum();
        let new_len = new_text.encode_utf16().count();
        self.ime_marked = Some(start_u16..start_u16 + new_len);
    }

    fn bounds_for_range(
        &mut self,
        _range: std::ops::Range<usize>,
        element_bounds: gpui::Bounds<gpui::Pixels>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        // IME candidate window anchors to the editor container
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<gpui::Pixels>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Chat {
    /// utf16 offset -> char offset for self.input
    fn utf16_to_char_offset(&self, u16_offset: usize) -> usize {
        let mut u16_count = 0usize;
        for (char_ix, ch) in self.input.chars().enumerate() {
            if u16_count >= u16_offset {
                return char_ix;
            }
            u16_count += ch.len_utf16();
        }
        self.input.chars().count()
    }
}

/// Invisible paint-phase element that registers the chat editor as the
/// window's InputHandler when focused (required for Windows IME).
pub struct EditorInputElement {
    focus: gpui::FocusHandle,
    view: gpui::Entity<Chat>,
    interactivity: gpui::Interactivity,
}

impl EditorInputElement {
    pub fn new(view: gpui::Entity<Chat>, focus: gpui::FocusHandle) -> Self {
        Self {
            focus,
            view,
            interactivity: gpui::Interactivity::new(),
        }
    }
}

impl gpui::IntoElement for EditorInputElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::Styled for EditorInputElement {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl gpui::Element for EditorInputElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

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
        window: &mut gpui::Window,
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
        _global_id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: gpui::Bounds<gpui::Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut gpui::Window,
        _cx: &mut gpui::App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _global_id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        window.handle_input(
            &self.focus,
            gpui::ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
    }
}


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
        // keep terminal focus alive across frames (render focuses chat input
        // otherwise, which would steal it back every redraw)
        //
        // dialog inputs own their focus handles; force-focus only when the
        // input isn't already focused so click-to-focus still works
        let dialog_input = match &self.dialog {
            Some(Dialog::ModelSelect { input }) => Some(input.clone()),
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

        let status: SharedString = self.status.clone().into();
        let streaming = self
            .state
            .as_ref()
            .is_some_and(|st| st.is_streaming);
        let model_label: SharedString = self
            .state
            .as_ref()
            .and_then(|s| s.model_label())
            .unwrap_or_else(|| tr("选择模型").into())
            .into();
        let thinking_label: SharedString = self
            .state
            .as_ref()
            .and_then(|s| s.thinking_level.clone())
            .unwrap_or_else(|| "medium".into())
            .into();

        let entity = cx.entity();
        let weak = entity.downgrade();
        let weak_for_dialog = weak.clone();

        let stats_right: SharedString = if let Some(st) = &self.stats {
            format!(
                "\u{2191}{} \u{2193}{} \u{27f3}{} ${:.2}  {}% / {}",
                fmt_compact(st.input),
                fmt_compact(st.output),
                fmt_compact(st.cache_read),
                st.cost,
                st.context_percent.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                st.context_window.map(fmt_compact).unwrap_or_else(|| "-".into())
            )
            .into()
        } else {
            SharedString::from("")
        };

        // editor focus + caret (render refreshes input_focused for the blink pump)
        let input_focused = self.focus.is_focused(window);
        self.input_focused = input_focused;
        let caret_on = self.caret_on;
        let this_input: SharedString = self.input.clone().into();
        let thinking_menu_open = self.pill_menu == Some(PillMenu::Thinking);
        let tools_menu_open = self.pill_menu == Some(PillMenu::Tools);
        let tools_label = self.tool_preset_label();
        let thinking_label: SharedString = self
            .thinking_override
            .clone()
            .or_else(|| {
                self.state
                    .as_ref()
                    .and_then(|st| st.thinking_level.clone())
            })
            .unwrap_or_else(|| "auto".to_string())
            .into();
        let right_px = 24.;
        // popup menu overlay anchored above the editor toolbar row
        let pill_menu_el = self.pill_menu.map(|menu| {
            let weak_menu = weak.clone();
            let rows: Vec<(String, String, bool)> = match menu {
                PillMenu::Thinking => [
                    ("auto", tr("使用 pi 默认设置"), self.thinking_override.is_none()),
                    ("low", tr("低强度推理"), self.thinking_override.as_deref() == Some("low")),
                    ("high", tr("高强度推理"), self.thinking_override.as_deref() == Some("high")),
                    ("max", tr("最强推理"), self.thinking_override.as_deref() == Some("max")),
                ]
                .iter()
                .map(|(k, d, on)| (k.to_string(), d.to_string(), *on))
                .collect(),
                PillMenu::Tools => [
                    ("configured", tr("取自 settings.json 的 defaultTools"), self.tool_preset_key() == "configured"),
                    ("chat-only", tr("仅聊天"), self.tool_preset_key() == "chat-only"),
                    ("read-only", tr("4 个只读内置工具"), self.tool_preset_key() == "read-only"),
                    ("default", tr("4 个内置工具"), self.tool_preset_key() == "default"),
                    ("full", tr("全部内置工具"), self.tool_preset_key() == "full"),
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

        // ---- main column -------------------------------------------------
        let chat_entity = entity.clone();
        let weak_for_msg = weak.clone();
        let main_col = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(t.assistant_bg))
            .text_color(rgb(t.text))
            .font_family("Segoe UI")
            // top toolbar
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .px_3()
                    .py_1p5()
                    .border_b_1()
                    .border_color(rgb(t.border))
                    .child(pill("tb-sidebar", "panel-left", SharedString::from("")))
                    .child(pill(
                        "tb-history",
                        "history",
                        SharedString::from(tr("完整历史")),
                    ))
                    .child(
                        div()
                            .id("tb-branch")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(t.border))
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_xs()
                            .text_color(rgb(t.text_muted))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                            .on_mouse_down(MouseButton::Left, cx.listener(
                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                    this.open_branch_tree(cx);
                                },
                            ))
                            .child(icon(
                                "git-branch",
                                12.,
                                if self
                                    .branch_tree
                                    .as_ref()
                                    .is_some_and(|(tr, _)| tree_has_branches(tr))
                                {
                                    t.accent
                                } else {
                                    t.text_muted
                                },
                            ))
                            .child(SharedString::from(tr("分支"))),
                    )
                    .child(
                        div()
                            .id("tb-title")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(t.border))
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_xs()
                            .text_color(rgb(t.text_muted))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                            .on_mouse_down(MouseButton::Left, cx.listener(
                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                    this.auto_title(cx);
                                },
                            ))
                            .child(icon("pencil", 12., t.text_muted))
                            .child(SharedString::from(tr("生成标题"))),
                    )
                    .child(
                        div()
                            .id("tb-system")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(if self.top_panel == Some(TopPanel::System) { t.accent } else { t.border }))
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_xs()
                            .text_color(rgb(if self.top_panel == Some(TopPanel::System) { t.accent } else { t.text_muted }))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                            .on_mouse_down(MouseButton::Left, cx.listener(
                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                    this.top_panel = match this.top_panel {
                                        Some(TopPanel::System) => None,
                                        _ => Some(TopPanel::System),
                                    };
                                    this.request_system_info(cx);
                                },
                            ))
                            .child(icon(
                                "file-text",
                                12.,
                                if self.sys_prompt.is_some() { t.accent } else { t.text_muted },
                            ))
                            .child(SharedString::from(tr("系统"))),
                    )
                    .child(
                        div()
                            .id("tb-tools")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(if self.top_panel == Some(TopPanel::Tools) { t.accent } else { t.border }))
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_xs()
                            .text_color(rgb(if self.top_panel == Some(TopPanel::Tools) { t.accent } else { t.text_muted }))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                            .on_mouse_down(MouseButton::Left, cx.listener(
                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                    this.top_panel = match this.top_panel {
                                        Some(TopPanel::Tools) => None,
                                        _ => Some(TopPanel::Tools),
                                    };
                                    this.request_system_info(cx);
                                },
                            ))
                            .child(icon(
                                "wrench",
                                12.,
                                if self.session_tools.is_some() { t.accent } else { t.text_muted },
                            ))
                            .child(SharedString::from(tr("工具"))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_right()
                            .text_xs()
                            .text_color(rgb(t.text_muted))
                            .child(stats_right),
                    ),
            )
            // message list (820px centered column, ChatWindow parity)
            .child(
                list(self.list.clone(), move |ix, _window, cx| {
                    let chat = chat_entity.read(cx);
                    let weak = weak_for_msg.clone();
                    match chat.messages.get(ix) {
                        Some(m) => div()
                            .w_full()
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(820.))
                                    .child(render_msg(
                                        m,
                                        ix,
                                        &weak,
                                        &chat.collapsed,
                                        t,
                                        &chat.model_label_text(),
                                        {
                                            let streaming = chat
                                                .state
                                                .as_ref()
                                                .is_some_and(|s| s.is_streaming);
                                            let is_last_assistant = streaming
                                                && Some(ix) == chat.messages.len().checked_sub(1)
                                                && m.role == Role::Assistant;
                                            if !is_last_assistant {
                                                None
                                            } else {
                                                let text: String = m
                                                    .blocks
                                                    .iter()
                                                    .map(|b| match b {
                                                        Block::Text { text, .. }
                                                        | Block::Thinking { text, .. } => {
                                                            text.as_str()
                                                        }
                                                        _ => "",
                                                    })
                                                    .collect();
                                                let est = estimate_tokens(&text);
                                                let tps = chat.stream_started.and_then(
                                                    |start| {
                                                        let secs =
                                                            start.elapsed().as_secs_f32();
                                                        (secs > 0.5 && est > 0)
                                                            .then(|| est as f32 / secs)
                                                    },
                                                );
                                                Some((est, tps))
                                            }
                                        },
                                    )),
                            )
                            .into_any_element(),
                        None => {
                            // pi-web ChatWindow phase label: pulsing text under
                            // the list while running with no streamed content
                            // yet (animate-[pulse_1.5s_infinite])
                            if chat.phase_row_visible() {
                                div()
                                    .w_full()
                                    .flex()
                                    .justify_center()
                                    .child(
                                        div()
                                            .w_full()
                                            .max_w(px(820.))
                                            .py_2()
                                            .text_size(px(13.))
                                            .text_color(rgb(t.text_muted))
                                            .child(SharedString::from(tr("正在等待模型...")))
                                            .with_animation(
                                                "phase-pulse",
                                                Animation::new(std::time::Duration::from_millis(
                                                    1500,
                                                ))
                                                .repeat()
                                                .with_easing(pulsating_between(0.5, 1.0)),
                                                |label, delta| label.opacity(delta),
                                            ),
                                    )
                                    .into_any_element()
                            } else {
                                div().w_full().into_any_element()
                            }
                        }
                    }
                })
                .flex_1()
                .min_h_0()
                .py_2(),
            )
            // empty new-session hero (pi-web ChatWindow isEmptyNew): logo row
            // directly above the editor, flex spacer below centers the pair
            .children((self.messages.is_empty()
                && !self
                    .state
                    .as_ref()
                    .is_some_and(|s| s.is_streaming))
            .then(|| {
                div()
                    .w_full()
                    .mb_3()
                    .px(px(16.))
                    .child(
                        div()
                            .max_w(px(820.))
                            .mx_auto()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .font_family("Consolas")
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2p5()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .size(px(32.))
                                            .rounded(px(8.))
                                            .bg(rgb(t.accent))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(20.))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(t.accent_contrast))
                                            .child("\u{3c0}"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(t.text))
                                            .child("pi-flash"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_end()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(rgb(t.text_muted))
                                            .child(SharedString::from(format!(
                                                "app v{}",
                                                env!("CARGO_PKG_VERSION")
                                            ))),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(rgb(t.text_muted))
                                            .child(SharedString::from(format!(
                                                "pi v{}",
                                                pi_link::vendor::vendored_version()
                                                    .unwrap_or_default()
                                            ))),
                                    ),
                            ),
                    )
                    .into_any_element()
            }))
            // extension widgets above the editor (setWidget aboveEditor)
            .children((!self.ext_widgets.is_empty()).then(|| {
                let rows: Vec<gpui::AnyElement> = self
                    .ext_widgets
                    .iter()
                    .filter(|(_, _, above)| *above)
                    .map(|(_, lines, _)| render_ext_widget(lines, t))
                    .collect();
                (!rows.is_empty()).then(|| div().px_4().flex().flex_col().gap_1().children(rows).into_any_element())
            }).flatten())
            .child(session::input::input_area(self, entity.clone(), &weak, streaming, input_focused, caret_on, this_input, model_label, thinking_menu_open, tools_menu_open, thinking_label, tools_label, cx))
            // extension widgets below the editor (setWidget belowEditor)
            .children((!self.ext_widgets.is_empty()).then(|| {
                let rows: Vec<gpui::AnyElement> = self
                    .ext_widgets
                    .iter()
                    .filter(|(_, _, above)| !above)
                    .map(|(_, lines, _)| render_ext_widget(lines, t))
                    .collect();
                (!rows.is_empty()).then(|| div().px_4().pb_1().flex().flex_col().gap_1().children(rows).into_any_element())
            }).flatten())
            .children((self.messages.is_empty()
                && !self
                    .state
                    .as_ref()
                    .is_some_and(|s| s.is_streaming))
            .then(|| div().flex_1().into_any_element()))
            // status bar (+ extension status items)
            .child(
                div()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.bg_panel))
                    .text_xs()
                    .text_color(rgb(t.text_muted))
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(status))
                    .children(self.ext_status.iter().map(|(k, text)| {
                        div()
                            .flex_shrink_0()
                            .font_family("Consolas")
                            .text_size(px(10.))
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(format!("{}: {}", k, text)))
                    })),
            );


        // ---- right panel: file + terminal tabs (pi-web AppShell panelTabs
        //      merge; fixed dark terminal surface in every theme) -----------
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
            .font_family("Segoe UI")
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
                    match &self.sys_prompt {
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
                    panel = panel.child(
                        div()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(t.text))
                            .child(tr("工具定义")),
                    );
                    match &self.session_tools {
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
    // theme: PI_FLASH_THEME (dev override) > app_settings.json (006) >
    // pi settings.json (shared with pi's TUI) > mist
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