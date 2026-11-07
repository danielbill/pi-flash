use super::*;
use serde_json::json;

fn assistant_with_calls(ids: &[&str]) -> Msg {
    Msg {
        role: Role::Assistant,
        blocks: ids
            .iter()
            .map(|id| Block::ToolCall {
                content_index: 0,
                id: (*id).into(),
                name: "agent".into(),
                args: "{}".into(),
                result: String::new(),
                is_error: false,
                images: Vec::new(),
                duration_s: None,
                details: None,
                args_partial: false,
                result_arrived: false,
            })
            .collect(),
        usage: None,
        entry_id: None,
        ts: None,
        end_ts: None,
        stop_reason: None,
        error_message: None,
        custom_type: None,
        custom_display: true,
        details: None,
        model: None,
        bash: None,
    }
}

fn details_of<'a>(m: &'a Msg, id: &str) -> Option<&'a serde_json::Value> {
    m.blocks.iter().find_map(|b| match b {
        Block::ToolCall {
            id: bid, details, ..
        } if bid == id => details.as_ref(),
        _ => None,
    })
}

// 044-0 B3②：委派被拒的失败 details 随 tool_execution_update 即时进块
#[test]
fn update_lands_details_on_the_matching_call() {
    let mut m = assistant_with_calls(&["t1", "t2"]);
    merge_tool_update(
        &mut m,
        "t2",
        json!({"status":"failed","error_code":"CAPACITY_REACHED"}),
    );
    assert!(details_of(&m, "t1").is_none(), "无 id 匹配不动别的块");
    assert_eq!(
        details_of(&m, "t2"),
        Some(&json!({"status":"failed","error_code":"CAPACITY_REACHED"}))
    );
}

#[test]
fn update_leaves_result_and_arrival_untouched() {
    let mut m = assistant_with_calls(&["t1"]);
    merge_tool_update(&mut m, "t1", json!({"status":"running"}));
    match m.blocks.first() {
        Some(Block::ToolCall {
            result,
            result_arrived,
            details,
            ..
        }) => {
            assert!(result.is_empty());
            assert!(!result_arrived);
            assert_eq!(details.as_ref(), Some(&json!({"status":"running"})));
        }
        _ => panic!("toolcall missing"),
    }
    // 终态 details 由 merge_tool_result 覆盖（update 是临时态）
    merge_tool_result(
        &mut m,
        Some("t1"),
        false,
        "done",
        Vec::new(),
        Some(json!({"stop_reason":"completed"})),
        None,
    );
    assert_eq!(
        details_of(&m, "t1"),
        Some(&json!({"stop_reason":"completed"}))
    );
}

#[test]
fn update_needs_an_id_match() {
    let mut m = assistant_with_calls(&["t1"]);
    merge_tool_update(&mut m, "ghost", json!({"status":"running"}));
    assert!(
        details_of(&m, "t1").is_none(),
        "不匹配的 update 丢弃，不盲写最后一块"
    );
    // 非 assistant 消息直接忽略
    let mut u = assistant_with_calls(&["t1"]);
    u.role = Role::User;
    merge_tool_update(&mut u, "t1", json!({"status":"running"}));
    assert!(details_of(&u, "t1").is_none());
}
