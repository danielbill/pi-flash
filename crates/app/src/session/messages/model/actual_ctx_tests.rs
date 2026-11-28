use super::*;
use serde_json::json;

/// 用 tail 解码夹具铸 assistant 消息（usage 走 wire 字段名 cacheRead/cacheWrite）。
fn assistant(usage: serde_json::Value, stop: Option<&str>) -> serde_json::Value {
    let mut m = json!({"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"好"}],"usage":usage}});
    if let Some(s) = stop {
        m["message"]["stopReason"] = json!(s);
    }
    m
}

#[test]
fn sums_last_completed_request_prompt() {
    let msgs = msgs_from_tail(vec![
        json!({"type":"message","message":{"role":"user","content":"hi"}}),
        assistant(
            json!({"input":10,"output":5,"cacheRead":100,"cacheWrite":20}),
            None,
        ),
        json!({"type":"message","message":{"role":"user","content":"更多"}}),
        assistant(
            json!({"input":48,"output":9,"cacheRead":800,"cacheWrite":0}),
            None,
        ),
    ]);
    // 最后一条 assistant：48+800+0 = 848（output 不计入请求 prompt）
    assert_eq!(last_actual_context_tokens(&msgs), Some(848));
}

#[test]
fn trailing_user_messages_dont_block_last_assistant() {
    let msgs = msgs_from_tail(vec![
        json!({"type":"message","message":{"role":"user","content":"hi"}}),
        assistant(
            json!({"input":10,"output":5,"cacheRead":100,"cacheWrite":20}),
            None,
        ),
        json!({"type":"message","message":{"role":"user","content":"草稿中"}}),
    ]);
    assert_eq!(last_actual_context_tokens(&msgs), Some(130));
}

#[test]
fn skips_aborted_and_error_falls_back_to_previous() {
    let msgs = msgs_from_tail(vec![
        json!({"type":"message","message":{"role":"user","content":"hi"}}),
        assistant(
            json!({"input":10,"output":5,"cacheRead":100,"cacheWrite":20}),
            None,
        ),
        assistant(
            json!({"input":999,"output":0,"cacheRead":0,"cacheWrite":0}),
            Some("aborted"),
        ),
        assistant(
            json!({"input":7,"output":0,"cacheRead":0,"cacheWrite":0}),
            Some("error"),
        ),
    ]);
    assert_eq!(
        last_actual_context_tokens(&msgs),
        Some(130),
        "跳过 aborted/error"
    );
}

#[test]
fn skips_zero_usage() {
    let msgs = msgs_from_tail(vec![
        assistant(
            json!({"input":10,"output":5,"cacheRead":100,"cacheWrite":20}),
            None,
        ),
        assistant(
            json!({"input":0,"output":0,"cacheRead":0,"cacheWrite":0}),
            None,
        ),
    ]);
    assert_eq!(
        last_actual_context_tokens(&msgs),
        Some(130),
        "全零 usage 不可信"
    );
}

#[test]
fn no_valid_assistant_returns_none() {
    assert_eq!(last_actual_context_tokens(&[]), None);
    let user_only = msgs_from_tail(vec![json!({
        "type":"message","message":{"role":"user","content":"hi"}
    })]);
    assert_eq!(last_actual_context_tokens(&user_only), None);
    let no_usage = msgs_from_tail(vec![json!({
        "type":"message",
        "message":{"role":"assistant","content":[{"type":"text","text":"无 usage"}]}
    })]);
    assert_eq!(last_actual_context_tokens(&no_usage), None);
}
