//! JSONL line parsing entry (`parse_line`/`parse_record`) plus text
//! utilities shared by the wire surface.

use super::blocks::content_blocks;
use super::events::{AssistantEvent, Event, ExtUiMethod, ExtensionUiRequest, Usage};
use serde_json::Value;

/// Extract an array of plain strings (`queue_update` steering/followUp 快照)
pub(super) fn string_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// `prompt` 成功 response 的 `disposition`（pi agent-session.js `preflightResult`，
/// rpc-mode.js `output(success(id, "prompt", { disposition }))`）：
/// - `"handled"`：扩展命令 / 输入 handler 吃掉了输入，**不开 run**——wire 上
///   只有命令自己的 UI 事件（extension_ui_request / extension_error），没有
///   agent_start / 同文回显 / agent_settled（PF #7 卡等待行的根因）；
/// - `"queued"`：isStreaming，消息入 steer/followUp 队列（回显下一轮边界才到）；
/// - `"started"`：run 立即开跑（response 从 run 启动回调发出）。
pub fn prompt_disposition(data: &Value) -> Option<&'static str> {
    match data["disposition"].as_str() {
        Some("handled") => Some("handled"),
        Some("queued") => Some("queued"),
        Some("started") => Some("started"),
        _ => None,
    }
}

/// Strip ANSI escape sequences from extension text (zi2s.8，pi-web
/// PlainTextTheme parity)。rpc-mode 的扩展拿到的是真彩色主题，setStatus/
/// setWidget 文本常带 SGR 序列（wire 实测 `statusText="\x1b[38;5;5m…\x1b[39m"`），
/// 原样渲染就是 `[38;5;5m` 之类的碎渣。剥 CSI 序列（ESC [ 参数 终字节）与
/// OSC（ESC ] … BEL/ST）；普通控制字符保留（\n 是合法的 widget 换行）。
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'[' => {
                    // CSI：跳过参数/中间字节（0x30-0x3F / 0x20-0x2F），吃掉终字节
                    let mut j = i + 2;
                    while j < bytes.len() && (0x20..=0x3f).contains(&bytes[j]) {
                        j += 1;
                    }
                    if j < bytes.len() && (0x40..=0x7e).contains(&bytes[j]) {
                        i = j + 1;
                        continue;
                    }
                    // 残缺序列：原样保留（宁可用乱码也不吞文本）
                }
                b']' => {
                    // OSC：BEL (0x07) 或 ST (ESC \) 结尾
                    let mut j = i + 2;
                    while j < bytes.len() {
                        if bytes[j] == 0x07 {
                            i = j + 1;
                            break;
                        }
                        if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                            i = j + 2;
                            break;
                        }
                        j += 1;
                    }
                    if j >= bytes.len() {
                        i = bytes.len(); // 未闭合的 OSC 尾巴整段丢弃
                    }
                    continue;
                }
                _ => {
                    // 两字节转义（ESC 7 / ESC ( B 等）：跳过
                    i += 2;
                    continue;
                }
            }
        }
        // 手动按字节推进，UTF-8 多字节原样拷贝
        let ch_len = utf8_len(bytes[i]);
        out.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn utf8_len(b: u8) -> usize {
    match b {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1, // 非法首字节：当 1 字节走（UTF-8 合法输入不会到这里）
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
            details: v["message"]["details"]
                .as_object()
                .map(|_| v["message"]["details"].clone()),
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
        Some("auto_retry_start") => Event::AutoRetryStart {
            attempt: v["attempt"].as_u64().unwrap_or(0) as u32,
            max_attempts: v["maxAttempts"].as_u64().unwrap_or(0) as u32,
            delay_ms: v["delayMs"].as_u64().unwrap_or(0),
            error_message: v["errorMessage"].as_str().map(str::to_string),
        },
        Some("auto_retry_end") => Event::AutoRetryEnd {
            success: v["success"].as_bool().unwrap_or(false),
            attempt: v["attempt"].as_u64().unwrap_or(0) as u32,
            final_error: v["finalError"].as_str().map(str::to_string),
        },
        Some("bash_execution_update") => Event::BashExecutionUpdate {
            id: v["id"].as_str().map(str::to_string),
            delta: v["delta"].as_str().unwrap_or("").to_string(),
        },
        Some("compaction_start") => Event::CompactionStart {
            reason: v["reason"].as_str().map(str::to_string),
        },
        Some("compaction_end") => Event::CompactionEnd {
            reason: v["reason"].as_str().map(str::to_string),
            aborted: v["aborted"].as_bool().unwrap_or(false),
            will_retry: v["willRetry"].as_bool().unwrap_or(false),
            error_message: v["errorMessage"].as_str().map(str::to_string),
        },
        Some("summarization_retry_scheduled") => Event::SummarizationRetryScheduled {
            attempt: v["attempt"].as_u64().unwrap_or(0) as u32,
            max_attempts: v["maxAttempts"].as_u64().unwrap_or(0) as u32,
            delay_ms: v["delayMs"].as_u64().unwrap_or(0),
            error_message: v["errorMessage"].as_str().map(str::to_string),
        },
        Some("summarization_retry_attempt_start") => Event::SummarizationRetryAttemptStart {
            source: v["source"].as_str().map(str::to_string),
        },
        Some("summarization_retry_finished") => Event::SummarizationRetryFinished,
        Some("extension_error") => Event::ExtensionError {
            extension_path: v["extensionPath"].as_str().map(str::to_string),
            event_name: v["event"].as_str().map(str::to_string),
            error: v["error"].as_str().map(str::to_string),
        },
        Some("queue_update") => Event::QueueUpdate {
            steering: string_list(&v["steering"]),
            follow_up: string_list(&v["followUp"]),
        },
        Some("thinking_level_changed") => Event::ThinkingLevelChanged {
            level: v["level"].as_str().unwrap_or_default().to_string(),
        },
        // app 暂不消费的会话事件：按 type 归类为 WireEvent，别落 Unparsed
        Some(
            "tool_execution_start"
            | "tool_execution_update"
            | "tool_execution_end"
            | "entry_appended"
            | "session_info_changed",
        ) => Event::WireEvent {
            kind: v["type"].as_str().unwrap_or("").to_string(),
            value: v.clone(),
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
