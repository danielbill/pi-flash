//! pi-flash — desktop shell for the pi coding agent.
//!
//! Component-by-component translation of pi-web (see PORT_PLAN.md). Layout
//! values (sizes, colors, spacing) come from pi-web sources: globals.css
//! theme tokens, panel-layout.ts, MessageView/ChatInput/AppShell structures.

use std::path::{Path, PathBuf};

use futures::{StreamExt, channel::mpsc::UnboundedReceiver};
use gpui::{
    App, Application, Context, FocusHandle, Focusable, KeyDownEvent, ListAlignment, ListState,
    MouseButton, ParentElement, Render, SharedString, Styled, WindowOptions, div, list,
    prelude::*, px, relative, rgb,
};
use pi_link::client::{PiSession, spawn as spawn_pi};
use pi_link::protocol::{
    AssistantEvent, Block, Command, Event, SessionState, SessionStats, SlashCommand, Usage,
    content_blocks,
};
use pi_link::sessions::{SessionInfo, list_sessions};

mod markdown;
mod theme;
use theme::theme as T;

// ---------------------------------------------------------------------------
// chat state
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
    /// slash commands from get_commands
    commands: Vec<SlashCommand>,
    /// project files (relative paths) for the @ lookup menu
    project_files: Vec<String>,
    /// sent-prompt history (pi-web chat input parity)
    history: Vec<String>,
    history_ix: Option<usize>,
    /// highlighted row in the active slash/@ menu
    menu_ix: usize,
    /// guards against stale events from a replaced sidecar process
    epoch: u64,
}

use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq)]
enum MenuKind {
    Slash,
    At,
}

#[derive(Debug, Clone)]
struct MenuItem {
    /// what gets inserted into the input when accepted
    insert: String,
    title: String,
    desc: String,
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
            let rel_path = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
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

impl Chat {
    fn new(cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let dialog_focus = cx.focus_handle();
        let cwd = std::env::var("PI_FLASH_CWD")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let branch = read_branch(&cwd);

        let (session, events) = spawn_with_epoch(&cwd, &[], 1);

        let mut list = ListState::new(0, ListAlignment::Bottom, px(1000.));
        list.reset(0);
        let mut sessions_list = ListState::new(0, ListAlignment::Top, px(500.));

        let connected = session.is_some();
        let mut chat = Self {
            focus,
            dialog_focus,
            dialog: None,
            input: String::new(),
            messages: Vec::new(),
            list,
            sessions: list_sessions(100),
            sessions_list,
            cwd: cwd.clone(),
            branch,
            session,
            status: status_line(connected, "idle"),
            state: None,
            stats: None,
            active_session_file: None,
            collapsed: HashSet::new(),
            commands: Vec::new(),
            project_files: Vec::new(),
            history: Vec::new(),
            history_ix: None,
            menu_ix: 0,
            epoch: 1,
        };
        chat.sessions_list.reset(chat.sessions.len());
        chat.load_project_files();
        if connected {
            chat.refresh_state();
        }

        if let Some(events) = events {
            cx.spawn(async move |this, cx| {
                consume_events(this, cx, events, 1).await;
            })
            .detach();
        }
        chat
    }

    /// The menu currently active for the input text, if any.
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

    fn refresh_state(&self) {
        if let Some(session) = &self.session {
            let _ = session.send(&Command::GetState);
            let _ = session.send(&Command::GetSessionStats);
            let _ = session.send(&Command::GetCommands);
        }
    }

    fn load_project_files(&mut self) {
        self.project_files = walk_files(&self.cwd, 3, 400);
    }

    /// The trailing assistant message, created on demand.
    fn last_assistant(&mut self) -> &mut Msg {
        if !matches!(self.messages.last(), Some(m) if m.role == Role::Assistant) {
            self.messages.push(Msg { role: Role::Assistant, blocks: Vec::new(), usage: None });
        }
        self.messages.last_mut().expect("just pushed")
    }

    /// Slot for streaming block at `content_index`: pads holes, replaces a
    /// block whose kind just changed at that index.
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

    /// Common ingestion for live wire messages and resumed history replay.
    fn ingest_message(
        &mut self,
        role: &str,
        blocks: Vec<Block>,
        usage: Option<Usage>,
        time: Option<String>,
        cx: &mut Context<Self>,
    ) {
        match role {
            "user" => {
                self.messages.push(Msg { role: Role::User, blocks, usage: None });
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
                        if let Some(Block::ToolCall { result, .. }) =
                            m.blocks.iter_mut().rev().find(|b| matches!(b, Block::ToolCall { .. }))
                        {
                            result.push_str(&text);
                        }
                    }
                }
            }
            _ => {}
        }
        self.list.reset(self.messages.len());
        cx.notify();
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
        // pi-web parity: while streaming, typed text steers the running agent
        let streaming = self.state.as_ref().is_some_and(|s| s.is_streaming);
        let cmd = if streaming {
            Command::Steer { message: text.clone() }
        } else {
            Command::Prompt { message: text.clone() }
        };
        match session.send(&cmd) {
            Ok(_) => {
                if self.history.last().map(|h| h != &text).unwrap_or(true) {
                    self.history.push(text);
                }
                self.history_ix = None;
                self.input.clear();
                self.status = if streaming { "steering" } else { "running" }.into();
            }
            Err(e) => self.status = e,
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

    /// Spawn a fresh sidecar for the current project.
    fn new_session(&mut self, cx: &mut Context<Self>) {
        self.epoch += 1;
        let (session, events) = spawn_with_epoch(&self.cwd, &[], self.epoch);
        self.session = session;
        self.messages.clear();
        self.state = None;
        self.stats = None;
        self.active_session_file = None;
        self.collapsed.clear();
        self.status = status_line(self.session.is_some(), "新会话");
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

    /// Resume a stored session: sidecar started with `--session <path>`,
    /// history replayed from the get_messages response.
    fn open_session(&mut self, path: PathBuf, cx: &mut Context<Self>) {
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
        self.branch = read_branch(&self.cwd);
        self.messages.clear();
        self.state = None;
        self.stats = None;
        self.active_session_file = None;
        self.collapsed.clear();
        self.status = status_line(self.session.is_some(), "resuming");
        if let Some(session) = &self.session {
            let _ = session.send(&Command::GetMessages);
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

    /// Delete a stored session file (pi-web parity). The active session's
    /// file is protected.
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

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Response { command, success, error, data, .. } => {
                if command == "get_state" && success {
                    if let Some(data) = &data {
                        self.state = Some(SessionState::parse(data));
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
                } else if command == "get_messages" && success {
                    if let Some(data) = &data {
                        for msg in data["messages"].as_array().into_iter().flatten() {
                            let role = msg["role"].as_str().unwrap_or("");
                            let blocks = content_blocks(&msg["content"]);
                            let usage = Usage::parse(&msg["usage"]);
                            self.ingest_message(role, blocks, usage, None, cx);
                        }
                    }
                    self.status = status_line(true, "resumed");
                } else if success {
                    self.status = format!("{command} ok");
                } else {
                    self.status = format!("{command} failed: {}", error.unwrap_or_default());
                }
            }
            Event::MessageStart { role, blocks, timestamp } => {
                let time = timestamp.map(fmt_hhmm).unwrap_or_default();
                match role.as_str() {
                    "user" => {
                        self.messages
                            .push(Msg { role: Role::User, blocks, usage: None });
                        let _ = time;
                    }
                    "assistant" => {
                        self.messages.push(Msg {
                            role: Role::Assistant,
                            blocks,
                            usage: None,
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
            Event::AgentEnd { .. } => {}
            Event::ExtensionUi(_) => {}
            Event::Unparsed(_) => {}
        }
        self.list.reset(self.messages.len());
        cx.notify();
    }
}

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

fn status_line(connected: bool, state: &str) -> String {
    if connected {
        format!("pi {} | {state}", pi_link::PI_VENDOR_VERSION)
    } else {
        "pi not available (vendor missing)".to_string()
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

/// pi-web format: "1,784 in · 574 out · 373,056 cache R · $0.0888"
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

/// "33秒前" / "2分钟前" / "28分钟前" / "5小时前" / "3天前"
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

/// branch name from `<cwd>/.git/HEAD`
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

// ---------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------

fn pretty_args(args: &str) -> String {
    serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| args.to_string())
}

fn render_block(b: &Block, msg_ix: usize, weak: &gpui::WeakEntity<Chat>, collapsed: &HashSet<(usize, usize)>, t: &theme::Theme) -> gpui::Div {
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
                .bg(rgb(t.bg_panel))
                .border_l_2()
                .border_color(rgb(t.border))
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id(SharedString::from(format!("th-{msg_ix}-{content_index}")))
                        .cursor_pointer()
                        .text_xs()
                        .italic()
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
                        .child(SharedString::from(if is_collapsed {
                            "thinking \u{25b8}".to_string()
                        } else {
                            "thinking \u{25be}".to_string()
                        })),
                );
            if !is_collapsed {
                block = block.child(
                    div()
                        .text_xs()
                        .italic()
                        .text_color(rgb(t.text_dim))
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
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.accent))
                        .child(SharedString::from(format!("tool \u{b7} {name}"))),
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

fn render_msg(m: &Msg, msg_ix: usize, weak: &gpui::WeakEntity<Chat>, collapsed: &HashSet<(usize, usize)>, t: &theme::Theme) -> gpui::Div {
    let mut col = div()
        .w_full()
        .mb_4()
        .flex()
        .flex_col();
    if m.role == Role::User {
        // MessageView.tsx: right-aligned bubble, --user-bg, radius 12, pad 8/12
        let text = m.plain_text();
        col = col.items_end().child(
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
    } else {
        col = col.child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.accent))
                .child("pi"),
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
                    .text_xs()
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(usage_footer(u)))
                    .child(SharedString::from(u.time.clone())),
            );
        }
    }
    col
}

fn pill(id: &'static str, label: SharedString) -> gpui::AnyElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(rgb(T().border))
        .text_xs()
        .text_color(rgb(T().text_muted))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(T().bg_hover)).text_color(rgb(T().text)))
        .child(label)
        .into_any_element()
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

impl Render for Chat {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = T();
        if self.dialog.is_some() {
            window.focus(&self.dialog_focus);
        } else {
            window.focus(&self.focus);
        }

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
        let session_title: SharedString = self
            .state
            .as_ref()
            .and_then(|s| s.session_name.clone())
            .unwrap_or_else(|| "pi-flash".into())
            .into();
        let entity = cx.entity();
        let weak = entity.downgrade();
        let weak_for_list = weak.clone();
        let weak_for_del = weak.clone();
        let weak_for_msg = weak.clone();
        let weak_for_dialog = weak.clone();
        let weak_menu = weak.clone();
        let pending_chip: Option<SharedString> = self
            .state
            .as_ref()
            .filter(|s| s.pending_message_count > 0)
            .map(|s| SharedString::from(format!("queued {}", s.pending_message_count)));

        // slash/@ popup menu state (derived from input text)
        let menu_now = self.menu_items();
        let menu_open = self.active_menu().is_some() && !menu_now.is_empty();
        let menu_sel = self.menu_ix.min(menu_now.len().saturating_sub(1));
        let menu_el: Option<gpui::AnyElement> = if menu_open {
            let rows: Vec<gpui::AnyElement> = menu_now
                .iter()
                .enumerate()
                .map(|(i, mi)| {
                    let selected = i == menu_sel;
                    let insert = mi.insert.clone();
                    let weak_i = weak_menu.clone();
                    let title: SharedString = mi.title.clone().into();
                    let desc: SharedString = mi.desc.clone().into();
                    div()
                        .id(SharedString::from(format!("menu-{i}")))
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .cursor_pointer()
                        .when(selected, |d| d.bg(rgb(t.bg_selected)))
                        .hover(move |s| s.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let ins = insert.clone();
                            let _ = weak_i.update(cx, |c, cx| c.accept_menu(ins, cx));
                        })
                        .flex()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .text_xs()
                                .font_family("Consolas")
                                .text_color(rgb(t.accent))
                                .child(title),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(t.text_muted))
                                .child(desc),
                        )
                        .into_any_element()
                })
                .collect();
            Some(
                div()
                    .flex_col()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.assistant_bg))
                    .shadow_lg()
                    .py_1()
                    .children(rows)
                    .into_any_element(),
            )
        } else {
            None
        };


        // ---- sidebar ----------------------------------------------------
        let sessions_entity = entity.clone();
        let weak_for_sessions = weak.clone();
        let stats_right = if let Some(st) = &self.stats {
            format!(
                "\u{2191}{} \u{2193}{} ${:.2}  {}% / {}",
                fmt_compact(st.input),
                fmt_compact(st.output),
                st.cost,
                st.context_percent.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                st.context_window.map(fmt_compact).unwrap_or_else(|| "-".into())
            )
        } else {
            String::new()
        };
        let stats_right: SharedString = stats_right.into();

        let cwd_text: SharedString = self.cwd.to_string_lossy().to_string().into();
        let branch_label: SharedString = if self.branch.is_empty() {
            "no git".into()
        } else {
            format!("\u{2387} {}", self.branch).into()
        };

        let files_entity = entity.clone();
        let sidebar = div()
            .w(px(260.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(rgb(t.bg))
            .border_r_1()
            .border_color(rgb(t.border))
            // header: brand + new + search
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
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .bg(rgb(t.assistant_bg))
                                    .text_xs()
                                    .text_color(rgb(t.text))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(t.bg_hover)))
                                    .on_mouse_down(MouseButton::Left, {
                                        let weak = weak_for_sessions.clone();
                                        move |_, _, cx| {
                                            let _ = weak.update(cx, |c, cx| c.new_session(cx));
                                        }
                                    })
                                    .child("+ 新建"),
                            )
                            .child(
                                div()
                                    .id("search")
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .bg(rgb(t.assistant_bg))
                                    .text_xs()
                                    .text_color(rgb(t.text))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(t.bg_hover)))
                                    .child("\u{1f50d}"),
                            ),
                    ),
            )
            // project box
            .child(
                div()
                    .mx_3()
                    .mb_1p5()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.assistant_bg))
                    .text_xs()
                    .text_color(rgb(t.text))
                    .child(cwd_text),
            )
            // branch box
            .child(
                div()
                    .mx_3()
                    .mb_2()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.assistant_bg))
                    .flex()
                    .justify_between()
                    .text_xs()
                    .child(
                        div()
                            .text_color(rgb(t.text))
                            .child(branch_label),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .text_color(rgb(t.text_muted))
                            .child("主分支")
                            .child("\u{25be}"),
                    ),
            )
            // sessions list
            .child(
                list(self.sessions_list.clone(), move |ix, _window, cx| {
                    let chat = sessions_entity.read(cx);
                    let Some(info) = chat.sessions.get(ix) else {
                        return div().into_any_element();
                    };
                    let is_active =
                        chat.active_session_file.as_deref() == Some(info.path.as_path());
                    let path = info.path.clone();
                    let preview: SharedString = if info.preview.is_empty() {
                        "(empty)".into()
                    } else {
                        info.preview.clone().into()
                    };
                    let meta: SharedString = format!(
                        "{} · {} 条消息",
                        time_ago(info.modified),
                        info.message_count
                    )
                    .into();
                    let weak = weak_for_sessions.clone();
                    let weak_del = weak_for_sessions.clone();
                    let p_del = info.path.clone();
                    div()
                        .w_full()
                        .flex()
                        .items_start()
                        .when(is_active, |d| {
                            d.bg(rgb(t.bg_selected))
                                .border_l_2()
                                .border_color(rgb(t.accent))
                        })
                        .when(!is_active, |d| d.border_l_2().border_color(rgb(t.bg)))
                        .child(
                            div()
                                .id(SharedString::from(format!("sess-{ix}")))
                                .flex_1()
                                .min_w_0()
                                .px_3()
                                .py_2()
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let p = path.clone();
                                    let _ = weak.update(cx, |c, cx| c.open_session(p, cx));
                                })
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(t.text))
                                        .child(preview),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(t.text_muted))
                                        .child(meta),
                                ),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("del-{ix}")))
                                .w(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_xs()
                                .text_color(rgb(t.text_muted))
                                .hover(|s| s.text_color(rgb(0xd9534f)))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let p = p_del.clone();
                                    let _ = weak_del.update(cx, |c, cx| c.delete_session(p, cx));
                                })
                                .child(if is_active { "\u{25cf}" } else { "\u{00d7}" }),
                        )
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
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(t.text))
                                    .child("\u{25be} 文件浏览器"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(rgb(t.text_muted))
                                    .child("\u{1f5a5}")
                                    .child("\u{1f50d}")
                                    .child("\u{2191}")
                                    .child("\u{21bb}"),
                            ),
                    )
                    .child(
                        div()
                            .px_3()
                            .pb_2()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(
                                top_level_entries(&self.cwd)
                                    .into_iter()
                                    .map(|(is_dir, name)| {
                                        div()
                                            .text_xs()
                                            .text_color(rgb(t.text_muted))
                                            .child(SharedString::from(format!(
                                                "{}{}",
                                                if is_dir { "\u{25b8} \u{1f4c1} " } else { "\u{1f4c4} " },
                                                name
                                            )))
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
                            .py_2()
                            .text_xs()
                            .text_color(rgb(t.text_muted))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .child("\u{2699} 模型"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .py_2()
                            .text_xs()
                            .text_color(rgb(t.text_muted))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .child("\u{2637} 技能"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .py_2()
                            .text_xs()
                            .text_color(rgb(t.text_muted))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .child("\u{2699} 设置"),
                    ),
            );

        // ---- main column -------------------------------------------------
        let chat_entity = entity.clone();
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
                    .child(pill("tb-sidebar", SharedString::from("\u{2630}")))
                    .child(pill("tb-history", SharedString::from("\u{1f550} 完整历史")))
                    .child(pill("tb-title", SharedString::from("\u{270e} 生成标题")))
                    .child(pill("tb-system", SharedString::from("\u{1f4c4} 系统")))
                    .child(pill("tb-tools", SharedString::from("\u{1f527} 工具")))
                    .child(
                        div()
                            .flex_1()
                            .text_right()
                            .text_xs()
                            .text_color(rgb(t.text_muted))
                            .child(stats_right),
                    ),
            )
            // message list
            .child(
                list(self.list.clone(), move |ix, _window, cx| {
                    let chat = chat_entity.read(cx);
                    let weak = weak_for_msg.clone();
                    match chat.messages.get(ix) {
                        Some(m) => div()
                            .w_full()
                            .px_4()
                            .child(render_msg(m, ix, &weak, &chat.collapsed, t))
                            .into_any_element(),
                        None => div().w_full().into_any_element(),
                    }
                })
                .flex_1()
                .min_h_0()
                .py_2(),
            )
            // input box + toolbar
            .child(
                div()
                    .px_4()
                    .pb_2()
                    .children(menu_el)
                    .child(
                        div()
                            .w_full()
                            .rounded(px(12.))
                            .border_1()
                            .border_color(rgb(t.border))
                            .bg(rgb(t.assistant_bg))
                            .px_4()
                            .py_3()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .id("input")
                                    .track_focus(&self.focus)
                                    .flex_1()
                                    .min_w_0()
                                    .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _w, cx| {
                                        let key = ev.keystroke.key.as_str();
                                        let shift = ev.keystroke.modifiers.shift;
                                        let menu_open = this.active_menu().is_some();
                                        let items = this.menu_items();
                                        match key {
                                            "enter" if shift => {
                                                this.input.push('\n');
                                                cx.notify();
                                            }
                                            "enter" if menu_open && !items.is_empty() => {
                                                let ix = this.menu_ix.min(items.len() - 1);
                                                let insert = items[ix].insert.clone();
                                                this.accept_menu(insert, cx);
                                            }
                                            "enter" => this.send_input(cx),
                                            "escape" if menu_open => {
                                                this.menu_ix = 0;
                                                // close the menu by terminating the query
                                                if this.active_menu() == Some(MenuKind::At) {
                                                    if let Some(at) = this.input.rfind('@') {
                                                        let q = this.input[at + 1..].to_string();
                                                        this.input =
                                                            format!("{}{} ", &this.input[..at], q);
                                                    }
                                                } else if !this.input.is_empty() {
                                                    this.input = format!("{} ", this.input);
                                                }
                                                cx.notify();
                                            }
                                            "escape" => this.abort(cx),
                                            "tab" if menu_open && !items.is_empty() => {
                                                let ix = this.menu_ix.min(items.len() - 1);
                                                let insert = items[ix].insert.clone();
                                                this.accept_menu(insert, cx);
                                            }
                                            "up" if menu_open && !items.is_empty() => {
                                                this.menu_ix = this.menu_ix.saturating_sub(1);
                                                cx.notify();
                                            }
                                            "down" if menu_open && !items.is_empty() => {
                                                this.menu_ix = (this.menu_ix + 1).min(items.len() - 1);
                                                cx.notify();
                                            }
                                            "up" if !this.history.is_empty() => {
                                                let ix = match this.history_ix {
                                                    None => this.history.len() - 1,
                                                    Some(i) => i.saturating_sub(1),
                                                };
                                                this.history_ix = Some(ix);
                                                this.input = this.history[ix].clone();
                                                cx.notify();
                                            }
                                            "down" => {
                                                if let Some(i) = this.history_ix {
                                                    if i + 1 < this.history.len() {
                                                        this.history_ix = Some(i + 1);
                                                        this.input = this.history[i + 1].clone();
                                                    } else {
                                                        this.history_ix = None;
                                                        this.input.clear();
                                                    }
                                                    cx.notify();
                                                }
                                            }
                                            "backspace" => {
                                                if !ev.keystroke.modifiers.modified() {
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
                                                    if let Some(c) = k.chars().next() {
                                                        this.input.push(c);
                                                        this.menu_ix = 0;
                                                        cx.notify();
                                                    }
                                                }
                                            }
                                        }
                                    }))
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
                                    .px_3()
                                    .py_1p5()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .bg(rgb(t.bg_panel))
                                    .text_sm()
                                    .text_color(rgb(t.text))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(t.bg_hover)))
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _w, cx| {
                                        this.send_input(cx);
                                    }))
                                    .child("\u{2192} 发送"),
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
                                    .child("\u{1f5bc}")
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .child("\u{2699}")
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
                                    .child(format!("\u{1f4a1} {thinking_label}"))
                                    .child("configured")
                                    .child("\u{2702} 压缩")
                                    .child("\u{1f50a}"),
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
        if let Some(overlay) = self.render_dialog(&weak_for_dialog, t) {
            root = root.child(overlay);
        }
        root
    }
}

impl Chat {
    /// Modal overlay for the current dialog (rename session for now).
    fn render_dialog(&mut self, weak: &gpui::WeakEntity<Chat>, t: &theme::Theme) -> Option<gpui::AnyElement> {
        let Dialog::RenameSession { value } = self.dialog.as_ref()?;
        let value: SharedString = if value.is_empty() {
            "session name".into()
        } else {
            value.clone().into()
        };
        let value_empty = value == "session name";
        let weak_input = weak.clone();
        let weak_ok = weak.clone();
        let weak_cancel = weak.clone();

        let input = div()
            .id("dialog-input")
            .track_focus(&self.dialog_focus)
            .on_key_down({
                let weak = weak_input.clone();
                move |ev: &KeyDownEvent, _w, cx| {
                    let key = ev.keystroke.key.as_str();
                    let _ = weak.update(cx, |this, cx| {
                        match key {
                            "enter" => this.confirm_rename(cx),
                            "escape" => {
                                this.dialog = None;
                                cx.notify();
                            }
                            "backspace" => {
                                if let Some(Dialog::RenameSession { value }) = &mut this.dialog {
                                    value.pop();
                                    cx.notify();
                                }
                            }
                            "space" => {
                                if let Some(Dialog::RenameSession { value }) = &mut this.dialog {
                                    value.push(' ');
                                    cx.notify();
                                }
                            }
                            k => {
                                let printable =
                                    k.chars().count() == 1 && !ev.keystroke.modifiers.modified();
                                if printable {
                                    if let (Some(c), Some(Dialog::RenameSession { value })) =
                                        (k.chars().next(), &mut this.dialog)
                                    {
                                        value.push(c);
                                        cx.notify();
                                    }
                                }
                            }
                        }
                    });
                }
            })
            .flex_1()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(t.bg))
            .border_1()
            .border_color(rgb(t.border))
            .text_color(if value_empty { rgb(t.text_dim) } else { rgb(t.text) })
            .child(value);

        let panel = div()
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
            .child(input)
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
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_cancel.update(cx, |c, cx| {
                                    c.dialog = None;
                                    cx.notify();
                                });
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
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_ok.update(cx, |c, cx| c.confirm_rename(cx));
                            })
                            .child("保存"),
                    ),
            );

        Some(
            div()
                .absolute()
                .inset_0()
                .bg(gpui::hsla(0., 0., 0., 0.35))
                .flex()
                .items_center()
                .justify_center()
                .child(panel)
                .into_any_element(),
        )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
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
