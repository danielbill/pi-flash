//! Transcript system prompt + tool declarations.
//!
//! pi 0.86+ moved both out of `get_state`: every run appends a
//! `role:"system"` message to the transcript carrying `content` (appended to
//! the base prompt), `sections` (patched by name, `null` deletes),
//! `toolsAdded` / `toolsRemoved` (declaration deltas). pi's own state getter
//! replays those messages (`@earendil-works/pi-ai`
//! `utils/transcript.js` + `utils/text.js`), and that replay is the only
//! supported read — the CLI RPC exposes neither `systemPrompt` nor
//! `get_tools`.
//!
//! Both `get_messages` payloads and session-file entries feed this module:
//! entries may be raw messages or whole session entries (`{message: …}`).

use serde_json::Value;

/// One tool declaration as it appears in the transcript (`toolsAdded`).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDecl {
    pub name: String,
    pub description: String,
    /// JSON Schema as declared to the model (may be `Null` when absent).
    pub parameters: Value,
}

/// Current system prompt text + declared tools of one transcript.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TranscriptSystem {
    pub prompt: String,
    pub tools: Vec<ToolDecl>,
}

/// Text of a `content` value: a string, or the concatenated text blocks of an
/// array (pi-ai `contentText`).
fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter(|b| b["type"].as_str() == Some("text"))
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Replay a transcript's system messages into the current system prompt and
/// tool set.
///
/// Mirrors pi-ai's `getCurrentSystemMessage` / `getSystemMessageText`:
///
/// - prompt = non-empty `content` parts followed by every live section value,
///   joined `\n\n` (section order = first-insert order, hence serde_json's
///   `preserve_order`)
/// - tools = replay `toolsRemoved` then `toolsAdded` over an insertion-ordered
///   map (re-declaring a name keeps its slot, exactly like a JS `Map.set`)
///
/// Returns `None` when the transcript carries no system message at all, so
/// callers can tell "not loaded" from "empty prompt".
pub fn transcript_system(entries: &[Value]) -> Option<TranscriptSystem> {
    let mut parts: Vec<String> = Vec::new();
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut tools: Vec<ToolDecl> = Vec::new();
    let mut seen_system = false;

    let tool_of = |v: &Value| -> Option<ToolDecl> {
        let name = v["name"].as_str()?.to_string();
        Some(ToolDecl {
            name,
            description: v["description"].as_str().unwrap_or("").to_string(),
            parameters: v.get("parameters").cloned().unwrap_or(Value::Null),
        })
    };

    for entry in entries {
        let m = entry.get("message").unwrap_or(entry);
        if m["role"].as_str() != Some("system") {
            continue;
        }
        seen_system = true;

        let text = content_text(&m["content"]);
        if !text.is_empty() {
            parts.push(text);
        }
        if let Some(patch) = m["sections"].as_object() {
            for (name, value) in patch {
                if value.is_null() {
                    sections.retain(|(n, _)| n != name);
                    continue;
                }
                let text = content_text(value);
                match sections.iter_mut().find(|(n, _)| n == name) {
                    Some(slot) => slot.1 = text,
                    None => sections.push((name.clone(), text)),
                }
            }
        }
        for removed in m["toolsRemoved"].as_array().into_iter().flatten() {
            if let Some(name) = removed["name"].as_str() {
                tools.retain(|t| t.name != name);
            }
        }
        for added in m["toolsAdded"].as_array().into_iter().flatten() {
            let Some(decl) = tool_of(added) else { continue };
            match tools.iter_mut().find(|t| t.name == decl.name) {
                Some(slot) => *slot = decl,
                None => tools.push(decl),
            }
        }
    }

    if !seen_system {
        return None;
    }
    parts.extend(sections.into_iter().map(|(_, text)| text));
    Some(TranscriptSystem {
        prompt: parts
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        tools,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// pi-ai transcript.js/text.js parity: sections patch by name, prompts
    /// append, tools replay add/remove, order follows first insertion.
    #[test]
    fn replays_sections_and_tools() {
        let entries = vec![
            json!({"role":"user","content":"hi"}),
            json!({"role":"system","content":"base",
                "sections":{"preamble":"P1","rules":"R1","tools":"T1"},
                "toolsAdded":[
                    {"name":"read","description":"read a file","parameters":{"type":"object"}},
                    {"name":"bash","description":"run","parameters":{"type":"object"}}
                ]}),
            json!({"role":"system","sections":{"rules":"R2","cwd":"/x","tools":null},
                "toolsRemoved":[{"name":"bash"}],
                "toolsAdded":[{"name":"edit","description":"edit","parameters":{"type":"object"}}]}),
        ];
        let sys = transcript_system(&entries).expect("has system message");
        // content + live sections in first-insert order; "tools" section deleted,
        // "cwd" appended, "rules" patched in place.
        assert_eq!(sys.prompt, "base\n\nP1\n\nR2\n\n/x");
        let names: Vec<&str> = sys.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["read", "edit"]);
        assert_eq!(sys.tools[0].description, "read a file");
    }

    /// Re-declaring a name replaces the definition but keeps its slot (JS Map.set).
    #[test]
    fn tool_redeclaration_keeps_slot() {
        let entries = vec![
            json!({"role":"system","toolsAdded":[{"name":"a","description":"1"},{"name":"b","description":"2"}]}),
            json!({"role":"system","toolsAdded":[{"name":"a","description":"1b"}]}),
        ];
        let sys = transcript_system(&entries).expect("has system message");
        let names: Vec<&str> = sys.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(sys.tools[0].description, "1b");
        assert_eq!(sys.tools[1].description, "2");
    }

    /// Whole session entries (`{message:{…}}`) parse the same as raw messages,
    /// and a transcript without any system message reports None (≠ empty prompt).
    #[test]
    fn accepts_entries_and_reports_absent() {
        let entries = vec![
            json!({"type":"message","id":"a","message":{"role":"system","sections":{"a":"A"}}}),
            json!({"type":"message","id":"b","message":{"role":"user","content":"x"}}),
        ];
        assert_eq!(transcript_system(&entries).expect("sys").prompt, "A");
        assert!(transcript_system(&[json!({"role":"user","content":"x"})]).is_none());
    }
}
