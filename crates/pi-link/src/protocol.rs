//! Typed pi RPC protocol (JSONL over stdio).
//!
//! Reference: vendored pi `docs/rpc.md`, `docs/json.md`, `docs/rpc-commands.md`.
//! Unknown record shapes are preserved as [`Event::Other`] / raw JSON so a pi
//! upgrade never silently drops data — conformance tests fail loudly instead.

use serde_json::{Value, json};

/// One pi RPC record kind we send. Correlated via `id` where a response is expected.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Prompt { message: String },
    FollowUp { message: String },
    Abort,
    GetState,
    GetMessages,
    SetModel { provider: String, model: String },
}

impl Command {
    pub fn kind(&self) -> &'static str {
        match self {
            Command::Prompt { .. } => "prompt",
            Command::FollowUp { .. } => "follow_up",
            Command::Abort => "abort",
            Command::GetState => "get_state",
            Command::GetMessages => "get_messages",
            Command::SetModel { .. } => "set_model",
        }
    }

    /// Serialize to a single JSONL record (no trailing newline).
    pub fn to_record(&self, id: &str) -> Value {
        let mut v = match self {
            Command::Prompt { message } | Command::FollowUp { message } => {
                json!({ "type": self.kind(), "message": message })
            }
            Command::Abort | Command::GetState | Command::GetMessages => {
                json!({ "type": self.kind() })
            }
            Command::SetModel { provider, model } => {
                json!({ "type": self.kind(), "provider": provider, "model": model })
            }
        };
        v["id"] = json!(id);
        v
    }
}

/// Assistant content block kinds carried by `message.content` arrays.
///
/// Mirrors the wire blocks pi emits (json.md): `text`, `thinking`, `toolcall`.
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
    },
}

impl Block {
    pub fn content_index(&self) -> usize {
        match self {
            Block::Text { content_index, .. }
            | Block::Thinking { content_index, .. }
            | Block::ToolCall { content_index, .. } => *content_index,
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

/// Flatten text blocks only (user echo, quick summaries).
pub fn content_text(content: &Value) -> String {
    content_blocks(content)
        .into_iter()
        .map(|b| match b {
            Block::Text { text, .. } => text,
            _ => String::new(),
        })
        .collect::<Vec<_>>()
        .join("")
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

/// One parsed record from pi's stdout.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Response to a command, correlated by id.
    Response { id: String, command: String, success: bool, error: Option<String> },
    MessageStart { role: String, blocks: Vec<Block> },
    MessageUpdate(AssistantEvent),
    /// Authoritative final message; blocks replace any streamed reconstruction.
    MessageEnd { role: String, blocks: Vec<Block> },
    AgentStart,
    AgentEnd { will_retry: bool },
    /// pi will not continue automatically (retries/queue drained).
    AgentSettled,
    /// Extension UI protocol records (dialogs/widgets/status), raw until M5.
    ExtensionUi(Value),
    Unparsed(Value),
}

/// Parse a single JSONL line from pi's stdout.
pub fn parse_line(line: &str) -> Option<Event> {
    let v: Value = serde_json::from_str(line).ok()?;
    Some(parse_record(&v))
}

pub fn parse_record(v: &Value) -> Event {
    match v["type"].as_str() {
        Some("response") => Event::Response {
            id: v["id"].as_str().unwrap_or("").to_string(),
            command: v["command"].as_str().unwrap_or("").to_string(),
            success: v["success"].as_bool().unwrap_or(false),
            error: v["error"].as_str().map(str::to_string),
        },
        // roles: "system" | "user" | "assistant" | "toolResult" (json.md wire)
        Some("message_start") => Event::MessageStart {
            role: v["message"]["role"].as_str().unwrap_or("").to_string(),
            blocks: content_blocks(&v["message"]["content"]),
        },
        Some("message_update") => {
            Event::MessageUpdate(AssistantEvent::parse(&v["assistantMessageEvent"]))
        }
        Some("message_end") => Event::MessageEnd {
            role: v["message"]["role"].as_str().unwrap_or("").to_string(),
            blocks: content_blocks(&v["message"]["content"]),
        },
        Some("agent_start") => Event::AgentStart,
        Some("agent_end") => Event::AgentEnd {
            will_retry: v["willRetry"].as_bool().unwrap_or(false),
        },
        Some("agent_settled") => Event::AgentSettled,
        Some("extension_ui_request") => Event::ExtensionUi(v.clone()),
        _ => Event::Unparsed(v.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures below are REAL records captured from pi 0.87.1 runs.

    #[test]
    fn prompt_record_shape() {
        let c = Command::Prompt { message: "hi".into() };
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
            json!({"id":"m1","type":"set_model","provider":"glm","model":"glm-5.3-flash"})
        );
    }

    #[test]
    fn response_success() {
        let e = parse_line(r#"{"id":"1","type":"response","command":"prompt","success":true}"#).unwrap();
        match e {
            Event::Response { id, command, success, error } => {
                assert_eq!(
                    (id.as_str(), command.as_str(), success, error.is_none()),
                    ("1", "prompt", true, true)
                );
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
            Event::MessageStart { role, blocks } => {
                assert_eq!(role, "user");
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
            Event::MessageEnd { role, blocks } => {
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
    fn assistant_message_start_partial_toolcall_uses_partial_json() {
        // REAL record: message_start carries a partial toolCall block
        let e = parse_line(
            r#"{"type":"message_start","message":{"role":"assistant","content":[{"type":"toolCall","id":"call_x","name":"bash","arguments":{},"partialJson":"","index":0}]}}"#,
        )
        .unwrap();
        match e {
            Event::MessageStart { role, blocks } => {
                assert_eq!(role, "assistant");
                assert!(matches!(&blocks[0], Block::ToolCall { name, args, .. } if name == "bash" && args.is_empty()));
            }
            other => panic!("wrong event: {other:?}"),
        }
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
    fn extension_ui_is_preserved_raw() {
        let e = parse_line(
            r#"{"type":"extension_ui_request","id":"x","method":"setStatus","statusKey":"goal"}"#,
        )
        .unwrap();
        assert!(matches!(e, Event::ExtensionUi(_)));
    }

    #[test]
    fn unknown_record_is_unparsed_not_dropped() {
        let e = parse_line(r#"{"type":"future_thing","a":1}"#).unwrap();
        assert!(matches!(e, Event::Unparsed(_)));
    }

    #[test]
    fn garbage_line_is_none() {
        assert!(parse_line("\x1b]0;pi title\x07").is_none());
    }
}
