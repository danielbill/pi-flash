use super::*;
use serde_json::json;

// 044-0 B4：settled 自定义条目/ completion 消息 → 卡片终态。真机症状（用户
// 2026-11-08）：子代理已完成、toast 已弹，卡片因只认 completion 消息（父会话
// 流式中被 steer 排队）而一直转圈，要等父 agent 下一轮才刷。

fn running_card(child_id: &str) -> Msg {
    Msg {
        role: Role::Assistant,
        blocks: vec![Block::ToolCall {
            content_index: 0,
            id: "t1".into(),
            name: "agent".into(),
            args: "{}".into(),
            result: String::new(),
            is_error: false,
            images: Vec::new(),
            duration_s: None,
            details: Some(json!({
                "child_id": child_id,
                "status": "started",
                "label": "台积电能力调研",
            })),
            args_partial: false,
            result_arrived: false,
        }],
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

fn user_msg() -> Msg {
    Msg {
        role: Role::User,
        blocks: vec![Block::Text {
            content_index: 0,
            text: "使用subagent".into(),
        }],
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

fn card_details(m: &Msg) -> &serde_json::Value {
    m.blocks
        .iter()
        .find_map(|b| match b {
            Block::ToolCall { details, .. } => details.as_ref(),
            _ => None,
        })
        .expect("card details")
}

// 终态合并键：camelCase 的宿主 payload → 卡片 details 的 snake_case 键
#[test]
fn settled_fields_map_to_card_keys() {
    let (cid, fields) = settled_patch_fields(&json!({
        "childId": "sub-1",
        "stopReason": "completed",
        "session_file": "C:\\x\\sub-1.jsonl",
        "durationMs": 42,
    }))
    .expect("fields");
    assert_eq!(cid, "sub-1");
    assert_eq!(fields["status"], json!("completed"));
    assert_eq!(fields["stop_reason"], json!("completed"));
    assert_eq!(fields["session_file"], json!("C:\\x\\sub-1.jsonl"));
    // durationMs 不进卡片键
    assert!(fields.get("duration_ms").is_none());

    let (_, err) = settled_patch_fields(&json!({
        "childId": "sub-2",
        "stopReason": "error",
        "errorCode": "CHILD_CRASHED",
    }))
    .expect("fields");
    assert_eq!(err["error_code"], json!("CHILD_CRASHED"));

    // 缺 childId / stopReason 直接放弃（不写半截终态）
    assert!(settled_patch_fields(&json!({"stopReason": "completed"})).is_none());
    assert!(settled_patch_fields(&json!({"childId": "sub-1"})).is_none());
}

// 即时补丁：跳过无关消息与无关 child，命中就置 result_arrived 并可点开会话
#[test]
fn settled_patch_flips_the_matching_card() {
    let mut msgs = vec![user_msg(), running_card("sub-1"), running_card("sub-2")];
    let (cid, fields) = settled_patch_fields(&json!({
        "childId": "sub-2",
        "stopReason": "completed",
        "session_file": "C:\\sessions\\sub-2.jsonl",
    }))
    .expect("fields");
    assert!(apply_child_settled(&mut msgs, &cid, &fields));

    let other = card_details(&msgs[1]);
    assert_eq!(other["status"], json!("started"));
    assert!(!matches!(
        &msgs[1].blocks[0],
        Block::ToolCall {
            result_arrived: true,
            ..
        }
    ));

    let hit = card_details(&msgs[2]);
    assert_eq!(hit["status"], json!("completed"));
    assert_eq!(hit["stop_reason"], json!("completed"));
    assert_eq!(hit["session_file"], json!("C:\\sessions\\sub-2.jsonl"));
    // 原有字段不丢
    assert_eq!(hit["child_id"], json!("sub-2"));
    assert_eq!(hit["label"], json!("台积电能力调研"));
    assert!(matches!(
        &msgs[2].blocks[0],
        Block::ToolCall {
            result_arrived: true,
            ..
        }
    ));
}

// 幂等：重复应用（entry 与 completion 两条通道都会来）不报变更
#[test]
fn settled_patch_is_idempotent() {
    let mut msgs = vec![running_card("sub-1")];
    let (cid, fields) =
        settled_patch_fields(&json!({"childId": "sub-1", "stopReason": "completed"}))
            .expect("fields");
    assert!(apply_child_settled(&mut msgs, &cid, &fields));
    assert!(!apply_child_settled(&mut msgs, &cid, &fields));
}

// 非 agent 卡片（同 details 形态的其它工具）不动
#[test]
fn settled_patch_ignores_non_agent_cards() {
    let mut m = running_card("sub-1");
    if let Block::ToolCall { name, .. } = &mut m.blocks[0] {
        *name = "bash".into();
    }
    let mut msgs = vec![m];
    let (cid, fields) =
        settled_patch_fields(&json!({"childId": "sub-1", "stopReason": "completed"}))
            .expect("fields");
    assert!(!apply_child_settled(&mut msgs, &cid, &fields));
    assert_eq!(card_details(&msgs[0])["status"], json!("started"));
}
