//! Inbound event types: assistant streaming events, extension UI requests,
//! and the top-level [`Event`] record enum.

use super::blocks::Block;
use serde_json::Value;

/// Incremental assistant message events (`assistantMessageEvent` in `message_update`).
///
/// Field names per json.md: `contentIndex`, `delta`, `content`, `id`, `toolName`,
/// `toolCall`. `*_end` variants carry authoritative content and replace the
/// streamed reconstruction.
#[derive(Debug, Clone, PartialEq)]
pub enum AssistantEvent {
    TextDelta {
        content_index: usize,
        delta: String,
    },
    TextEnd {
        content_index: usize,
        content: String,
    },
    ThinkingStart {
        content_index: usize,
    },
    ThinkingDelta {
        content_index: usize,
        delta: String,
    },
    ThinkingEnd {
        content_index: usize,
        content: String,
    },
    ToolCallStart {
        content_index: usize,
        id: String,
        tool_name: String,
    },
    ToolCallDelta {
        content_index: usize,
        delta: String,
    },
    ToolCallEnd {
        content_index: usize,
        tool_call: Value,
    },
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
            Some("thinking_start") => AssistantEvent::ThinkingStart {
                content_index: idx(),
            },
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
            cost: v["cost"]["total"]
                .as_f64()
                .or_else(|| v["cost"].as_f64())
                .unwrap_or(0.0),
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
    Select {
        title: String,
        options: Vec<String>,
    },
    Confirm {
        title: String,
        message: String,
    },
    Input {
        title: String,
        placeholder: Option<String>,
    },
    Editor {
        title: String,
        prefill: Option<String>,
    },
    Notify {
        message: String,
        notify_type: Option<String>,
    },
    SetStatus {
        status_key: String,
        status_text: Option<String>,
    },
    SetWidget {
        widget_key: String,
        widget_lines: Option<Vec<String>>,
        /// "aboveEditor" | "belowEditor" (None -> pi-web default aboveEditor)
        placement: Option<String>,
    },
    SetTitle {
        title: String,
    },
    SetEditorText {
        text: String,
    },
    /// Unrecognized method (pi upgrade adds a new UI surface). Must never be
    /// dropped: the whole point is that an unknown method still gets an
    /// `extension_ui_response` (cancelled) so the extension handler's await
    /// settles instead of blocking forever (#7-shaped deadlock, invisible).
    Unknown {
        method: String,
    },
}

impl ExtUiMethod {
    /// 阻塞原语：扩展 handler 的 promise 挂在 select/confirm/input/editor
    /// 上，pi 停在原地等答案——PF 必须排队留痕并「叫人」（#问题卡不弹）。
    /// 其余（notify/setStatus/setWidget/setTitle/setEditorText/unknown）是
    /// fire-and-forget：发出方不等回执，不得点亮「等你回答」徽标，
    /// 也不得阻止后台运行时退役。
    pub fn is_blocking(&self) -> bool {
        matches!(
            self,
            Self::Select { .. } | Self::Confirm { .. } | Self::Input { .. } | Self::Editor { .. }
        )
    }
}

impl ExtensionUiRequest {
    pub(super) fn parse(v: &Value) -> Option<ExtensionUiRequest> {
        let id = v["id"].as_str()?.to_string();
        let method = match v["method"].as_str()? {
            "select" => ExtUiMethod::Select {
                title: v["title"].as_str().unwrap_or("").to_string(),
                options: v["options"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|o| o.as_str().map(str::to_string))
                            .collect()
                    })
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
                    a.iter()
                        .filter_map(|l| l.as_str().map(str::to_string))
                        .collect()
                }),
                placement: v["widgetPlacement"].as_str().map(str::to_string),
            },
            "setTitle" => ExtUiMethod::SetTitle {
                title: v["title"].as_str().unwrap_or("").to_string(),
            },
            "set_editor_text" => ExtUiMethod::SetEditorText {
                text: v["text"].as_str().unwrap_or("").to_string(),
            },
            // 未知 method 不丢弃（zi2s.2）：原 `_ => return None` 会把整条
            // 请求扔掉且零痕迹，pi 升级新增 UI 方法即永久阻塞扩展 handler。
            // 改回 Unknown 由 app 侧立刻回 cancelled + eprintln 留痕。
            other => ExtUiMethod::Unknown {
                method: other.to_string(),
            },
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
    AgentEnd {
        will_retry: bool,
    },
    /// pi will not continue automatically (retries/queue drained).
    AgentSettled,
    /// `auto_retry_start`：可重试错误（429/5xx/网络断）后 pi 计划自动重试，
    /// 退避 sleep 中——轮**未**结束，agent 语义上仍在跑（isStreaming 持续）。
    AutoRetryStart {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
        error_message: Option<String>,
    },
    /// `auto_retry_end`：重试序列收尾。success=true = 某次重试尝试的助手
    /// 响应已通过（轮内继续）；false = 重试耗尽/被中止，真轮末事件
    /// （agent_end{willRetry:false} + agent_settled）随后就到。
    AutoRetryEnd {
        success: bool,
        attempt: u32,
        final_error: Option<String>,
    },
    /// `bash_execution_update`（rpc `bash` 执行中的流式输出增量；id 关联发起
    /// 的命令，delta 追加到已显示的输出尾部）
    BashExecutionUpdate {
        id: Option<String>,
        delta: String,
    },
    /// Extension UI protocol records (dialogs/widgets/status/notify).
    ExtensionUi(ExtensionUiRequest),
    /// pi-link 合成事件（**非 pi wire**）：侧车进程退出，waiter 线程 reap 到
    /// 的退出码。进程轮中死亡时 app 据此补全轮终语义（此前 EOF 只会让
    /// 事件通道静默关闭，界面无任何轮终通知）。
    ProcessExited {
        code: Option<i64>,
    },
    /// `compaction_start`：上下文自动压缩开始（manual|threshold|overflow）。
    /// 此前落 Unparsed——轮中自动压缩时界面毫无说明地「卡住」。
    CompactionStart {
        reason: Option<String>,
    },
    /// `compaction_end`：压缩收尾。失败原因在 `error_message`（此前看不见）。
    CompactionEnd {
        reason: Option<String>,
        aborted: bool,
        will_retry: bool,
        error_message: Option<String>,
    },
    /// `summarization_retry_scheduled`：上下文溢出后的摘要重试退避（与
    /// auto_retry 同构——退避窗口内轮未结束，isStreaming 持续）。
    SummarizationRetryScheduled {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
        error_message: Option<String>,
    },
    /// `summarization_retry_attempt_start`：一次摘要重试尝试开始。
    SummarizationRetryAttemptStart {
        source: Option<String>,
    },
    /// `summarization_retry_finished`：摘要重试序列收尾。
    SummarizationRetryFinished,
    /// `extension_error`（rpc-mode 对扩展装载/运行错误的输出）。扩展崩溃
    /// 不终止轮，但此前被静默丢弃，状态行毫无痕迹。
    ExtensionError {
        extension_path: Option<String>,
        event_name: Option<String>,
        error: Option<String>,
    },
    /// `queue_update`：pi 侧 steering/followUp 队列变动后的全量快照（文本
    /// 数组，未投递的原文）。此前归 WireEvent 忽略——PF 对 pi 队列零可见，
    /// 「已排队」状态行只能本地猜；abort 撤回（clear_queue）也要靠它对账。
    QueueUpdate {
        steering: Vec<String>,
        follow_up: Vec<String>,
    },
    /// `thinking_level_changed`：思考档**实际**落地后的广播（pi
    /// `setThinkingLevel` 的 level 会被模型能力截断——deepseek-flash 只有
    /// off/low/high/max，请求 `minimal` 真值是 `low`、请求 `medium` 是
    /// `high`）。档位变更是这个事件的唯一权威来源：`get_state` 只在轮末/
    /// 换模型时问一次，且 set_thinking_level 的 response **不带** level。
    ThinkingLevelChanged {
        level: String,
    },
    /// `tool_execution_start`：工具开始执行。`parent_tool_call_id` 非空 =
    /// **嵌套调用**——由另一个工具在运行中发起（codemode 脚本里的
    /// `ctx.executeTool()`，即 `tools.agent`）。嵌套调用在 transcript 里不留
    /// 独立块（父块只带一份有损的 `nestedCalls`），事件流是唯一完整可见处。
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        parent_tool_call_id: Option<String>,
        args: Option<Value>,
    },
    /// `tool_execution_end`：工具执行收尾。嵌套调用的 `result`（含
    /// `details.child_id` / `details.stop_reason`）**只**在这里出现。
    ToolExecutionEnd {
        tool_call_id: String,
        tool_name: String,
        parent_tool_call_id: Option<String>,
        is_error: bool,
        result: Option<Value>,
        duration_ms: Option<u64>,
    },
    /// pi 会话事件中 app 暂不消费的杂项（tool_execution_update /
    /// entry_appended / session_info_changed）：
    /// 解析出 type 免落 [`Event::Unparsed`] 噪声，字段留原 Value。
    WireEvent {
        kind: String,
        value: Value,
    },
    Unparsed(Value),
}
