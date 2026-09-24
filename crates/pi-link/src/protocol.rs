//! Typed pi RPC protocol (JSONL over stdio).
//!
//! Reference: vendored pi `docs/rpc.md`, `docs/json.md`, `docs/rpc-commands.md`.
//! Unknown record shapes are preserved as [`Event::Other`] / raw JSON so a pi
//! upgrade never silently drops data — conformance tests fail loudly instead.

use serde::{Deserialize, Serialize};
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
        // abort/set_model responses are correlated too; always attach an id
        v["id"] = json!(id);
        v
    }
}

/// A content block as it appears in `message.content` arrays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    Thinking { thinking: String },
    #[serde(rename_all = "camelCase")]
    ToolCall { id: Option<String>, name: Option<String>, arguments: Option<Value> },
    Image,
    Other,
}

/// Flatten `content: string | ContentBlock[]` into plain text (per json.md).
pub fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| match block_kind(b) {
                BlockKind::Text => b["text"].as_str().map(str::to_string),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

enum BlockKind {
    Text,
    Other,
}

fn block_kind(b: &Value) -> BlockKind {
    match b["type"].as_str() {
        Some("text") => BlockKind::Text,
        _ => BlockKind::Other,
    }
}

/// Incremental assistant message update events (`assistantMessageEvent`).
#[derive(Debug, Clone, PartialEq)]
pub enum AssistantEvent {
    /// append text to block `content_index`
    TextDelta { content_index: usize, delta: String },
    TextEnd { content_index: usize },
    ThinkingDelta { delta: String },
    ToolCallStart { name: String },
    ToolCallEnd { name: String },
    Other(Value),
}

impl AssistantEvent {
    pub fn parse(v: &Value) -> AssistantEvent {
        match v["type"].as_str() {
            Some("text_delta") => AssistantEvent::TextDelta {
                content_index: v["contentIndex"].as_u64().unwrap_or(0) as usize,
                delta: v["delta"].as_str().unwrap_or("").to_string(),
            },
            Some("text_end") => AssistantEvent::TextEnd {
                content_index: v["contentIndex"].as_u64().unwrap_or(0) as usize,
            },
            Some("thinking_delta") => AssistantEvent::ThinkingDelta {
                delta: v["delta"].as_str().unwrap_or("").to_string(),
            },
            Some("toolcall_start") => AssistantEvent::ToolCallStart {
                name: v["name"].as_str().unwrap_or("").to_string(),
            },
            Some("toolcall_end") => AssistantEvent::ToolCallEnd {
                name: v["name"].as_str().unwrap_or("").to_string(),
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
    MessageStart { role: String, text: String },
    MessageUpdate(AssistantEvent),
    /// Authoritative final message for a role.
    MessageEnd { role: String, text: String },
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
        Some("message_start") => Event::MessageStart {
            role: v["message"]["role"].as_str().unwrap_or("").to_string(),
            text: content_text(&v["message"]["content"]),
        },
        Some("message_update") => Event::MessageUpdate(AssistantEvent::parse(&v["assistantMessageEvent"])),
        Some("message_end") => Event::MessageEnd {
            role: v["message"]["role"].as_str().unwrap_or("").to_string(),
            text: content_text(&v["message"]["content"]),
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
                assert_eq!((id.as_str(), command.as_str(), success, error.is_none()), ("1", "prompt", true, true));
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
            Event::MessageStart { role, text } => {
                assert_eq!(role, "user");
                assert_eq!(text, "say OK");
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
