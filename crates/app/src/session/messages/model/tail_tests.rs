use super::*;
use serde_json::json;

#[test]
fn converts_user_assistant_and_merges_toolresult() {
    let vals = vec![
        json!({"type":"message","timestamp":"2026-09-27T01:02:03.456Z","message":{"role":"user","timestamp":1790470923456i64,"content":"查一下"}}),
        json!({"type":"message","message":{"role":"assistant","content":[
                {"type":"thinking","text":"想"},
                {"type":"toolCall","id":"t1","name":"web","arguments":{"q":"x"},"result":""}
            ],
            "usage":{"input":10,"output":5,"cache_read":0,"cost":0.01}}}),
        json!({"type":"message","message":{"role":"toolResult","content":[
                {"type":"text","text":"结果内容"}]}}),
        json!({"type":"message","message":{"role":"system","content":"skip"}}),
    ];
    let msgs = msgs_from_tail(vals);
    assert_eq!(msgs.len(), 2, "toolResult merges; system skipped");
    assert_eq!(msgs[0].role, Role::User);
    // message-object timestamp wins over the entry ISO stamp
    assert_eq!(msgs[0].ts, Some(1790470923456));
    assert_eq!(msgs[1].role, Role::Assistant);
    let usage = msgs[1].usage.as_ref().expect("assistant usage kept");
    assert_eq!((usage.input, usage.output), (10, 5));
    let merged = msgs[1]
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::ToolCall { result, .. } => Some(result.as_str()),
            _ => None,
        })
        .expect("toolcall present");
    assert_eq!(merged, "结果内容");
}

#[test]
fn iso_entry_timestamp_falls_back() {
    let vals = vec![
        json!({"type":"message","timestamp":"2026-09-27T01:02:03.456Z","message":{"role":"user","content":"hi"}}),
    ];
    let msgs = msgs_from_tail(vals);
    assert_eq!(msgs[0].ts, Some(1790470923456));
}
