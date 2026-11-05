//! Outbound client -> pi commands (one JSONL record each; `to_record`).

use serde_json::{Value, json};

/// One pi RPC record kind we send. Correlated via `id` where a response is expected.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Prompt {
        message: String,
        /// image contents: [{"type":"image","data":<b64>,"mimeType":...}]
        images: Vec<Value>,
        /// pi-web parity（rpc-manager.ts:800 恒带 streamingBehavior）：排队与否
        /// 由 pi 以自身 isStreaming **原子**判定——在跑→入队（等价旧裸
        /// follow_up/steer），空闲→立即开跑。裸 `follow_up` RPC 命令不做这个
        /// 判定（`_queueUserInput` 无 isStreaming 检查，且入队前还要 await
        /// 扩展 handler）：PF 猜「在跑」而 pi 恰好收尾时，消息滞留在空闲
        /// agent 的 followUpQueue 里，直到**下一轮跑完**才被冲出来——真机
        /// 表现就是「agent 工作时排队的内容几分钟后重复发送」（2026-10-17
        /// 会话 01a1498d 实证：排队消息落在下一轮 assistant 终条同一秒）。
        streaming_behavior: Option<String>,
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
    /// Clear pi's pending steering/followUp queues (rpc `clear_queue`)。
    /// pi 的 `Abort` 只掐当前 run **不清队列**——已排队未投递的消息会滞留到
    /// 下一轮跑完时被 would-stop drain 冲出来当新轮执行（真机表现「停止后
    /// 消息又自己执行了一遍」，2026-10-19 会话 01a1564b 实证）。abort 时必须
    /// 配对发本命令撤回队列；response data 携带被撤回的原文
    /// `{steering:[text..], followUp:[text..]}`，供 app 回填输入框
    /// （AgentSession.clearQueue 的设计语义就是 "restoring to editor when
    /// user aborts"，pi-web parity）。
    ClearQueue,
    /// Summarize/compact the session context (rpc compact)
    Compact,
    GetState,
    GetMessages,
    GetSessionStats,
    SetModel {
        provider: String,
        model: String,
    },
    SetSessionName {
        name: String,
    },
    /// Toggle pi's auto-compaction (rpc `set_auto_compaction`；zi2s.5
    /// /auto-compact 用。pi-web 对齐：get_state 不回显该值，本地跟踪）。
    SetAutoCompaction {
        enabled: bool,
    },
    GetCommands,
    GetAvailableModels,
    ExportHtml,
    SetThinkingLevel {
        level: String,
    },
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
    Fork {
        entry_id: String,
    },
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
            Command::ClearQueue => "clear_queue",
            Command::Compact => "compact",
            Command::GetState => "get_state",
            Command::GetMessages => "get_messages",
            Command::GetSessionStats => "get_session_stats",
            Command::SetModel { .. } => "set_model",
            Command::SetSessionName { .. } => "set_session_name",
            Command::SetAutoCompaction { .. } => "set_auto_compaction",
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
        if let Command::ExtensionUiResponse {
            id,
            value,
            confirmed,
            cancelled,
        } = self
        {
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
            Command::Prompt {
                message,
                images,
                streaming_behavior,
            } => {
                let mut v = json!({ "type": self.kind(), "message": message });
                if !images.is_empty() {
                    v["images"] = json!(images);
                }
                if let Some(b) = streaming_behavior {
                    v["streamingBehavior"] = json!(b);
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
            | Command::ClearQueue
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
            Command::SetAutoCompaction { enabled } => {
                json!({ "type": self.kind(), "enabled": enabled })
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
