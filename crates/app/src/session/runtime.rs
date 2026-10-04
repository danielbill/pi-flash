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

use std::path::PathBuf;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{Context, Entity, EventEmitter, Render, prelude::*};
use pi_link::protocol::{
    AssistantEvent, Block, Command, Event, SessionState, SessionStats, SlashCommand, Usage,
    TreeNode, content_blocks, parse_tree,
};

use crate::agent_session::AgentSession;
use crate::session::chat_list::ChatList;
use crate::session::messages::{Msg, Role, UsageLine, merge_tool_result, msgs_from_tail, result_payload};
use pi_link::sessions::read_leaf_messages;

/// no cap for the disk-side leaf-chain rebuild: the repair must be longer
/// than the truncated RPC snapshot to win the length contest, and the RPC
/// path already renders the full (broken) chain — same render scale
const LEAF_REPAIR_MAX: usize = usize::MAX;
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
    /// disk-side message count at open time — get_messages shorter than
    /// this means pi's restored leaf chain is truncated (mis-parented
    /// non-message entry) and the display rebuilds from the file instead
    pub disk_msg_count: usize,
    /// session file size at last read — an external writer (pi-web on the
    /// same session) grows the file behind our pi process's back, which
    /// never notices; the tick compares and re-reads from disk
    pub disk_file_len: u64,
    /// pi process ownership (None while recycled/lazy)
    pub agent: AgentSession,

    // ---- message log ----
    pub messages: Vec<Msg>,
    /// 滚屏状态机（chat_list.rs）：gpui 列表 + 翻页锚点 + 垫片 + splice 记账
    pub pager: ChatList,
    /// explicit open/close overrides; absent = per-state default (thinking
    /// blocks open, process group closed when the turn has a final answer)
    pub collapsed: std::collections::HashMap<(usize, usize), bool>,
    pub phase_waiting: bool,
    /// text/thinking/toolcall deltas in flight (pi-web hasStreamingContent
    /// parity): the running/waiting phase row yields while content streams
    pub streaming_content: bool,
    /// 乐观发送的用户文本：只用于把 pi 回显的同文 user 消息就地升级（去重）。
    /// 与 phase_waiting 解耦 —— 回显到达 ≠ agent 已应答，等待行不能被它掐掉。
    pub pending_echo: Option<String>,
    pub stream_started: Option<std::time::Instant>,
    pub agent_running: bool,
    pub status: String,

    // ---- live session state ----
    pub state: Option<SessionState>,
    pub stats: Option<SessionStats>,
    pub branch_tree: Option<(Vec<TreeNode>, Option<String>)>,
    pub active_user_entry_ids: Vec<String>,
    pub pending_rename: bool,
    /// 手动压缩进行中（圆环弹窗按钮防连点；compact 响应清除）
    pub compacting: bool,
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
        Self {
            key,
            cwd,
            file: file.clone(),
            disk_msg_count: 0,
            disk_file_len: file
                .as_deref()
                .and_then(|f| std::fs::metadata(f).ok())
                .map(|m| m.len())
                .unwrap_or(0),
            agent: AgentSession::new(1),
            messages: Vec::new(),
            pager: ChatList::new(),
            collapsed: std::collections::HashMap::new(),
            phase_waiting: false,
            streaming_content: false,
            pending_echo: None,
            stream_started: None,
            agent_running: false,
            status: String::new(),
            state: None,
            stats: None,
            branch_tree: None,
            active_user_entry_ids: Vec::new(),
            pending_rename: false,
            compacting: false,
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
            rt.compacting = false;
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
        // MessageStart(user) 置位翻页锚点（见函数尾 page_turn）
        let mut user_arrived = false;
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
                } else if command == "set_model" && success {
                    // pi 1.0 swaps the model asynchronously — a get_state sent
                    // right after the command still reports the PREVIOUS model
                    // (measured), which made the pill lag one click behind.
                    // The set_model response carries the swapped model: apply
                    // it immediately, then re-sync state (thinking level now
                    // reflects the new model, e.g. "off" for non-reasoning).
                    if let Some(data) = &data {
                        if let Some(st) = self.state.as_mut() {
                            st.model = pi_link::protocol::parse_model_info(data);
                        }
                    }
                    self.refresh_state();
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
                        // fresh branched file: RPC snapshot is authoritative
                        self.disk_msg_count = 0;
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
                        let rpc_msgs = data["messages"].as_array().cloned().unwrap_or_default();
                        // leaf-chain repair: pi anchors its restored leaf at the
                        // last entry of ANY type; a mis-parented custom entry
                        // (plan-mode-state 实测) strands whole turns off the
                        // chain and get_messages comes back short. Rebuild from
                        // the file anchored at the last message entry instead.
                        let mut repaired = false;
                        if rpc_msgs.len() < self.disk_msg_count {
                            if let Some(path) = &self.file {
                                let msgs =
                                    msgs_from_tail(read_leaf_messages(path, LEAF_REPAIR_MAX));
                                if msgs.len() > rpc_msgs.len() {
                                    self.messages = msgs;
                                    self.pending_echo = None;
                                    // entry-id mapping stays the RPC/GetTree view
                                    // (fork anchors best-effort on repaired chains)
                                    let mut ids = self.active_user_entry_ids.iter();
                                    for m in self.messages.iter_mut() {
                                        if m.role == Role::User {
                                            m.entry_id = ids.next().cloned();
                                        }
                                    }
                                    self.apply_pending_locate(cx);
                                    self.notify_list(cx);
                                    self.status =
                                        "resumed (leaf chain repaired from disk)".into();
                                    repaired = true;
                                }
                            }
                        }
                        if repaired {
                            return;
                        }
                        // authoritative projection: replace any disk-direct /
                        // cached pre-render instead of appending (fixes
                        // doubled rows after the tail pre-render)
                        self.messages.clear();
                        self.pending_echo = None;
                        // 快照重建：锚点作废，reset 后按 Bottom 对齐贴底
                        self.pager.release();
                        for msg in &rpc_msgs {
                            let blocks = content_blocks(&msg["content"]);
                            let usage = Usage::parse(&msg["usage"]);
                            self.ingest_message(msg, blocks, usage, None, cx);
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
                } else if command == "compact" {
                    // 手动压缩（圆环弹窗按钮）：response 在摘要 LLM 完成后
                    // 才回（带 summary/usage）——清防连点标志并重拉
                    // stats/messages（compaction 卡片 + 上下文环跟着变）
                    self.compacting = false;
                    if success {
                        self.refresh_state();
                        if let Some(s) = self.agent.session.as_ref() {
                            let _ = s.send(&Command::GetMessages);
                        }
                    } else {
                        self.status =
                            format!("compact failed: {}", error.unwrap_or_default());
                    }
                } else if success {
                    self.status = format!("{command} ok");
                } else {
                    self.status =
                        format!("{command} failed: {}", error.unwrap_or_default());
                    if command == "prompt" {
                        // 发送失败：等待行不能一直转下去；锚点也无从跟随
                        self.phase_waiting = false;
                        self.pending_echo = None;
                        self.pager.release();
                    }
                }
            }
            Event::MessageStart { role, blocks, timestamp, is_error, tool_call_id, custom_type, custom_display, details } => {
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
                        // 去重只看 pending_echo：pi 回显 user 消息 ≠ agent 已开始应答，
                        // 等待行（spark + 正在思考… + shimmer）必须活到第一个 assistant 事件
                        let optimistic = self.pending_echo.is_some()
                            && matches!(self.messages.last(), Some(m)
                                if m.role == Role::User && m.plain_text() == echo_text);
                        self.pending_echo = None;
                        if optimistic {
                            if let Some(m) = self.messages.last_mut() {
                                m.blocks = blocks;
                            }
                        } else {
                            self.messages.push(Msg {
                                role: Role::User,
                                blocks,
                                usage: None,
                                entry_id: None,
                                ts: timestamp,
                                end_ts: None,
                                stop_reason: None,
                                error_message: None,
                                custom_type: None,
                                custom_display: true,
                                details: None,
                                model: None,
                            });
                        }
                        user_arrived = true;
                    }
                    "assistant" => {
                        self.phase_waiting = false;
                        self.pending_echo = None;
                        self.streaming_content = false;
                        self.messages.push(Msg {
                            role: Role::Assistant,
                            blocks,
                            usage: None,
                            entry_id: None,
                            ts: timestamp,
                            end_ts: None,
                            stop_reason: None,
                            error_message: None,
                            custom_type: None,
                            custom_display: true,
                            details: None,
                            model: None,
                        });
                    }
                    "toolResult" => {
                        let (text, images) = result_payload(&blocks);
                        if let Some(m) = self.messages.last_mut() {
                            merge_tool_result(
                                m,
                                tool_call_id.as_deref(),
                                is_error,
                                &text,
                                images,
                                details.clone(),
                                timestamp,
                            );
                        }
                    }
                    "custom" => {
                        let _ = &details;
                        self.messages.push(Msg {
                            role: Role::Custom,
                            blocks,
                            usage: None,
                            entry_id: None,
                            ts: timestamp,
                            end_ts: None,
                            stop_reason: None,
                            error_message: None,
                            custom_type,
                            custom_display,
                            details: None,
                            model: None,
                        });
                    }
                    _ => {}
                }
                let _ = timestamp;
            }
            Event::MessageUpdate(assistant_event) => {
                self.phase_waiting = false;
                self.pending_echo = None;
                self.streaming_content = true;
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
                        self.collapsed.insert((msg_ix, content_index), false);
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
                            is_error: false,
                            images: Vec::new(),
                            duration_s: None,
                            details: None,
                            args_partial: true,
                            result_arrived: false,
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
                            is_error: false,
                            images: Vec::new(),
                            duration_s: None,
                            details: None,
                            args_partial: true,
                            result_arrived: false,
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
                            is_error: false,
                            images: Vec::new(),
                            duration_s: None,
                            details: None,
                            args_partial: false,
                            result_arrived: false,
                        },
                    ) {
                        *name = name_c;
                        *args = args_c;
                    }
                }
                AssistantEvent::Other(_) => {}
                }
            }
            Event::MessageEnd { role, blocks, usage, timestamp, stop_reason, error_message, model, .. } => {
                if role == "assistant" {
                    if let Some(m) = self.messages.last_mut() {
                        if m.role == Role::Assistant {
                            m.blocks = blocks;
                            m.usage = usage.map(|u| UsageLine {
                                input: u.input,
                                output: u.output,
                                cache_read: u.cache_read,
                                cache_write: u.cache_write,
                                cost: u.cost,
                            });
                            m.ts = timestamp.or(m.ts);
                            m.end_ts = Some(crate::services::format::now_ms());
                            m.stop_reason = stop_reason;
                            m.error_message = error_message;
                            m.model = model;
                            self.streaming_content = false;
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
                self.pending_echo = None;
                self.streaming_content = false;
                // 一轮结束**不**退役锚点：pi-web 只是 promptAnchorActive=false
                // 让垫片收敛，容器 scrollTop 保持——短回复留在屏顶、长回复由
                // 胶水跟随尾部。锚点在此退役会把内容拽回屏底（消息从屏顶跳走，
                // 等于立刻撤销「发言钉顶」）。锚点留待用户滚轮 / 发送失败 /
                // 快照重建 / 会话切换退役。
                self.status = status_line(true, "idle");
                self.stream_started = None;
                self.refresh_state();
            }            Event::AgentEnd { .. } => {
                self.agent_running = false;
                self.phase_waiting = false;
                self.pending_echo = None;
                self.streaming_content = false;
                // 同上：轮末不退役锚点（短回复继续钉在屏顶）
                self.stream_started = None;
                // our own writer advanced the file — re-baseline so the
                // external-append tick doesn't re-read our own turn
                self.sync_disk_baseline();
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
        // 用户消息到达（发送回显升级 / steer 推入）→ 翻页：该消息钉视口
        // 顶，历史滚出屏（回显升级同锚点重入不重钉，见 page_turn）
        if user_arrived {
            let ix = self.messages.len() - 1;
            self.pager
                .page_turn(ix, self.messages.len(), self.phase_row_visible());
        }
        self.notify_list(cx);
    }

    /// Remember the session file's current size (our own writer's progress):
    /// the external-append tick only re-reads when someone ELSE grew it.
    pub(crate) fn sync_disk_baseline(&mut self) {
        if let Some(f) = &self.file {
            if let Ok(meta) = std::fs::metadata(f) {
                self.disk_file_len = meta.len();
            }
        }
    }

    /// External-append watch (pi-web session-revision parity): another
    /// writer (pi-web on the same session file) appends behind our pi
    /// process's back, and pi's in-memory chain never notices. While idle,
    /// a size change triggers a disk leaf-chain re-read.
    pub(crate) fn check_external_append(&mut self, cx: &mut Context<Self>) {
        if self.agent_running || self.pending_echo.is_some() {
            return;
        }
        let Some(f) = self.file.clone() else { return };
        let Ok(meta) = std::fs::metadata(&f) else { return };
        let len = meta.len();
        if len == self.disk_file_len || len == 0 {
            return;
        }
        let msgs = msgs_from_tail(read_leaf_messages(&f, LEAF_REPAIR_MAX));
        self.disk_file_len = len;
        if msgs.len() != self.messages.len() {
            // 整表重读 → 锚点索引失效（内容整体换过），先退役
            self.pager.release();
            self.messages = msgs;
            // fork anchors best-effort (same as the open-time repair path)
            let mut ids = self.active_user_entry_ids.iter();
            for m in self.messages.iter_mut() {
                if m.role == Role::User {
                    m.entry_id = ids.next().cloned();
                }
            }
            self.apply_pending_locate(cx);
            self.notify_list(cx);
            self.status = status_line(true, "synced from disk");
        }
    }

    /// Backfill message stamps from the session-file tail: snapshots never
    /// carry completion times, and entry write-times are the only end-stamp
    /// source for history (aligned by suffix — tail conversion mirrors the
    /// same ingest semantics).
    fn merge_tail_stamps(&mut self) {
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
            self.pager.reveal(ix);
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
                self.pager.reveal(ix);
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

    /// Convert one wire message (get_messages snapshot shape) into render
    /// state. Mirrors msgs_from_tail semantics exactly: user/assistant push,
    /// toolResult merges into the paired tool call, other roles skipped.
    fn ingest_message(
        &mut self,
        msg: &serde_json::Value,
        blocks: Vec<Block>,
        usage: Option<Usage>,
        entry_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let role = msg["role"].as_str().unwrap_or("");
        let ts = msg["timestamp"].as_i64();
        match role {
            "user" => {
                // 快照重建路径（打开会话/刷新循环调用）：不做清屏，
                // reset 后 Bottom 对齐保持贴底；清屏只属活事件路径
                self.messages.push(Msg {
                    role: Role::User,
                    blocks,
                    usage: None,
                    entry_id,
                    ts,
                    end_ts: None,
                    stop_reason: None,
                    error_message: None,
                    custom_type: None,
                    custom_display: true,
                    details: None,
                    model: None,
                });
            }
            "assistant" => {
                self.messages.push(Msg {
                    role: Role::Assistant,
                    blocks,
                    usage: usage.map(|u| UsageLine {
                        input: u.input,
                        output: u.output,
                        cache_read: u.cache_read,
                        cache_write: u.cache_write,
                        cost: u.cost,
                    }),
                    entry_id: None,
                    ts,
                    end_ts: None,
                    stop_reason: msg["stopReason"].as_str().map(str::to_string),
                    error_message: msg["errorMessage"].as_str().map(str::to_string),
                    model: msg["model"].as_str().map(str::to_string),
                    custom_type: None,
                    custom_display: true,
                    details: None,
                });
            }
            "toolResult" => {
                let (text, images) = result_payload(&blocks);
                let is_error = msg["isError"].as_bool().unwrap_or(false);
                let tcid = msg["toolCallId"].as_str();
                let details = msg["details"].as_object().map(|_| msg["details"].clone());
                if let Some(m) = self.messages.last_mut() {
                    merge_tool_result(m, tcid, is_error, &text, images, details, ts);
                }
            }
            "custom" => {
                self.messages.push(Msg {
                    role: Role::Custom,
                    blocks,
                    usage: None,
                    entry_id: None,
                    ts,
                    end_ts: None,
                    stop_reason: None,
                    error_message: None,
                    custom_type: Some(msg["customType"].as_str().unwrap_or("").to_string()),
                    custom_display: msg["display"].as_bool().unwrap_or(true),
                    details: msg["details"].as_object().map(|_| msg["details"].clone()),
                    model: None,
                });
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
                    stop_reason: None,
                    error_message: None,
                    custom_type: None,
                    custom_display: true,
                    details: None,
                    model: None,
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

    /// pi-web ChatWindow agentRunning && !hasStreamingContent parity: the
    /// waiting/running row shows while no content is streaming — before the
    /// first assistant event (waiting) and during tool execution (running).
    pub(crate) fn phase_row_visible(&self) -> bool {
        (self.phase_waiting || self.agent_running) && !self.streaming_content
    }

    /// pi-web phaseLabel parity: running tool names derive from the last
    /// assistant message's unanswered tool calls; none pending = waiting.
    pub(crate) fn phase_label(&self) -> String {
        let tools: Vec<String> = self
            .messages
            .last()
            .filter(|m| m.role == Role::Assistant)
            .map(|m| {
                m.blocks
                    .iter()
                    .filter_map(|b| match b {
                        Block::ToolCall { name, result, .. }
                            if !name.is_empty() && result.is_empty() =>
                        {
                            Some(name.to_string())
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        match tools.len() {
            0 => tr("正在等待模型...").to_string(),
            1 => crate::i18n::tf("正在运行 {name}...", &[("name", tools[0].clone())]),
            n if n <= 3 => crate::i18n::tf("正在运行 {names}...", &[("names", tools.join(", "))]),
            n => crate::i18n::tf(
                "正在运行 {names}（另有 {count} 个）...",
                &[
                    ("names", tools[..2].join(", ")),
                    ("count", (n - 2).to_string()),
                ],
            ),
        }
    }

    /// 列表同步：结构手术（splice/reset）与翻页垫片结算全在 ChatList
    /// （chat_list.rs，pi-web useAgentSession 滚屏层的移植）。
    pub(crate) fn notify_list(&mut self, cx: &mut Context<Self>) {
        self.pager.sync(self.messages.len(), self.phase_row_visible());
        cx.notify();
    }

    /// 引导：中断当前运行并立即注入此消息（rpc steer）。乐观上屏与翻页
    /// 和 prompt 发送一致——发送帧气泡钉视口顶，不等回显。
    pub(crate) fn steer_input(&mut self, cx: &mut Context<Self>) {
        let text = self.input.trim().to_string();
        if text.is_empty() && self.pending_images.is_empty() {
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
        match self
            .agent
            .session
            .as_ref()
            .map(|session| session.send(&Command::Steer { message: text.clone(), images }))
        {
            Some(Ok(_)) => {
                self.input.clear();
                self.pending_images.clear();
                if !text.is_empty() && !text.starts_with("/skill:") {
                    self.optimistic_send(text, cx);
                } else {
                    cx.notify();
                }
            }
            Some(Err(e)) => self.status = e,
            None => {}
        }
    }

    /// 乐观上屏 + 发送帧翻页（prompt/steer 共用，pi-web optimistic user
    /// message parity）：气泡立即显示并钉视口顶，RPC 回显经 pending_echo
    /// 去重就地升级。
    fn optimistic_send(&mut self, text: String, cx: &mut Context<Self>) {
        self.pending_echo = Some(text.clone());
        self.messages.push(Msg {
            role: Role::User,
            blocks: vec![Block::Text { content_index: 0, text }],
            usage: None,
            entry_id: None,
            ts: Some(crate::services::format::now_ms()),
            end_ts: None,
            stop_reason: None,
            error_message: None,
            custom_type: None,
            custom_display: true,
            details: None,
            model: None,
        });
        self.phase_waiting = true;
        // 发送帧翻页：新用户消息立即钉视口顶、历史滚出屏，不等回显不等
        // bounds（page_turn 内部完成 sync 落账）
        let ix = self.messages.len() - 1;
        let msgs = self.messages.len();
        let phase = self.phase_row_visible();
        self.pager.page_turn(ix, msgs, phase);
        cx.notify();
    }

    /// 后续消息：Agent 完成后排队此消息（rpc follow_up，pi 1.0 原生携带
    /// images）。
    pub(crate) fn follow_up_input(&mut self, cx: &mut Context<Self>) {
        // 压缩锁：UI 已锁死，这里是绕过 UI 调用的兜底
        if self.compacting {
            return;
        }
        let text = self.input.trim().to_string();
        if text.is_empty() && self.pending_images.is_empty() {
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
            let _ = session.send(&Command::FollowUp { message: text, images });
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
        // 中止：等待行立即收起（不等 agent_settled）
        self.phase_waiting = false;
        self.pending_echo = None;
        self.notify_list(cx);
        cx.notify();
    }

    /// 「回到最新」按钮（pi-web scrollToBottom）：reveal 列表末条（锚点期
    /// 含垫片——reveal 垫片底 = 回到钉顶位，与 pi-web scrollToBottom 落在
    /// 垫片底同款）。落底后 Bottom 对齐自动归 None 恢复跟随。
    pub(crate) fn scroll_to_bottom(&mut self) {
        self.pager.jump_to_bottom();
    }

    pub(crate) fn send_input(&mut self, cx: &mut Context<Self>) {
        // 压缩锁：UI 已锁死，这里是绕过 UI 调用的兜底
        if self.compacting {
            return;
        }
        let text = self.input.trim().to_string();
        // 空文本+图片可发送（pi-web handleSend：!msg && !images 才拦）
        if text.is_empty() && self.pending_images.is_empty() {
            return;
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("未连接").into();
            cx.notify();
            return;
        };
        // 运行中发消息 = steer（事件驱动 agent_running 优先：快照 is_streaming
        // 一轮内恒 false，会让「引导」变成新 prompt）
        let streaming = self.agent_running || self.state.as_ref().is_some_and(|s| s.is_streaming);
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
                // 纯图片发送不进历史、不做乐观回显（空文本气泡无内容可显，
                // 等 RPC 回显带图的完整用户消息）
                if !text.is_empty() && self.history.last().map(|h| h != &text).unwrap_or(true) {
                    self.history.push(text.clone());
                }
                self.history_ix = None;
                self.input.clear();
                self.pending_images.clear();
                self.status = if streaming { "steering" } else { "running" }.into();
                // 技能命令不做乐观回显（pi-web parity）：RPC 回显的是展开
                // 信封文本（渲染层折叠成紧凑命令），乐观插入裸 "/skill:xxx"
                // 会多出一条重复气泡。prompt 与 steer 一致——发送帧即上屏
                // 翻页，不等回显
                if !text.is_empty() && !text.starts_with("/skill:") {
                    self.optimistic_send(text, cx);
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
        if self.agent_running
            || self
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
        // no immediate refresh_state here: pi 1.0 completes the swap
        // asynchronously, so an eager get_state reports the PREVIOUS model —
        // the label updates from the set_model response instead (see the
        // set_model response arm)
        cx.notify();
    }

    /// 压缩按钮可用性：agent 跑动中不可（compact 会中止当前 turn）、
    /// 压缩中防连点、未连接不可。除此之外不设门槛——用不用由用户决定。
    pub(crate) fn can_compact(&self) -> bool {
        !self.compacting && !self.agent_running && self.agent.session.is_some()
    }

    /// 圆环弹窗底部手动压缩：rpc compact。`compacting` 期间输入面板锁死
    /// （placeholder 提示 + 变更丢弃）；完成后 pi 回 compact response，
    /// 由其分支清标志并刷新 stats/messages。
    pub(crate) fn compact(&mut self, cx: &mut Context<Self>) {
        if !self.can_compact() {
            return;
        }
        match self.agent.session.as_ref().map(|s| s.send(&Command::Compact)) {
            Some(Ok(_)) => {
                self.compacting = true;
                self.status = tr("压缩中…").into();
            }
            Some(Err(e)) => self.status = e,
            None => self.status = tr("未连接").into(),
        }
        cx.emit(SessionEvent::Changed);
        cx.notify();
    }

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
