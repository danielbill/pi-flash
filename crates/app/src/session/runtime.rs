//! SessionRuntime (030 会话池): ONE resident agent runtime per session —
//! pi-web `__piSessions` parity. Holds the pi process (via AgentSession),
//! the full message log, live session state, and the session's own
//! inputPanel state (031: draft text, attachments, history, thinking
//! override, tools preset, model). Created on first open, destroyed only on
//! explicit delete; switching sessions NEVER touches processes — Chat just
//! moves its attention pointer.
//!
//! Events bubble to the shell via SessionEvent; Chat does UI-side effects
//! (sidebar refresh, sound, git refresh, ext dialogs) for the active
//! session only.

use std::collections::HashSet;
use std::path::PathBuf;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{Context, Entity, EventEmitter, ListAlignment, ListState, Render, prelude::*, px};
use pi_link::protocol::{
    AssistantEvent, Block, Command, Event, SessionState, SessionStats, SlashCommand, Usage,
    TreeNode, content_blocks, parse_tree,
};

use crate::agent_session::AgentSession;
use crate::session::messages::{Msg, Role, UsageLine};
use crate::i18n::tr;
use crate::services::format::status_line;

/// Shell-level effects a runtime bubbles up (Chat subscribes).
pub(crate) enum SessionEvent {
    /// repaint-worthy state change
    Changed,
    /// sidebar list should refresh (rename flush, fork, file landed)
    ListDirty,
    /// extension UI request (active session only surfaces the dialog)
    ExtUi(pi_link::protocol::ExtensionUiRequest),
    /// get_state arrived with a pending rename prefill
    RenameReady(String),
}

pub(crate) struct SessionRuntime {
    /// stable pool key: session file path, or "draft-N" before first prompt
    pub key: String,
    pub cwd: PathBuf,
    /// None until pi persists the draft (first prompt)
    pub file: Option<PathBuf>,
    /// pi process ownership (None while recycled/lazy)
    pub agent: AgentSession,

    // ---- message log ----
    pub messages: Vec<Msg>,
    pub list: ListState,
    pub collapsed: HashSet<(usize, usize)>,
    pub phase_waiting: bool,
    pub stream_started: Option<std::time::Instant>,
    pub agent_running: bool,
    pub status: String,

    // ---- live session state ----
    pub state: Option<SessionState>,
    pub stats: Option<SessionStats>,
    pub branch_tree: Option<(Vec<TreeNode>, Option<String>)>,
    pub active_user_entry_ids: Vec<String>,
    pub pending_rename: bool,
    pub commands: Vec<SlashCommand>,
    pub available_models: Vec<pi_link::protocol::ModelInfo>,
    /// system prompt / tool summary from export_html (top-panel display)
    pub sys_prompt: Option<String>,
    pub session_tools: Option<Vec<(String, String)>>,

    // ---- inputPanel (031) per-session state ----
    pub input: String,
    pub pending_images: Vec<crate::AttachedImage>,
    pub history: Vec<String>,
    pub history_ix: Option<usize>,
    pub thinking_override: Option<String>,
    /// tools preset id (chat-only/read-only/default/full/configured) —
    /// applied at spawn via CLI flags (RPC has no live tool switching)
    pub tools_preset: String,

    /// last user-visible activity (idle recycle)
    pub last_activity: std::time::Instant,
    /// queued ext requests while not active (G+ surfaces a badge)
    pub ext_queue: Vec<pi_link::protocol::ExtensionUiRequest>,

    /// (msg_ix, flashed_at) — 复制 pill's 已复制 flash (032)
    pub copy_flash: Option<(usize, std::time::Instant)>,
    /// 013: jump target waiting for the message snapshot to land
    pub pending_locate: Option<(Option<i64>, String)>,
}

impl SessionRuntime {
    pub(crate) fn new(key: String, cwd: PathBuf, file: Option<PathBuf>) -> Self {
        let list = ListState::new(0, ListAlignment::Bottom, px(1000.));
        Self {
            key,
            cwd,
            file,
            agent: AgentSession::new(1),
            messages: Vec::new(),
            list,
            collapsed: HashSet::new(),
            phase_waiting: false,
            stream_started: None,
            agent_running: false,
            status: String::new(),
            state: None,
            stats: None,
            branch_tree: None,
            active_user_entry_ids: Vec::new(),
            pending_rename: false,
            commands: Vec::new(),
            available_models: Vec::new(),
            sys_prompt: None,
            session_tools: None,
            input: String::new(),
            pending_images: Vec::new(),
            history: Vec::new(),
            history_ix: None,
            thinking_override: None,
            tools_preset: "default".into(),
            last_activity: std::time::Instant::now(),
            ext_queue: Vec::new(),
            copy_flash: None,
            pending_locate: None,
        }
    }

    /// Spawn (or respawn) the pi process for this session. `background`
    /// callers wrap this on the background executor; returns the event
    /// receiver for the pump.
    pub(crate) fn spawn(&mut self) -> Option<UnboundedReceiver<Event>> {
        let mut extra: Vec<String> = Vec::new();
        if let Some(f) = &self.file {
            extra.push("--session".into());
            extra.push(f.to_string_lossy().into());
        }
        match self.tools_preset.as_str() {
            // spawn-arg tool selection (pi-web uses in-process
            // setActiveToolsByName; the RPC surface has no live switch)
            "chat-only" => extra.push("--no-tools".into()),
            "read-only" => {
                extra.push("--tools".into());
                extra.push("read,grep,find,ls".into());
            }
            _ => {}
        }
        self.agent.spawn_with(&self.cwd, &extra)
    }

    /// Kill the process but keep everything else (idle recycle / soft drop).
    pub(crate) fn shutdown_process(&mut self) {
        self.agent.session = None;
    }

    pub(crate) fn model_label_text(&self) -> String {
        self.state
            .as_ref()
            .and_then(|s| s.model_label())
            .unwrap_or_else(|| "pi".to_string())
    }

    pub(crate) fn touch(&mut self) {
        self.last_activity = std::time::Instant::now();
    }

    pub(crate) fn refresh_state(&self) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::GetState);
            let _ = session.send(&Command::GetSessionStats);
            let _ = session.send(&Command::GetCommands);
            let _ = session.send(&Command::GetAvailableModels);
        }
    }

    /// Attach the event pump for a freshly spawned receiver. Callers pass
    /// the epoch they just created — reading the entity here would be a
    /// re-entrant borrow (we are typically inside `rt.update`).
    pub(crate) fn attach_pump(
        _this: &Entity<Self>,
        rx: UnboundedReceiver<Event>,
        epoch: u64,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |weak, cx| {
            consume_runtime_events(weak, cx, rx, epoch).await;
        })
        .detach();
    }
}

impl EventEmitter<SessionEvent> for SessionRuntime {}

// placeholder Render so the entity can exist before the view wires up
impl Render for SessionRuntime {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::div()
    }
}

/// Runtime-local event pump: routes THIS session's events into THIS runtime
/// (stale-guarded by epoch). Chat is not involved — running sessions keep
/// accumulating while parked.
async fn consume_runtime_events(
    this: gpui::WeakEntity<SessionRuntime>,
    cx: &mut gpui::AsyncApp,
    mut rx: UnboundedReceiver<Event>,
    epoch: u64,
) {
    while let Some(event) = rx.next().await {
        let stale = this
            .update(cx, |rt, cx| {
                if rt.agent.epoch != epoch {
                    return true;
                }
                rt.touch();
                rt.on_event(event, cx);
                false
            })
            .unwrap_or(true);
        if stale {
            return;
        }
    }
    let _ = this.update(cx, |rt, cx| {
        if rt.agent.epoch == epoch {
            rt.status = "pi exited".into();
            cx.emit(SessionEvent::Changed);
            cx.notify();
        }
    });
}

// ---------------------------------------------------------------------------
// migrated from Chat (main.rs) — session-local behavior
// ---------------------------------------------------------------------------

impl SessionRuntime {
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
                        cx.emit(SessionEvent::RenameReady(prefill));
                    }
                } else if command == "get_session_stats" && success {
                    if let Some(data) = &data {
                        self.stats = Some(SessionStats::parse(data));
                        if let Some(f) = data["sessionFile"].as_str().map(PathBuf::from) {
                            self.file = Some(f);
                        }
                    }
                } else if command == "set_session_name" && success {
                    // pi flushed the name to the session file — reload the
                    // sidebar labels (pi-web onRenamed → loadSessions), with
                    // one delayed pass to cover flush lag
                    cx.emit(SessionEvent::ListDirty);
                    cx.notify();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(500))
                            .await;
                        let _ = this.update(cx, |_c, cx| {
                            cx.emit(SessionEvent::ListDirty);
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
                        cx.emit(SessionEvent::ListDirty);
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
                        // authoritative projection: replace any disk-direct /
                        // cached pre-render instead of appending (fixes
                        // doubled rows after the tail pre-render)
                        self.messages.clear();
                        for msg in data["messages"].as_array().into_iter().flatten() {
                            let role = msg["role"].as_str().unwrap_or("");
                            let blocks = content_blocks(&msg["content"]);
                            let usage = Usage::parse(&msg["usage"]);
                            let ts = msg["timestamp"].as_i64();
                            self.ingest_message(role, blocks, usage, ts, None, cx);
                        }
                        // map user messages to active-path entry ids (fork anchors)
                        let mut ids = self.active_user_entry_ids.iter();
                        for m in self.messages.iter_mut() {
                            if m.role == Role::User {
                                m.entry_id = ids.next().cloned();
                            }
                        }
                        // get_messages payloads carry generation-START stamps
                        // only; completion stamps (回复用时) come from the
                        // session file's entry write-times
                        self.merge_tail_stamps();
                        self.apply_pending_locate(cx);
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
                            self.messages.push(Msg {
                                role: Role::User,
                                blocks,
                                usage: None,
                                entry_id: None,
                                ts: timestamp,
                                end_ts: None,
                            });
                        }
                    }
                    "assistant" => {
                        self.phase_waiting = false;
                        self.messages.push(Msg {
                            role: Role::Assistant,
                            blocks,
                            usage: None,
                            entry_id: None,
                            ts: timestamp,
                            end_ts: None,
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
                            });
                            m.ts = timestamp.or(m.ts);
                            m.end_ts = Some(crate::services::format::now_ms());
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
                self.stream_started = None;
                // the session file exists now — make the new session show up
                // in the sidebar (pi-web refreshKey-on-agent_end parity)
                cx.emit(SessionEvent::ListDirty);
                // refresh branch tree so newly-sent user messages gain entry ids
                if let Some(s) = self.agent.session.as_ref() {
                    let _ = s.send(&Command::GetTree);
                }
                // agent may have written files: refresh git status
                            }
            Event::ExtensionUi(req) => cx.emit(SessionEvent::ExtUi(req)),
            Event::Unparsed(_) => {}
        }
        self.notify_list(cx);
    }

    /// Backfill message stamps from the session-file tail: snapshots never
    /// carry completion times, and entry write-times are the only end-stamp
    /// source for history (aligned by suffix — tail conversion mirrors the
    /// same ingest semantics).
    fn merge_tail_stamps(&mut self) {
        use crate::session::messages::msgs_from_tail;
        let Some(f) = self.file.clone() else {
            return;
        };
        let entries = pi_link::sessions::read_tail_messages(&f, 256 * 1024, 400);
        if entries.is_empty() {
            return;
        }
        let tail = msgs_from_tail(entries);
        if tail.len() > self.messages.len() {
            return; // projection shorter than file (compaction) — skip unsafe align
        }
        let start = self.messages.len() - tail.len();
        for (i, tm) in tail.into_iter().enumerate() {
            let m = &mut self.messages[start + i];
            if m.role == tm.role {
                if m.ts.is_none() {
                    m.ts = tm.ts;
                }
                if m.end_ts.is_none() {
                    m.end_ts = tm.end_ts;
                }
            }
        }
    }

    /// Reveal a search hit (013): scroll the message list to the matched row —
    /// by payload timestamp, falling back to the first message containing the
    /// needle. Returns false when messages aren't loaded yet (caller parks a
    /// pending_locate for the reconcile to apply).
    pub fn locate_message(&mut self, ts: Option<i64>, needle: &str, _cx: &mut Context<Self>) -> bool {
        if self.messages.is_empty() {
            self.pending_locate = Some((ts, needle.to_string()));
            return false;
        }
        let ix = self.hit_index(ts, needle);
        if let Some(ix) = ix {
            self.list.scroll_to_reveal_item(ix);
        }
        ix.is_some()
    }

    fn hit_index(&self, ts: Option<i64>, needle: &str) -> Option<usize> {
        if let Some(ts) = ts {
            if let Some(ix) = self.messages.iter().rposition(|m| m.ts == Some(ts)) {
                return Some(ix);
            }
        }
        let needle_lc = needle.to_lowercase();
        if needle_lc.is_empty() {
            return None;
        }
        self.messages
            .iter()
            .position(|m| m.plain_text().to_lowercase().contains(&needle_lc))
    }

    /// Apply a parked 013 jump once messages exist (reconcile path).
    fn apply_pending_locate(&mut self, _cx: &mut Context<Self>) {
        if let Some((ts, needle)) = self.pending_locate.take() {
            if let Some(ix) = self.hit_index(ts, &needle) {
                self.list.scroll_to_reveal_item(ix);
            }
        }
    }

    /// Clear the 复制 flash ~1.5s after it lit (pi-web copied-reset parity).
    pub fn spawn_flash_clear(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |r, cx| {
                r.copy_flash = None;
                cx.notify();
            });
        })
        .detach();
    }

    fn ingest_message(
        &mut self,
        role: &str,
        blocks: Vec<Block>,
        usage: Option<Usage>,
        ts: Option<i64>,
        entry_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        match role {
            "user" => {
                self.messages
                    .push(Msg { role: Role::User, blocks, usage: None, entry_id, ts, end_ts: None });
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
                    }),
                    entry_id: None,
                    ts,
                    end_ts: None,
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

    fn last_assistant(&mut self) -> &mut Msg {
        if !matches!(self.messages.last(), Some(m) if m.role == Role::Assistant) {
            self.messages
                .push(Msg {
                    role: Role::Assistant,
                    blocks: Vec::new(),
                    usage: None,
                    entry_id: None,
                    ts: None,
                    end_ts: None,
                });
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

    pub(crate) fn phase_row_visible(&self) -> bool {
        self.phase_waiting
            && !matches!(self.messages.last(), Some(m) if m.role == Role::Assistant)
    }

    fn notify_list(&mut self, cx: &mut Context<Self>) {
        self.list
            .reset(self.messages.len() + usize::from(self.phase_row_visible()));
        cx.notify();
    }

    /// 引导：中断当前运行并立即注入此消息（rpc steer）。
    pub(crate) fn steer_input(&mut self, cx: &mut Context<Self>) {
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
    pub(crate) fn follow_up_input(&mut self, cx: &mut Context<Self>) {
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
    pub(crate) fn abort_stream(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::Abort);
        }
        self.stream_started = None;
        cx.notify();
    }

    pub(crate) fn send_input(&mut self, cx: &mut Context<Self>) {
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
                        ts: Some(crate::services::format::now_ms()),
                        end_ts: None,
                    });
                    self.phase_waiting = true;
                    self.notify_list(cx);
                }
            }
            Err(e) => self.status = e,
        }
        cx.notify();
    }


    /// Fork a new session branching before the given user-message entry.
    /// pi rebinds this process to the branched session; the "fork" response
    /// handler reloads state/messages/tree.
    pub(crate) fn fork_from_entry(&mut self, entry_id: String, cx: &mut Context<Self>) {
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
        }
        cx.notify();
    }

    /// LLM session title (pi-web lib/session-title.ts parity via a one-off
    /// `pi --no-session --print` run; the in-process SDK call pi-web uses is
    /// not reachable over RPC).
    pub(crate) fn set_thinking_level(&mut self, level: &str, cx: &mut Context<Self>) {
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

    pub(crate) fn select_model(&mut self, provider: String, id: String, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::SetModel { provider, model: id });
        }
        self.refresh_state();
        cx.notify();
    }

    /// Editor toolbar 压缩: rpc compact (summarize the context).
    pub(crate) fn tool_preset_key(&self) -> &str {
        &self.tools_preset
    }

    pub(crate) fn tool_preset_label(&self) -> String {
        let key = self.tool_preset_key();
        if key.is_empty() { "configured".into() } else { key.to_string() }
    }

}

// ---------------------------------------------------------------------------
// moved from services/{title,branch}.rs (v54 sweep: those modules are gone,
// these two helpers are still live on the export/get_tree paths)
// ---------------------------------------------------------------------------

fn html_unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// Extract (systemPrompt, [(tool name, description)]) from an exported
/// session HTML (core/export-html/template.js markers).
fn parse_export_html(html: &str) -> (Option<String>, Vec<(String, String)>) {
    let mut prompt = None;
    if let Some(pos) = html.find("class=\"system-prompt-full\"") {
        if let Some(gt) = html[pos..].find('>') {
            let from = pos + gt + 1;
            if let Some(close) = html[from..].find("</div>") {
                let raw = &html[from..from + close];
                let text = html_unescape(raw).trim().to_string();
                if !text.is_empty() {
                    prompt = Some(text);
                }
            }
        }
    }
    let mut tools = Vec::new();
    let needle = "<span class=\"tool-item-name\">";
    let mut search_from = 0usize;
    while let Some(rel) = html[search_from..].find(needle) {
        let name_from = search_from + rel + needle.len();
        let Some(name_end) = html[name_from..].find("</span>") else { break };
        let name = html_unescape(&html[name_from..name_from + name_end]);
        let after_name = name_from + name_end + "</span>".len();
        let desc_needle = " - <span class=\"tool-item-desc\">";
        let Some(drel) = html[after_name..].find(desc_needle) else { break };
        let desc_from = after_name + drel + desc_needle.len();
        let Some(desc_end) = html[desc_from..].find("</span>") else { break };
        let desc = html_unescape(&html[desc_from..desc_from + desc_end]);
        tools.push((name, desc));
        search_from = desc_from + desc_end;
    }
    (prompt, tools)
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
