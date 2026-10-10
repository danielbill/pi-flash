//! Typed pi RPC protocol (JSONL over stdio).
//!
//! Reference: vendored pi `docs/rpc.md`, `docs/json.md`, `docs/rpc-commands.md`.
//! Unknown record shapes are preserved as [`Event::Other`] / raw JSON so a pi
//! upgrade never silently drops data — conformance tests fail loudly instead.

use serde_json::{Value, json};

/// One pi RPC record kind we send. Correlated via `id` where a response is expected.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Prompt {
        message: String,
        /// image contents: [{"type":"image","data":<b64>,"mimeType":...}]
        images: Vec<Value>,
    },
    /// Queued while the agent is running; delivered after the current
    /// assistant turn finishes its tool calls (rpc-commands.md 「steer」).
    Steer {
        message: String,
        images: Vec<Value>,
    },
    FollowUp {
        message: String,
        /// image contents: [{"type":"image","data":<b64>,"mimeType":...}]
        /// (pi 1.0 follow_up 原生携带 images：session.followUp(message, images))
        images: Vec<Value>,
    },
    Abort,
    /// Summarize/compact the session context (rpc compact)
    Compact,
    GetState,
    GetMessages,
    GetSessionStats,
    SetModel { provider: String, model: String },
    SetSessionName { name: String },
    GetCommands,
    GetAvailableModels,
    ExportHtml,
    SetThinkingLevel { level: String },
    /// Full session tree with branch structure
    GetTree,
    /// Flat entry list + leafId (rpc get_entries). The tree-shaped sibling of
    /// GetTree, and the one that scales: pi builds get_tree by recursing the
    /// node chain, so a ~1200-message session kills pi itself with "Maximum
    /// call stack size exceeded" before the client ever parses it. The flat
    /// list never nests, so fork anchors (the 「新分支」 entry ids) come from
    /// here — pi-web parity: lib/session-reader.ts sliceActiveBranch walks
    /// parentId over the very same flat entries.
    GetEntries,
    /// Final assistant text of the last turn (rpc get_last_assistant_text)
    GetLastAssistantText,
    /// Fork a new session branching before the given user-message entry
    /// (rpc `fork`; pi only accepts position "before" here, so the entry must
    /// be a user message and IT IS NOT carried over to the branch)
    Fork { entry_id: String },
    /// Clone the whole current session into a new branch file (rpc `clone` =
    /// `runtimeHost.fork(leafId, {position:"at"})`). This is the only wire
    /// command that can branch *at* an entry, so "keep everything up to and
    /// including the latest agent reply" rides on it.
    Clone,
    /// Answer to a blocking extension UI request (rpc-mode reads this at the
    /// raw-line level, before command dispatch).
    ExtensionUiResponse {
        id: String,
        value: Option<String>,
        confirmed: Option<bool>,
        cancelled: bool,
    },
    /// Run a shell command on the session cwd (rpc `bash`; composer 的 `!` /
    /// `!!` 前缀)。pi 立即执行并把结果记成 `role:"bashExecution"` 消息：
    /// 流式增量走 `bash_execution_update` 事件，最终结果由本命令的 response
    /// 携带；输出进模型上下文的时机是**下一次 prompt**（pi convertToLlm 折叠
    /// 成 user 文本）。`exclude_from_context` = `!!`——会话文件里有记录，
    /// 模型看不到。
    Bash {
        command: String,
        exclude_from_context: bool,
    },
    /// Abort the running bash command (rpc `abort_bash`)
    AbortBash,
}

impl Command {
    pub fn kind(&self) -> &'static str {
        match self {
            Command::Prompt { .. } => "prompt",
            Command::Steer { .. } => "steer",
            Command::FollowUp { .. } => "follow_up",
            Command::Abort => "abort",
            Command::Compact => "compact",
            Command::GetState => "get_state",
            Command::GetMessages => "get_messages",
            Command::GetSessionStats => "get_session_stats",
            Command::SetModel { .. } => "set_model",
            Command::SetSessionName { .. } => "set_session_name",
            Command::GetCommands => "get_commands",
            Command::ExportHtml => "export_html",
            Command::GetAvailableModels => "get_available_models",
            Command::SetThinkingLevel { .. } => "set_thinking_level",
            Command::GetTree => "get_tree",
            Command::GetEntries => "get_entries",
            Command::GetLastAssistantText => "get_last_assistant_text",
            Command::Fork { .. } => "fork",
            Command::Clone => "clone",
            Command::ExtensionUiResponse { .. } => "extension_ui_response",
            Command::Bash { .. } => "bash",
            Command::AbortBash => "abort_bash",
        }
    }

    /// Serialize to a single JSONL record (no trailing newline).
    pub fn to_record(&self, id: &str) -> Value {
        // extension_ui_response is correlated by the REQUEST id at pi's
        // raw-line reader — no cmd sequence id
        if let Command::ExtensionUiResponse { id, value, confirmed, cancelled } = self {
            let mut v = json!({ "type": "extension_ui_response", "id": id });
            if let Some(value) = value {
                v["value"] = json!(value);
            }
            if let Some(confirmed) = confirmed {
                v["confirmed"] = json!(confirmed);
            }
            if *cancelled {
                v["cancelled"] = json!(true);
            }
            return v;
        }
        let mut v = match self {
            Command::Prompt { message, images } => {
                let mut v = json!({ "type": self.kind(), "message": message });
                if !images.is_empty() {
                    v["images"] = json!(images);
                }
                v
            }
            Command::Steer { message, images } => {
                let mut v = json!({ "type": self.kind(), "message": message });
                if !images.is_empty() {
                    v["images"] = json!(images);
                }
                v
            }
            Command::FollowUp { message, images } => {
                let mut v = json!({ "type": self.kind(), "message": message });
                if !images.is_empty() {
                    v["images"] = json!(images);
                }
                v
            }
            Command::Compact
            | Command::Abort
            | Command::GetState
            | Command::GetMessages
            | Command::GetSessionStats
            | Command::GetCommands
            | Command::GetAvailableModels
            | Command::ExportHtml => {
                json!({ "type": self.kind() })
            }
            Command::SetThinkingLevel { level } => {
                json!({ "type": self.kind(), "level": level })
            }
            // wire field is modelId (rpc-commands.md set_model)
            Command::SetModel { provider, model } => {
                json!({ "type": self.kind(), "provider": provider, "modelId": model })
            }
            Command::SetSessionName { name } => {
                json!({ "type": self.kind(), "name": name })
            }
            Command::GetTree
            | Command::GetEntries
            | Command::Clone
            | Command::GetLastAssistantText => {
                json!({ "type": self.kind() })
            }
            // wire field is entryId (rpc-types.d.ts fork)
            Command::Fork { entry_id } => {
                json!({ "type": self.kind(), "entryId": entry_id })
            }
            // rpc `bash`：excludeFromContext 只在 true 时发（false 是 pi 侧默认）
            Command::Bash {
                command,
                exclude_from_context,
            } => {
                let mut v = json!({ "type": self.kind(), "command": command });
                if *exclude_from_context {
                    v["excludeFromContext"] = json!(true);
                }
                v
            }
            Command::AbortBash => json!({ "type": self.kind() }),
            // handled by the early return above (request-id correlation)
            Command::ExtensionUiResponse { .. } => unreachable!(),
        };
        v["id"] = json!(id);
        v
    }
}

/// Assistant content block kinds carried by `message.content` arrays.
///
/// Mirrors the wire blocks pi emits (json.md): `text`, `thinking`, `toolcall`,
/// `image` (v56: tool results and user messages may carry images).
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Text { content_index: usize, text: String },
    Thinking { content_index: usize, text: String },
    ToolCall {
        content_index: usize,
        id: String,
        name: String,
        args: String,
        /// Output text delivered by a following role="toolResult" message.
        result: String,
        /// ToolResultMessage.isError — set when the result message merges in.
        is_error: bool,
        /// Images delivered by the toolResult message content.
        images: Vec<ImageData>,
        /// result arrival − message start, seconds (pi-web {n}s duration)
        duration_s: Option<i64>,
        /// ToolResultMessage.details (write/edit patch, apply_patch preview)
        details: Option<Value>,
        /// args still streaming (toolcall_delta); flips false on toolcall_end
        args_partial: bool,
        /// a toolResult message has merged in (empty text ≠ "no result yet")
        result_arrived: bool,
    },
    /// ImageContent { type:"image", data: b64, mimeType } — user message
    /// attachments; tool-result images merge into the paired ToolCall.
    Image {
        content_index: usize,
        mime: String,
        data: String,
    },
}

/// Decoded image payload (`ImageContent` on the wire): b64 `data` + mime.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageData {
    pub mime: String,
    pub data: String,
}

impl Block {
    pub fn content_index(&self) -> usize {
        match self {
            Block::Text { content_index, .. }
            | Block::Thinking { content_index, .. }
            | Block::ToolCall { content_index, .. }
            | Block::Image { content_index, .. } => *content_index,
        }
    }
}

/// Extract ordered content blocks from a `message.content` value
/// (string form is treated as a single text block).
pub fn content_blocks(content: &Value) -> Vec<Block> {
    match content {
        Value::String(s) => vec![Block::Text { content_index: 0, text: s.clone() }],
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(ix, b)| {
                let idx = b["index"].as_u64().unwrap_or(ix as u64) as usize;
                match b["type"].as_str() {
                    Some("thinking") => Block::Thinking {
                        content_index: idx,
                        text: b["thinking"].as_str().unwrap_or("").to_string(),
                    },
                    Some("image") => Block::Image {
                        content_index: idx,
                        mime: b["mimeType"].as_str().unwrap_or("image/png").to_string(),
                        data: b["data"].as_str().unwrap_or("").to_string(),
                    },
                    Some("toolCall") | Some("toolcall") => {
                        let args = b["partialJson"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| {
                                b["arguments"]
                                    .as_object()
                                    .filter(|o| !o.is_empty())
                                    .map(|o| Value::Object(o.clone()).to_string())
                                    .unwrap_or_default()
                            });
                        Block::ToolCall {
                            content_index: idx,
                            id: b["id"].as_str().unwrap_or("").to_string(),
                            name: b["name"]
                                .as_str()
                                .or_else(|| b["toolName"].as_str())
                                .unwrap_or("")
                                .to_string(),
                            args,
                            result: String::new(),
                            is_error: false,
                            images: Vec::new(),
                            duration_s: None,
                            details: None,
                            args_partial: false,
                            result_arrived: false,
                        }
                    }
                    _ => Block::Text {
                        content_index: idx,
                        text: b["text"].as_str().unwrap_or("").to_string(),
                    },
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Incremental assistant message events (`assistantMessageEvent` in `message_update`).
///
/// Field names per json.md: `contentIndex`, `delta`, `content`, `id`, `toolName`,
/// `toolCall`. `*_end` variants carry authoritative content and replace the
/// streamed reconstruction.
#[derive(Debug, Clone, PartialEq)]
pub enum AssistantEvent {
    TextDelta { content_index: usize, delta: String },
    TextEnd { content_index: usize, content: String },
    ThinkingStart { content_index: usize },
    ThinkingDelta { content_index: usize, delta: String },
    ThinkingEnd { content_index: usize, content: String },
    ToolCallStart { content_index: usize, id: String, tool_name: String },
    ToolCallDelta { content_index: usize, delta: String },
    ToolCallEnd { content_index: usize, tool_call: Value },
    Other(Value),
}

impl AssistantEvent {
    pub fn parse(v: &Value) -> AssistantEvent {
        let idx = || v["contentIndex"].as_u64().unwrap_or(0) as usize;
        match v["type"].as_str() {
            Some("text_delta") => AssistantEvent::TextDelta {
                content_index: idx(),
                delta: v["delta"].as_str().unwrap_or("").to_string(),
            },
            Some("text_end") => AssistantEvent::TextEnd {
                content_index: idx(),
                content: v["content"].as_str().unwrap_or("").to_string(),
            },
            Some("thinking_start") => AssistantEvent::ThinkingStart { content_index: idx() },
            Some("thinking_delta") => AssistantEvent::ThinkingDelta {
                content_index: idx(),
                delta: v["delta"].as_str().unwrap_or("").to_string(),
            },
            Some("thinking_end") => AssistantEvent::ThinkingEnd {
                content_index: idx(),
                content: v["content"].as_str().unwrap_or("").to_string(),
            },
            Some("toolcall_start") => AssistantEvent::ToolCallStart {
                content_index: idx(),
                id: v["id"].as_str().unwrap_or("").to_string(),
                tool_name: v["toolName"].as_str().unwrap_or("").to_string(),
            },
            Some("toolcall_delta") => AssistantEvent::ToolCallDelta {
                content_index: idx(),
                delta: v["delta"].as_str().unwrap_or("").to_string(),
            },
            Some("toolcall_end") => AssistantEvent::ToolCallEnd {
                content_index: idx(),
                tool_call: v["toolCall"].clone(),
            },
            _ => AssistantEvent::Other(v.clone()),
        }
    }
}

/// Token usage + cost for one assistant message (json.md `usage`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost: f64,
}

impl Usage {
    pub fn parse(v: &Value) -> Option<Usage> {
        Some(Usage {
            input: v["input"].as_u64()?,
            output: v["output"].as_u64()?,
            cache_read: v["cacheRead"].as_u64().unwrap_or(0),
            cache_write: v["cacheWrite"].as_u64().unwrap_or(0),
            cost: v["cost"]["total"].as_f64().or_else(|| v["cost"].as_f64()).unwrap_or(0.0),
        })
    }
}

/// A runnable slash command (extension commands, prompt templates, skills).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
}

impl SlashCommand {
    pub fn parse_list(data: &Value) -> Vec<SlashCommand> {
        data["commands"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|c| SlashCommand {
                        name: c["name"].as_str().unwrap_or("").to_string(),
                        description: c["description"].as_str().unwrap_or("").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Extension UI request (`extension_ui_request`): the RPC-mode surface of
/// pi's extension ui context. Blocking methods (select/confirm/input/editor)
/// expect an `extension_ui_response`; the rest are fire-and-forget.
/// `custom` is not exposed by RPC mode (rpc-mode.js returns undefined).
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionUiRequest {
    pub id: String,
    pub method: ExtUiMethod,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExtUiMethod {
    Select { title: String, options: Vec<String> },
    Confirm { title: String, message: String },
    Input { title: String, placeholder: Option<String> },
    Editor { title: String, prefill: Option<String> },
    Notify { message: String, notify_type: Option<String> },
    SetStatus { status_key: String, status_text: Option<String> },
    SetWidget {
        widget_key: String,
        widget_lines: Option<Vec<String>>,
        /// "aboveEditor" | "belowEditor" (None -> pi-web default aboveEditor)
        placement: Option<String>,
    },
    SetTitle { title: String },
    SetEditorText { text: String },
}

impl ExtensionUiRequest {
    fn parse(v: &Value) -> Option<ExtensionUiRequest> {
        let id = v["id"].as_str()?.to_string();
        let method = match v["method"].as_str()? {
            "select" => ExtUiMethod::Select {
                title: v["title"].as_str().unwrap_or("").to_string(),
                options: v["options"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|o| o.as_str().map(str::to_string)).collect())
                    .unwrap_or_default(),
            },
            "confirm" => ExtUiMethod::Confirm {
                title: v["title"].as_str().unwrap_or("").to_string(),
                message: v["message"].as_str().unwrap_or("").to_string(),
            },
            "input" => ExtUiMethod::Input {
                title: v["title"].as_str().unwrap_or("").to_string(),
                placeholder: v["placeholder"].as_str().map(str::to_string),
            },
            "editor" => ExtUiMethod::Editor {
                title: v["title"].as_str().unwrap_or("").to_string(),
                prefill: v["prefill"].as_str().map(str::to_string),
            },
            "notify" => ExtUiMethod::Notify {
                message: v["message"].as_str().unwrap_or("").to_string(),
                notify_type: v["notifyType"].as_str().map(str::to_string),
            },
            "setStatus" => ExtUiMethod::SetStatus {
                status_key: v["statusKey"].as_str().unwrap_or("").to_string(),
                status_text: v["statusText"].as_str().map(str::to_string),
            },
            "setWidget" => ExtUiMethod::SetWidget {
                widget_key: v["widgetKey"].as_str().unwrap_or("").to_string(),
                widget_lines: v["widgetLines"].as_array().map(|a| {
                    a.iter().filter_map(|l| l.as_str().map(str::to_string)).collect()
                }),
                placement: v["widgetPlacement"].as_str().map(str::to_string),
            },
            "setTitle" => ExtUiMethod::SetTitle {
                title: v["title"].as_str().unwrap_or("").to_string(),
            },
            "set_editor_text" => ExtUiMethod::SetEditorText {
                text: v["text"].as_str().unwrap_or("").to_string(),
            },
            _ => return None,
        };
        Some(ExtensionUiRequest { id, method })
    }
}

/// One parsed record from pi's stdout.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Response to a command, correlated by id. `data` carries command
    /// payloads (e.g. get_messages -> {"messages": [...]}).
    Response {
        id: String,
        command: String,
        success: bool,
        error: Option<String>,
        data: Option<Value>,
    },
    MessageStart {
        role: String,
        blocks: Vec<Block>,
        timestamp: Option<i64>,
        /// ToolResultMessage.isError (false for other roles)
        is_error: bool,
        /// ToolResultMessage.toolCallId — correlates the result with its call
        tool_call_id: Option<String>,
        /// CustomMessage.customType (None for non-custom roles)
        custom_type: Option<String>,
        /// CustomMessage.display (true default for non-custom roles)
        custom_display: bool,
        /// ToolResultMessage.details (write/edit patch, apply_patch preview)
        details: Option<Value>,
        /// Full raw message, `role:"system"` only. The system message carries
        /// `content` / `sections` / `toolsAdded` / `toolsRemoved` patches
        /// (transcript replay input) that the parsed `blocks` lose — pi
        /// appends one per run and emits it complete on message_start.
        raw_system: Option<Value>,
    },
    MessageUpdate(AssistantEvent),
    /// Authoritative final message; blocks replace any streamed reconstruction.
    MessageEnd {
        role: String,
        blocks: Vec<Block>,
        usage: Option<Usage>,
        timestamp: Option<i64>,
        /// AssistantMessage.stopReason ("stop" | "length" | "toolUse" |
        /// "error" | "aborted" | "deferred"); absent on other roles.
        stop_reason: Option<String>,
        /// AssistantMessage.errorMessage — set when stopReason == "error".
        error_message: Option<String>,
        /// ToolResultMessage.isError (false for other roles)
        is_error: bool,
        /// ToolResultMessage.toolCallId
        tool_call_id: Option<String>,
        /// CustomMessage.customType (None for non-custom roles)
        custom_type: Option<String>,
        /// CustomMessage.display (true default for non-custom roles)
        custom_display: bool,
        /// AssistantMessage.model — per-message model label source
        model: Option<String>,
    },
    AgentStart,
    AgentEnd { will_retry: bool },
    /// pi will not continue automatically (retries/queue drained).
    AgentSettled,
    /// `bash_execution_update`（rpc `bash` 执行中的流式输出增量；id 关联发起
    /// 的命令，delta 追加到已显示的输出尾部）
    BashExecutionUpdate {
        id: Option<String>,
        delta: String,
    },
    /// Extension UI protocol records (dialogs/widgets/status/notify).
    ExtensionUi(ExtensionUiRequest),
    Unparsed(Value),
}

/// Parse `get_available_models` data: {"models": [...]}
pub fn parse_model_list(data: &Value) -> Vec<ModelInfo> {
    data["models"]
        .as_array()
        .map(|arr| arr.iter().filter_map(ModelInfo::parse).collect())
        .unwrap_or_default()
}

/// Parse a single model object (the `set_model` response carries the
/// swapped-in model).
pub fn parse_model_info(v: &Value) -> Option<ModelInfo> {
    ModelInfo::parse(v)
}

/// A node of the session branch tree (`get_tree` response).
/// Mirrors pi's SessionTreeNode: entry identity + role/text for message
/// entries, plus recursive children. Text is truncated to 80 chars because
/// the UI only ever shows a bounded preview.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TreeNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub entry_type: String,
    /// "user" | "assistant" | "toolResult" | "system" for message entries
    pub role: Option<String>,
    /// bounded message text preview (80 chars)
    pub text: Option<String>,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    fn parse(v: &Value) -> Option<TreeNode> {
        let entry = &v["entry"];
        let mut node = TreeNode {
            id: entry["id"].as_str()?.to_string(),
            parent_id: entry["parentId"].as_str().map(str::to_string),
            entry_type: entry["type"].as_str().unwrap_or("").to_string(),
            role: entry["message"]["role"].as_str().map(str::to_string),
            text: None,
            children: v["children"]
                .as_array()
                .map(|a| a.iter().filter_map(TreeNode::parse).collect())
                .unwrap_or_default(),
        };
        if node.entry_type == "message" {
            node.text = Some(extract_message_preview(&entry["message"]["content"]));
        }
        Some(node)
    }

    /// First user-message entry id on this node or its single-child chain
    /// (pi fork position "before" requires a user message entry).
    pub fn forkable_entry_id(&self) -> Option<&str> {
        let mut cur = Some(self);
        while let Some(n) = cur {
            if n.role.as_deref() == Some("user") {
                return Some(&n.id);
            }
            cur = n.children.first();
        }
        None
    }
}

/// Extract a bounded text preview from message content (string or blocks).
fn extract_message_preview(content: &Value) -> String {
    let mut text = String::new();
    if let Some(s) = content.as_str() {
        text = s.to_string();
    } else if let Some(arr) = content.as_array() {
        for b in arr {
            if b["type"].as_str() == Some("text") {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(b["text"].as_str().unwrap_or(""));
            }
        }
    }
    text.chars().take(80).collect()
}

/// Parse a `get_tree` response: {"tree": [...], "leafId": "..."|null}
pub fn parse_tree(data: &Value) -> (Vec<TreeNode>, Option<String>) {
    let tree = data["tree"]
        .as_array()
        .map(|a| a.iter().filter_map(TreeNode::parse).collect())
        .unwrap_or_default();
    let leaf_id = data["leafId"].as_str().map(str::to_string);
    (tree, leaf_id)
}


/// Final result of the rpc `bash` command (pi `BashResult`, bash-executor.d.ts).
/// Streaming deltas arrive earlier as [`Event::BashExecutionUpdate`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BashResult {
    /// Combined stdout+stderr output (sanitized, possibly truncated)
    pub output: String,
    /// Process exit code (None if killed/cancelled)
    pub exit_code: Option<i64>,
    pub cancelled: bool,
    pub truncated: bool,
    /// Temp file with the full output (set when output exceeded pi's cap)
    pub full_output_path: Option<String>,
}

impl BashResult {
    pub fn parse(v: &Value) -> BashResult {
        BashResult {
            output: v["output"].as_str().unwrap_or("").to_string(),
            exit_code: v["exitCode"].as_i64(),
            cancelled: v["cancelled"].as_bool().unwrap_or(false),
            truncated: v["truncated"].as_bool().unwrap_or(false),
            full_output_path: v["fullOutputPath"].as_str().map(str::to_string),
        }
    }
}

/// One session entry as `get_entries` returns it: flat, parentId-linked.
/// Only the fields the client acts on are lifted out (fork anchors need
/// id/parentId/type/role); the whole entry stays in `raw` for callers that
/// want the original (nav panel previews).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionEntry {
    pub id: String,
    pub parent_id: Option<String>,
    pub entry_type: String,
    /// "user" | "assistant" | "toolResult" | "system" for message entries
    pub role: Option<String>,
    pub timestamp: Option<String>,
    pub raw: Value,
}

impl SessionEntry {
    fn parse(v: &Value) -> Option<SessionEntry> {
        Some(SessionEntry {
            id: v["id"].as_str()?.to_string(),
            parent_id: v["parentId"].as_str().map(str::to_string),
            entry_type: v["type"].as_str().unwrap_or("").to_string(),
            role: v["message"]["role"].as_str().map(str::to_string),
            timestamp: v["timestamp"].as_str().map(str::to_string),
            raw: v.clone(),
        })
    }
}

/// Parse a `get_entries` response: {"entries": [...], "leafId": "..."|null}
pub fn parse_entries(data: &Value) -> (Vec<SessionEntry>, Option<String>) {
    let entries = data["entries"]
        .as_array()
        .map(|a| a.iter().filter_map(SessionEntry::parse).collect())
        .unwrap_or_default();
    let leaf_id = data["leafId"].as_str().map(str::to_string);
    (entries, leaf_id)
}

/// Entry ids of the user messages on the active chain (leaf → root walk,
/// returned oldest → newest).
///
/// pi-web parity: `sliceActiveBranch` (lib/session-reader.ts) walks parentId
/// over the flat entry list for exactly this purpose (fork anchors +
/// navigation targets). This replaces the get_tree version, which cannot
/// survive a long session: pi answers deep trees with "Maximum call stack
/// size exceeded", i.e. no anchors at all.
pub fn active_user_entry_ids(entries: &[SessionEntry], leaf_id: Option<&str>) -> Vec<String> {
    let Some(leaf) = leaf_id else {
        return Vec::new();
    };
    let mut by_id: std::collections::HashMap<&str, &SessionEntry> =
        std::collections::HashMap::with_capacity(entries.len());
    for e in entries {
        by_id.insert(e.id.as_str(), e);
    }
    let mut chain: Vec<&SessionEntry> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut cur = by_id.get(leaf).copied();
    // a corrupt parentId cycle must never hang the UI thread (and must not
    // duplicate the entries it loops over)
    while let Some(e) = cur {
        if !seen.insert(e.id.as_str()) {
            break;
        }
        chain.push(e);
        cur = e.parent_id.as_deref().and_then(|p| by_id.get(p).copied());
    }
    chain.reverse();
    chain
        .into_iter()
        .filter(|e| e.entry_type == "message" && e.role.as_deref() == Some("user"))
        .map(|e| e.id.clone())
        .collect()
}

/// Snapshot of `get_state` response data.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionState {
    pub model: Option<ModelInfo>,
    pub thinking_level: Option<String>,
    pub is_streaming: bool,
    pub session_name: Option<String>,
    /// pi 当前绑定的会话文件（rpc-types.d.ts RpcSessionState.sessionFile）。
    /// draft 首条 prompt 落盘、fork/clone 换分支都会让它变——shell 的身份
    /// （pool key / 侧栏高亮 / 重开目标）全挂在它上面，runtime 靠它跟随
    /// pi 的进程内重绑定。
    pub session_file: Option<String>,
    pub session_id: Option<String>,
    pub message_count: u64,
    pub pending_message_count: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub context_window: Option<u64>,
}

impl ModelInfo {
    fn parse(v: &Value) -> Option<ModelInfo> {
        Some(ModelInfo {
            id: v["id"].as_str()?.to_string(),
            name: v["name"].as_str().unwrap_or("").to_string(),
            provider: v["provider"].as_str().unwrap_or("").to_string(),
            context_window: v["contextWindow"].as_u64(),
        })
    }

    pub fn label(&self) -> String {
        if self.name.is_empty() {
            self.id.clone()
        } else {
            self.name.clone()
        }
    }
}

impl SessionState {
    pub fn parse(data: &Value) -> SessionState {
        SessionState {
            model: ModelInfo::parse(&data["model"]),
            thinking_level: data["thinkingLevel"].as_str().map(str::to_string),
            is_streaming: data["isStreaming"].as_bool().unwrap_or(false),
            session_name: data["sessionName"].as_str().map(str::to_string),
            session_file: data["sessionFile"].as_str().map(str::to_string),
            session_id: data["sessionId"].as_str().map(str::to_string),
            message_count: data["messageCount"].as_u64().unwrap_or(0),
            pending_message_count: data["pendingMessageCount"].as_u64().unwrap_or(0),
        }
    }

    pub fn model_label(&self) -> Option<String> {
        self.model.as_ref().map(|m| m.label())
    }
}

/// Snapshot of `get_session_stats` response data (tokens/cost/context).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionStats {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub tokens_total: u64,
    pub cost: f64,
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub context_percent: Option<f64>,
}

impl SessionStats {
    pub fn parse(data: &Value) -> SessionStats {
        SessionStats {
            input: data["tokens"]["input"].as_u64().unwrap_or(0),
            output: data["tokens"]["output"].as_u64().unwrap_or(0),
            cache_read: data["tokens"]["cacheRead"].as_u64().unwrap_or(0),
            cache_write: data["tokens"]["cacheWrite"].as_u64().unwrap_or(0),
            tokens_total: data["tokens"]["total"].as_u64().unwrap_or(0),
            cost: data["cost"].as_f64().unwrap_or(0.0),
            context_tokens: data["contextUsage"]["tokens"].as_u64(),
            context_window: data["contextUsage"]["contextWindow"].as_u64(),
            context_percent: data["contextUsage"]["percent"].as_f64(),
        }
    }

    /// "ctx 30% - $0.45" (context part omitted when unavailable)
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(p) = self.context_percent {
            parts.push(format!("ctx {p:.0}%"));
        }
        parts.push(format!("${:.2}", self.cost));
        parts.join(" \u{b7} ")
    }

    /// 缓存命中率 = cacheRead / (input + cacheWrite + cacheRead)（分母覆盖
    /// 全部输入类 token；pi-web session-info-popover 同式）。
    pub fn cache_hit_rate(&self) -> Option<f64> {
        let denom = self.input + self.cache_write + self.cache_read;
        (denom > 0).then(|| self.cache_read as f64 / denom as f64)
    }
}

/// Parse a single JSONL line from pi's stdout.
pub fn parse_line(line: &str) -> Option<Event> {
    // NOT serde_json::from_str: get_tree nests the whole conversation
    // (~4-5 JSON levels per turn), so any session past ~25 turns blows
    // serde_json's 128-level default and the whole response is lost.
    let v = crate::json::parse_value(line).ok()?;
    Some(parse_record(&v))
}

pub fn parse_record(v: &Value) -> Event {
    match v["type"].as_str() {
        Some("response") => Event::Response {
            id: v["id"].as_str().unwrap_or("").to_string(),
            command: v["command"].as_str().unwrap_or("").to_string(),
            success: v["success"].as_bool().unwrap_or(false),
            error: v["error"].as_str().map(str::to_string),
            data: v.get("data").cloned(),
        },
        // roles: "system" | "user" | "assistant" | "toolResult" (json.md wire)
        Some("message_start") => Event::MessageStart {
            role: v["message"]["role"].as_str().unwrap_or("").to_string(),
            blocks: content_blocks(&v["message"]["content"]),
            timestamp: v["message"]["timestamp"].as_i64(),
            is_error: v["message"]["isError"].as_bool().unwrap_or(false),
            tool_call_id: v["message"]["toolCallId"].as_str().map(str::to_string),
            custom_type: v["message"]["customType"].as_str().map(str::to_string),
            custom_display: v["message"]["display"].as_bool().unwrap_or(true),
            details: v["message"]["details"].as_object().map(|_| v["message"]["details"].clone()),
            raw_system: (v["message"]["role"].as_str() == Some("system"))
                .then(|| v["message"].clone()),
        },
        Some("message_update") => {
            Event::MessageUpdate(AssistantEvent::parse(&v["assistantMessageEvent"]))
        }
        Some("message_end") => Event::MessageEnd {
            role: v["message"]["role"].as_str().unwrap_or("").to_string(),
            blocks: content_blocks(&v["message"]["content"]),
            usage: Usage::parse(&v["message"]["usage"]),
            timestamp: v["message"]["timestamp"].as_i64(),
            stop_reason: v["message"]["stopReason"].as_str().map(str::to_string),
            error_message: v["message"]["errorMessage"].as_str().map(str::to_string),
            is_error: v["message"]["isError"].as_bool().unwrap_or(false),
            tool_call_id: v["message"]["toolCallId"].as_str().map(str::to_string),
            custom_type: v["message"]["customType"].as_str().map(str::to_string),
            custom_display: v["message"]["display"].as_bool().unwrap_or(true),
            model: v["message"]["model"].as_str().map(str::to_string),
        },
        Some("agent_start") => Event::AgentStart,
        Some("agent_end") => Event::AgentEnd {
            will_retry: v["willRetry"].as_bool().unwrap_or(false),
        },
        Some("agent_settled") => Event::AgentSettled,
        Some("bash_execution_update") => Event::BashExecutionUpdate {
            id: v["id"].as_str().map(str::to_string),
            delta: v["delta"].as_str().unwrap_or("").to_string(),
        },
        Some("extension_ui_request") => {
            Event::ExtensionUi(ExtensionUiRequest::parse(v).unwrap_or(ExtensionUiRequest {
                id: String::new(),
                method: ExtUiMethod::Notify {
                    message: String::new(),
                    notify_type: None,
                },
            }))
        }
        _ => Event::Unparsed(v.clone()),
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures below are REAL records captured from pi 0.87.1 runs.

    #[test]
    fn prompt_record_shape() {
        let c = Command::Prompt { message: "hi".into(), images: vec![] };
        assert_eq!(
            c.to_record("req-1"),
            json!({"id":"req-1","type":"prompt","message":"hi"})
        );
    }

    #[test]
    fn set_model_record_shape() {
        let c = Command::SetModel { provider: "glm".into(), model: "glm-5.3-flash".into() };
        assert_eq!(
            c.to_record("m1"),
            json!({"id":"m1","type":"set_model","provider":"glm","modelId":"glm-5.3-flash"})
        );
    }

    #[test]
    fn response_success() {
        let e = parse_line(r#"{"id":"1","type":"response","command":"prompt","success":true}"#).unwrap();
        match e {
            Event::Response { id, command, success, error, data } => {
                assert_eq!(
                    (id.as_str(), command.as_str(), success, error.is_none(), data.is_none()),
                    ("1", "prompt", true, true, true)
                );
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn get_messages_response_carries_data() {
        let e = parse_line(
            r#"{"type":"response","command":"get_messages","success":true,"data":{"messages":[{"role":"user","content":"hi"}]}}"#,
        )
        .unwrap();
        match e {
            Event::Response { command, data, .. } => {
                assert_eq!(command, "get_messages");
                let data = data.expect("data");
                assert_eq!(data["messages"][0]["role"], "user");
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn user_message_start_with_block_array() {
        // user content is a block array, not a string (wire format, pi 0.87)
        let e = parse_line(
            r#"{"type":"message_start","message":{"role":"user","content":[{"type":"text","text":"say OK"}],"timestamp":1790206311858}}"#,
        )
        .unwrap();
        match e {
            Event::MessageStart { role, blocks, timestamp, .. } => {
                assert_eq!(role, "user");
                assert_eq!(timestamp, Some(1790206311858));
                assert_eq!(blocks, vec![Block::Text { content_index: 0, text: "say OK".into() }]);
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn text_delta_uses_camel_case_fields() {
        let e = parse_line(
            r#"{"type":"message_update","usage":{"totalTokens":101},"assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"Hello "}}"#,
        )
        .unwrap();
        match e {
            Event::MessageUpdate(AssistantEvent::TextDelta { content_index, delta }) => {
                assert_eq!(content_index, 0);
                assert_eq!(delta, "Hello ");
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn toolcall_events_carry_tool_name_and_content() {
        let start = parse_line(
            r#"{"type":"message_update","assistantMessageEvent":{"type":"toolcall_start","contentIndex":1,"id":"tc_1","toolName":"bash","partial":{}}}"#,
        )
        .unwrap();
        assert!(matches!(
            start,
            Event::MessageUpdate(AssistantEvent::ToolCallStart { content_index: 1, tool_name, .. })
                if tool_name == "bash"
        ));

        let end = parse_line(
            r#"{"type":"message_update","assistantMessageEvent":{"type":"toolcall_end","contentIndex":1,"toolCall":{"id":"tc_1","name":"bash","arguments":{"command":"ls"}}}}"#,
        )
        .unwrap();
        match end {
            Event::MessageUpdate(AssistantEvent::ToolCallEnd { content_index, tool_call }) => {
                assert_eq!(content_index, 1);
                assert_eq!(tool_call["name"], "bash");
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn thinking_events() {
        let e = parse_line(
            r#"{"type":"message_update","assistantMessageEvent":{"type":"thinking_delta","contentIndex":0,"delta":"hmm"}}"#,
        )
        .unwrap();
        assert!(matches!(
            e,
            Event::MessageUpdate(AssistantEvent::ThinkingDelta { delta, .. }) if delta == "hmm"
        ));
    }

    #[test]
    fn message_end_blocks_extract_toolcall_with_tool_name() {
        // assistant final content uses toolName; index comes from the block itself
        let e = parse_line(
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"thinking","thinking":"","index":0},{"type":"toolCall","id":"tc_1","name":"read","arguments":{"path":"a.rs"},"index":1},{"type":"text","text":"done","index":2}]}}"#,
        )
        .unwrap();
        match e {
            Event::MessageEnd { role, blocks, .. } => {
                assert_eq!(role, "assistant");
                assert_eq!(blocks.len(), 3);
                assert!(matches!(&blocks[0], Block::Thinking { content_index: 0, text } if text.is_empty()));
                assert!(matches!(&blocks[1], Block::ToolCall { content_index: 1, name, .. } if name == "read"));
                assert!(matches!(&blocks[2], Block::Text { content_index: 2, text } if text == "done"));
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn message_end_carries_stop_reason_and_error() {
        let e = parse_line(
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"x","index":0}],"stopReason":"error","errorMessage":"boom"}}"#,
        )
        .unwrap();
        match e {
            Event::MessageEnd { stop_reason, error_message, .. } => {
                assert_eq!(stop_reason.as_deref(), Some("error"));
                assert_eq!(error_message.as_deref(), Some("boom"));
            }
            other => panic!("wrong event: {other:?}"),
        }
        // non-error messages: stopReason present, errorMessage absent
        let e = parse_line(
            r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"stop"}}"#,
        )
        .unwrap();
        match e {
            Event::MessageEnd { stop_reason, error_message, .. } => {
                assert_eq!(stop_reason.as_deref(), Some("stop"));
                assert!(error_message.is_none());
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn image_blocks_parse_with_mime_and_data() {
        let e = parse_line(
            r#"{"type":"message_start","message":{"role":"user","content":[{"type":"text","text":"look","index":0},{"type":"image","data":"QUJD","mimeType":"image/png","index":1}]}}"#,
        )
        .unwrap();
        match e {
            Event::MessageStart { blocks, .. } => {
                assert_eq!(blocks.len(), 2);
                assert!(matches!(&blocks[1], Block::Image { mime, data, .. } if mime == "image/png" && data == "QUJD"));
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn custom_message_carries_custom_type_and_display() {
        let e = parse_line(
            r#"{"type":"message_start","message":{"role":"custom","customType":"compaction","content":"summary text","display":true,"timestamp":1790206311858}}"#,
        )
        .unwrap();
        match e {
            Event::MessageStart { role, blocks, custom_type, custom_display, .. } => {
                assert_eq!(role, "custom");
                assert_eq!(custom_type.as_deref(), Some("compaction"));
                assert!(custom_display);
                assert_eq!(blocks, vec![Block::Text { content_index: 0, text: "summary text".into() }]);
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn tool_result_message_carries_is_error_and_tool_call_id() {
        let e = parse_line(
            r#"{"type":"message_start","message":{"role":"toolResult","toolCallId":"tc_1","toolName":"bash","content":[{"type":"text","text":"nope","index":0}],"isError":true,"timestamp":1790206311858}}"#,
        )
        .unwrap();
        match e {
            Event::MessageStart { is_error, tool_call_id, .. } => {
                assert!(is_error);
                assert_eq!(tool_call_id.as_deref(), Some("tc_1"));
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn assistant_message_start_partial_toolcall_uses_partial_json() {
        // REAL record: message_start carries a partial toolCall block
        let e = parse_line(
            r#"{"type":"message_start","message":{"role":"assistant","content":[{"type":"toolCall","id":"call_x","name":"bash","arguments":{},"partialJson":"","index":0}]}}"#,
        )
        .unwrap();
        match e {
            Event::MessageStart { role, blocks, .. } => {
                assert_eq!(role, "assistant");
                assert!(matches!(&blocks[0], Block::ToolCall { name, args, .. } if name == "bash" && args.is_empty()));
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn get_session_stats_parses() {
        let e = parse_line(
            r#"{"type":"response","command":"get_session_stats","success":true,"data":{"tokens":{"input":50000,"output":10000,"cacheRead":40000,"cacheWrite":5000,"total":105000},"cost":0.45,"contextUsage":{"tokens":60000,"contextWindow":200000,"percent":30}}}"#,
        )
        .unwrap();
        match e {
            Event::Response { command, data, .. } => {
                assert_eq!(command, "get_session_stats");
                let st = SessionStats::parse(&data.expect("data"));
                assert_eq!(st.tokens_total, 105000);
                assert_eq!(st.cache_write, 5000);
                assert!((st.cost - 0.45).abs() < 1e-9);
                assert_eq!(st.context_percent, Some(30.0));
                assert_eq!(st.summary(), "ctx 30% \u{b7} $0.45");
                // cacheRead / (input + cacheWrite + cacheRead)
                let hit = st.cache_hit_rate().expect("rate");
                assert!((hit - 40_000. / 95_000.).abs() < 1e-9);
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn get_state_parses_snapshot() {
        let e = parse_line(
            r#"{"type":"response","command":"get_state","success":true,"data":{"model":{"id":"glm-5.3-flash","name":"GLM-5.3-Flash","provider":"glm","contextWindow":200000},"thinkingLevel":"high","isStreaming":false,"sessionId":"abc","messageCount":7,"pendingMessageCount":1}}"#,
        )
        .unwrap();
        match e {
            Event::Response { command, data, .. } => {
                assert_eq!(command, "get_state");
                let st = SessionState::parse(&data.expect("data"));
                assert_eq!(st.model_label().as_deref(), Some("GLM-5.3-Flash"));
                assert_eq!(st.thinking_level.as_deref(), Some("high"));
                assert!(!st.is_streaming);
                assert_eq!(st.message_count, 7);
                assert_eq!(st.pending_message_count, 1);
                assert!(st.session_name.is_none());
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn steer_record_shape() {
        let c = Command::Steer { message: "stop".into(), images: vec![] };
        assert_eq!(c.to_record("s1"), json!({"id":"s1","type":"steer","message":"stop"}));
    }

    #[test]
    fn prompt_with_images_omits_empty_and_includes_present() {
        let c = Command::Prompt { message: "hi".into(), images: vec![] };
        assert_eq!(c.to_record("a"), json!({"id":"a","type":"prompt","message":"hi"}));
        let img = json!({"type":"image","data":"QUJD","mimeType":"image/png"});
        let c = Command::Prompt { message: "hi".into(), images: vec![img.clone()] };
        assert_eq!(
            c.to_record("b"),
            json!({"id":"b","type":"prompt","message":"hi","images":[img]})
        );
    }

    #[test]
    fn set_session_name_record_shape() {
        let c = Command::SetSessionName { name: "my-feature".into() };
        assert_eq!(c.to_record("n1"), json!({"id":"n1","type":"set_session_name","name":"my-feature"}));
    }

    #[test]
    fn follow_up_record_shape_with_images() {
        // pi 1.0 rpc-mode: session.followUp(command.message, command.images)
        let c = Command::FollowUp { message: "queued".into(), images: vec![] };
        assert_eq!(c.to_record("u1"), json!({"id":"u1","type":"follow_up","message":"queued"}));
        let img = json!({"type":"image","data":"QUJD","mimeType":"image/png"});
        let c = Command::FollowUp { message: "queued".into(), images: vec![img.clone()] };
        assert_eq!(
            c.to_record("u2"),
            json!({"id":"u2","type":"follow_up","message":"queued","images":[img]})
        );
    }

    #[test]
    fn fork_record_shape() {
        // wire field is entryId (rpc-types.d.ts fork command)
        let c = Command::Fork { entry_id: "e-42".into() };
        assert_eq!(c.to_record("f1"), json!({"id":"f1","type":"fork","entryId":"e-42"}));
        assert_eq!(Command::GetTree.to_record("t1"), json!({"id":"t1","type":"get_tree"}));
        assert_eq!(Command::Clone.to_record("c1"), json!({"id":"c1","type":"clone"}));
    }

    #[test]
    fn parse_tree_response() {
        // Mirrors pi get_tree: entries wrap {entry:{id,parentId,type,message}, children:[]}
        let data = json!({
            "leafId": "c2",
            "tree": [{
                "entry": {"id":"a1","parentId":null,"type":"message",
                    "message": {"role":"user","content":"fix the login bug"}},
                "children": [
                    {"entry": {"id":"b1","parentId":"a1","type":"message",
                        "message": {"role":"assistant","content":"sure, on it"}},
                        "children": []},
                    {"entry": {"id":"c1","parentId":"a1","type":"message",
                        "message": {"role":"user","content":"actually try tests first",
                            "extra": 1}},
                        "children": [
                            {"entry": {"id":"c2","parentId":"c1","type":"model_change"},
                             "children": []}
                        ]}
                ]
            }]
        });
        let (tree, leaf) = parse_tree(&data);
        assert_eq!(leaf.as_deref(), Some("c2"));
        assert_eq!(tree.len(), 1);
        let root = &tree[0];
        assert_eq!(root.id, "a1");
        assert_eq!(root.role.as_deref(), Some("user"));
        assert_eq!(root.text.as_deref(), Some("fix the login bug"));
        assert_eq!(root.children.len(), 2);
        // model_change entry has no message text
        let c2 = &root.children[1].children[0];
        assert_eq!(c2.entry_type, "model_change");
        assert!(c2.text.is_none());
        // forkable: assistant chain skips to first user message
        assert_eq!(root.children[0].forkable_entry_id(), None); // assistant leaf, no user below
        assert_eq!(root.forkable_entry_id(), Some("a1"));
    }


    /// 回归锁：pi get_tree 的会话树按 children 逐层嵌套（每层 entry+children
    /// ≈ 4~5 层 JSON）。85 条消息的实测树深 95 → 整行超过 serde_json 默认
    /// 128 层上限 → 整条 get_tree 响应被丢弃 → 所有用户消息拿不到 entry id
    /// → 消息下的「新分支」按钮点不动。此处造一条 400 层深的链锁死行为。
    #[test]
    fn deep_tree_line_survives_parse_line() {
        // 400 层递归解析 debug 帧超 Windows 默认 2MB 测试线程栈（bead 0bk）：
        // 测试自抬栈，跑 `cargo test` 的人不需要配 RUST_MIN_STACK。
        let handle = std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(deep_tree_body)
            .expect("spawn deep_tree_body thread");
        if let Err(payload) = handle.join() {
            std::panic::resume_unwind(payload);
        }
    }

    fn deep_tree_body() {
        // 从叶子往根拼：node -> {entry, children:[node]}
        let depth = 400;
        let mut node = String::from(r#"{"entry":{"id":"n0","parentId":null,"type":"message","message":{"role":"user","content":"hi"}},"children":[]}"#);
        for i in 1..depth {
            node = format!(
                r#"{{"entry":{{"id":"n{i}","parentId":"n{}","type":"message","message":{{"role":"assistant","content":"a{i}"}}}},"children":[{node}]}}"#,
                i - 1
            );
        }
        let line = format!(
            r#"{{"id":"t1","type":"response","command":"get_tree","success":true,"data":{{"leafId":"n{leaf}","tree":[{node}]}}}}"#,
            leaf = depth - 1
        );
        // serde_json 的默认上限会在这里直接失败——用同款解析器做对照锁
        assert!(
            serde_json::from_str::<Value>(&line).is_err(),
            "fixture no longer exceeds the default recursion limit"
        );
        let Some(Event::Response {
            command, data, ..
        }) = crate::protocol::parse_line(&line)
        else {
            panic!("deep get_tree line was dropped by parse_line");
        };
        assert_eq!(command, "get_tree");
        let (tree, leaf) = parse_tree(data.as_ref().expect("data"));
        assert_eq!(leaf.as_deref(), Some(format!("n{}", depth - 1).as_str()));
        let mut n = 0;
        let mut cur = &tree[0];
        while !cur.children.is_empty() {
            n += 1;
            cur = &cur.children[0];
        }
        assert_eq!(n, depth - 1, "full chain survived the parse");
        assert_eq!(cur.role.as_deref(), Some("user"));
    }


    /// get_entries → active chain 的 user 锚点。这是「新分支」fork 的输入，
    /// 必须与 pi-web sliceActiveBranch 同语义：沿 parentId 从 leaf 回溯，
    /// 取 user message entry，最旧在前。
    #[test]
    fn active_user_ids_from_flat_entries() {
        let data = json!({
            "leafId": "c2",
            "entries": [
                {"id":"a1","parentId":null,"type":"message","message":{"role":"user","content":"first"}},
                {"id":"m1","parentId":"a1","type":"model_change","modelId":"x"},
                {"id":"b1","parentId":"m1","type":"message","message":{"role":"assistant","content":"ok"}},
                {"id":"c1","parentId":"b1","type":"message","message":{"role":"user","content":"second"}},
                {"id":"c2","parentId":"c1","type":"message","message":{"role":"assistant","content":"done"}},
                // sibling branch off c1 — must NOT appear on the active chain
                {"id":"c1b","parentId":"b1","type":"message","message":{"role":"user","content":"other branch"}}
            ]
        });
        let (entries, leaf) = parse_entries(&data);
        assert_eq!(leaf.as_deref(), Some("c2"));
        assert_eq!(active_user_entry_ids(&entries, leaf.as_deref()), vec!["a1", "c1"]);
        // no leaf → nothing is forkable (empty session)
        assert!(active_user_entry_ids(&entries, None).is_empty());
        // unknown leaf → nothing (not a panic)
        assert!(active_user_entry_ids(&entries, Some("nope")).is_empty());
    }

    /// parentId 环（坏文件）不能把调用方挂死
    #[test]
    fn active_user_ids_survive_parent_cycle() {
        let data = json!({
            "leafId": "x2",
            "entries": [
                {"id":"x1","parentId":"x2","type":"message","message":{"role":"user","content":"a"}},
                {"id":"x2","parentId":"x1","type":"message","message":{"role":"user","content":"b"}}
            ]
        });
        let (entries, leaf) = parse_entries(&data);
        let ids = active_user_entry_ids(&entries, leaf.as_deref());
        assert!(ids.len() <= entries.len());
    }


    #[test]
    fn slash_commands_parse_list() {
        let data = serde_json::json!({"commands":[
            {"name":"fix-tests","description":"Fix failing tests","source":"prompt"},
            {"name":"review","description":"","source":"skill"}
        ]});
        let cmds = SlashCommand::parse_list(&data);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].name, "fix-tests");
        assert_eq!(cmds[0].description, "Fix failing tests");
    }

    #[test]
    fn model_list_parses_and_thinking_record_shape() {
        let data = serde_json::json!({"models":[
            {"id":"glm-5.3-flash","name":"GLM 5.3 Flash","provider":"glm","contextWindow":200000},
            {"id":"m2","name":"M2","provider":"p"}
        ]});
        let models = parse_model_list(&data);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].provider, "glm");
        let c = Command::SetThinkingLevel { level: "high".into() };
        assert_eq!(c.to_record("t1"), json!({"id":"t1","type":"set_thinking_level","level":"high"}));
    }

    #[test]
    fn agent_lifecycle() {
        assert!(matches!(parse_line(r#"{"type":"agent_start"}"#), Some(Event::AgentStart)));
        assert!(matches!(
            parse_line(r#"{"type":"agent_end","messages":[],"willRetry":false}"#),
            Some(Event::AgentEnd { will_retry: false })
        ));
        assert!(matches!(parse_line(r#"{"type":"agent_settled"}"#), Some(Event::AgentSettled)));
    }

    #[test]
    fn extension_ui_requests_are_typed() {
        // select
        let e = parse_line(
            r#"{"type":"extension_ui_request","id":"r1","method":"select","title":"Pick","options":["a","b"],"timeout":5000}"#,
        )
        .unwrap();
        let Event::ExtensionUi(req) = e else { panic!("not extension ui") };
        assert_eq!(req.id, "r1");
        match req.method {
            ExtUiMethod::Select { title, options } => {
                assert_eq!(title, "Pick");
                assert_eq!(options, vec!["a", "b"]);
            }
            _ => panic!("wrong method"),
        }
        // setStatus / setWidget / notify / set_editor_text / confirm / editor / setTitle
        let e = parse_line(
            r#"{"type":"extension_ui_request","id":"r2","method":"setStatus","statusKey":"goal","statusText":"42%"}"#,
        )
        .unwrap();
        assert!(matches!(
            e,
            Event::ExtensionUi(ExtensionUiRequest {
                method: ExtUiMethod::SetStatus { status_key, status_text },
                ..
            }) if status_key == "goal" && status_text.as_deref() == Some("42%")
        ));
        let e = parse_line(
            r#"{"type":"extension_ui_request","id":"r3","method":"setWidget","widgetKey":"plan","widgetLines":["step 1"],"widgetPlacement":"belowEditor"}"#,
        )
        .unwrap();
        assert!(matches!(
            e,
            Event::ExtensionUi(ExtensionUiRequest {
                method: ExtUiMethod::SetWidget { placement, .. },
                ..
            }) if placement.as_deref() == Some("belowEditor")
        ));
        let e = parse_line(
            r#"{"type":"extension_ui_request","id":"r4","method":"notify","message":"done","notifyType":"warning"}"#,
        )
        .unwrap();
        assert!(matches!(
            e,
            Event::ExtensionUi(ExtensionUiRequest {
                method: ExtUiMethod::Notify { notify_type, .. },
                ..
            }) if notify_type.as_deref() == Some("warning")
        ));
        let e = parse_line(
            r#"{"type":"extension_ui_request","id":"r5","method":"input","title":"Name","placeholder":"x"}"#,
        )
        .unwrap();
        assert!(matches!(
            e,
            Event::ExtensionUi(ExtensionUiRequest { method: ExtUiMethod::Input { .. }, .. })
        ));
        let e = parse_line(
            r#"{"type":"extension_ui_request","id":"r6","method":"confirm","title":"Sure?","message":"go"}"#,
        )
        .unwrap();
        assert!(matches!(
            e,
            Event::ExtensionUi(ExtensionUiRequest { method: ExtUiMethod::Confirm { .. }, .. })
        ));
        let e = parse_line(
            r#"{"type":"extension_ui_request","id":"r7","method":"set_editor_text","text":"hi"}"#,
        )
        .unwrap();
        assert!(matches!(
            e,
            Event::ExtensionUi(ExtensionUiRequest { method: ExtUiMethod::SetEditorText { .. }, .. })
        ));
    }

    #[test]
    fn extension_ui_response_record_shape() {
        let c = Command::ExtensionUiResponse {
            id: "r1".into(),
            value: Some("a".into()),
            confirmed: None,
            cancelled: false,
        };
        let v = c.to_record("ignored");
        assert_eq!(v["type"], "extension_ui_response");
        assert_eq!(v["id"], "r1");
        assert_eq!(v["value"], "a");
        assert!(v.get("confirmed").is_none());
        assert!(v.get("cancelled").is_none());
        let cancel = Command::ExtensionUiResponse {
            id: "r2".into(),
            value: None,
            confirmed: Some(true),
            cancelled: true,
        };
        let v = cancel.to_record("ignored");
        assert_eq!(v["confirmed"], true);
        assert_eq!(v["cancelled"], true);
    }
    #[test]
    fn unknown_record_is_unparsed_not_dropped() {
        let e = parse_line(r#"{"type":"future_thing","a":1}"#).unwrap();
        assert!(matches!(e, Event::Unparsed(_)));
    }

    // 实测（pi 1.0.0/1.1.0 RPC，tmp/rpc_probe.js）：每个 run 的 turn_start 后 pi 都
    // 会发 role:"system" 的 message_start/message_end，全量携带 transcript 补丁
    // ——app 靠 raw_system 做 live 重放（新会话第一轮点亮系统提示词面板）。
    #[test]
    fn system_message_start_carries_raw_message() {
        let e = parse_line(
            r#"{"type":"message_start","message":{"role":"system","content":"base",
"sections":{"rules":"R2"},"timestamp":1790000000000,
"toolsAdded":[{"name":"read","description":"d","parameters":{"type":"object"}}]}}"#,
        )
        .unwrap();
        match e {
            Event::MessageStart { role, raw_system, .. } => {
                assert_eq!(role, "system");
                let raw = raw_system.expect("system start carries raw message");
                assert_eq!(raw["sections"]["rules"], "R2");
                assert_eq!(raw["toolsAdded"][0]["name"], "read");
            }
            other => panic!("wrong event: {other:?}"),
        }
        // 非 system 角色不带（省掉每条 user/assistant 消息的整包 clone）
        let e = parse_line(
            r#"{"type":"message_start","message":{"role":"user","content":"hi","timestamp":1}}"#,
        )
        .unwrap();
        match e {
            Event::MessageStart { role, raw_system, .. } => {
                assert_eq!(role, "user");
                assert!(raw_system.is_none());
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn garbage_line_is_none() {
        assert!(parse_line("\x1b]0;pi title\x07").is_none());
    }

    // rpc `bash`：excludeFromContext=true 才带字段（pi 侧 ?? false 默认）
    #[test]
    fn bash_record_shape() {
        let c = Command::Bash { command: "ls -la".into(), exclude_from_context: false };
        assert_eq!(c.to_record("b1"), json!({"id":"b1","type":"bash","command":"ls -la"}));
        let c = Command::Bash { command: "ls".into(), exclude_from_context: true };
        assert_eq!(
            c.to_record("b2"),
            json!({"id":"b2","type":"bash","command":"ls","excludeFromContext":true})
        );
        assert_eq!(Command::AbortBash.to_record("b3"), json!({"id":"b3","type":"abort_bash"}));
    }

    // bash response data = BashResult（rpc-commands.md bash 节实测形态）
    #[test]
    fn bash_response_parses_bash_result() {
        let e = parse_line(
            r#"{"id":"b1","type":"response","command":"bash","success":true,"data":{"output":"file1\nfile2\n","exitCode":0,"cancelled":false,"truncated":false}}"#,
        )
        .unwrap();
        match e {
            Event::Response { command, data, .. } => {
                assert_eq!(command, "bash");
                let r = BashResult::parse(&data.expect("data"));
                assert_eq!(r.output, "file1\nfile2\n");
                assert_eq!(r.exit_code, Some(0));
                assert!(!r.cancelled && !r.truncated);
                assert!(r.full_output_path.is_none());
            }
            other => panic!("wrong event: {other:?}"),
        }
        let e = parse_line(
            r#"{"type":"response","command":"bash","success":true,"data":{"output":"partial","exitCode":null,"cancelled":true,"truncated":true,"fullOutputPath":"/tmp/pi-bash-x.log"}}"#,
        )
        .unwrap();
        match e {
            Event::Response { data, .. } => {
                let r = BashResult::parse(&data.expect("data"));
                assert_eq!(r.exit_code, None);
                assert!(r.cancelled && r.truncated);
                assert_eq!(r.full_output_path.as_deref(), Some("/tmp/pi-bash-x.log"));
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    // 执行中的流式输出增量（agent_session._emit 原样转发，json-event 不改写）
    #[test]
    fn bash_execution_update_event() {
        let e = parse_line(
            r#"{"type":"bash_execution_update","id":"req-9","delta":"hello "}"#,
        )
        .unwrap();
        match e {
            Event::BashExecutionUpdate { id, delta } => {
                assert_eq!(id.as_deref(), Some("req-9"));
                assert_eq!(delta, "hello ");
            }
            other => panic!("wrong event: {other:?}"),
        }
    }
}
