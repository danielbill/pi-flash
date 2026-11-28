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

// ---- bd pi-flash-sdja：compaction 卡截停 + 压缩后窗口近似估算 ----

/// compaction 卡（wire 形状，read.rs renderable_message 同款）。
fn compaction_card(summary: &str) -> serde_json::Value {
    json!({"type":"message","message":{"role":"custom","customType":"compaction","content":summary}})
}

#[test]
fn compaction_card_stops_actual_lookup() {
    let msgs = msgs_from_tail(vec![
        json!({"type":"message","message":{"role":"user","content":"hi"}}),
        assistant(
            json!({"input":100,"output":5,"cacheRead":900,"cacheWrite":0}),
            None,
        ),
        compaction_card("摘要"),
    ]);
    assert_eq!(
        last_actual_context_tokens(&msgs),
        None,
        "卡之前的 usage 是压缩前口径，不可信（内核 getContextUsage 同款判定）"
    );
}

#[test]
fn post_compaction_assistant_is_trusted() {
    let msgs = msgs_from_tail(vec![
        assistant(
            json!({"input":100,"output":5,"cacheRead":900,"cacheWrite":0}),
            None,
        ),
        compaction_card("摘要"),
        json!({"type":"message","message":{"role":"user","content":"继续"}}),
        assistant(
            json!({"input":40,"output":9,"cacheRead":800,"cacheWrite":0}),
            None,
        ),
    ]);
    assert_eq!(
        last_actual_context_tokens(&msgs),
        Some(840),
        "卡之后的响应才可信"
    );
}

#[test]
fn projected_estimate_scopes_to_compaction_card_and_onward() {
    let msgs = msgs_from_tail(vec![
        json!({"type":"message","message":{"role":"user","content":"压缩前的大正文"}}),
        assistant(
            json!({"input":10,"output":1,"cacheRead":0,"cacheWrite":0}),
            None,
        ),
        compaction_card("摘要文本"),
        json!({"type":"message","message":{"role":"user","content":"abc"}}),
    ]);
    // 卡（含）之后 = 摘要文本（4 CJK = 4）+ "abc"（3 ASCII → 0）；卡前不计
    assert_eq!(projected_context_tokens(&msgs, None), Some(4));
}

#[test]
fn projected_estimate_without_card_uses_all_messages() {
    let msgs = msgs_from_tail(vec![
        json!({"type":"message","message":{"role":"user","content":"你好"}}),
        assistant(
            json!({"input":10,"output":1,"cacheRead":0,"cacheWrite":0}),
            None,
        ),
    ]);
    // 无卡 → 全量："你好"(2 CJK) + "好"(1 CJK) = 3
    assert_eq!(projected_context_tokens(&msgs, None), Some(3));
}

#[test]
fn projected_estimate_counts_tool_args_and_result() {
    let msgs = msgs_from_tail(vec![
        json!({"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"x"},{"type":"toolCall","id":"c1","name":"read","arguments":{"path":"a.txt"}}]}}),
        json!({"type":"message","message":{"role":"toolResult","toolCallId":"c1","content":[{"type":"text","text":"工具结果"}]}}),
    ]);
    // ToolCall 的 arguments JSON + 合并进 result 的工具结果都要折算
    let est = projected_context_tokens(&msgs, None).unwrap();
    assert!(est >= 8, "args JSON + 工具结果文本计入估算（得到 {est}）");
}

#[test]
fn projected_estimate_empty_is_none() {
    assert_eq!(projected_context_tokens(&[], None), None);
}
