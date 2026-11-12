//! JSONL line parsing entry (`parse_line`/`parse_record`) plus text
//! utilities shared by the wire surface.

use super::blocks::content_blocks;
use super::events::{AssistantEvent, Event, ExtUiMethod, ExtensionUiRequest, Usage};
use serde_json::{Map, Value};

// ---- map 读/取小助手（047-内存削减 P1-1：parse_record 消费所有权，大字段
// ---- args/result/data/raw_system/WireEvent.value 全部 move 不 clone——旧行为
// ---- 解析一条 5MB 工具输出事件要同时持有 line + 整行 Value + clone 出的
// ---- 子树三份，峰值 ~3×；move 化后砍掉最后一层翻倍）----

fn s(map: &Map<String, Value>, k: &str) -> String {
    map.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}
fn os(map: &Map<String, Value>, k: &str) -> Option<String> {
    map.get(k).and_then(Value::as_str).map(str::to_string)
}
fn b(map: &Map<String, Value>, k: &str) -> bool {
    map.get(k).and_then(Value::as_bool).unwrap_or(false)
}
fn opt_b(map: &Map<String, Value>, k: &str) -> Option<bool> {
    map.get(k).and_then(Value::as_bool)
}
fn opt_i(map: &Map<String, Value>, k: &str) -> Option<i64> {
    map.get(k).and_then(Value::as_i64)
}
fn opt_u(map: &Map<String, Value>, k: &str) -> Option<u64> {
    map.get(k).and_then(Value::as_u64)
}
/// move 出子字段；缺失/None 等价。null → None（对应旧 `!v[k].is_null()` 口径）。
fn take_not_null(map: &mut Map<String, Value>, k: &str) -> Option<Value> {
    map.remove(k).filter(|v| !v.is_null())
}
/// move 出嵌套 object；非 object（含缺失/null）一律空表——旧 `v[k][..]`
/// 索引链对非 object 取值恒 Null，行为等价。
fn take_map(map: &mut Map<String, Value>, k: &str) -> Map<String, Value> {
    match map.remove(k) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

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
    Some(parse_record(v))
}

/// 消费所有权版（047-内存削减 P1-1）：旧签名 `(&Value)` 迫使每个大字段
/// `.clone()`（data/args/result/raw_system/WireEvent.value），解析峰值 = 整行
/// Value + clone 子树两份并存。move 化后子树直接摘走。字段语义逐条保持与
/// 旧实现等价（见各臂注释与 wire_parse 测试的 null/missing 区分用例）。
pub fn parse_record(v: Value) -> Event {
    let mut map = match v {
        Value::Object(map) => map,
        // 旧口径：非 object 的 v["type"] 索引恒 Null → 落 Unparsed(v.clone())
        other => return Event::Unparsed(other),
    };
    // type 先拷一份小串（type 键 ~20B，与 Value 树无关）——scrutinee 借用
    // 与臂内 map.remove 的互斥解法
    let ty = map.get("type").and_then(Value::as_str).map(str::to_string);
    match ty.as_deref() {
        Some("response") => Event::Response {
            id: s(&map, "id"),
            command: s(&map, "command"),
            success: b(&map, "success"),
            error: os(&map, "error"),
            // 旧口径 get().cloned()：缺失 → None，null → Some(Null)
            data: map.remove("data"),
        },
        // roles: "system" | "user" | "assistant" | "toolResult" (json.md wire)
        Some("message_start") => {
            let mut msg = take_map(&mut map, "message");
            let role = s(&msg, "role");
            let is_system = role == "system";
            // content 借读不摘：raw_system 必须携带完整 message 本体（含
            // content/sections/toolsAdded——app live 重放的数据源）
            let blocks = content_blocks(msg.get("content").unwrap_or(&Value::Null));
            // system：details 与 raw_system 同源于完整 message——details 保持
            // clone 共存；非 system 无 raw_system，details 直接 move
            let details = if is_system {
                msg.get("details").filter(|d| d.is_object()).cloned()
            } else {
                msg.remove("details").filter(|d| d.is_object())
            };
            Event::MessageStart {
                role,
                blocks,
                timestamp: opt_i(&msg, "timestamp"),
                is_error: b(&msg, "isError"),
                tool_call_id: os(&msg, "toolCallId"),
                custom_type: os(&msg, "customType"),
                custom_display: msg.get("display").and_then(Value::as_bool).unwrap_or(true),
                details,
                // 全量 message 本体 move（含 details/content/sections/toolsAdded）
                raw_system: is_system.then(|| Value::Object(std::mem::take(&mut msg))),
            }
        }
        Some("message_update") => Event::MessageUpdate(AssistantEvent::parse(
            map.get("assistantMessageEvent").unwrap_or(&Value::Null),
        )),
        Some("message_end") => {
            let mut msg = take_map(&mut map, "message");
            let content = msg.remove("content").unwrap_or(Value::Null);
            let blocks = content_blocks(&content);
            Event::MessageEnd {
                role: s(&msg, "role"),
                blocks,
                usage: Usage::parse(msg.get("usage").unwrap_or(&Value::Null)),
                timestamp: opt_i(&msg, "timestamp"),
                stop_reason: os(&msg, "stopReason"),
                error_message: os(&msg, "errorMessage"),
                is_error: b(&msg, "isError"),
                tool_call_id: os(&msg, "toolCallId"),
                custom_type: os(&msg, "customType"),
                custom_display: msg.get("display").and_then(Value::as_bool).unwrap_or(true),
                model: os(&msg, "model"),
            }
        }
        Some("agent_start") => Event::AgentStart,
        Some("agent_end") => Event::AgentEnd {
            will_retry: b(&map, "willRetry"),
        },
        Some("agent_settled") => Event::AgentSettled,
        Some("auto_retry_start") => Event::AutoRetryStart {
            attempt: opt_u(&map, "attempt").unwrap_or(0) as u32,
            max_attempts: opt_u(&map, "maxAttempts").unwrap_or(0) as u32,
            delay_ms: opt_u(&map, "delayMs").unwrap_or(0),
            error_message: os(&map, "errorMessage"),
        },
        Some("auto_retry_end") => Event::AutoRetryEnd {
            success: b(&map, "success"),
            attempt: opt_u(&map, "attempt").unwrap_or(0) as u32,
            final_error: os(&map, "finalError"),
        },
        Some("bash_execution_update") => Event::BashExecutionUpdate {
            id: os(&map, "id"),
            delta: s(&map, "delta"),
        },
        Some("compaction_start") => Event::CompactionStart {
            reason: os(&map, "reason"),
        },
        Some("compaction_end") => Event::CompactionEnd {
            reason: os(&map, "reason"),
            aborted: b(&map, "aborted"),
            will_retry: b(&map, "willRetry"),
            error_message: os(&map, "errorMessage"),
        },
        Some("summarization_retry_scheduled") => Event::SummarizationRetryScheduled {
            attempt: opt_u(&map, "attempt").unwrap_or(0) as u32,
            max_attempts: opt_u(&map, "maxAttempts").unwrap_or(0) as u32,
            delay_ms: opt_u(&map, "delayMs").unwrap_or(0),
            error_message: os(&map, "errorMessage"),
        },
        Some("summarization_retry_attempt_start") => Event::SummarizationRetryAttemptStart {
            source: os(&map, "source"),
        },
        Some("summarization_retry_finished") => Event::SummarizationRetryFinished,
        Some("extension_error") => Event::ExtensionError {
            extension_path: os(&map, "extensionPath"),
            event_name: os(&map, "event"),
            error: os(&map, "error"),
        },
        Some("queue_update") => Event::QueueUpdate {
            steering: string_list(map.get("steering").unwrap_or(&Value::Null)),
            follow_up: string_list(map.get("followUp").unwrap_or(&Value::Null)),
        },
        Some("thinking_level_changed") => Event::ThinkingLevelChanged {
            level: map
                .get("level")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        // 工具执行起收：立正式事件（嵌套调用的完整 result 只在这里出现，
        // 派发账本靠它落 child_id / stop_reason）。args/result 是大字段主
        // 载体——move 直摘
        Some("tool_execution_start") => Event::ToolExecutionStart {
            tool_call_id: s(&map, "toolCallId"),
            tool_name: s(&map, "toolName"),
            parent_tool_call_id: os(&map, "parentToolCallId"),
            args: take_not_null(&mut map, "args"),
        },
        Some("tool_execution_end") => Event::ToolExecutionEnd {
            tool_call_id: s(&map, "toolCallId"),
            tool_name: s(&map, "toolName"),
            parent_tool_call_id: os(&map, "parentToolCallId"),
            is_error: b(&map, "isError"),
            result: take_not_null(&mut map, "result"),
            duration_ms: opt_u(&map, "durationMs"),
        },
        // app 暂不消费的会话事件：按 type 归类为 WireEvent，别落 Unparsed
        Some(kind @ ("tool_execution_update" | "entry_appended" | "session_info_changed")) => {
            Event::WireEvent {
                kind: kind.to_string(),
                value: Value::Object(map),
            }
        }
        Some("extension_ui_request") => Event::ExtensionUi(
            ExtensionUiRequest::parse(&Value::Object(std::mem::take(&mut map))).unwrap_or(
                ExtensionUiRequest {
                    id: String::new(),
                    method: ExtUiMethod::Notify {
                        message: String::new(),
                        notify_type: None,
                    },
                },
            ),
        ),
        _ => Event::Unparsed(Value::Object(map)),
    }
}
