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
    WindowOptions, div, list, prelude::*, pulsating_between, px, relative, rgb,
};
use pi_link::client::{PiSession, spawn as spawn_pi};
use pi_link::protocol::{
    AssistantEvent, Block, Command, Event, SessionState, SessionStats, SlashCommand, TreeNode,
    Usage, content_blocks, parse_tree,
};
use pi_link::sessions::{SessionInfo, list_sessions};

mod assets;
mod i18n;
mod markdown;
mod models_config;
mod theme;
mod services;
mod settings;
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
use terminal::{TermStatus, TerminalTab};
use ui::TextInput;
use ui::{icon, pill, spinner};

// ---------------------------------------------------------------------------
// state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct UsageLine {
    input: u64,
    output: u64,
    cache_read: u64,
    cost: f64,
    time: String,
}

struct Msg {
    role: Role,
    blocks: Vec<Block>,
    usage: Option<UsageLine>,
    /// session entry id for user messages (fork anchor), filled from get_entries
    entry_id: Option<String>,
}

impl Msg {
    fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(|b| match b {
                Block::Text { text, .. } => text.as_str(),
                _ => "",
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

#[derive(Debug, Clone)]
enum Dialog {
    ModelSelect { input: gpui::Entity<TextInput> },
    BranchTree,
    ProjectSelect,
    GitDiff { path: PathBuf, patch: String },
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
    input: String,
    messages: Vec<Msg>,
    list: ListState,
    sessions: Vec<SessionInfo>,
    sessions_list: ListState,
    cwd: PathBuf,
    branch: String,
    session: Option<PiSession>,
    status: String,
    state: Option<SessionState>,
    stats: Option<SessionStats>,
    active_session_file: Option<PathBuf>,
    collapsed: HashSet<(usize, usize)>,
    expanded_dirs: HashSet<PathBuf>,
    /// working-tree changes for the current project
    git_files: Vec<GitFile>,
    git_add_del: (u64, u64),
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
    epoch: u64,
    /// built-in terminal tabs (pi-web TerminalPanel); cwd-keyed dedupe
    terminals: Vec<TerminalTab>,
    active_terminal: Option<usize>,
    right_panel_open: bool,
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
    right_panel_width: f32,
    /// right panel tabs: file viewers + terminals in one TabBar (pi-web
    /// AppShell panelTabs merge)
    panel_tabs: Vec<PanelTab>,
    active_panel_tab: Option<usize>,
    /// right-panel drag: (start pointer x, start width)
    resizing_panel: Option<(gpui::Pixels, f32)>,
    /// markdown Source/Preview toggle for file tabs (per-path)
    file_preview_mode: std::collections::HashMap<PathBuf, bool>,
    /// cached content of open file tabs
    file_cache: std::collections::HashMap<PathBuf, FileTab>,
    /// sessions-pane height as a fraction of the sidebar (pi-web
    /// --sidebar-session-pane-height; default half)
    sidebar_sessions_frac: f32,
    /// active sidebar pane drag: (start pointer y, start fraction)
    resizing_sidebar: Option<(gpui::Pixels, f32)>,
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
    File(PathBuf),
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
        let branch = read_branch(&cwd);

        let (session, events) = spawn_with_epoch(&cwd, &[], 1);
        let connected = session.is_some();

        let list = ListState::new(0, ListAlignment::Bottom, px(1000.));
        list.reset(0);
        let sessions_list = ListState::new(0, ListAlignment::Top, px(500.));

        let mut chat = Self {
            focus,
            dialog_focus,
            dialog: None,
            input: String::new(),
            messages: Vec::new(),
            list,
            sessions: {
                let cwd_text = cwd.to_string_lossy().to_string();
                list_sessions(100)
                    .into_iter()
                    .filter(|s| same_ws(&s.cwd, &cwd_text))
                    .collect()
            },
            sessions_list,
            cwd: cwd.clone(),
            branch,
            session,
            status: status_line(connected, "idle"),
            state: None,
            stats: None,
            active_session_file: None,
            collapsed: HashSet::new(),
            expanded_dirs: HashSet::new(),
            git_files: Vec::new(),
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
            epoch: 1,
            terminals: Vec::new(),
            active_terminal: None,
            right_panel_open: false,
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
            resizing_sidebar: None,
            right_panel_width: 560.,
            panel_tabs: Vec::new(),
            active_panel_tab: None,
            resizing_panel: None,
            file_preview_mode: std::collections::HashMap::new(),
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
        let last_ws = get_last_workspace();
        let target_ws = last_ws.unwrap_or_else(|| cwd.to_string_lossy().to_string());
        if !same_ws(&target_ws, &cwd.to_string_lossy()) {
            let ws_path = PathBuf::from(&target_ws);
            if ws_path.is_dir() {
                chat.cwd = ws_path;
                chat.branch = read_branch(&chat.cwd);
                let cwd_text = chat.cwd.to_string_lossy().to_string();
                chat.sessions = list_sessions(100)
                    .into_iter()
                    .filter(|s| same_ws(&s.cwd, &cwd_text))
                    .collect();
                chat.load_project_files();
            }
        }
        chat.refresh_git();
        // models panel state (enabledModels whitelist + credentials) for the
        // picker filter — loaded once at startup, refreshed when opened
        chat.reload_settings_panel();
        if let Some(p) = get_last_open(&chat.cwd.to_string_lossy()) {
            let path = PathBuf::from(&p);
            if path.exists() {
                chat.open_session(path, false, cx);
            }
        }
        chat
    }

    fn refresh_state(&self) {
        if let Some(session) = &self.session {
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
        self.sessions = list_sessions(100)
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
        self.right_panel_open = true;
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
                        self.right_panel_open = false;
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
                    self.right_panel_open = false;
                    None
                } else {
                    Some(a.saturating_sub(1))
                }
            }
            other => other,
        };
        if self.right_panel_open && self.active_terminal.is_none() {
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
        if let Some(session) = &self.session {
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
        if let Some(session) = &self.session {
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
        if let Some(session) = &self.session {
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
        if let Some(session) = &self.session {
            let _ = session.send(&Command::FollowUp { message: text });
        }
        self.input.clear();
        self.pending_images.clear();
        cx.notify();
    }

    /// 停止（rpc abort）。
    fn abort_stream(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
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
        let Some(session) = &self.session else {
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
        if let Some(session) = &self.session {
            let _ = session.send(&Command::FollowUp { message: text });
            self.input.clear();
            self.pending_images.clear();
            self.status = "queued".into();
        }
        cx.notify();
    }

    fn abort(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
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
        if let Some(session) = &self.session {
            let _ = session.send(&Command::SetSessionName { name });
        }
        self.refresh_state();
        self.renaming = None;
        self.rename_input = None;
        cx.notify();
    }

    fn select_model(&mut self, provider: String, id: String, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
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
        if let Some(session) = &self.session {
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
        if let Some(session) = &self.session {
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
                if let Some(session) = &self.session {
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
        if let Some(session) = &self.session {
            let _ = session.send(&Command::ExportHtml);
        }
        cx.notify();
    }

    fn export_html(&mut self, cx: &mut Context<Self>) {
        self.request_system_info(cx);
    }

    fn new_session(&mut self, cx: &mut Context<Self>) {
        self.epoch += 1;
        let (session, events) = spawn_with_epoch(&self.cwd, &[], self.epoch);
        self.session = session;
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
        self.status = status_line(self.session.is_some(), tr("新会话"));
        self.refresh_state();
        self.refresh_git();
        self.load_project_files();
        if let Some(events) = events {
            let epoch = self.epoch;
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
        self.epoch += 1;
        let (session, events) =
            spawn_with_epoch(&cwd, &["--session", &path.to_string_lossy()], self.epoch);
        self.session = session;
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
        self.status = status_line(self.session.is_some(), "resuming");
        if let Some(session) = &self.session {
            let _ = session.send(&Command::GetMessages);
            // branch tree snapshot for the fork panel
            let _ = session.send(&Command::GetTree);
        }
        self.refresh_state();
        self.load_project_files();
        if let Some(events) = events {
            let epoch = self.epoch;
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
            if let Some(session) = &self.session {
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
                self.status = status_line(self.session.is_some(), "session deleted");
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
        self.right_panel_open = true;
        if let Some(ix) = self
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::File(p) if *p == path))
        {
            self.active_panel_tab = Some(ix);
            cx.notify();
            return;
        }
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
        let tab = PanelTab::File(path.clone());
        self.file_cache.insert(path.clone(), FileTab { path: path.clone(), content, truncated: too_big });
        self.panel_tabs.push(tab);
        self.active_panel_tab = Some(self.panel_tabs.len() - 1);
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
        if let PanelTab::File(p) = &removed {
            self.file_cache.remove(p);
        }
        self.active_panel_tab = match self.active_panel_tab {
            Some(a) if a >= self.panel_tabs.len() => {
                if self.panel_tabs.is_empty() {
                    self.right_panel_open = false;
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
                        if let Some(s) = self.session.as_ref() {
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
                if let Some(s) = self.session.as_ref() {
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
                if chat.epoch != epoch {
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
        if chat.epoch == epoch {
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

fn spawn_with_epoch(
    cwd: &PathBuf,
    extra_args: &[&str],
    _epoch: u64,
) -> (Option<PiSession>, Option<UnboundedReceiver<Event>>) {
    match spawn_pi(cwd, extra_args) {
        Ok((s, ev)) => (Some(s), Some(ev)),
        Err(e) => {
            eprintln!("{e}");
            (None, None)
        }
    }
}


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

fn render_block(
    b: &Block,
    msg_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashSet<(usize, usize)>,
    t: &theme::Theme,
) -> gpui::Div {
    match b {
        Block::Text { text, .. } if !text.trim().is_empty() => {
            div().w_full().child(markdown::render_themed(text))
        }
        Block::Thinking { text, content_index } if !text.trim().is_empty() => {
            let key = (msg_ix, *content_index);
            let expanded = !collapsed.contains(&key);
            let weak = weak.clone();
            // pi-web getThinkingPreview: first line, up to 240 chars, trimmed
            let preview: String = text
                .trim_start()
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(240)
                .collect::<String>()
                .trim_end()
                .to_string();
            // pi-web ThinkingBlock: flat var(--bg) strip with 1px border —
            // [lightbulb] [one-line preview] collapsed, [lightbulb] [pre-wrap
            // muted body] expanded; amber bulb while expanded, no chevron, no
            // "thinking" label
            let toggle = div()
                .id(SharedString::from(format!("th-{msg_ix}-{content_index}")))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_1p5()
                .min_w_0()
                .text_color(rgb(t.text_muted))
                .when(expanded, |d| d.flex_shrink_0())
                .when(!expanded, |d| d.flex_1())
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let k = key;
                    let _ = weak.update(cx, |c, cx| {
                        if !c.collapsed.remove(&k) {
                            c.collapsed.insert(k);
                        }
                        cx.notify();
                    });
                })
                .child(if expanded {
                    icon("lightbulb", 14., 0xd4a017)
                } else {
                    icon("lightbulb", 14., t.text_muted)
                })
                .children((!expanded).then(|| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(if preview.is_empty() {
                            SharedString::from("...").into_any_element()
                        } else {
                            SharedString::from(preview).into_any_element()
                        })
                }));
            let mut block = div()
                .w_full()
                .my_1()
                .flex()
                .items_start()
                .gap_1p5()
                .min_w_0()
                .px(px(10.))
                .py(px(6.))
                .rounded(px(7.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg))
                .font_family("Consolas")
                .text_size(px(11.))
                .line_height(relative(1.5))
                .child(toggle);
            if expanded {
                block = block.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(rgb(t.text_muted))
                        .child(SharedString::from(text.clone())),
                );
            }
            block
        }
        Block::ToolCall { name, args, result, .. } if !name.is_empty() => {
            let mut card = div()
                .w_full()
                .my_1()
                .rounded_md()
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.tool_bg))
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .font_family("Consolas")
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.accent))
                        .child(SharedString::from(name.clone())),
                )
                .child(
                    div()
                        .px_2()
                        .pb_1()
                        .font_family("Consolas")
                        .text_xs()
                        .text_color(rgb(t.text_muted))
                        .child(SharedString::from(pretty_args(args))),
                );
            if !result.is_empty() {
                card = card.child(
                    div()
                        .px_2()
                        .pb_1()
                        .mt_1()
                        .border_t_1()
                        .border_color(rgb(t.border))
                        .font_family("Consolas")
                        .text_xs()
                        .text_color(rgb(t.text))
                        .child(SharedString::from(result.clone())),
                );
            }
            card
        }
        _ => div().w_full(),
    }
}

/// Recursive file-explorer rows (FileExplorer.tsx TreeNodeView parity):
/// 24px rows, indent 8+depth*14, directories toggle lazily on click,
/// files open the preview dialog.
fn collect_tree_rows(
    dir: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    git_map: &std::collections::HashMap<PathBuf, GitStatus>,
    changed_dirs: &HashSet<PathBuf>,
    weak: &gpui::WeakEntity<Chat>,
    t: &theme::Theme,
    out: &mut Vec<gpui::AnyElement>,
) {
    // defensive depth cap: pathological trees must not kill the app
    // (Windows main stack; see .cargo/config.toml for the 16MB bump)
    if depth >= 12 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if e.file_type().map(|ty| ty.is_dir()).unwrap_or(false) {
            dirs.push(e.path());
        } else {
            files.push(e.path());
        }
    }
    dirs.sort();
    files.sort();
    dirs.truncate(300);
    files.truncate(300);
    for path in dirs.into_iter().chain(files.into_iter()) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let is_dir = path.is_dir();
        let open = is_dir && expanded.contains(&path);
        let mut row = div()
            .id(SharedString::from(format!("tree-{}", path.display())))
            .w_full()
            .h(px(24.))
            .flex()
            .items_center()
            .gap_1()
            .overflow_hidden()
            .pl(px(8. + depth as f32 * 14.))
            .pr(px(8.))
            .rounded(px(4.))
            .text_xs()
            .text_color(rgb(t.text))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)));
        if is_dir {
            row = row
                .child(
                    div()
                        .flex_shrink_0()
                        .child(if open {
                            icon("chevron-down", 10., t.text_dim)
                        } else {
                            icon("chevron-right", 10., t.text_dim)
                        }),
                )
                .child(
                    div().flex_shrink_0().child(icon("folder", 14., t.text_dim)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(name)),
                );
            let weak_toggle = weak.clone();
            let dir_path = path.clone();
            row = row.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let d = dir_path.clone();
                let _ = weak_toggle.update(cx, |c, cx| {
                    if !c.expanded_dirs.remove(&d) {
                        c.expanded_dirs.insert(d);
                    }
                    cx.notify();
                });
            });
        } else {
            row = row
                .child(div().w(px(10.)).flex_shrink_0())
                .child(
                    div().flex_shrink_0().child(icon("file", 14., t.text_dim)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(name)),
                );
            let weak_open = weak.clone();
            let fp = path.clone();
            let is_changed = git_map.contains_key(&path);
            row = row.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_open.update(cx, |c, cx| {
                    if is_changed {
                        c.open_git_diff(fp.clone(), cx);
                    } else {
                        c.open_file_tab(fp.clone(), cx);
                    }
                });
            });
        }
        // git badge on files; dot on directories containing changes
        if is_dir {
            if changed_dirs.contains(&path) {
                row = row.child(
                    div()
                        .size(px(6.))
                        .rounded_full()
                        .ml_auto()
                        .flex_shrink_0()
                        .bg(rgb(0xd6a84b)),
                );
            }
        } else if let Some(st) = git_map.get(&path) {
            let color = st.color();
            row = row.child(
                div()
                    .ml_auto()
                    .flex_shrink_0()
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(color))
                    .child(st.badge()),
            );
        }
        out.push(row.into_any_element());
        if open {
            collect_tree_rows(&path, depth + 1, expanded, git_map, changed_dirs, weak, t, out);
        }
    }
}

fn render_msg(
    m: &Msg,
    msg_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashSet<(usize, usize)>,
    t: &theme::Theme,
    model_label: &str,
    // while this message is streaming: (estimated tokens, tok/s)
    stream_info: Option<(u64, Option<f32>)>,
) -> gpui::Div {
    let mut col = div().w_full().mb_4().flex().flex_col();
    if m.role == Role::User {
        // MessageView.tsx: right-aligned bubble, --user-bg, radius 12, pad 8/12.
        // UserMessageView hover toolbar: fork button (git-branch 11px) appears
        // on hover and forks the session before this user message.
        let text = m.plain_text();
        let entry = m.entry_id.clone();
        let weak_fork = weak.clone();
        let mut row = div()
            .id(SharedString::from(format!("msgrow-{msg_ix}")))
            .group("usermsg")
            .w_full()
            .flex()
            .flex_col()
            .items_end()
            .gap_0p5();
        if entry.is_some() {
            row = row.child(
                div()
                    .id(SharedString::from(format!("fork-{msg_ix}")))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(5.))
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .opacity(0.)
                    .group_hover("usermsg", |s| s.opacity(1.))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.accent)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        if let Some(eid) = entry.clone() {
                            let _ = weak_fork.update(cx, |c, cx| {
                                c.fork_from_entry(eid, cx)
                            });
                        }
                    })
                    .child(icon("git-branch", 11., t.text_dim))
                    .child(SharedString::from(tr("新分支"))),
            );
        }
        row = row.child(
            div()
                .max_w(relative(0.85))
                .px_3()
                .py_2()
                .rounded(px(12.))
                .bg(rgb(t.user_bg))
                .border_1()
                .border_color(gpui::rgba(0x3b82f633))
                .text_color(rgb(t.text))
                .text_size(px(14.))
                .child(SharedString::from(text)),
        );
        col = col.child(row);
    } else {
        // MessageView: model label 11px --text-dim, margin-bottom 4; while
        // streaming add the estimated-token arrow + speed badge
        col = col.child(
            div()
                .text_xs()
                .text_color(rgb(t.text_dim))
                .mb_1()
                .flex()
                .items_center()
                .gap_1p5()
                .child(SharedString::from(model_label.to_string()))
                .children(stream_info.and_then(|(est, tps)| {
                    (est > 0).then(|| {
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_color(rgb(t.text))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_0p5()
                                    .text_size(px(11.))
                                    .child("\u{2193}"),
                            )
                            .child(SharedString::from(est.to_string()))
                            .children(tps.map(|v| {
                                div()
                                    .ml(px(6.))
                                    .px(px(6.))
                                    .py(px(1.))
                                    .rounded(px(4.))
                                    .bg(rgb(tps_color(v)))
                                    .text_size(px(11.))
                                    .text_color(gpui::rgb(0xffffff))
                                    .child(SharedString::from(format!("{:.1} t/s", v)))
                            }))
                    })
                })),
        );
        for b in &m.blocks {
            col = col.child(render_block(b, msg_ix, weak, collapsed, t));
        }
        if let Some(u) = &m.usage {
            col = col.child(
                div()
                    .flex()
                    .justify_between()
                    .mt_2()
                    .font_family("Consolas")
                    .text_xs()
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(usage_footer(u.input, u.output, u.cache_read, u.cost)))
                    .child(SharedString::from(u.time.clone())),
            );
        }
    }
    col
}

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
            let handle = input.read(cx).focus_handle();
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
        } else if !self.terminals.iter().any(|t| t.focus.is_focused(window)) {
            window.focus(&self.focus);
        }
        let t = T();

        let status: SharedString = self.status.clone().into();
        let streaming = self
            .state
            .as_ref()
            .is_some_and(|st| st.is_streaming);
        let input_ph: SharedString = if streaming {
            tr("立即引导 / 排队后续消息...").into()
        } else if self.input.is_empty() {
            tr("消息...输入 / 使用命令，输入 @ 查找文件").into()
        } else {
            self.input.clone().into()
        };
        let input_empty = self.input.is_empty();
        let can_queue = !input_empty || !self.pending_images.is_empty();
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
        let right_px = if self.right_panel_open {
            self.right_panel_width + 24.
        } else {
            24.
        };
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

        let cwd_text: SharedString = self.cwd.to_string_lossy().to_string().into();
        let branch: SharedString = if self.branch.is_empty() {
            "no git".into()
        } else {
            self.branch.clone().into()
        };

        // ---- sidebar ----------------------------------------------------
        let sessions_entity = entity.clone();
        let weak_for_sessions = weak.clone();
        let sidebar = div()
            .w(px(260.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(rgb(t.bg))
            .border_r_1()
            .border_color(rgb(t.border))
            // brand + new + search
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .child(
                        div()
                            .text_base()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(t.text))
                            .child("pi-flash"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1p5()
                            .child(
                                div()
                                    .id("new-session")
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(7.))
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .bg(rgb(t.bg_hover))
                                    .text_xs()
                                    .text_color(rgb(t.text))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(t.bg_selected)))
                                    .on_mouse_down(MouseButton::Left, {
                                        let weak = weak_for_sessions.clone();
                                        move |_, _, cx| {
                                            let _ =
                                                weak.update(cx, |c, cx| c.new_session(cx));
                                        }
                                    })
                                    .child(icon("plus", 12., t.text))
                                    .child(SharedString::from(tr("新建"))),
                            )
                            .child(
                                div()
                                    .id("search")
                                    .w(px(30.))
                                    .h(px(26.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(7.))
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .bg(rgb(t.bg_hover))
                                    .text_color(rgb(t.text_muted))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(t.bg_selected)))
                                                                        .on_mouse_down(MouseButton::Left, cx.listener(
                                        |this, _: &gpui::MouseDownEvent, window, cx| {
                                            this.search_open = !this.search_open;
                                            let input = this.search_input.clone();
                                            input.update(cx, |ti, cx| ti.set_value(String::new(), cx));
                                            if this.search_open {
                                                let handle = input.read(cx).focus_handle();
                                                window.focus(&handle);
                                            }
                                            this.refresh_sessions();
                                            cx.notify();
                                        },
                                    ))
.child(icon("search", 12., t.text_muted)),
                            ),
                    ),
            )
            // project box
            .child(
                div()
                    .id("project-frame")
                    .mx_3()
                    .mb_1p5()
                    .px_2p5()
                    .py_1p5()
                    .rounded(px(7.))
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.assistant_bg))
                    .text_xs()
                    .text_color(rgb(t.text))
                    .cursor_pointer()
                    .hover(|s| s.border_color(rgb(t.text_dim)))
                    .on_mouse_down(MouseButton::Left, cx.listener(
                        |this, _: &gpui::MouseDownEvent, _w, cx| {
                            this.open_project_select(cx);
                        },
                    ))
                    .child(cwd_text),
            )
            // branch box
            .child(
                div()
                    .mx_3()
                    .mb_2()
                    .px_2p5()
                    .py_1p5()
                    .rounded(px(7.))
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.assistant_bg))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_color(rgb(t.text))
                            .child(icon("git-branch", 12., t.text))
                            .child(branch),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_color(rgb(t.text_muted))
                            .child(SharedString::from(tr("主分支")))
                            .child(icon("chevron-down", 10., t.text_muted)),
                    ),
            )
            // session search row (pi-web SessionSearch input)
            .children(self.search_open.then(|| {
                div()
                    .id("session-search-row")
                    .mx_3()
                    .mb_1p5()
                    .child(self.search_input.clone())
            }))
            // sessions list (pi-web SessionSearch: query filters the list)
            .child({
                let q = self.search_input.read(cx).value().to_lowercase();
                let session_display: Vec<usize> = if q.is_empty() {
                    (0..self.sessions.len()).collect()
                } else {
                    self.sessions
                        .iter()
                        .enumerate()
                        .filter(|(_, i)| {
                            i.preview.to_lowercase().contains(&q)
                                || i
                                    .path
                                    .to_string_lossy()
                                    .to_lowercase()
                                    .contains(&q)
                        })
                        .map(|(i, _)| i)
                        .collect()
                };
                if self.sessions_list_count != session_display.len() {
                    self.sessions_list.reset(session_display.len());
                    self.sessions_list_count = session_display.len();
                }
                let session_display = std::sync::Arc::new(session_display);
                list(
                    self.sessions_list.clone(),
                    move |ix, _window, cx| {
                        let chat = sessions_entity.read(cx);
                        let Some(orig) = session_display.get(ix).copied() else {
                            return div().into_any_element();
                        };
                        let Some(info) = chat.sessions.get(orig) else {
                            return div().into_any_element();
                        };
                    let is_active = chat
                        .active_session_file
                        .as_deref()
                        == Some(info.path.as_path());
                    let path = info.path.clone();
                    let preview: SharedString = if info.preview.is_empty() {
                        "(empty)".into()
                    } else {
                        info.preview.clone().into()
                    };
                    let time_text = time_ago(info.modified);
                    // pi-web runningSessionIds: the spinner replaces the
                    // timestamp on the running session's row (one embedded
                    // agent → the active session is the running one)
                    let streaming = is_active
                        && (chat.agent_running
                            || chat
                                .state
                                .as_ref()
                                .is_some_and(|s| s.is_streaming));
                    let hovered = chat.hovered_session == Some(ix);
                    let confirming =
                        chat.confirm_delete.as_deref() == Some(info.path.as_path());
                    // pi-web renaming: this row's content swaps for the input
                    let renaming =
                        chat.renaming.as_deref() == Some(info.path.as_path());
                    let weak_del2 = weak_for_sessions.clone();
                    let weak_del3 = weak_for_sessions.clone();
                    // pi-web: "删除 {title}？" truncates the title at 22 chars
                    let confirm_title: String = {
                        let mut s = preview.to_string();
                        if s.chars().count() > 22 {
                            s = s.chars().take(22).collect::<String>() + "…";
                        }
                        s
                    };
                    let weak = weak_for_sessions.clone();
                    let weak_del = weak_for_sessions.clone();
                    let weak_ren = weak_for_sessions.clone();
                    let weak_hover = weak_for_sessions.clone();
                    let p_del = info.path.clone();
                    div()
                        .id(SharedString::from(format!("row-{ix}")))
                        .w_full()
                        .h(px(54.))
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .pl_3p5()
                        .pr_2()
                        .overflow_hidden()
                        .border_l_2()
                        // pi-web: confirm state paints the row red and
                        // swallows the row click
                        .when(confirming, |d| {
                            d.cursor(gpui::CursorStyle::Arrow)
                                .bg(gpui::rgba(0xef44440f))
                                .border_color(rgb(0xef4444))
                        })
                        .when(!confirming, |d| {
                            d.cursor_pointer()
                                .when(is_active, |d| {
                                    d.bg(rgb(t.bg_selected))
                                        .border_color(rgb(t.accent))
                                })
                                .when(!is_active, |d| d.border_color(rgb(t.bg)))
                                .when(hovered && !is_active, |d| d.bg(rgb(t.bg_hover)))
                        })
                        .when(!confirming && !renaming, |d| {
                            d.on_mouse_down(MouseButton::Left, {
                                let p = path.clone();
                                move |_, _, cx| {
                                    let _ = weak.update(cx, |c, cx| {
                                        c.open_session(p.clone(), false, cx)
                                    });
                                }
                            })
                        })
                        .on_hover(move |h, _, cx| {
                            let _ = weak_hover.update(cx, |c, cx| {
                                // only the owning row may clear its own hover;
                                // row A's (false) must not erase row B's (true)
                                // when leave/enter events arrive out of order
                                if *h {
                                    if c.hovered_session != Some(ix) {
                                        c.hovered_session = Some(ix);
                                        cx.notify();
                                    }
                                } else if c.hovered_session == Some(ix) {
                                    c.hovered_session = None;
                                    cx.notify();
                                }
                            });
                        })
                        .children(if renaming {
                            // pi-web: "Rename: input fills the same row"
                            (chat.rename_input.clone().map(|input| {
                                div().flex_1().min_w_0().child(input)
                            }))
                        } else {
                            None
                        })
                        .children((!confirming && !renaming).then(|| {
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .text_xs()
                                        .line_height(relative(1.4))
                                        .font_weight(if is_active {
                                            gpui::FontWeight::MEDIUM
                                        } else {
                                            gpui::FontWeight::NORMAL
                                        })
                                        .text_color(rgb(t.text))
                                        .child(preview),
                                )
                                .child(
                                    div()
                                        .mt(px(2.))
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .text_size(px(11.))
                                        .min_w_0()
                                        .text_color(rgb(t.text_dim))
                                        .child(if streaming {
                                            spinner(14., t.accent)
                                        } else {
                                            SharedString::from(time_text.clone())
                                                .into_any_element()
                                        })
                                        .child(SharedString::from(crate::i18n::tf(
                                            "{n} 条消息",
                                            &[("n", info.message_count.to_string())],
                                        ))),
                                )
                        }))
                        .children(if confirming {
                            // pi-web delete confirmation: the row content
                            // swaps in place — "删除 {title}？" + red 删除/取消
                            Some(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .text_size(px(12.))
                                            .text_color(rgb(t.text))
                                            .child(SharedString::from(crate::i18n::tf(
                                                "删除 {title}？",
                                                &[("title", confirm_title)],
                                            ))),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("delok-{ix}")))
                                            .h(px(30.))
                                            .px(px(11.))
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .rounded(px(6.))
                                            .bg(rgb(0xef4444))
                                            .text_size(px(12.))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(rgb(0xffffff))
                                            .cursor_pointer()
                                            .flex_shrink_0()
                                            .child(icon("trash", 12., 0xffffff))
                                            .child(SharedString::from(tr("删除")))
                                            .on_mouse_down(MouseButton::Left, {
                                                let p = p_del.clone();
                                                move |_, _, cx| {
                                                    let _ = weak_del2.update(cx, |c, cx| {
                                                        c.delete_session(p.clone(), cx)
                                                    });
                                                }
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("delno-{ix}")))
                                            .h(px(30.))
                                            .px(px(11.))
                                            .flex()
                                            .items_center()
                                            .rounded(px(6.))
                                            .border_1()
                                            .border_color(rgb(t.border))
                                            .bg(rgb(t.bg_hover))
                                            .text_size(px(12.))
                                            .text_color(rgb(t.text_muted))
                                            .cursor_pointer()
                                            .flex_shrink_0()
                                            .child(SharedString::from(tr("取消")))
                                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                                let _ = weak_del3.update(cx, |c, cx| {
                                                    c.confirm_delete = None;
                                                    cx.notify();
                                                });
                                            }),
                                    ),
                            )
                        } else {
                            None
                        })
                        .children(if hovered && !confirming && !renaming {
                            Some(
                                div()
                                    .flex()
                                    .gap_1()
                                    .flex_shrink_0()
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("ren-{ix}")))
                                            .size(px(28.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px(7.))
                                            .border_1()
                                            .border_color(rgb(t.border))
                                            .bg(rgb(t.bg_hover))
                                            .text_color(rgb(t.text_dim))
                                            .cursor_pointer()
                                            .hover(|s| {
                                                s.bg(rgb(t.bg_selected))
                                                    .text_color(rgb(t.accent))
                                            })
                                            .on_mouse_down(MouseButton::Left, {
                                                let p = path.clone();
                                                move |_, _, cx| {
                                                    cx.stop_propagation();
                                                    let _ = weak_ren.update(cx, |c, cx| {
                                                        if c.active_session_file.as_deref()
                                                            == Some(p.as_path())
                                                        {
                                                            // pi-web: inline rename, no modal
                                                            let prefill = c
                                                                .state
                                                                .as_ref()
                                                                .and_then(|s| {
                                                                    s.session_name.clone()
                                                                })
                                                                .or_else(|| {
                                                                    c.messages
                                                                        .iter()
                                                                        .find(|m| {
                                                                            matches!(m.role, Role::User)
                                                                        })
                                                                        .map(|m| m.plain_text())
                                                                })
                                                                .map(|v| {
                                                                    v.chars().take(50).collect::<String>()
                                                                })
                                                                .unwrap_or_default();
                                                            c.start_rename(p.clone(), prefill, cx);
                                                        } else {
                                                            c.open_session(p.clone(), true, cx);
                                                        }
                                                    });
                                                }
                                            })
                                            .child(icon("pencil", 14., t.text_dim)),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("del-{ix}")))
                                            .size(px(28.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px(7.))
                                            .border_1()
                                            .border_color(rgb(t.border))
                                            .bg(rgb(t.bg_hover))
                                            .text_color(rgb(t.text_dim))
                                            .cursor_pointer()
                                            .hover(|s| {
                                                s.bg(gpui::rgba(0xef444414))
                                                    .text_color(rgb(0xef4444))
                                            })
                                            .on_mouse_down(MouseButton::Left, {
                                                let p = p_del.clone();
                                                move |ev, _, cx| {
                                                    cx.stop_propagation();
                                                    let _ = weak_del.update(cx, |c, cx| {
                                                        if ev.modifiers.shift {
                                                            // pi-web: shift+click deletes
                                                            // without confirmation
                                                            c.delete_session(p.clone(), cx);
                                                        } else {
                                                            c.confirm_delete = Some(p.clone());
                                                            cx.notify();
                                                        }
                                                    });
                                                }
                                            })
                                            .child(icon("trash", 14., t.text_dim)),
                                    ),
                            )
                        } else {
                            None
                        })
                        .into_any_element()
                    }  // move closure
                    )  // list(
                // sessions pane height = persisted fraction of the sidebar
                // (pi-web --sidebar-session-pane-height; default half)
                .flex_basis(relative(self.sidebar_sessions_frac))
                .min_h_0()
                .overflow_hidden()
            })  // .child({ ... }) block
            // pane resize handle (pi-web sidebar-section-resize-handle)
            .child(
                div()
                    .id("sidebar-resize")
                    .h(px(12.))
                    .flex_shrink_0()
                    .cursor(gpui::CursorStyle::ResizeUpDown)
                    .hover(|s| s.bg(rgb(t.bg_hover)))
                    .on_mouse_down(MouseButton::Left, cx.listener(
                        |this, ev: &gpui::MouseDownEvent, _w, _cx| {
                            this.resizing_sidebar =
                                Some((ev.position.y, this.sidebar_sessions_frac));
                        },
                    )),
            )
            // file explorer section (flex rest)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .border_t_1()
                    .border_color(rgb(t.border))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_3()
                            .py_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(t.text))
                                    .child(icon("chevron-down", 10., t.text))
                                    .child(SharedString::from(tr("文件浏览器"))),
                            )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .text_color(rgb(t.text_muted))
                            .child(icon("monitor", 12., t.text_muted))
                            .child(icon("search", 12., t.text_muted))
                            .child(icon("upload", 12., t.text_muted))
                            // open terminal for the selected cwd (pi-web
                            // SessionSidebar explorer terminal button)
                            .child(
                                div()
                                    .id("open-terminal")
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(rgb(t.text)))
                                    .on_mouse_down(MouseButton::Left, cx.listener(
                                        |this, _: &gpui::MouseDownEvent, window, cx| {
                                            this.open_terminal(window, cx);
                                        },
                                    ))
                                    .child(icon("terminal", 13., t.text_muted)),
                            )
                            .child(if self.git_add_del.0 + self.git_add_del.1 > 0 {
                                        div()
                                            .text_xs()
                                            .font_family("Consolas")
                                            .text_color(rgb(0xd6a84b))
                                            .child(SharedString::from(format!(
                                                "+{} -{}",
                                                self.git_add_del.0, self.git_add_del.1
                                            )))
                                            .into_any_element()
                                    } else {
                                        div().into_any_element()
                                    })
                                    .child(icon("refresh", 12., t.text_muted)),
                            ),
                    )
                    .child(
                        div()
                            .px_3()
                            .pb_2()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                            div()
                                .id("file-tree-scroll")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .children({
                                let mut rows: Vec<gpui::AnyElement> = Vec::new();
                                let git_map: std::collections::HashMap<
                                    PathBuf,
                                    GitStatus,
                                > = self
                                    .git_files
                                    .iter()
                                    .map(|f| (f.path.clone(), f.status))
                                    .collect();
                                let changed_dirs: HashSet<PathBuf> = git_map
                                    .keys()
                                    .filter_map(|p| p.parent().map(|d| d.to_path_buf()))
                                    .collect();
                                collect_tree_rows(
                                    &self.cwd,
                                    0,
                                    &self.expanded_dirs,
                                    &git_map,
                                    &changed_dirs,
                                    &weak_for_dialog,
                                    t,
                                    &mut rows,
                                );
                                rows
                            }),
                            ),
                    ),
            )
            // bottom nav
            .child(
                div()
                    .flex()
                    .border_t_1()
                    .border_color(rgb(t.border))
                    .child(
                        div()
                            .id("nav-models")
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_1p5()
                            .py_2()
                            .text_xs()
                            .text_color(rgb(t.text_muted))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .on_mouse_down(MouseButton::Left, cx.listener(
                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                    this.open_settings(0, cx);
                                },
                            ))
                            .child(icon("settings", 12., t.text_muted))
                            .child(SharedString::from(tr("模型"))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_1p5()
                            .py_2()
                            .text_xs()
                            .text_color(rgb(t.text_muted))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .on_mouse_down(MouseButton::Left, cx.listener(
                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                    this.open_settings(1, cx);
                                },
                            ))
                            .child(icon("layers", 12., t.text_muted))
                            .child(SharedString::from(tr("技能"))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_1p5()
                            .py_2()
                            .text_xs()
                            .text_color(rgb(t.text_muted))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .on_mouse_down(MouseButton::Left, cx.listener(
                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                    this.open_settings(2, cx);
                                },
                            ))
                            .child(icon("settings", 12., t.text_muted))
                            .child(SharedString::from(tr("插件"))),
                    ),
            );

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
            // input area
            .child(
                div()
                    .px_4()
                    .pb_2()
                    .children(if self.pending_images.is_empty() {
                        None
                    } else {
                        let rows: Vec<gpui::AnyElement> = self
                            .pending_images
                            .iter()
                            .enumerate()
                            .map(|(i, img)| {
                                let weak_i = weak_for_dialog.clone();
                                let name: SharedString = img.name.clone().into();
                                div()
                                    .id(SharedString::from(format!("img-{i}")))
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(rgb(t.bg_panel))
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .text_xs()
                                    .text_color(rgb(t.text_muted))
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        let _ = weak_i.update(cx, |c, cx| {
                                            if i < c.pending_images.len() {
                                                c.pending_images.remove(i);
                                            }
                                            cx.notify();
                                        });
                                    })
                                    .child(SharedString::from(format!(
                                        "\u{1f5bc} {name} \u{00d7}"
                                    )))
                                    .into_any_element()
                            })
                            .collect();
                        Some(
                            div()
                                .w_full()
                                .mb_1p5()
                                .flex()
                                .gap_2()
                                .children(rows)
                                .into_any_element(),
                        )
                    })
                    .child(
                        div()
                            .w_full()
                            .rounded(px(14.))
                            .border_1()
                            .border_color(if streaming {
                                gpui::rgba(0xeab30866) // amber, pi-web streaming
                            } else if input_focused {
                                rgb(t.accent)
                            } else {
                                rgb(t.border)
                            })
                            .bg(rgb(t.bg))
                            .pl_3p5()
                            .pr_2p5()
                            .py_2p5()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id("input")
                                    .track_focus(&self.focus)
                                    .relative()
                                    .flex_1()
                                    .min_w_0()
                                    .rounded_md()
                                    .on_key_down(cx.listener(
                                        |this, ev: &KeyDownEvent, _w, cx| {
                                            let key = ev.keystroke.key.as_str();
                                            let shift = ev.keystroke.modifiers.shift;
                                            // while the IME is composing, the
                                            // keyboard belongs to the IME — text
                                            // arrives via the InputHandler
                                            if this.ime_marked.is_some() {
                                                return;
                                            }
                                            let menu_open =
                                                this.active_menu().is_some();
                                            let items = this.menu_items();
                                            let streaming = this
                                                .state
                                                .as_ref()
                                                .is_some_and(|s| s.is_streaming);
                                            let can_queue = !this.input.is_empty()
                                                || !this.pending_images.is_empty();
                                            match key {
                                                "enter" if shift && streaming => {
                                                    if can_queue {
                                                        this.follow_up_input(cx);
                                                    }
                                                }
                                                "enter" if shift => {
                                                    this.input.push('\n');
                                                    cx.notify();
                                                }
                                                "enter"
                                                    if menu_open && !items.is_empty() => {
                                                    let ix = this
                                                        .menu_ix
                                                        .min(items.len() - 1);
                                                    let insert =
                                                        items[ix].insert.clone();
                                                    this.accept_menu(insert, cx);
                                                }
                                                "enter" if streaming => {
                                                    if can_queue {
                                                        this.steer_input(cx);
                                                    }
                                                }
                                                "enter" => this.send_input(cx),
                                                "escape" if streaming && !menu_open => {
                                                    this.abort_stream(cx);
                                                }
                                                "escape" if menu_open => {
                                                    this.menu_ix = 0;
                                                    if this.active_menu()
                                                        == Some(MenuKind::At)
                                                    {
                                                        if let Some(at) =
                                                            this.input.rfind('@')
                                                        {
                                                            let q = this.input
                                                                [at + 1..]
                                                                .to_string();
                                                            this.input = format!(
                                                                "{}{} ",
                                                                &this.input[..at],
                                                                q
                                                            );
                                                        }
                                                    } else if !this.input.is_empty() {
                                                        this.input =
                                                            format!("{} ", this.input);
                                                    }
                                                    cx.notify();
                                                }
                                                "escape" => this.abort(cx),
                                                "tab" if menu_open && !items.is_empty() => {
                                                    let ix = this
                                                        .menu_ix
                                                        .min(items.len() - 1);
                                                    let insert =
                                                        items[ix].insert.clone();
                                                    this.accept_menu(insert, cx);
                                                }
                                                "up" if menu_open && !items.is_empty() => {
                                                    this.menu_ix =
                                                        this.menu_ix.saturating_sub(1);
                                                    cx.notify();
                                                }
                                                "down"
                                                    if menu_open && !items.is_empty() =>
                                                {
                                                    this.menu_ix = (this.menu_ix + 1)
                                                        .min(items.len() - 1);
                                                    cx.notify();
                                                }
                                                "up" if !this.history.is_empty() => {
                                                    let ix = match this.history_ix {
                                                        None => this.history.len() - 1,
                                                        Some(i) => i.saturating_sub(1),
                                                    };
                                                    this.history_ix = Some(ix);
                                                    this.input =
                                                        this.history[ix].clone();
                                                    cx.notify();
                                                }
                                                "down" => {
                                                    if let Some(i) = this.history_ix {
                                                        if i + 1 < this.history.len() {
                                                            this.history_ix = Some(i + 1);
                                                            this.input =
                                                                this.history[i + 1]
                                                                    .clone();
                                                        } else {
                                                            this.history_ix = None;
                                                            this.input.clear();
                                                        }
                                                        cx.notify();
                                                    }
                                                }
                                                "backspace" => {
                                                    if !ev.keystroke.modifiers.modified()
                                                    {
                                                        this.input.pop();
                                                        this.menu_ix = 0;
                                                        cx.notify();
                                                    }
                                                }
                                                _ => {}
                                            }
                                        },
                                    ))
                                    .text_sm()
                                    .child(
                                        // text + blinking caret (gpui editor is
                                        // hand-rolled; the caret marks the end)
                                        div()
                                            .relative()
                                            .flex()
                                            .items_center()
                                            .min_w_0()
                                            // caret is an absolute overlay so its
                                            // blinking never shifts the text
                                            .when(
                                                input_focused && caret_on && input_empty,
                                                |d| {
                                                    d.child(
                                                        div()
                                                            .absolute()
                                                            .left_0()
                                                            .top(px(3.))
                                                            .w(px(1.5))
                                                            .h(px(16.))
                                                            .bg(rgb(t.accent)),
                                                    )
                                                },
                                            )
                                            .when(input_empty, |d| {
                                                d.child(
                                                    div()
                                                        .min_w_0()
                                                        .whitespace_nowrap()
                                                        .overflow_hidden()
                                                        .text_color(rgb(t.text_dim))
                                                        .opacity(0.55)
                                                        .child(SharedString::from(
                                                            input_ph.clone(),
                                                        )),
                                                )
                                            })
                                            .when(!input_empty, |d| {
                                                d.child(SharedString::from(
                                                    this_input.clone(),
                                                ))
                                            })
                                            .when(
                                                input_focused && caret_on && !input_empty,
                                                |d| {
                                                    d.child(
                                                        div()
                                                            .w(px(1.5))
                                                            .h(px(16.))
                                                            .flex_shrink_0()
                                                            .bg(rgb(t.accent)),
                                                    )
                                                },
                                            ),
                                    )
                                    // paint-phase input handler: routes the
                                    // Windows IME into the hand-rolled editor
                                    .child(
                                        EditorInputElement::new(entity.clone(), self.focus.clone())
                                            .absolute()
                                            .inset_0(),
                                    ),
                            )
                            .child(
                                div()
                                    .children(if streaming {
                                        // steer / follow-up pair (pi-web
                                        // ChatInput streaming mode)
                                        let weak_b = weak_for_dialog.clone();
                                        Some(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1p5()
                                                .child(
                                                    div()
                                                        .id("steer")
                                                        .px_3()
                                                        .py_1p5()
                                                        .rounded_lg()
                                                        .border_1()
                                                        .border_color(gpui::rgba(
                                                            0xeab30800 | 0x35,
                                                        ))
                                                        .bg(gpui::rgba(0xeab30800 | 0x12))
                                                        .text_sm()
                                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                                        .text_color(gpui::rgb(0xb48200))
                                                        .cursor_pointer()
                                                        .when(!can_queue, |d| d.opacity(0.5))
                                                        .on_mouse_down(MouseButton::Left, {
                                                            let weak = weak_b.clone();
                                                            move |_, _, cx| {
                                                                let _ = weak.update(
                                                                    cx,
                                                                    |c, cx| {
                                                                        if can_queue {
                                                                            c.steer_input(cx)
                                                                        }
                                                                    },
                                                                );
                                                            }
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .id("followup")
                                                        .px_3()
                                                        .py_1p5()
                                                        .rounded_lg()
                                                        .border_1()
                                                        .border_color(gpui::rgba(
                                                            0x818cf400 | 0x35,
                                                        ))
                                                        .bg(gpui::rgba(0x818cf400 | 0x12))
                                                        .text_sm()
                                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                                        .text_color(gpui::rgb(0x6366f1))
                                                        .cursor_pointer()
                                                        .when(!can_queue, |d| d.opacity(0.5))
                                                        .on_mouse_down(MouseButton::Left, {
                                                            let weak = weak_b.clone();
                                                            move |_, _, cx| {
                                                                let _ = weak.update(
                                                                    cx,
                                                                    |c, cx| {
                                                                        if can_queue {
                                                                            c.follow_up_input(cx)
                                                                        }
                                                                    },
                                                                );
                                                            }
                                                        }),
                                                ),
                                        )
                                    } else {
                                        None
                                    })
                                    .child(if streaming {
                                        div().into_any_element()
                                    } else {
                                        div()
                                            .id("send")
                                            .flex_shrink_0()
                                            .flex()
                                            .items_center()
                                            .gap_1p5()
                                            .px_3()
                                            .py_1p5()
                                            .rounded_lg()
                                            .bg(if can_queue {
                                                rgb(t.accent)
                                            } else {
                                                rgb(t.bg_panel)
                                            })
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(if can_queue {
                                                rgb(t.accent_contrast)
                                            } else {
                                                rgb(t.text_dim)
                                            })
                                            .cursor_pointer()
                                            .on_mouse_down(MouseButton::Left, cx.listener(
                                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                    this.send_input(cx);
                                                },
                                            ))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1p5()
                                                    .child(icon("send", 12., t.text))
                                                    .child(SharedString::from(tr("发送"))),
                                            )
                                            .into_any_element()
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_2()
                            .pt_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .text_xs()
                                    .text_color(rgb(t.text_muted))
                                    .child(
                                        div()
                                            .id("attach-image")
                                            .flex()
                                            .items_center()
                                            .cursor_pointer()
                                            .hover(|s| s.text_color(rgb(t.text)))
                                            .on_mouse_down(MouseButton::Left, cx.listener(
                                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                    this.attach_images(cx);
                                                },
                                            ))
                                            .child(icon("image", 12., t.text_muted)),
                                    )
                                    .child(
                                        div()
                                            .id("open-model-select")
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .cursor_pointer()
                                            .hover(|s| s.text_color(rgb(t.text)))
                                            .on_mouse_down(MouseButton::Left, {
                                                let weak = weak_for_dialog.clone();
                                                move |_, _, cx| {
                                                    let _ = weak.update(cx, |c, cx| {
                                                        if c.available_models.is_empty() {
                                                            c.refresh_state();
                                                        }
                                                        c.dialog =
                                                            Some(Self::model_select_dialog(cx));
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .child(icon("settings", 12., t.text_muted))
                                            .child(model_label),
                                    ),
                            )
                            .child(
                                div()
                                    .relative()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .text_xs()
                                    .text_color(rgb(t.text_muted))
                                    // thinking level pill -> popup menu
                                    // (pi-web ChatInput thinking dropdown)
                                    .child(
                                        div()
                                            .id("thinking-menu")
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .px(px(4.))
                                            .py(px(3.))
                                            .rounded(px(5.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                            .on_mouse_down(MouseButton::Left, cx.listener(
                                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                    this.pill_menu =
                                                        match this.pill_menu {
                                                            Some(PillMenu::Thinking) => None,
                                                            _ => Some(PillMenu::Thinking),
                                                        };
                                                    cx.notify();
                                                },
                                            ))
                                            .child(icon(
                                                "lightbulb",
                                                12.,
                                                if thinking_menu_open { t.accent } else { t.text_muted },
                                            ))
                                            .child(thinking_label),
                                    )
                                    // tools preset pill -> popup menu
                                    .child(
                                        div()
                                            .id("tools-menu")
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .px(px(4.))
                                            .py(px(3.))
                                            .rounded(px(5.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                            .on_mouse_down(MouseButton::Left, cx.listener(
                                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                    this.pill_menu =
                                                        match this.pill_menu {
                                                            Some(PillMenu::Tools) => None,
                                                            _ => Some(PillMenu::Tools),
                                                        };
                                                    cx.notify();
                                                },
                                            ))
                                            .child(icon(
                                                "wrench",
                                                12.,
                                                if tools_menu_open { t.accent } else { t.text_muted },
                                            ))
                                            .child(SharedString::from(tools_label)),
                                    )
                                    // compact (rpc compact)
                                    .child(
                                        div()
                                            .id("compact")
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .px(px(4.))
                                            .py(px(3.))
                                            .rounded(px(5.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                            .on_mouse_down(MouseButton::Left, cx.listener(
                                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                    this.compact_session(cx);
                                                },
                                            ))
                                            .child(icon("scissors", 12., t.text_muted))
                                            .child(SharedString::from(tr("压缩"))),
                                    )
                                    // 停止 (pi-web chat.stop; abort the run)
                                    .when(streaming, |d| {
                                        d.child(
                                            div()
                                                .id("stop")
                                                .flex()
                                                .items_center()
                                                .gap_1p5()
                                                .px_2()
                                                .py(px(3.))
                                                .rounded(px(5.))
                                                .border_1()
                                                .border_color(gpui::rgba(0xef44444d))
                                                .bg(gpui::rgba(0xef444414))
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(gpui::rgb(0xef4444))
                                                .cursor_pointer()
                                                .hover(|s| {
                                                    s.bg(gpui::rgba(0xef444429))
                                                })
                                                .on_mouse_down(MouseButton::Left, cx.listener(
                                                    |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                        this.abort_stream(cx);
                                                    },
                                                ))
                                                .child(
                                                    div()
                                                        .size(px(7.))
                                                        .rounded(px(1.5))
                                                        .bg(gpui::rgb(0xef4444)),
                                                )
                                                .child(SharedString::from(tr("停止"))),
                                        )
                                    })
                                    // notification sound toggle
                                    .child(
                                        div()
                                            .id("sound")
                                            .flex()
                                            .items_center()
                                            .px(px(4.))
                                            .py(px(3.))
                                            .rounded(px(5.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                            .on_mouse_down(MouseButton::Left, cx.listener(
                                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                    this.sound_on = !this.sound_on;
                                                    save_sound_pref(this.sound_on);
                                                    if this.sound_on {
                                                        play_notify_sound();
                                                    }
                                                    cx.notify();
                                                },
                                            ))
                                            .child(icon(
                                                "volume",
                                                12.,
                                                if self.sound_on { t.accent } else { t.text_muted },
                                            )),
                                    )
                            ),
                    ),
            )
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

        let mut root = div()
            .size_full()
            .relative()
            .flex()
            .flex_row()
            .bg(rgb(t.bg))
            .text_color(rgb(t.text))
            .font_family("Segoe UI")
            .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, window, cx| {
                if let Some((start_y, start_frac)) = this.resizing_sidebar {
                    let height = f32::from(window.viewport_size().height);
                    let frac = start_frac + (f32::from(ev.position.y) - f32::from(start_y)) / height;
                    this.sidebar_sessions_frac = frac.clamp(0.12, 0.85);
                    cx.notify();
                }
                if let Some((start_x, start_width)) = this.resizing_panel {
                    // growth direction left: dragging left widens the panel
                    let width = start_width - (f32::from(ev.position.x) - f32::from(start_x));
                    this.right_panel_width = width.clamp(300., 1200.);
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(
                |this, _: &gpui::MouseUpEvent, _w, cx| {
                    if this.resizing_sidebar.take().is_some()
                        || this.resizing_panel.take().is_some()
                    {
                        cx.notify();
                    }
                },
            ))
            .child(sidebar)
            .child(main_col);

        // ---- right panel: file + terminal tabs (pi-web AppShell panelTabs
        //      merge; fixed dark terminal surface in every theme) -----------
        if self.right_panel_open && !self.panel_tabs.is_empty() {
            let weak_for_tabs = weak.clone();
            let tabbar = div()
                .flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .children(self.panel_tabs.iter().enumerate().map(|(ix, tab)| {
                    let active = self.active_panel_tab == Some(ix);
                    let (icon_name, label, title_text) = match tab {
                        PanelTab::File(p) => (
                            "file-text",
                            p.file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| p.to_string_lossy().to_string()),
                            p.to_string_lossy().to_string(),
                        ),
                        PanelTab::Term(id) => {
                            let title = self
                                .terminals
                                .iter()
                                .find(|t| t.id == *id)
                                .map(|t| t.title.clone())
                                .unwrap_or_default();
                            let cwd = self
                                .terminals
                                .iter()
                                .find(|t| t.id == *id)
                                .map(|t| t.cwd.to_string_lossy().to_string())
                                .unwrap_or_default();
                            ("terminal", title, cwd)
                        }
                    };
                    let label: SharedString = label.into();
                    let title_text: SharedString = title_text.into();
                    let weak_tab = weak_for_tabs.clone();
                    let weak_close = weak_for_tabs.clone();
                    let term_focus: Option<gpui::FocusHandle> = match tab {
                        PanelTab::Term(id) => self
                            .terminals
                            .iter()
                            .find(|t| t.id == *id)
                            .map(|t| t.focus.clone()),
                        _ => None,
                    };
                    div()
                        .id(SharedString::from(format!("ptab-{ix}")))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .pl(px(12.))
                        .pr(px(6.))
                        .min_w(px(80.))
                        .max_w(px(180.))
                        .border_r_1()
                        .border_color(rgb(t.border))
                        .bg(if active { rgb(t.bg) } else { rgb(t.bg_panel) })
                        .text_xs()
                        .font_weight(if active {
                            gpui::FontWeight::MEDIUM
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .text_color(if active { rgb(t.text) } else { rgb(t.text_muted) })
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            let _ = weak_tab.update(cx, |c, cx| {
                                c.active_panel_tab = Some(ix);
                                cx.notify();
                            });
                            if let Some(f) = term_focus.clone() {
                                window.focus(&f);
                            }
                        })
                        // middle-click closes the tab (pi-web TabBar auxclick)
                        .on_mouse_down(MouseButton::Middle, {
                            let w = weak_for_tabs.clone();
                            move |_, _, cx| {
                                let _ = w.update(cx, |c, cx| c.close_panel_tab(ix, cx));
                            }
                        })
                        .child(icon(icon_name, 13., if active { t.text } else { t.text_muted }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .overflow_hidden()
                                .child(label),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("ptab-x-{ix}")))
                                .size(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .cursor_pointer()
                                .text_color(rgb(t.text_muted))
                                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                .on_mouse_down(MouseButton::Left, {
                                    let w = weak_close.clone();
                                    move |_, _, cx| {
                                        cx.stop_propagation();
                                        let _ = w.update(cx, |c, cx| c.close_panel_tab(ix, cx));
                                    }
                                })
                                .child(icon("x", 11., t.text_muted)),
                        )
                        .into_any_element()
                }));

            let body: Option<gpui::AnyElement> = self
                .active_panel_tab
                .and_then(|ix| self.panel_tabs.get(ix).cloned())
                .map(|tab| match tab {
                    PanelTab::Term(id) => {
                        // terminal panel (header + banners + grid)
                        let tix = self.terminals.iter().position(|t| t.id == id);
                        let Some(tix) = tix else {
                            return div().into_any_element();
                        };
                        let tab = &self.terminals[tix];
                        let (dot, _status) = match &tab.status {
                            TermStatus::Ready => (0x4ade80, ""),
                            TermStatus::Exited(_) | TermStatus::Failed(_) => (0xf87171, ""),
                        };
                        let cwd_text: SharedString = tab.cwd.to_string_lossy().to_string().into();
                        let weak_restart = weak.clone();
                        let mut col = div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(px(38.))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .pl(px(13.))
                                    .pr(px(10.))
                                    .bg(rgb(0x181b21))
                                    .border_b_1()
                                    .border_color(rgb(0x2f3540))
                                    .child(
                                        div()
                                            .size(px(7.))
                                            .rounded_full()
                                            .flex_shrink_0()
                                            .bg(rgb(dot)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(11.))
                                            .font_family(terminal::FONT_FAMILY)
                                            .text_color(rgb(0x9ca3af))
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .overflow_hidden()
                                            .child(cwd_text),
                                    )
                                    .child(
                                        div()
                                            .id("term-restart")
                                            .h(px(27.))
                                            .px(px(8.))
                                            .flex()
                                            .items_center()
                                            .rounded(px(5.))
                                            .border_1()
                                            .border_color(rgb(0x343a46))
                                            .text_color(rgb(0x9ca3af))
                                            .cursor_pointer()
                                            .hover(|s| {
                                                s.bg(rgb(0x242932)).text_color(rgb(0xe5e7eb))
                                            })
                                            .on_mouse_down(MouseButton::Left, {
                                                let rix = tix;
                                                move |_, _, cx| {
                                                    let _ = weak_restart.update(cx, |c, cx| {
                                                        c.restart_terminal(rix, cx);
                                                    });
                                                }
                                            })
                                            .child(icon("refresh", 12., 0x9ca3af)),
                                    ),
                            );
                        match &tab.status {
                            TermStatus::Failed(e) => {
                                col = col.child(
                                    div()
                                        .py(px(7.))
                                        .px(px(12.))
                                        .bg(rgb(0x321b1b))
                                        .border_b_1()
                                        .border_color(rgb(0x5f2424))
                                        .text_size(px(11.))
                                        .font_family(terminal::FONT_FAMILY)
                                        .text_color(rgb(0xfca5a5))
                                        .child(SharedString::from(e.clone())),
                                );
                            }
                            TermStatus::Exited(code) => {
                                let code_text = code
                                    .map(|c| c.to_string())
                                    .unwrap_or_else(|| tr("unknown").to_string());
                                col = col.child(
                                    div()
                                        .py(px(7.))
                                        .px(px(12.))
                                        .text_size(px(11.))
                                        .font_family(terminal::FONT_FAMILY)
                                        .text_color(rgb(0x9ca3af))
                                        .child(SharedString::from(crate::i18n::tf(
                                            "Process exited with code {code_text}",
                                            &[("code_text", code_text)],
                                        ))),
                                );
                            }
                            TermStatus::Ready => {}
                        }
                        col.child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .bg(rgb(0x111318))
                                .child(
                                    terminal::TerminalElement::new(tab, weak.clone())
                                        .track_focus(&tab.focus)
                                        .flex_1()
                                        .h_full()
                                        .on_mouse_down(MouseButton::Left, {
                                            let f = tab.focus.clone();
                                            move |_, window, _cx| {
                                                window.focus(&f);
                                            }
                                        })
                                        .on_key_down(cx.listener(
                                            |this, ev: &KeyDownEvent, _w, cx| {
                                                this.terminal_key(ev, cx);
                                            },
                                        )),
                                ),
                        )
                        .into_any_element()
                    }
                    PanelTab::File(path) => {
                        // file viewer (pi-web FileViewer header + source/preview)
                        let Some(fc) = self.file_cache.get(&path) else {
                            return div().into_any_element();
                        };
                        let content = fc.content.clone();
                        let meta: SharedString =
                            Self::file_meta(&path, &content).into();
                        let rel: SharedString = path
                            .strip_prefix(&self.cwd)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| path.to_string_lossy().to_string())
                            .into();
                        let is_md = Self::is_markdown(&path);
                        let preview = is_md
                            && self.file_preview_mode.get(&path).copied().unwrap_or(false);
                        let weak_mode = weak.clone();
                        let mode_path = path.clone();
                        let mut col = div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .bg(rgb(t.bg))
                            .child(
                                div()
                                    .h(px(38.))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .border_b_1()
                                    .border_color(rgb(t.border))
                                    .child(
                                        div()
                                            .font_family("Consolas")
                                            .text_size(px(11.))
                                            .text_color(rgb(t.text))
                                            .whitespace_nowrap()
                                            .child(rel),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(rgb(t.text_dim))
                                            .whitespace_nowrap()
                                            .child(meta),
                                    )
                                    .child(div().flex_1())
                                    .children(is_md.then(|| {
                                        div()
                                            .flex()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .id("fv-source")
                                                    .px_2()
                                                    .py(px(3.))
                                                    .rounded(px(4.))
                                                    .text_size(px(11.))
                                                    .cursor_pointer()
                                                    .bg(if preview {
                                                        rgb(t.bg_panel)
                                                    } else {
                                                        rgb(t.bg_selected)
                                                    })
                                                    .text_color(rgb(t.text))
                                                    .on_mouse_down(MouseButton::Left, {
                                                        let w = weak_mode.clone();
                                                        let p2 = mode_path.clone();
                                                        move |_, _, cx| {
                                                            let _ = w.update(cx, |c, cx| {
                                                                c.file_preview_mode.insert(p2.clone(), false);
                                                                cx.notify();
                                                            });
                                                        }
                                                    })
                                                    .child("Source"),
                                            )
                                            .child(
                                                div()
                                                    .id("fv-preview")
                                                    .px_2()
                                                    .py(px(3.))
                                                    .rounded(px(4.))
                                                    .text_size(px(11.))
                                                    .cursor_pointer()
                                                    .bg(if preview {
                                                        rgb(t.bg_selected)
                                                    } else {
                                                        rgb(t.bg_panel)
                                                    })
                                                    .text_color(rgb(t.text))
                                                    .on_mouse_down(MouseButton::Left, {
                                                        let w = weak_mode.clone();
                                                        let p2 = mode_path.clone();
                                                        move |_, _, cx| {
                                                            let _ = w.update(cx, |c, cx| {
                                                                c.file_preview_mode.insert(p2.clone(), true);
                                                                cx.notify();
                                                            });
                                                        }
                                                    })
                                                    .child("Preview"),
                                            )
                                    })),
                            );
                        if preview {
                            col = col.child(
                                div()
                                    .id("fv-scroll")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .py_4()
                                    .child(markdown::render(&content, &t)),
                            );
                        } else {
                            col = col.child(
                                div()
                                    .id("fv-scroll")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .py_2()
                                    .px_3()
                                    .font_family("Consolas")
                                    .text_size(px(12.))
                                    .text_color(rgb(t.text))
                                    .flex()
                                    .flex_col()
                                    .children(content.lines().map(|l| {
                                        div().child(SharedString::from(l.to_string()))
                                    })),
                            );
                        }
                        col.into_any_element()
                    }
                });

            root = root.child(
                div()
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    // drag handle (pi-web panel-resize-handle; growth left)
                    .child(
                        div()
                            .id("panel-resize")
                            .w(px(4.))
                            .h_full()
                            .cursor(gpui::CursorStyle::ResizeLeftRight)
                            .hover(|s| s.bg(rgb(t.accent)))
                            .on_mouse_down(MouseButton::Left, cx.listener(
                                |this, ev: &gpui::MouseDownEvent, _w, _cx| {
                                    this.resizing_panel =
                                        Some((ev.position.x, this.right_panel_width));
                                },
                            )),
                    )
                    .child(
                        div()
                            .w(px(self.right_panel_width))
                            .h_full()
                            .flex()
                            .flex_col()
                            .bg(rgb(t.bg))
                            .border_l_1()
                            .border_color(rgb(t.border))
                            .child(
                                div()
                                    .flex()
                                    .h(px(36.))
                                    .flex_shrink_0()
                                    .bg(rgb(t.bg_panel))
                                    .border_b_1()
                                    .border_color(rgb(t.border))
                                    .child(tabbar)
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .id("panel-close")
                                            .w(px(36.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .text_color(rgb(t.text_muted))
                                            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                            .on_mouse_down(MouseButton::Left, cx.listener(
                                                |this, _: &gpui::MouseDownEvent, window, cx| {
                                                    this.right_panel_open = false;
                                                    window.focus(&this.focus);
                                                    cx.notify();
                                                },
                                            ))
                                            .child(icon("x", 13., t.text_muted)),
                                    ),
                            )
                            .children(body),
                    ),
            );
        }

        // dialogs
        if let Some(Dialog::ModelSelect { input: filter_input }) = self.dialog.as_ref() {
            let flt = filter_input.read(cx).value().to_lowercase();
            // enabledModels whitelist narrows the picker (pi-web /api/models
            // resolveVisibleModels parity)
            let picker_enabled = !self.mc_state.all_enabled;
            let rows: Vec<gpui::AnyElement> = self
                .available_models
                .iter()
                .filter(|m| {
                    if picker_enabled {
                        let r = format!("{}/{}", m.provider, m.id);
                        if !self.mc_state.enabled.iter().any(|e| e == &r) {
                            return false;
                        }
                    }
                    flt.is_empty()
                        || m.id.to_lowercase().contains(&flt)
                        || m.name.to_lowercase().contains(&flt)
                        || m.provider.to_lowercase().contains(&flt)
                })
                .take(12)
                .map(|m| {
                    let provider = m.provider.clone();
                    let id = m.id.clone();
                    let weak_row = weak_for_dialog.clone();
                    let label: SharedString =
                        format!("{} / {}", m.provider, m.label()).into();
                    let ctx: SharedString = m
                        .context_window
                        .map(|c| format!("{}k", c / 1000))
                        .unwrap_or_default()
                        .into();
                    div()
                        .id(SharedString::from(format!("model-{provider}-{id}")))
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .cursor_pointer()
                        .rounded_md()
                        .hover(|s| s.bg(rgb(t.bg_selected)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let (p, mid) = (provider.clone(), id.clone());
                            let _ = weak_row.update(cx, |c, cx| c.select_model(p, mid, cx));
                        })
                        .flex()
                        .justify_between()
                        .child(div().text_xs().text_color(rgb(t.text)).child(label))
                        .child(div().text_xs().text_color(rgb(t.text_dim)).child(ctx))
                        .into_any_element()
                })
                .collect();
            let list_panel = if rows.is_empty() {
                div()
                    .py_2()
                    .text_xs()
                    .text_color(rgb(t.text_dim))
                    .child("no models match")
                    .into_any_element()
            } else {
                div().flex().flex_col().gap_0p5().children(rows).into_any_element()
            };
            root = root.child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::hsla(0., 0., 0., 0.35))
                    .track_focus(&self.dialog_focus)
                    .on_key_down({
                        let weak = weak_for_dialog.clone();
                        move |ev: &KeyDownEvent, _w, cx| {
                            if ev.keystroke.key == "escape" {
                                let _ = weak.update(cx, |this, cx| {
                                    this.dialog = None;
                                    cx.notify();
                                });
                            }
                        }
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(520.))
                            .max_h(px(560.))
                            .bg(rgb(t.bg_panel))
                            .border_1()
                            .border_color(rgb(t.border))
                            .rounded_lg()
                            .p_4()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .shadow_lg()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(rgb(t.text))
                                            .child("select model"),
                                    )
                                    .child(
                                        div()
                                            .id("model-close")
                                            .px_2()
                                            .cursor_pointer()
                                            .text_color(rgb(t.text_muted))
                                            .hover(|s| s.text_color(rgb(t.text)))
                                            .on_mouse_down(MouseButton::Left, {
                                                let weak = weak_for_dialog.clone();
                                                move |_, _, cx| {
                                                    let _ = weak.update(cx, |c, cx| {
                                                        c.dialog = None;
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .child(icon("x", 12., t.text_muted)),
                                    ),
                            )
                            .child(filter_input.clone())
                            .child(list_panel),
                    ),
            );
        }
        if self.dialog.as_ref().is_some_and(|d| matches!(d, Dialog::BranchTree)) {
            let t = T();
            let weak = weak_for_dialog.clone();
            let (has_session, tree, leaf_id) = match &self.branch_tree {
                Some((tree, leaf)) => (self.session.is_some(), tree.clone(), leaf.clone()),
                None => (self.session.is_some(), Vec::new(), None),
            };
            let has_branches = tree_has_branches(&tree);
            let active_path = build_active_path(&tree, leaf_id.as_deref());
            let top_level = select_top_level_branches(&tree);

            // ── node rows (BranchNavigator TreeNodeView, token-level) ──
            fn push_node(
                node: &TreeNode,
                skipped: usize,
                label: &str,
                is_last: bool,
                parent_lines: &[bool],
                active_path: &std::collections::HashSet<String>,
                weak: &gpui::WeakEntity<Chat>,
                out: &mut Vec<gpui::AnyElement>,
            ) {
                let t = T();
                let is_on_path = active_path.contains(&node.id);
                let is_active = is_on_path;
                let role = node.role.clone().unwrap_or_default();
                let mut row = div()
                    .w_full()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(t.bg_hover)));
                // indent guide lines
                for has_line in parent_lines {
                    row = row.child(
                        div()
                            .w(px(16.))
                            .h_full()
                            .border_l_1()
                            .border_color(if *has_line {
                                rgb(t.border)
                            } else {
                                gpui::rgba(0x00000000)
                            }),
                    );
                }
                // connector: vertical line + horizontal tick
                let mut connector = div()
                    .w(px(16.))
                    .h_full()
                    .flex()
                    .items_center()
                    .border_l_1()
                    .border_color(rgb(t.border));
                if !node.children.is_empty() || skipped > 0 {
                    connector = connector.child(
                        div()
                            .w(px(9.))
                            .h(px(1.))
                            .bg(rgb(t.border)),
                    );
                }
                row = row.child(connector);
                // node dot
                row = row.child(
                    div()
                        .size(px(7.))
                        .rounded_full()
                        .mr_1p5()
                        .flex_shrink_0()
                        .bg(if is_active {
                            rgb(t.accent)
                        } else if is_on_path {
                            rgb(t.text_dim)
                        } else {
                            rgb(t.border)
                        }),
                );
                // role badge
                if role == "user" || role == "assistant" {
                    row = row.child(
                        div()
                            .px_1()
                            .mr_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(if role == "user" {
                                rgb(t.accent)
                            } else {
                                rgb(t.border)
                            })
                            .text_size(px(9.))
                            .line_height(px(14.))
                            .text_color(if role == "user" {
                                rgb(t.accent)
                            } else {
                                rgb(t.text_dim)
                            })
                            .child(if role == "user" {
                                "U"
                            } else {
                                "A"
                            }),
                    );
                }
                // skipped indicator
                if skipped > 0 {
                    row = row.child(
                        div()
                            .text_size(px(10.))
                            .mr_1()
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(format!("+{skipped}"))),
                    );
                }
                // label
                row = row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(px(11.))
                        .font_weight(if is_active {
                            gpui::FontWeight::MEDIUM
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .text_color(if is_active {
                            rgb(t.text)
                        } else if is_on_path {
                            rgb(t.text_dim)
                        } else {
                            rgb(0x9ca3af)
                        })
                        .child(SharedString::from(label.to_string())),
                );
                // click = fork from this node's user message
                if let Some(entry_id) = node.forkable_entry_id() {
                    let entry_id = entry_id.to_string();
                    let weak_click = weak.clone();
                    row = row.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_click.update(cx, |c, cx| {
                            c.fork_from_entry(entry_id.clone(), cx);
                        });
                    });
                }
                out.push(row.into_any_element());
                let n_children = node.children.len();
                for (i, child) in node.children.iter().enumerate() {
                    let (rep, sk, lab) = compress_chain(child);
                    push_node(
                        &rep,
                        sk,
                        &lab,
                        i == n_children - 1,
                        &[
                            parent_lines,
                            &[!is_last as bool],
                        ]
                        .concat(),
                        active_path,
                        weak,
                        out,
                    );
                }
            }

            let mut rows: Vec<gpui::AnyElement> = Vec::new();
            for (i, node) in top_level.iter().enumerate() {
                let (rep, sk, lab) = compress_chain(node);
                push_node(
                    &rep,
                    sk,
                    &lab,
                    i == top_level.len() - 1,
                    &[],
                    &active_path,
                    &weak,
                    &mut rows,
                );
            }

            let body: gpui::AnyElement = if !has_session {
                div()
                    .px_4()
                    .py_2p5()
                    .text_xs()
                    .text_color(rgb(t.text_muted))
                    .child(tr("无活动会话"))
                    .into_any_element()
            } else if !has_branches || rows.is_empty() {
                div()
                    .px_4()
                    .py_2p5()
                    .text_xs()
                    .text_color(rgb(t.text_muted))
                    .child(tr("暂无分支"))
                    .into_any_element()
            } else {
                div()
                    .px_3()
                    .pt_1()
                    .pb_2()
                    .max_h(px(260.))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .children(rows)
                    .into_any_element()
            };

            root = root.child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::hsla(0., 0., 0., 0.35))
                    .track_focus(&self.dialog_focus)
                    .on_key_down({
                        let weak = weak_for_dialog.clone();
                        move |ev: &KeyDownEvent, _w, cx| {
                            if ev.keystroke.key == "escape" {
                                let _ = weak.update(cx, |this, cx| {
                                    this.dialog = None;
                                    cx.notify();
                                });
                            }
                        }
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(520.))
                            .max_h(px(560.))
                            .bg(rgb(t.bg_panel))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(rgb(t.border))
                            .shadow_lg()
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(rgb(t.text))
                                            .child(tr("分支")),
                                    )
                                    .child(
                                        div()
                                            .id("branch-close")
                                            .px_2()
                                            .cursor_pointer()
                                            .text_color(rgb(t.text_muted))
                                            .hover(|s| s.text_color(rgb(t.text)))
                                            .on_mouse_down(MouseButton::Left, {
                                                let weak = weak.clone();
                                                move |_, _, cx| {
                                                    let _ = weak.update(cx, |c, cx| {
                                                        c.dialog = None;
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .child(icon("x", 12., t.text_muted)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(t.text_dim))
                                    .child(tr("点击节点：从该用户消息处创建分支新会话")),
                            )
                            .child(body),
                    ),
            );
        }
        if self.dialog.as_ref().is_some_and(|d| matches!(d, Dialog::ProjectSelect)) {
            let t = T();
            let weak = weak_for_dialog.clone();
            // recent projects: unique cwds by latest activity (getRecentProjects parity)
            let mut latest: std::collections::HashMap<String, (PathBuf, std::time::SystemTime)> =
                std::collections::HashMap::new();
            for s in list_sessions(200) {
                let entry = latest.entry(s.cwd.clone()).or_insert((PathBuf::from(&s.cwd), s.modified));
                if s.modified > entry.1 {
                    entry.1 = s.modified;
                }
            }
            let mut projects: Vec<(String, PathBuf)> = latest.into_iter().map(|(k, v)| (k, v.0)).collect();
            projects.sort_by(|a, b| a.0.cmp(&b.0));
            let current = self.cwd.to_string_lossy().to_string();

            let mut rows: Vec<gpui::AnyElement> = Vec::new();
            for (cwd_text, cwd_path) in &projects {
                let is_current = same_ws(cwd_text, &current);
                let cwd_clone = cwd_path.clone();
                let weak_row = weak.clone();
                rows.push(
                    div()
                        .id(SharedString::from(format!("proj-{}", cwd_text)))
                        .w_full()
                        .px_3()
                        .py_2()
                        .rounded(px(7.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .cursor_pointer()
                        .when(is_current, |d| {
                            d.bg(rgb(t.bg_selected))
                                .border_1()
                                .border_color(rgb(t.accent))
                        })
                        .when(!is_current, |d| {
                            d.border_1()
                                .border_color(rgb(t.border))
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                        })
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let p = cwd_clone.clone();
                            let _ = weak_row.update(cx, |c, cx| {
                                c.switch_project(p, cx);
                            });
                        })
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(t.text))
                                .child(SharedString::from(cwd_text.clone())),
                        )
                        .child(if is_current {
                            icon("check", 12., t.accent)
                                .into_any_element()
                        } else {
                            div().into_any_element()
                        })
                        .into_any_element()
            );
            }
            root = root.child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::hsla(0., 0., 0., 0.35))
                    .track_focus(&self.dialog_focus)
                    .on_key_down({
                        let weak = weak_for_dialog.clone();
                        move |ev: &KeyDownEvent, _w, cx| {
                            if ev.keystroke.key == "escape" {
                                let _ = weak.update(cx, |this, cx| {
                                    this.dialog = None;
                                    cx.notify();
                                });
                            }
                        }
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(520.))
                            .max_h(px(560.))
                            .bg(rgb(t.bg_panel))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(rgb(t.border))
                            .shadow_lg()
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(rgb(t.text))
                                            .child(tr("选择项目")),
                                    )
                                    .child(
                                        div()
                                            .id("project-close")
                                            .px_2()
                                            .cursor_pointer()
                                            .text_color(rgb(t.text_muted))
                                            .hover(|s| s.text_color(rgb(t.text)))
                                            .on_mouse_down(MouseButton::Left, {
                                                let weak = weak_for_dialog.clone();
                                                move |_, _, cx| {
                                                    let _ = weak.update(cx, |c, cx| {
                                                        c.dialog = None;
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .child(icon("x", 12., t.text_muted)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(t.text_dim))
                                    .child(tr("切换后仅显示该项目的会话，并恢复上次打开的会话")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1p5()
                                    .max_h(px(380.))
                                    .overflow_hidden()
                                    .children(rows),
                            ),
                    ),
            );
        }
        if let Some(Dialog::GitDiff { path, patch }) = self.dialog.as_ref() {
            let path_text: SharedString = path.to_string_lossy().to_string().into();
            let mut body = patch.clone();
            if body.chars().count() > 60000 {
                body = body.chars().take(60000).collect();
                body.push_str("\n\n\u{2026} (truncated)");
            }
            root = root.child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::hsla(0., 0., 0., 0.35))
                    .track_focus(&self.dialog_focus)
                    .on_key_down({
                        let weak = weak_for_dialog.clone();
                        move |ev: &KeyDownEvent, _w, cx| {
                            if ev.keystroke.key == "escape" {
                                let _ = weak.update(cx, |this, cx| {
                                    this.dialog = None;
                                    cx.notify();
                                });
                            }
                        }
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(760.))
                            .max_h(px(640.))
                            .bg(rgb(t.bg_panel))
                            .border_1()
                            .border_color(rgb(t.border))
                            .rounded(px(8.))
                            .p_4()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .shadow_lg()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(icon("git-branch", 12., t.accent))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_family("Consolas")
                                                    .text_color(rgb(t.text_muted))
                                                    .child(path_text),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("diff-close")
                                            .px_2()
                                            .cursor_pointer()
                                            .text_color(rgb(t.text_muted))
                                            .hover(|s| s.text_color(rgb(t.text)))
                                            .on_mouse_down(MouseButton::Left, {
                                                let weak = weak_for_dialog.clone();
                                                move |_, _, cx| {
                                                    let _ = weak.update(cx, |c, cx| {
                                                        c.dialog = None;
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .child(icon("x", 12., t.text_muted)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .max_h(px(520.))
                                    .overflow_hidden()
                                    .p_2()
                                    .rounded(px(6.))
                                    .bg(rgb(t.bg))
                                    .font_family("Consolas")
                                    .text_size(px(11.))
                                    .text_color(rgb(t.text))
                                    .child(SharedString::from(body)),
                            ),
                    ),
            );
        }
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

/// One extension widget block (mono lines, pi-web widget rendering).
fn render_ext_widget(lines: &[String], t: &crate::theme::Theme) -> gpui::AnyElement {
    let text: String = lines.join("\n");
    div()
        .w_full()
        .px_3()
        .py_2()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.tool_bg))
        .font_family("Consolas")
        .text_size(px(11.))
        .text_color(rgb(t.text_muted))
        .child(SharedString::from(text))
        .into_any_element()
}

/// Blocking extension UI dialog (select/confirm/input/editor).
fn render_ext_dialog(
    chat: &mut Chat,
    req: pi_link::protocol::ExtensionUiRequest,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    use pi_link::protocol::ExtUiMethod;
    let t = T();
    let weak = weak.clone();
    let (title, body): (String, gpui::AnyElement) = match &req.method {
        ExtUiMethod::Select { title, options } => {
            let weak_opts = weak.clone();
            let opts: Vec<gpui::AnyElement> = options
                .iter()
                .enumerate()
                .map(|(ix, o)| {
                    let weak_o = weak_opts.clone();
                    let v = o.clone();
                    div()
                        .id(SharedString::from(format!("ext-opt-{ix}")))
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .text_size(px(12.))
                        .text_color(rgb(t.text))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_selected)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_o.update(cx, |c, cx| {
                                c.ext_respond(Some(v.clone()), None, false, cx)
                            });
                        })
                        .child(SharedString::from(o.clone()))
                        .into_any_element()
                })
                .collect();
            (
                title.clone(),
                div().flex().flex_col().gap_1().children(opts).into_any_element(),
            )
        }
        ExtUiMethod::Confirm { title, message } => {
            let msg: SharedString = message.clone().into();
            (
                title.clone(),
                div().text_size(px(12.)).text_color(rgb(t.text_muted)).child(msg).into_any_element(),
            )
        }
        ExtUiMethod::Input { title, .. } | ExtUiMethod::Editor { title, .. } => {
            (title.clone(), chat.ext_input.clone().into_any_element())
        }
        _ => (String::new(), div().into_any_element()),
    };
    let title: SharedString = title.into();
    let is_confirm = matches!(req.method, ExtUiMethod::Confirm { .. });
    let is_select = matches!(req.method, ExtUiMethod::Select { .. });
    let weak_cancel = weak.clone();
    let weak_ok = weak.clone();
    div()
        .absolute()
        .inset_0()
        .bg(gpui::hsla(0., 0., 0., 0.35))
        .track_focus(&chat.dialog_focus)
        .on_key_down({
            let weak = weak_cancel.clone();
            move |ev: &KeyDownEvent, _w, cx| {
                if ev.keystroke.key == "escape" {
                    let _ = weak.update(cx, |c, cx| c.ext_respond(None, None, true, cx));
                }
            }
        })
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(460.))
                .bg(rgb(t.bg_panel))
                .border_1()
                .border_color(rgb(t.border))
                .rounded_lg()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .shadow_lg()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(title),
                )
                .child(body)
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .children((!is_select).then(|| {
                            div()
                                .id("ext-cancel")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(t.border))
                                .text_xs()
                                .text_color(rgb(t.text_muted))
                                .cursor_pointer()
                                .hover(|s| s.text_color(rgb(t.text)))
                                .on_mouse_down(MouseButton::Left, {
                                    let weak = weak_cancel.clone();
                                    move |_, _, cx| {
                                        let _ = weak.update(cx, |c, cx| {
                                            c.ext_respond(None, None, true, cx)
                                        });
                                    }
                                })
                                .child(tr("取消"))
                                .into_any_element()
                        }))
                        .children(is_confirm.then(|| {
                            div()
                                .id("ext-no")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(t.border))
                                .text_xs()
                                .text_color(rgb(t.text))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .on_mouse_down(MouseButton::Left, {
                                    let weak = weak_ok.clone();
                                    move |_, _, cx| {
                                        let _ = weak.update(cx, |c, cx| {
                                            c.ext_respond(None, Some(false), false, cx)
                                        });
                                    }
                                })
                                .child(tr("否"))
                                .into_any_element()
                        }))
                        .children((!is_select).then(|| {
                            let label = if is_confirm { tr("是") } else { tr("提交") };
                            div()
                                .id("ext-ok")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(rgb(t.accent))
                                .text_xs()
                                .text_color(rgb(t.accent_contrast))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.accent_hover)))
                                .on_mouse_down(MouseButton::Left, {
                                    let weak = weak_ok.clone();
                                    move |_, _, cx| {
                                        let _ = weak.update(cx, |c, cx| {
                                            if is_confirm {
                                                c.ext_respond(None, Some(true), false, cx);
                                            } else {
                                                let v =
                                                    c.ext_input.read(cx).value().to_string();
                                                c.ext_respond(Some(v), None, false, cx);
                                            }
                                        });
                                    }
                                })
                                .child(label)
                                .into_any_element()
                        })),
                ),
        )
        .into_any_element()
}

/// Models panel dialog (pi-web ModelsConfig parity): 900px surface, 240px
/// provider sidebar, detail pane with API-key editor + per-provider
/// enabledModels list (36px rows, 32×18 ConfigSwitch, pi-web tokens).


fn main() {
    // theme: PI_FLASH_THEME (dev override) > persisted settings.json > mist
    if std::env::var("PI_FLASH_THEME").ok().and_then(|n| theme::set_by_name(&n).then_some(())).is_none() {
        if let Some(name) = pi_link::config::read_theme(&pi_link::config::settings_path()) {
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
            let bounds = gpui::Bounds::centered(None, gpui::size(px(1180.), px(760.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("pi-flash".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| cx.new(Chat::new),
            )
            .unwrap();
            cx.activate(true);
        });
}

