//! pi-flash M1 shell: GPUI chat over the vendored pi (pi-link).
//!
//! UI note: interaction parity with pi-web comes first; visual polish last
//! (see PORT_PLAN.md). This is the M1 chat core, not the final look.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use futures::{StreamExt, channel::mpsc::UnboundedReceiver};
use gpui::{
    App, Application, Context, FocusHandle, Focusable, KeyDownEvent, ListAlignment, ListState,
    MouseButton, ParentElement, Render, SharedString, Styled, WindowOptions, div, list,
    prelude::*, px, rgb,
};
use pi_link::client::{PiSession, spawn as spawn_pi};
use pi_link::protocol::{AssistantEvent, Block, Command, Event, SessionState, SessionStats, content_blocks};
use pi_link::sessions::{SessionInfo, list_sessions};

mod markdown;

// ---------------------------------------------------------------------------
// palette (placeholder theme; visual polish is deliberately deferred)
// ---------------------------------------------------------------------------

const COL_BG: u32 = 0x1a1b1e;
const COL_PANEL: u32 = 0x232428;
const COL_SIDEBAR: u32 = 0x141518;
const COL_TEXT: u32 = 0xd7dadd;
const COL_USER: u32 = 0x8ab4f8;
const COL_ASSISTANT: u32 = 0x81c995;
const COL_STATUS: u32 = 0x9aa0a6;
const COL_THINKING: u32 = 0x7a7f87;
const COL_CARD_BORDER: u32 = 0x3a3d44;

// ---------------------------------------------------------------------------
// chat state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Role {
    User,
    Assistant,
}

struct Msg {
    role: Role,
    blocks: Vec<Block>,
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
    session: Option<PiSession>,
    status: String,
    state: Option<SessionState>,
    stats: Option<SessionStats>,
    active_session_file: Option<PathBuf>,
    /// (msg_ix, content_index) of thinking blocks rendered collapsed
    collapsed: HashSet<(usize, usize)>,
    /// guards against stale events from a replaced sidecar process
    epoch: u64,
}

impl Chat {
    fn refresh_state(&self) {
        if let Some(session) = &self.session {
            let _ = session.send(&Command::GetState);
            let _ = session.send(&Command::GetSessionStats);
        }
    }

    /// Delete a stored session file (pi-web parity: delete from the picker).
    /// The active session's file is protected — close/switch first.
    fn delete_session(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.active_session_file.as_deref() == Some(path.as_path()) {
            self.status = "cannot delete the active session".into();
            cx.notify();
            return;
        }
        match std::fs::remove_file(&path) {
            Ok(_) => {
                self.sessions.retain(|s| s.path != path);
                self.sync_sidebar();
                self.status = "session deleted".into();
            }
            Err(e) => self.status = format!("delete failed: {e}"),
        }
        cx.notify();
    }
}

impl Chat {
    fn new(cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let dialog_focus = cx.focus_handle();
        let cwd = std::env::var("PI_FLASH_CWD")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

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
            cwd,
            session,
            status: status_line(connected, "starting"),
            state: None,
            stats: None,
            active_session_file: None,
            collapsed: HashSet::new(),
            epoch: 1,
        };
        chat.sync_sidebar();
        chat.status = status_line(chat.session.is_some(), "idle");
        chat.refresh_state();

        if let Some(events) = events {
            cx.spawn(async move |this, cx| {
                consume_events(this, cx, events, 1).await;
            })
            .detach();
        }
        chat
    }

    fn sync_sidebar(&mut self) {
        self.sessions_list.reset(self.sessions.len());
    }

    fn notify_repaint(&mut self, cx: &mut Context<Self>) {
        self.list.reset(self.messages.len());
        cx.notify();
    }

    /// The trailing assistant message, created on demand.
    fn last_assistant(&mut self) -> &mut Msg {
        if !matches!(self.messages.last(), Some(m) if m.role == Role::Assistant) {
            self.messages.push(Msg { role: Role::Assistant, blocks: Vec::new() });
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
    fn ingest_message(&mut self, role: &str, blocks: Vec<Block>, cx: &mut Context<Self>) {
        match role {
            "user" => self.messages.push(Msg { role: Role::User, blocks }),
            "assistant" => self.messages.push(Msg { role: Role::Assistant, blocks }),
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
        self.notify_repaint(cx);
    }

    fn send_input(&mut self, cx: &mut Context<Self>) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(session) = &self.session else {
            self.status = "not connected".into();
            cx.notify();
            return;
        };
        // pi-web parity: while streaming, typed text steers the running agent
        let streaming = self.state.as_ref().is_some_and(|s| s.is_streaming);
        let cmd = if streaming {
            Command::Steer { message: text }
        } else {
            Command::Prompt { message: text }
        };
        match session.send(&cmd) {
            Ok(_) => {
                self.input.clear();
                self.status = if streaming { "steering" } else { "running" }.into();
            }
            Err(e) => self.status = e,
        }
        cx.notify();
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

    fn abort(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
            let _ = session.send(&Command::Abort);
            self.status = "aborting".into();
            cx.notify();
        }
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
        self.status = status_line(self.session.is_some(), "new session");
        self.refresh_state();
        if let Some(events) = events {
            let epoch = self.epoch;
            cx.spawn(async move |this, cx| {
                consume_events(this, cx, events, epoch).await;
            })
            .detach();
        }
        self.notify_repaint(cx);
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
        let (session, events) = spawn_with_epoch(&cwd, &["--session", &path.to_string_lossy()], self.epoch);
        self.session = session;
        self.cwd = cwd;
        self.messages.clear();
        self.stats = None;
        self.active_session_file = None;
        self.collapsed.clear();
        self.status = status_line(self.session.is_some(), "resuming");
        if let Some(session) = &self.session {
            let _ = session.send(&Command::GetMessages);
            let _ = session.send(&Command::GetState);
        }
        if let Some(events) = events {
            let epoch = self.epoch;
            cx.spawn(async move |this, cx| {
                consume_events(this, cx, events, epoch).await;
            })
            .detach();
        }
        self.notify_repaint(cx);
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
                        self.active_session_file = data["sessionFile"]
                            .as_str()
                            .map(PathBuf::from);
                    }
                } else if command == "get_messages" && success {
                    if let Some(data) = &data {
                        for msg in data["messages"].as_array().into_iter().flatten() {
                            let role = msg["role"].as_str().unwrap_or("");
                            let blocks = content_blocks(&msg["content"]);
                            self.ingest_message(role, blocks, cx);
                        }
                    }
                    self.status = "resumed".into();
                } else if success {
                    self.status = format!("{command} ok");
                } else {
                    self.status = format!("{command} failed: {}", error.unwrap_or_default());
                }
            }
            Event::MessageStart { role, blocks } => {
                self.ingest_message(&role, blocks, cx);
            }
            Event::MessageUpdate(assistant_event) => match assistant_event {
                AssistantEvent::TextDelta { content_index, delta } => {
                    if let Block::Text { text, .. } = self
                        .assistant_slot(content_index, Block::Text { content_index, text: String::new() })
                    {
                        text.push_str(&delta);
                    }
                }
                AssistantEvent::TextEnd { content_index, content } => {
                    if let Block::Text { text, .. } = self
                        .assistant_slot(content_index, Block::Text { content_index, text: String::new() })
                    {
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
                    // authoritative name/arguments overwrite the streamed build
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
            Event::MessageEnd { role, blocks } => {
                // authoritative final content replaces streamed reconstruction
                if role == "assistant" {
                    if let Some(m) = self.messages.last_mut() {
                        if m.role == Role::Assistant {
                            m.blocks = blocks;
                        }
                    }
                }
            }
            Event::AgentStart => self.status = "running".into(),
            Event::AgentSettled => {
                self.status = "idle".into();
                self.refresh_state();
            }
            Event::AgentEnd { .. } => {}
            Event::ExtensionUi(_) => {}
            Event::Unparsed(_) => {}
        }
        self.notify_repaint(cx);
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
// rendering
// ---------------------------------------------------------------------------

fn pretty_args(args: &str) -> String {
    serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| args.to_string())
}

fn render_block(b: &Block, msg_ix: usize, weak: &gpui::WeakEntity<Chat>, collapsed: &HashSet<(usize, usize)>) -> gpui::Div {
    match b {
        Block::Text { text, .. } if !text.trim().is_empty() => {
            div().w_full().child(markdown::render(text))
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
                .bg(rgb(COL_PANEL))
                .border_l_2()
                .border_color(rgb(COL_THINKING))
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id(SharedString::from(format!("th-{msg_ix}-{content_index}")))
                        .cursor_pointer()
                        .text_xs()
                        .italic()
                        .text_color(rgb(COL_THINKING))
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
                        .text_color(rgb(COL_THINKING))
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
                .border_color(rgb(COL_CARD_BORDER))
                .bg(rgb(COL_PANEL))
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(COL_ASSISTANT))
                        .child(SharedString::from(format!("tool \u{b7} {name}"))),
                )
                .child(
                    div()
                        .px_2()
                        .pb_1()
                        .font_family("Consolas")
                        .text_xs()
                        .text_color(rgb(COL_STATUS))
                        .child(SharedString::from(pretty_args(args))),
                );
            if !result.is_empty() {
                card = card.child(
                    div()
                        .px_2()
                        .pb_1()
                        .mt_1()
                        .border_t_1()
                        .border_color(rgb(COL_CARD_BORDER))
                        .font_family("Consolas")
                        .text_xs()
                        .text_color(rgb(COL_TEXT))
                        .child(SharedString::from(result.clone())),
                );
            }
            card
        }
        _ => div().w_full(),
    }
}

fn render_msg(m: &Msg, msg_ix: usize, weak: &gpui::WeakEntity<Chat>, collapsed: &HashSet<(usize, usize)>) -> gpui::Div {
    let (label, color) = match m.role {
        Role::User => ("you", rgb(COL_USER)),
        Role::Assistant => ("pi", rgb(COL_ASSISTANT)),
    };
    let mut col = div()
        .w_full()
        .px_3()
        .py_1()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(color)
                .child(label),
        );
    if m.role == Role::User {
        let text = m.plain_text();
        col = col.child(
            div()
                .text_color(rgb(COL_TEXT))
                .child(SharedString::from(text)),
        );
    } else {
        for b in &m.blocks {
            col = col.child(render_block(b, msg_ix, weak, collapsed));
        }
    }
    col
}

fn cwd_tail(cwd: &str) -> String {
    cwd.rsplit(['/', '\\']).next().unwrap_or(cwd).to_string()
}

impl Render for Chat {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.dialog.is_some() {
            window.focus(&self.dialog_focus);
        } else {
            window.focus(&self.focus);
        }

        let session_title: SharedString = self
            .state
            .as_ref()
            .and_then(|s| s.session_name.clone())
            .unwrap_or_else(|| "pi-flash".into())
            .into();
        let rename_prefill = self
            .state
            .as_ref()
            .and_then(|s| s.session_name.clone())
            .unwrap_or_default();
        let pending_chip: Option<SharedString> = self
            .state
            .as_ref()
            .filter(|s| s.pending_message_count > 0)
            .map(|s| SharedString::from(format!("queued {}", s.pending_message_count)));
        let model_label: SharedString = self
            .state
            .as_ref()
            .and_then(|s| s.model_label())
            .unwrap_or_else(|| "no model".into())
            .into();
        let stats_label: SharedString = self
            .stats
            .as_ref()
            .map(|s| s.summary())
            .unwrap_or_default()
            .into();
        let status: SharedString = self.status.clone().into();
        let input: SharedString = if self.input.is_empty() {
            "type a prompt, Enter to send, Esc to abort".into()
        } else {
            self.input.clone().into()
        };
        let input_empty = self.input.is_empty();
        let entity = cx.entity();
        let weak = entity.downgrade();
        let weak_for_list = weak.clone();
        let weak_for_del = weak.clone();
        let weak_for_msg = weak.clone();
        let weak_for_dialog = weak.clone();

        // session sidebar rows
        let sessions_entity = entity.clone();
        let sessions_weak = weak.clone();
        let sidebar = div()
            .w(px(280.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(rgb(COL_SIDEBAR))
            .border_r_1()
            .border_color(rgb(COL_CARD_BORDER))
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
                            .text_color(rgb(COL_STATUS))
                            .child("SESSIONS"),
                    )
                    .child(
                        div()
                            .id("new-session")
                            .px_2()
                            .py_0p5()
                            .rounded_md()
                            .bg(rgb(COL_PANEL))
                            .text_xs()
                            .text_color(rgb(COL_TEXT))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = sessions_weak.update(cx, |c, cx| c.new_session(cx));
                            })
                            .child("+ new"),
                    ),
            )
            .child(
                list(self.sessions_list.clone(), move |ix, _window, cx| {
                    let chat = sessions_entity.read(cx);
                    let Some(info) = chat.sessions.get(ix) else {
                        return div().into_any_element();
                    };
                    let path = info.path.clone();
                    let preview: SharedString = if info.preview.is_empty() {
                        "(empty)".into()
                    } else {
                        info.preview.clone().into()
                    };
                    let project: SharedString = cwd_tail(&info.cwd).into();
                    let weak = weak_for_list.clone();
                    let weak_del = weak_for_del.clone();
                    let p_del = info.path.clone();
                    let active = chat.active_session_file.as_deref() == Some(info.path.as_path());
                    // sibling layout: [clickable row][x] - no event bubbling
                    // between them; the active session cannot be deleted
                    div()
                        .w_full()
                        .flex()
                        .items_start()
                        .border_b_1()
                        .border_color(rgb(COL_PANEL))
                        .child(
                            div()
                                .id(SharedString::from(format!("sess-{ix}")))
                                .flex_1()
                                .min_w_0()
                                .px_3()
                                .py_2()
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(COL_PANEL)))
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
                                        .text_color(rgb(COL_TEXT))
                                        .child(preview),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(COL_STATUS))
                                        .child(project),
                                ),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("del-{ix}")))
                                .w(px(28.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_xs()
                                .text_color(rgb(COL_STATUS))
                                .hover(|s| s.bg(rgb(COL_PANEL)).text_color(rgb(0xf28b82)))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let p = p_del.clone();
                                    let _ = weak_del.update(cx, |c, cx| c.delete_session(p, cx));
                                })
                                .child(if active { "\u{25cf}" } else { "\u{d7}" }),
                        )
                        .into_any_element()
                })
                .flex_1()
                .min_h_0(),
            );

        let main_col = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(COL_BG))
            .text_color(rgb(COL_TEXT))
            .font_family("Segoe UI")
            .text_sm()
            // header
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .bg(rgb(COL_PANEL))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(session_title),
                            )
                            .child(
                                div()
                                    .id("rename")
                                .mx_2()
                                .px_1p5()
                                .py_0p5()
                                .rounded_md()
                                .bg(rgb(COL_SIDEBAR))
                                .text_xs()
                                .text_color(rgb(COL_STATUS))
                                .cursor_pointer()
                                .hover(|s| s.text_color(rgb(COL_TEXT)))
                                .on_mouse_down(MouseButton::Left, {
                                    let weak = weak_for_dialog.clone();
                                    move |_, _, cx| {
                                        let _ = weak.update(cx, |c, cx| {
                                            c.dialog = Some(Dialog::RenameSession {
                                                value: c
                                                    .state
                                                    .as_ref()
                                                    .and_then(|s| s.session_name.clone())
                                                    .unwrap_or_default(),
                                            });
                                            cx.notify();
                                        });
                                    }
                                })
                                .child("\u{270e}"),
                            )
                            .children(pending_chip.map(|c| {
                                div()
                                    .text_xs()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(rgb(COL_SIDEBAR))
                                    .text_color(rgb(COL_USER))
                                    .child(c)
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(rgb(COL_STATUS))
                            .child(model_label)
                            .child(SharedString::from("|"))
                            .child(stats_label)
                            .child(SharedString::from("|"))
                            .child(status),
                    ),
            )
            // message list (bottom-aligned: sticks to the newest message)
            .child(
                list(self.list.clone(), move |ix, _window, cx| {
                    let chat = entity.read(cx);
                    let weak = weak_for_msg.clone();
                    match chat.messages.get(ix) {
                        Some(m) => div()
                            .w_full()
                            .child(render_msg(m, ix, &weak, &chat.collapsed))
                            .into_any_element(),
                        None => div().w_full().into_any_element(),
                    }
                })
                .flex_1()
                .min_h_0()
                .py_2(),
            )
            // input row
            .child(
                div()
                    .flex()
                    .items_center()
                    .px_3()
                    .py_2()
                    .bg(rgb(COL_PANEL))
                    .child(
                        div()
                            .id("input")
                            .track_focus(&self.focus)
                            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _w, cx| {
                                let key = ev.keystroke.key.as_str();
                                match key {
                                    "enter" => this.send_input(cx),
                                    "escape" => this.abort(cx),
                                    "backspace" => {
                                        if !ev.keystroke.modifiers.modified() {
                                            this.input.pop();
                                            cx.notify();
                                        }
                                    }
                                    "space" => {
                                        this.input.push(' ');
                                        cx.notify();
                                    }
                                    k => {
                                        let printable = k.chars().count() == 1
                                            && !ev.keystroke.modifiers.control
                                            && !ev.keystroke.modifiers.alt;
                                        if printable {
                                            if let Some(c) = k.chars().next() {
                                                this.input.push(c);
                                                cx.notify();
                                            }
                                        }
                                    }
                                }
                            }))
                            .flex_1()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(COL_BG))
                            .text_color(if input_empty {
                                rgb(COL_STATUS)
                            } else {
                                rgb(COL_TEXT)
                            })
                            .child(input),
                    ),
            );

        let mut root = div()
            .size_full()
            .relative()
            .flex()
            .flex_row()
            .child(sidebar)
            .child(main_col);
        if let Some(overlay) = self.render_dialog(&weak_for_dialog) {
            root = root.child(overlay);
        }
        root
    }
}

impl Chat {
    /// Modal overlay for the current dialog (rename session for now).
    fn render_dialog(&mut self, weak: &gpui::WeakEntity<Chat>) -> Option<gpui::AnyElement> {
        let Dialog::RenameSession { value } = self.dialog.as_ref()?;
        let value: SharedString = if value.is_empty() {
            "session name".into()
        } else {
            value.clone().into()
        };
        let value_empty = value == "session name";
        let weak_input = weak.clone();
        let weak_cancel = weak.clone();
        let weak_ok = weak.clone();
        let prefill_input = weak_input.clone();

        let input = div()
            .id("dialog-input")
            .track_focus(&self.dialog_focus)
            .on_key_down(cx_dialog_listener(weak, move |this, ev, cx| {
                let key = ev.keystroke.key.as_str();
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
                let _ = &prefill_input;
            }))
            .flex_1()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(COL_BG))
            .text_color(if value_empty { rgb(COL_STATUS) } else { rgb(COL_TEXT) })
            .child(value);

        let panel = div()
            .w(px(420.))
            .bg(rgb(COL_PANEL))
            .border_1()
            .border_color(rgb(COL_CARD_BORDER))
            .rounded_lg()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(COL_TEXT))
                    .child("rename session"),
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
                            .bg(rgb(COL_SIDEBAR))
                            .text_xs()
                            .text_color(rgb(COL_STATUS))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(COL_TEXT)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_cancel.update(cx, |c, cx| {
                                    c.dialog = None;
                                    cx.notify();
                                });
                            })
                            .child("cancel"),
                    )
                    .child(
                        div()
                            .id("dialog-ok")
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(COL_ASSISTANT))
                            .text_xs()
                            .text_color(rgb(0x10120f))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_ok.update(cx, |c, cx| c.confirm_rename(cx));
                            })
                            .child("save"),
                    ),
            );

        Some(
            div()
                .absolute()
                .inset_0()
                .bg(gpui::hsla(0., 0., 0., 0.55))
                .flex()
                .items_center()
                .justify_center()
                .child(panel)
                .into_any_element(),
        )
    }
}

/// Build a chat-state listener usable inside non-element closures.
fn cx_dialog_listener(
    weak: &gpui::WeakEntity<Chat>,
    f: impl Fn(&mut Chat, &KeyDownEvent, &mut gpui::Context<Chat>) + 'static,
) -> impl Fn(&KeyDownEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    let weak = weak.clone();
    move |ev, _w, cx| {
        let _ = weak.update(cx, |chat, cx| f(chat, ev, cx));
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = gpui::Bounds::centered(None, gpui::size(px(980.), px(620.)), cx);
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
