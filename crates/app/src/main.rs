//! pi-flash — desktop shell for the pi coding agent.
//!
//! Component-by-component translation of pi-web (see PORT_PLAN.md). Layout
//! values (sizes, colors, spacing) come from pi-web sources: globals.css
//! theme tokens, panel-layout.ts, MessageView/ChatInput/AppShell structures.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use futures::{StreamExt, channel::mpsc::UnboundedReceiver};
use gpui::{
    App, Application, Context, FocusHandle, Focusable, KeyDownEvent, ListAlignment, ListState,
    MouseButton, ParentElement, Render, SharedString, Styled, WindowOptions, div, list,
    prelude::*, px, relative, rgb,
};
use pi_link::client::{PiSession, spawn as spawn_pi};
use pi_link::protocol::{
    AssistantEvent, Block, Command, Event, SessionState, SessionStats, SlashCommand, TreeNode,
    Usage, content_blocks, parse_tree,
};
use pi_link::sessions::{SessionInfo, list_sessions};

mod assets;
mod markdown;
mod theme;
mod terminal;
use theme::theme as T;
use terminal::{TermStatus, TerminalTab};

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

#[derive(Debug, Clone, PartialEq)]
enum Dialog {
    RenameSession { value: String },
    ModelSelect { filter: String },
    FilePreview { path: PathBuf, content: String },
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
        };
        chat.sessions_list.reset(chat.sessions.len());
        chat.load_project_files();
        chat.refresh_state();

        if let Some(events) = events {
            cx.spawn(async move |this, cx| {
                consume_events(this, cx, events, 1).await;
            })
            .detach();
        }
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

    fn notify_list(&mut self, cx: &mut Context<Self>) {
        self.list.reset(self.messages.len());
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

    fn send_input(&mut self, cx: &mut Context<Self>) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(session) = &self.session else {
            self.status = "未连接".into();
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
                    self.history.push(text);
                }
                self.history_ix = None;
                self.input.clear();
                self.pending_images.clear();
                self.status = if streaming { "steering" } else { "running" }.into();
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

    fn confirm_rename(&mut self, cx: &mut Context<Self>) {
        if let Some(Dialog::RenameSession { value }) = &self.dialog {
            let name = value.trim().to_string();
            if let Some(session) = &self.session {
                let _ = session.send(&Command::SetSessionName { name });
            }
            self.refresh_state();
        }
        self.dialog = None;
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

    fn cycle_thinking(&mut self, cx: &mut Context<Self>) {
        const LEVELS: [&str; 7] =
            ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
        let cur = self
            .state
            .as_ref()
            .and_then(|s| s.thinking_level.clone())
            .unwrap_or_else(|| "medium".into());
        let idx = LEVELS.iter().position(|&l| l == cur).map(|i| i + 1).unwrap_or(0);
        let next = LEVELS[idx % LEVELS.len()].to_string();
        if let Some(session) = &self.session {
            let _ = session.send(&Command::SetThinkingLevel { level: next.clone() });
        }
        self.refresh_state();
        self.status = format!("thinking: {next}");
        cx.notify();
    }

    fn auto_title(&mut self, cx: &mut Context<Self>) {
        let title = self
            .messages
            .iter()
            .find(|m| m.role == Role::User)
            .map(|m| m.plain_text())
            .unwrap_or_default();
        let mut title = title.trim().replace('\n', " ").to_string();
        if title.is_empty() {
            self.status = "nothing to title yet".into();
            cx.notify();
            return;
        }
        if title.chars().count() > 40 {
            let mut cut = 40;
            while !title.is_char_boundary(cut) {
                cut -= 1;
            }
            title.truncate(cut);
            title.push('\u{2026}');
        }
        if let Some(session) = &self.session {
            let _ = session.send(&Command::SetSessionName { name: title });
        }
        self.refresh_state();
        cx.notify();
    }

    fn export_html(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
            let _ = session.send(&Command::ExportHtml);
            self.status = "exporting...".into();
        }
        cx.notify();
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
        clear_last_open(&self.cwd.to_string_lossy());
        self.status = status_line(self.session.is_some(), "新会话");
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

    fn delete_session(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.active_session_file.as_deref() == Some(path.as_path()) {
            self.status = "cannot delete the active session".into();
            cx.notify();
            return;
        }
        match std::fs::remove_file(&path) {
            Ok(_) => {
                self.sessions.retain(|s| s.path != path);
                self.sessions_list.reset(self.sessions.len());
                self.status = "session deleted".into();
            }
            Err(e) => self.status = format!("delete failed: {e}"),
        }
        cx.notify();
    }

    fn open_file_preview(&mut self, path: PathBuf, cx: &mut Context<Self>) {
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
        self.dialog = Some(Dialog::FilePreview { path, content });
        cx.notify();
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
                        self.dialog = Some(Dialog::RenameSession {
                            value: self
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
                                .unwrap_or_default(),
                        });
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
                        self.messages
                            .push(Msg { role: Role::User, blocks, usage: None, entry_id: None });
                    }
                    "assistant" => {
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
            Event::MessageUpdate(assistant_event) => match assistant_event {
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
            },
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
            Event::AgentStart => self.status = "running".into(),
            Event::AgentSettled => {
                self.status = status_line(true, "idle");
                self.refresh_state();
            }
            Event::AgentEnd { .. } => {
                // refresh branch tree so newly-sent user messages gain entry ids
                if let Some(s) = self.session.as_ref() {
                    let _ = s.send(&Command::GetTree);
                }
                // agent may have written files: refresh git status
                self.refresh_git();
            }
            Event::ExtensionUi(_) => {}
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

/// Key holding the globally-last active workspace (startup target).
const WS_LAST_KEY: &str = "__last";

/// Per-workspace "last open session" memory (pi-web workspace-memory parity).
/// Stored at ~/.pi/agent/pi-flash-workspace.json as { "<cwd>": "<session path>" };
/// an empty string means "this workspace was left on a blank new session".
fn workspace_memory_path() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    Some(
        Path::new(&home)
            .join(".pi")
            .join("agent")
            .join("pi-flash-workspace.json"),
    )
}

fn load_workspace_memory() -> serde_json::Map<String, serde_json::Value> {
    let Some(path) = workspace_memory_path() else {
        return serde_json::Map::new();
    };
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return serde_json::Map::new(),
    };
    let parsed = match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => v,
        Err(_) => return serde_json::Map::new(),
    };
    let mut out = serde_json::Map::new();
    if let Some(obj) = parsed.as_object() {
        for (k, v) in obj {
            // `__last` stays as-is; workspace keys get normalized once.
            if k == WS_LAST_KEY {
                out.insert(k.clone(), v.clone());
            } else {
                out.insert(ws_key(k), v.clone());
            }
        }
    }
    out
}

fn save_workspace_memory(map: &serde_json::Map<String, serde_json::Value>) {
    let Some(path) = workspace_memory_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(raw) = serde_json::to_string_pretty(map) {
        let _ = std::fs::write(path, raw);
    }
}

// ---------------------------------------------------------------------------
// git status/diff (lib/git-changes.ts parity)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflict,
}

impl GitStatus {
    fn badge(&self) -> &'static str {
        match self {
            GitStatus::Modified => "M",
            GitStatus::Added => "A",
            GitStatus::Deleted => "D",
            GitStatus::Renamed => "R",
            GitStatus::Untracked => "U",
            GitStatus::Conflict => "C",
        }
    }

    fn color(&self) -> u32 {
        match self {
            GitStatus::Modified => 0xd6a84b,
            GitStatus::Added | GitStatus::Untracked => 0x4ade80,
            GitStatus::Deleted | GitStatus::Conflict => 0xf87171,
            GitStatus::Renamed => 0x60a5fa,
        }
    }
}

#[derive(Debug, Clone)]
struct GitFile {
    path: PathBuf,
    status: GitStatus,
}

fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// `git status --porcelain=v1 -z --untracked-files=all` parsed to files.
fn git_status_files(cwd: &Path) -> Vec<GitFile> {
    let Some(out) = run_git(cwd, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])
    else {
        return Vec::new();
    };
    let mut files = Vec::new();
    // -z: NUL-separated records; rename records embed a TAB then the new name
    for rec in out.split('\0') {
        if rec.len() < 4 {
            continue;
        }
        let xy = &rec[..2];
        let rest = &rec[3..];
        let (status, file_path) = match xy {
            "??" => (GitStatus::Untracked, rest),
            _ => {
                let x = xy.as_bytes()[0];
                let y = xy.as_bytes()[1];
                let st = if x == b'D' || y == b'D' {
                    GitStatus::Deleted
                } else if x == b'A' {
                    GitStatus::Added
                } else if x == b'R' || y == b'R' {
                    GitStatus::Renamed
                } else if x == b'U' || y == b'U' || (x == b'A' && y == b'A') {
                    GitStatus::Conflict
                } else {
                    GitStatus::Modified
                };
                // renames: "orig	new" — track the new name
                let p = match rest.split_once('\t') {
                    Some((_, new)) => new,
                    None => rest,
                };
                (st, p)
            }
        };
        let full = cwd.join(file_path);
        files.push(GitFile { path: full, status });
    }
    files
}

/// Total (+, -) line counts of tracked changes (numstat HEAD summary).
fn git_numstat(cwd: &Path) -> (u64, u64) {
    let Some(out) = run_git(
        cwd,
        &["diff", "--no-color", "--no-ext-diff", "--numstat", "HEAD"],
    ) else {
        return (0, 0);
    };
    let (mut add, mut del) = (0u64, 0u64);
    for line in out.lines() {
        let mut it = line.split('\t');
        let (Some(a), Some(d)) = (it.next(), it.next()) else { continue };
        if let Ok(n) = a.parse::<u64>() { add += n; }
        if let Ok(n) = d.parse::<u64>() { del += n; }
    }
    (add, del)
}

/// Unified diff for one file; untracked files render as all-added content.
fn git_file_diff(cwd: &Path, path: &Path, untracked: bool) -> String {
    if untracked {
        if let Ok(text) = std::fs::read_to_string(path) {
            let rel = path
                .strip_prefix(cwd)
                .unwrap_or(path)
                .to_string_lossy()
                .replace("\\", "/");
            let mut out = format!("@@ -0,0 +1,L @@\n");
            for line in text.lines() {
                out.push('+');
                out.push_str(line);
                out.push('\n');
            }
            let _ = rel;
            return out;
        }
        return String::new();
    }
    run_git(
        cwd,
        &["diff", "--no-color", "--no-ext-diff", "--", &path.to_string_lossy()],
    )
    .unwrap_or_default()
}

/// Normalized workspace key: Path components joined with "\\", so
/// "D:/a/b" and "D:\a\b" share one memory slot (Windows path parity).
fn ws_key(cwd: &str) -> String {
    // String-level normalization: "/" -> "\\", strip trailing separator,
    // case-fold (Windows paths are case-insensitive).
    let mut k = cwd.replace('/', "\\");
    while k.ends_with('\\') {
        k.pop();
    }
    k.to_ascii_lowercase()
}

/// Workspace-key-aware path equality.
fn same_ws(a: &str, b: &str) -> bool {
    ws_key(a) == ws_key(b)
}

/// Path equality via the same string-level normalization as same_ws
/// (Windows Path::components is not reusable as a key).
fn same_path(a: &Path, b: &Path) -> bool {
    same_ws(&a.to_string_lossy(), &b.to_string_lossy())
}

/// Remember `session_path` as the last open session for `cwd` and mark it
/// as the globally-last active workspace (startup restore target).
fn set_last_open(cwd: &str, session_path: &str) {
    let mut map = load_workspace_memory();
    let key = ws_key(cwd);
    map.insert(WS_LAST_KEY.to_string(), serde_json::Value::String(key.clone().into()));
    map.insert(key, serde_json::Value::String(session_path.into()));
    save_workspace_memory(&map);
}

/// Mark `cwd` as the globally-last active workspace.
fn set_last_workspace(cwd: &str) {
    let mut map = load_workspace_memory();
    map.insert(
        WS_LAST_KEY.to_string(),
        serde_json::Value::String(ws_key(cwd).into()),
    );
    save_workspace_memory(&map);
}

/// The workspace the app was last used in, if known.
fn get_last_workspace() -> Option<String> {
    load_workspace_memory()
        .get(WS_LAST_KEY)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Remember that `cwd` was left on a blank new session.
fn clear_last_open(cwd: &str) {
    let mut map = load_workspace_memory();
    map.insert(ws_key(cwd), serde_json::Value::String(String::new()));
    save_workspace_memory(&map);
}

/// The remembered session path for `cwd`, if any.
fn get_last_open(cwd: &str) -> Option<String> {
    load_workspace_memory()
        .get(&ws_key(cwd))?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn status_line(connected: bool, state: &str) -> String {
    if connected {
        format!("pi {} | {state}", pi_link::PI_VENDOR_VERSION)
    } else {
        "pi not available (vendor missing)".to_string()
    }
}

fn usage_footer(u: &UsageLine) -> String {
    let mut s = format!(
        "{} in · {} out",
        fmt_thousands(u.input),
        fmt_thousands(u.output)
    );
    if u.cache_read > 0 {
        s.push_str(&format!(" · {} cache R", fmt_thousands(u.cache_read)));
    }
    s.push_str(&format!(" · ${:.4}", u.cost));
    s
}

fn fmt_thousands(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::new();
    for (i, ch) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*ch as char);
    }
    out
}

fn fmt_hhmm(ms: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_millis_opt(ms) {
        chrono::LocalResult::Single(t) => t.format("%H:%M").to_string(),
        _ => String::new(),
    }
}

fn time_ago(modified: std::time::SystemTime) -> String {
    let secs = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?;
            Some(now.as_secs() as i64 - d.as_secs() as i64)
        })
        .unwrap_or(0)
        .max(0);
    match secs {
        0..=59 => format!("{secs}秒前"),
        60..=3599 => format!("{}分钟前", secs / 60),
        3600..=86399 => format!("{}小时前", secs / 3600),
        _ => format!("{}天前", secs / 86400),
    }
}

fn read_branch(cwd: &Path) -> String {
    let Ok(head) = std::fs::read_to_string(cwd.join(".git").join("HEAD")) else {
        return String::new();
    };
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .unwrap_or(head.trim())
        .to_string()
}

fn cwd_tail(cwd: &str) -> String {
    cwd.rsplit(['/', '\\']).next().unwrap_or(cwd).to_string()
}

fn top_level_entries(cwd: &Path) -> Vec<(bool, String)> {
    let Ok(rd) = std::fs::read_dir(cwd) else { return Vec::new() };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            dirs.push(name);
        } else {
            files.push(name);
        }
    }
    dirs.sort();
    files.sort();
    dirs.iter()
        .map(|n| (true, n.clone()))
        .chain(files.iter().map(|n| (false, n.clone())))
        .take(12)
        .collect()
}

fn walk_files(cwd: &Path, depth: usize, cap: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    fn rec(dir: &Path, rel: &str, depth: usize, cap: usize, out: &mut Vec<String>) {
        if depth == 0 || out.len() >= cap {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= cap {
                return;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "dist"
            {
                continue;
            }
            let rel_path =
                if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                rec(&e.path(), &rel_path, depth - 1, cap, out);
            } else {
                out.push(rel_path);
            }
        }
    }
    rec(cwd, "", depth, cap, &mut out);
    out
}

fn mime_from_ext(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png".to_string(),
        Some("jpg") | Some("jpeg") => "image/jpeg".to_string(),
        Some("gif") => "image/gif".to_string(),
        Some("webp") => "image/webp".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

fn pretty_args(args: &str) -> String {
    serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| args.to_string())
}

fn fmt_compact(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

// ---------------------------------------------------------------------------
// branch tree helpers (BranchNavigator.tsx parity)
// ---------------------------------------------------------------------------

/// Iterative check: does the tree branch anywhere?
fn tree_has_branches(nodes: &[TreeNode]) -> bool {
    if nodes.len() > 1 {
        return true;
    }
    let mut stack: Vec<&TreeNode> = nodes.iter().collect();
    while let Some(n) = stack.pop() {
        if n.children.len() > 1 {
            return true;
        }
        for c in &n.children {
            stack.push(c);
        }
    }
    false
}

/// Ids on the root→leaf path (iterative DFS, BranchNavigator parity).
fn build_active_path(nodes: &[TreeNode], leaf_id: Option<&str>) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let Some(target) = leaf_id else {
        return out;
    };
    let mut stack: Vec<(&TreeNode, Vec<String>)> =
        nodes.iter().map(|n| (n, vec![n.id.clone()])).collect();
    while let Some((node, path)) = stack.pop() {
        if node.id == target {
            out.extend(path);
            break;
        }
        for c in &node.children {
            let mut p = path.clone();
            p.push(c.id.clone());
            stack.push((c, p));
        }
    }
    out
}

/// Compress a single-child chain into its branching/leaf representative.
/// Returns (representative, skipped count, label). Label prefers the first
/// message text on the chain (40 chars, pi-web getLabel parity).
fn compress_chain(node: &TreeNode) -> (TreeNode, usize, String) {
    let mut current = node.clone();
    let mut label_entry: Option<String> = message_label(node);
    let mut skipped = 0usize;
    while current.children.len() == 1 {
        current = current.children[0].clone();
        if label_entry.is_none() {
            label_entry = message_label(&current);
        }
        skipped += 1;
    }
    let label = label_entry
        .or_else(|| message_label(&current))
        .unwrap_or_else(|| current.entry_type.clone());
    (current, skipped, label)
}

/// 40-char label for message entries (getLabel parity).
fn message_label(node: &TreeNode) -> Option<String> {
    if node.entry_type != "message" || node.role.as_deref() == Some("system") {
        return None;
    }
    let mut text = node.text.clone()?;
    if text.is_empty() {
        if node.role.as_deref() == Some("assistant") {
            text = "[assistant]".into();
        } else {
            return None;
        }
    }
    let mut t: String = text.chars().take(40).collect();
    if text.chars().count() > 40 {
        t.push('…');
    }
    Some(t)
}

/// User-message entry ids along the root→leaf path (fork anchors for the
/// per-message fork button). Ordering matches the projected user messages.
fn collect_path_user_ids(nodes: &[TreeNode], leaf_id: Option<&str>) -> Vec<String> {
    let Some(target) = leaf_id else {
        return Vec::new();
    };
    fn flatten<'a>(nodes: &'a [TreeNode], map: &mut std::collections::HashMap<String, &'a TreeNode>) {
        for n in nodes {
            map.insert(n.id.clone(), n);
            flatten(&n.children, map);
        }
    }
    let mut map = std::collections::HashMap::new();
    flatten(nodes, &mut map);
    let mut chain: Vec<TreeNode> = Vec::new();
    let mut cur = map.get(target);
    while let Some(n) = cur {
        chain.push((*n).clone());
        cur = n.parent_id.as_deref().and_then(|pid| map.get(pid));
    }
    chain.reverse();
    chain
        .into_iter()
        .filter(|n| n.role.as_deref() == Some("user"))
        .map(|n| n.id)
        .collect()
}

/// Top-level rows: multiple roots => the roots; otherwise children of the
/// first branching node (empty when the session is linear).
fn select_top_level_branches(tree: &[TreeNode]) -> Vec<TreeNode> {
    if tree.len() > 1 {
        return tree.to_vec();
    }
    if tree.is_empty() {
        return Vec::new();
    }
    let first = compress_chain(&tree[0]).0;
    if first.children.len() > 1 {
        first.children.clone()
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------

fn icon(name: &'static str, size: f32, color: u32) -> gpui::AnyElement {
    gpui::svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .text_color(rgb(color))
        .size(px(size))
        .into_any_element()
}

fn pill(
    id: &'static str,
    icon_name: &'static str,
    label: SharedString,
) -> gpui::AnyElement {
    let t = T();
    div()
        .id(id)
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
        .child(icon(icon_name, 12., t.text_muted))
        .child(label)
        .into_any_element()
}

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
            let is_collapsed = collapsed.contains(&key);
            let weak = weak.clone();
            let mut block = div()
                .w_full()
                .my_1()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg_subtle))
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "th-{msg_ix}-{content_index}"
                        )))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_xs()
                        .text_color(rgb(t.text_dim))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let k = key;
                            let _ = weak.update(cx, |c, cx| {
                                if !c.collapsed.remove(&k) {
                                    c.collapsed.insert(k);
                                }
                                cx.notify();
                            });
                        })
                        .child(icon("lightbulb", 11., t.text_dim))
                        .child(SharedString::from("thinking"))
                        .child(if is_collapsed {
                            icon("chevron-right", 10., t.text_dim)
                        } else {
                            icon("chevron-down", 10., t.text_dim)
                        }),
                );
            if !is_collapsed {
                block = block.child(
                    div()
                        .text_sm()
                        .text_color(rgb(t.text))
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
                        c.open_file_preview(fp.clone(), cx);
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
                    .child(SharedString::from("新分支")),
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
        // MessageView: model label 11px --text-dim, margin-bottom 4
        col = col.child(
            div()
                .text_xs()
                .text_color(rgb(t.text_dim))
                .mb_1()
                .child(SharedString::from(model_label.to_string())),
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
                    .child(SharedString::from(usage_footer(u)))
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
        if self.dialog.is_some() {
            window.focus(&self.dialog_focus);
        } else if !self.terminals.iter().any(|t| t.focus.is_focused(window)) {
            window.focus(&self.focus);
        }
        let t = T();

        let status: SharedString = self.status.clone().into();
        let input_ph: SharedString = if self.input.is_empty() {
            "消息...输入 / 使用命令，输入 @ 查找文件".into()
        } else {
            self.input.clone().into()
        };
        let input_empty = self.input.is_empty();
        let model_label: SharedString = self
            .state
            .as_ref()
            .and_then(|s| s.model_label())
            .unwrap_or_else(|| "选择模型".into())
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
                                    .child(SharedString::from("新建")),
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
                            .child(SharedString::from("主分支"))
                            .child(icon("chevron-down", 10., t.text_muted)),
                    ),
            )
            // sessions list
            .child(
                list(self.sessions_list.clone(), move |ix, _window, cx| {
                    let chat = sessions_entity.read(cx);
                    let Some(info) = chat.sessions.get(ix) else {
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
                    let streaming = is_active
                        && chat
                            .state
                            .as_ref()
                            .is_some_and(|s| s.is_streaming);
                    let hovered = chat.hovered_session == Some(ix);
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
                        .cursor_pointer()
                        .border_l_2()
                        .when(is_active, |d| {
                            d.bg(rgb(t.bg_selected))
                                .border_color(rgb(t.accent))
                        })
                        .when(!is_active, |d| d.border_color(rgb(t.bg)))
                        .when(hovered && !is_active, |d| d.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, {
                            let p = path.clone();
                            move |_, _, cx| {
                                let _ = weak.update(cx, |c, cx| c.open_session(p.clone(), false, cx));
                            }
                        })
                        .on_hover(move |h, _, cx| {
                            let _ = weak_hover.update(cx, |c, cx| {
                                let next = if *h { Some(ix) } else { None };
                                if c.hovered_session != next {
                                    c.hovered_session = next;
                                    cx.notify();
                                }
                            });
                        })
                        .child(
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
                                        .child(if is_active && streaming {
                                            icon("loader", 14., t.accent).into_any_element()
                                        } else {
                                            SharedString::from(time_text.clone())
                                                .into_any_element()
                                        })
                                        .child(SharedString::from(format!(
                                            "{} 条消息",
                                            info.message_count
                                        ))),
                                ),
                        )
                        .children(if hovered {
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
                                                            c.dialog =
                                                                Some(Dialog::RenameSession {
                                                                value: c
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
                                                                    .unwrap_or_default(),
                                                                });
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
                                                s.bg(rgb(0xf6e9e9))
                                                    .text_color(rgb(0xef4444))
                                            })
                                            .on_mouse_down(MouseButton::Left, {
                                                let p = p_del.clone();
                                                move |_, _, cx| {
                                                    cx.stop_propagation();
                                                    let _ = weak_del.update(cx, |c, cx| {
                                                        c.delete_session(p.clone(), cx);
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
                })
                .flex_1()
                .min_h_0(),
            )
            // file explorer section
            .child(
                div()
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
                                    .child(SharedString::from("文件浏览器")),
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
                            .child(icon("settings", 12., t.text_muted))
                            .child(SharedString::from("模型")),
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
                            .child(icon("layers", 12., t.text_muted))
                            .child(SharedString::from("技能")),
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
                            .child(icon("settings", 12., t.text_muted))
                            .child(SharedString::from("设置")),
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
                        SharedString::from("完整历史"),
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
                            .child(SharedString::from("分支")),
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
                            .child(SharedString::from("生成标题")),
                    )
                    .child(pill("tb-system", "file-text", SharedString::from("系统")))
                    .child(pill("tb-tools", "wrench", SharedString::from("工具")))
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
                                    )),
                            )
                            .into_any_element(),
                        None => div().w_full().into_any_element(),
                    }
                })
                .flex_1()
                .min_h_0()
                .py_2(),
            )
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
                            .border_color(rgb(t.border))
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
                                    .flex_1()
                                    .min_w_0()
                                    .on_key_down(cx.listener(
                                        |this, ev: &KeyDownEvent, _w, cx| {
                                            let key = ev.keystroke.key.as_str();
                                            let shift = ev.keystroke.modifiers.shift;
                                            let menu_open =
                                                this.active_menu().is_some();
                                            let items = this.menu_items();
                                            match key {
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
                                                "enter" => this.send_input(cx),
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
                                                "space" => {
                                                    this.input.push(' ');
                                                    this.menu_ix = 0;
                                                    cx.notify();
                                                }
                                                k => {
                                                    let printable = k.chars().count() == 1
                                                        && !ev.keystroke.modifiers.control
                                                        && !ev.keystroke.modifiers.alt;
                                                    if printable {
                                                        if let Some(c) = k.chars().next()
                                                        {
                                                            this.input.push(c);
                                                            this.menu_ix = 0;
                                                            cx.notify();
                                                        }
                                                    }
                                                }
                                            }
                                        },
                                    ))
                                    .text_sm()
                                    .text_color(if input_empty {
                                        rgb(t.text_dim)
                                    } else {
                                        rgb(t.text)
                                    })
                                    .child(input_ph),
                            )
                            .child(
                                div()
                                    .id("send")
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .px_3()
                                    .py_1p5()
                                    .rounded_lg()
                                    .bg(if !input_empty
                                        || !self.pending_images.is_empty()
                                    {
                                        rgb(t.accent)
                                    } else {
                                        rgb(t.bg_panel)
                                    })
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(if !input_empty
                                        || !self.pending_images.is_empty()
                                    {
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
                                            .child(SharedString::from("发送")),
                                    ),
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
                                                            Some(Dialog::ModelSelect {
                                                                filter: String::new(),
                                                            });
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
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .text_xs()
                                    .text_color(rgb(t.text_muted))
                                    .child(
                                        div()
                                            .id("thinking-cycle")
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .cursor_pointer()
                                            .hover(|s| s.text_color(rgb(t.text)))
                                            .on_mouse_down(MouseButton::Left, cx.listener(
                                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                    this.cycle_thinking(cx);
                                                },
                                            ))
                                            .child(icon(
                                                "lightbulb",
                                                12.,
                                                t.text_muted,
                                            ))
                                            .child(thinking_label),
                                    )
                                    .child(SharedString::from("configured"))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .child(icon("scissors", 12., t.text_muted))
                                            .child(SharedString::from("压缩")),
                                    )
                                    .child(icon("volume", 12., t.text_muted)),
                            ),
                    ),
            )
            // status bar
            .child(
                div()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.bg_panel))
                    .text_xs()
                    .text_color(rgb(t.text_muted))
                    .child(status),
            );

        let mut root = div()
            .size_full()
            .relative()
            .flex()
            .flex_row()
            .bg(rgb(t.bg))
            .text_color(rgb(t.text))
            .font_family("Segoe UI")
            .child(sidebar)
            .child(main_col);

        // ---- right panel: terminal tabs (pi-web right-panel TabBar +
        //      TerminalPanel; fixed dark surface in every theme) -----------
        if self.right_panel_open && !self.terminals.is_empty() {
            let weak_for_tabs = weak.clone();
            let tabbar = div()
                .flex()
                .h(px(36.))
                .flex_shrink_0()
                .bg(rgb(t.bg_panel))
                .border_b_1()
                .border_color(rgb(t.border))
                .children(self.terminals.iter().enumerate().map(|(ix, tab)| {
                    let active = self.active_terminal == Some(ix);
                    let title: SharedString = tab.title.clone().into();
                    let weak_tab = weak_for_tabs.clone();
                    let weak_close = weak_for_tabs.clone();
                    let focus_for_tab = tab.focus.clone();
                    div()
                        .id(SharedString::from(format!("term-tab-{ix}")))
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
                                c.active_terminal = Some(ix);
                                cx.notify();
                            });
                            window.focus(&focus_for_tab);
                        })
                        // middle-click closes the tab (pi-web TabBar auxclick)
                        .on_mouse_down(MouseButton::Middle, move |_, window, cx| {
                            let _ = weak_close.update(cx, |c, cx| {
                                c.close_terminal(ix, window, cx);
                            });
                        })
                        .child(icon(
                            "terminal",
                            13.,
                            if active { t.text } else { t.text_muted },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .overflow_hidden()
                                .child(title),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("term-close-{ix}")))
                                .size(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .cursor_pointer()
                                .text_color(rgb(t.text_muted))
                                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                .on_mouse_down(MouseButton::Left, {
                                    let weak_x = weak_for_tabs.clone();
                                    move |_, window, cx| {
                                        cx.stop_propagation();
                                        let _ = weak_x.update(cx, |c, cx| {
                                            c.close_terminal(ix, window, cx);
                                        });
                                    }
                                })
                                .child(icon("x", 11., t.text_muted)),
                        )
                        .into_any_element()
                }));

            let panel_body: Option<gpui::AnyElement> = self.active_terminal.and_then(|ix| {
                self.terminals.get(ix).map(|tab| {
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
                        // header: 38px, status dot + cwd + restart
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
                                        .hover(|s| s.bg(rgb(0x242932)).text_color(rgb(0xe5e7eb)))
                                        .on_mouse_down(MouseButton::Left, {
                                            let rix = ix;
                                            move |_, _, cx| {
                                                let _ = weak_restart.update(cx, |c, cx| {
                                                    c.restart_terminal(rix, cx);
                                                });
                                            }
                                        })
                                        .child(icon("refresh", 12., 0x9ca3af)),
                                ),
                        );
                    // banners: error (role=alert) / exit code (pi-web parity)
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
                                .unwrap_or_else(|| "unknown".into());
                            col = col.child(
                                div()
                                    .py(px(7.))
                                    .px(px(12.))
                                    .text_size(px(11.))
                                    .font_family(terminal::FONT_FAMILY)
                                    .text_color(rgb(0x9ca3af))
                                    .child(SharedString::from(format!(
                                        "Process exited with code {code_text}"
                                    ))),
                            );
                        }
                        TermStatus::Ready => {}
                    }
                    // xterm host: fixed #111318 surface, pi-web padding
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
                                    .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _w, cx| {
                                        this.terminal_key(ev, cx);
                                    })),
                            ),
                    )
                    .into_any_element()
                })
            });

            root = root.child(
                div()
                    .w(px(560.))
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .bg(rgb(t.bg))
                    .border_l_1()
                    .border_color(rgb(t.border))
                    .child(tabbar)
                    .children(panel_body),
            );
        }

        // dialogs
        if let Some(Dialog::ModelSelect { filter }) = self.dialog.as_ref() {
            let flt = filter.to_lowercase();
            let rows: Vec<gpui::AnyElement> = self
                .available_models
                .iter()
                .filter(|m| {
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
                            .child(
                                div()
                                    .px_2()
                                    .py_1p5()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .bg(rgb(t.bg))
                                    .text_xs()
                                    .text_color(rgb(t.text_dim))
                                    .child(SharedString::from("filter models...")),
                            )
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
                    .child("无活动会话")
                    .into_any_element()
            } else if !has_branches || rows.is_empty() {
                div()
                    .px_4()
                    .py_2p5()
                    .text_xs()
                    .text_color(rgb(t.text_muted))
                    .child("暂无分支")
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
                                            .child("分支"),
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
                                    .child("点击节点：从该用户消息处创建分支新会话"),
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
                                            .child("选择项目"),
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
                                    .child("切换后仅显示该项目的会话，并恢复上次打开的会话"),
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
        if let Some(Dialog::FilePreview { path, content }) = self.dialog.as_ref() {
            let path_text: SharedString = path.to_string_lossy().to_string().into();
            let mut body = content.clone();
            if body.chars().count() > 20000 {
                let mut cut = 20000;
                while !body.is_char_boundary(cut) {
                    cut -= 1;
                }
                body.truncate(cut);
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
                            .rounded_lg()
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
                                            .text_xs()
                                            .font_family("Consolas")
                                            .text_color(rgb(t.text_muted))
                                            .child(path_text),
                                    )
                                    .child(
                                        div()
                                            .id("file-close")
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
                                    .font_family("Consolas")
                                    .text_xs()
                                    .text_color(rgb(t.text))
                                    .child(SharedString::from(body)),
                            ),
                    ),
            );
        }
        if let Some(Dialog::RenameSession { value }) = self.dialog.as_ref() {
            let value_view: SharedString = if value.is_empty() {
                "session name".into()
            } else {
                value.clone().into()
            };
            let value_empty = value.is_empty();
            let weak_ok = weak_for_dialog.clone();
            let weak_cancel = weak_for_dialog.clone();
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
                            .w(px(420.))
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
                                    .child("重命名会话"),
                            )
                            .child(
                                div()
                                    .id("dialog-input")
                                    .track_focus(&self.dialog_focus)
                                    .on_key_down({
                                        let weak = weak_for_dialog.clone();
                                        move |ev: &KeyDownEvent, _w, cx| {
                                            let key = ev.keystroke.key.as_str();
                                            let _ = weak.update(cx, |this, cx| {
                                                if let Some(Dialog::RenameSession {
                                                    value,
                                                }) = &mut this.dialog
                                                {
                                                    match key {
                                                        "enter" => this.confirm_rename(cx),
                                                        "escape" => {
                                                            this.dialog = None;
                                                            cx.notify();
                                                        }
                                                        "backspace" => {
                                                            value.pop();
                                                            cx.notify();
                                                        }
                                                        "space" => {
                                                            value.push(' ');
                                                            cx.notify();
                                                        }
                                                        k => {
                                                            if k.chars().count() == 1 {
                                                                if let Some(c) =
                                                                    k.chars().next()
                                                                {
                                                                    value.push(c);
                                                                    cx.notify();
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            });
                                        }
                                    })
                                    .px_2()
                                    .py_1p5()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .bg(rgb(t.bg))
                                    .text_sm()
                                    .text_color(if value_empty {
                                        rgb(t.text_dim)
                                    } else {
                                        rgb(t.text)
                                    })
                                    .child(value_view),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id("dialog-cancel")
                                            .px_3()
                                            .py_1()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(rgb(t.border))
                                            .bg(rgb(t.assistant_bg))
                                            .text_xs()
                                            .text_color(rgb(t.text_muted))
                                            .cursor_pointer()
                                            .hover(|s| s.text_color(rgb(t.text)))
                                            .on_mouse_down(MouseButton::Left, {
                                                let weak = weak_cancel.clone();
                                                move |_, _, cx| {
                                                    let _ = weak.update(cx, |c, cx| {
                                                        c.dialog = None;
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .child("取消"),
                                    )
                                    .child(
                                        div()
                                            .id("dialog-ok")
                                            .px_3()
                                            .py_1()
                                            .rounded_md()
                                            .bg(rgb(t.accent))
                                            .text_xs()
                                            .text_color(rgb(t.accent_contrast))
                                            .cursor_pointer()
                                            .on_mouse_down(MouseButton::Left, {
                                                let weak = weak_ok.clone();
                                                move |_, _, cx| {
                                                    let _ = weak.update(cx, |c, cx| {
                                                        c.confirm_rename(cx)
                                                    });
                                                }
                                            })
                                            .child("保存"),
                                    ),
                            ),
                    ),
            );
        }
        root
    }
}

fn main() {
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
