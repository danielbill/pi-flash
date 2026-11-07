//! `scan_agent_calls` 增量归组回归：轮号口径、只追加、最后一条重扫、
//! fork/回退作废、非 agent 调用忽略、childId 反查映射。

use std::collections::HashMap;

use super::*;
use crate::session::Role;

fn call(id: &str, name: &str) -> Block {
    Block::ToolCall {
        content_index: 0,
        id: id.into(),
        name: name.into(),
        args: "{}".into(),
        result: String::new(),
        is_error: false,
        images: Vec::new(),
        duration_s: None,
        details: None,
        args_partial: false,
        result_arrived: false,
    }
}

/// 派发 result details（扩展 tool.ts）：child_id 必带。
fn dispatch(id: &str, child: &str) -> Block {
    let mut b = call(id, "agent");
    if let Block::ToolCall { details, .. } = &mut b {
        *details = Some(serde_json::json!({ "child_id": child, "status": "started" }));
    }
    b
}

fn msg(role: Role, ts: i64, blocks: Vec<Block>) -> crate::session::messages::Msg {
    crate::session::messages::Msg {
        role,
        blocks,
        usage: None,
        entry_id: None,
        ts: Some(ts),
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

fn user(ts: i64) -> crate::session::messages::Msg {
    msg(Role::User, ts, Vec::new())
}

fn assistant(ts: i64, blocks: Vec<Block>) -> crate::session::messages::Msg {
    msg(Role::Assistant, ts, blocks)
}

#[test]
fn first_scan_maps_calls_to_their_turn() {
    let msgs = vec![
        user(1),
        assistant(2, vec![call("a1", "agent"), call("a2", "agent")]),
        user(3),
        assistant(4, vec![call("b1", "agent")]),
    ];
    let mut scan = TurnScan::default();
    let mut map = HashMap::new();
    let mut by_child = HashMap::new();
    scan_agent_calls(&msgs, &mut scan, &mut map, &mut by_child);

    assert_eq!(scan.turns, 2);
    assert_eq!(map.get("a1").map(|c| c.turn), Some(0));
    assert_eq!(
        map.get("a2").copied(),
        Some(CallSite {
            turn: 0,
            msg: 1,
            block: 1,
        }),
        "落点必须能回指会话流里的块（completion 回灌用）"
    );
    assert_eq!(map.get("b1").map(|c| c.turn), Some(1));
    assert!(by_child.is_empty(), "无 child_id 的调用不进反查映射");
}

#[test]
fn dispatch_details_feed_the_child_lookup() {
    let msgs = vec![user(1), assistant(2, vec![dispatch("a1", "sub-1")])];
    let mut scan = TurnScan::default();
    let mut map = HashMap::new();
    let mut by_child = HashMap::new();
    scan_agent_calls(&msgs, &mut scan, &mut map, &mut by_child);

    assert_eq!(
        by_child.get("sub-1").copied(),
        Some(CallSite {
            turn: 0,
            msg: 1,
            block: 0,
        })
    );
}

#[test]
fn appended_turns_are_scanned_incrementally() {
    let mut msgs = vec![user(1), assistant(2, vec![call("a1", "agent")])];
    let mut scan = TurnScan::default();
    let mut map = HashMap::new();
    let mut by_child = HashMap::new();
    scan_agent_calls(&msgs, &mut scan, &mut map, &mut by_child);
    assert_eq!(map.len(), 1);

    msgs.push(user(3));
    msgs.push(assistant(4, vec![call("b1", "agent"), call("b2", "agent")]));
    scan_agent_calls(&msgs, &mut scan, &mut map, &mut by_child);

    assert_eq!(scan.turns, 2);
    assert_eq!(map.get("a1").map(|c| c.turn), Some(0), "旧轮映射保留");
    assert_eq!(map.get("b1").map(|c| c.turn), Some(1));
    assert_eq!(map.get("b2").map(|c| c.turn), Some(1));
}

#[test]
fn last_message_is_rescanned_for_late_blocks() {
    let mut msgs = vec![user(1), assistant(2, Vec::new())];
    let mut scan = TurnScan::default();
    let mut map = HashMap::new();
    let mut by_child = HashMap::new();
    scan_agent_calls(&msgs, &mut scan, &mut map, &mut by_child);
    assert!(map.is_empty());

    // 流式 / toolCallEnd 后到的块长在最后一条上：不回退重扫就会漏掉。
    if let Some(last) = msgs.last_mut() {
        last.blocks.push(call("late", "agent"));
    }
    scan_agent_calls(&msgs, &mut scan, &mut map, &mut by_child);

    assert_eq!(map.get("late").map(|c| c.turn), Some(0));
}

#[test]
fn shrinking_history_voids_the_cursor() {
    let msgs = vec![user(1), assistant(2, vec![call("a1", "agent")])];
    let mut scan = TurnScan::default();
    let mut map = HashMap::new();
    let mut by_child = HashMap::new();
    scan_agent_calls(&msgs, &mut scan, &mut map, &mut by_child);
    assert_eq!(scan.len, 2);

    // fork / 回退到第一轮（消息换了一份、首条时间戳也变）
    let forked = vec![user(9), assistant(10, vec![call("f1", "agent")])];
    scan_agent_calls(&forked, &mut scan, &mut map, &mut by_child);

    assert!(map.get("a1").is_none(), "旧会话的映射必须清掉");
    assert_eq!(map.get("f1").map(|c| c.turn), Some(0));
    assert_eq!(scan.turns, 1);
}

#[test]
fn non_agent_calls_are_ignored() {
    let msgs = vec![user(1), assistant(2, vec![call("t1", "bash")])];
    let mut scan = TurnScan::default();
    let mut map = HashMap::new();
    let mut by_child = HashMap::new();
    scan_agent_calls(&msgs, &mut scan, &mut map, &mut by_child);
    assert!(map.is_empty());
}
