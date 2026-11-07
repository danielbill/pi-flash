//! `scan_agent_calls` 增量归组回归：轮号口径、只追加、最后一条重扫、
//! fork/回退作废、非 agent 调用忽略。

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
    scan_agent_calls(&msgs, &mut scan, &mut map);

    assert_eq!(scan.turns, 2);
    assert_eq!(map.get("a1").map(|c| c.turn), Some(0));
    assert_eq!(
        map.get("a2").copied(),
        Some(CallSite {
            turn: 0,
            msg: 1,
            block: 1,
        }),
        "落点必须能回指会话流里的块（终态回灌用）"
    );
    assert_eq!(map.get("b1").map(|c| c.turn), Some(1));
}

#[test]
fn appended_turns_are_scanned_incrementally() {
    let mut msgs = vec![user(1), assistant(2, vec![call("a1", "agent")])];
    let mut scan = TurnScan::default();
    let mut map = HashMap::new();
    scan_agent_calls(&msgs, &mut scan, &mut map);
    assert_eq!(map.len(), 1);

    msgs.push(user(3));
    msgs.push(assistant(4, vec![call("b1", "agent"), call("b2", "agent")]));
    scan_agent_calls(&msgs, &mut scan, &mut map);

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
    scan_agent_calls(&msgs, &mut scan, &mut map);
    assert!(map.is_empty());

    // 流式 / toolCallEnd 后到的块长在最后一条上：不回退重扫就会漏掉。
    if let Some(last) = msgs.last_mut() {
        last.blocks.push(call("late", "agent"));
    }
    scan_agent_calls(&msgs, &mut scan, &mut map);

    assert_eq!(map.get("late").map(|c| c.turn), Some(0));
}

#[test]
fn shrinking_history_voids_the_cursor() {
    let msgs = vec![user(1), assistant(2, vec![call("a1", "agent")])];
    let mut scan = TurnScan::default();
    let mut map = HashMap::new();
    scan_agent_calls(&msgs, &mut scan, &mut map);
    assert_eq!(scan.len, 2);

    // fork / 回退到第一轮（消息换了一份、首条时间戳也变）
    let forked = vec![user(9), assistant(10, vec![call("f1", "agent")])];
    scan_agent_calls(&forked, &mut scan, &mut map);

    assert!(map.get("a1").is_none(), "旧会话的映射必须清掉");
    assert_eq!(map.get("f1").map(|c| c.turn), Some(0));
    assert_eq!(scan.turns, 1);
}

#[test]
fn non_agent_calls_are_ignored() {
    let msgs = vec![user(1), assistant(2, vec![call("t1", "bash")])];
    let mut scan = TurnScan::default();
    let mut map = HashMap::new();
    scan_agent_calls(&msgs, &mut scan, &mut map);
    assert!(map.is_empty());
}

fn row(
    child: &str,
    phase: &str,
    stop: Option<&str>,
    err: Option<&str>,
    sf: Option<&str>,
) -> CatalogRow {
    CatalogRow {
        child_id: child.into(),
        provider: "pi".into(),
        label: Some(format!("{child}-label")),
        profile: Some("explorer".into()),
        depth: 0,
        phase: phase.into(),
        stop_reason: stop.map(str::to_string),
        error_code: err.map(str::to_string),
        session_file: sf.map(str::to_string),
        parent_tool_call_id: Some(format!("call-{child}")),
        usage_total: None,
        turns: None,
        agent_messages: None,
        tool_calls: None,
        started_at: String::new(),
        updated_at: String::new(),
    }
}

#[test]
fn row_patch_carries_card_fields_and_omits_absent_ones() {
    let done = row(
        "c1",
        "completed",
        Some("completed"),
        None,
        Some(r"D:\s\c1.jsonl"),
    );
    let v = row_details_patch(&done);
    assert_eq!(v["child_id"], "c1");
    assert_eq!(v["status"], "completed");
    assert_eq!(v["stop_reason"], "completed");
    assert_eq!(v["session_file"], r"D:\s\c1.jsonl");
    assert!(v.get("error_code").is_none(), "缺字段不写空值");

    let failed = row("c2", "failed", Some("error"), Some("PROVIDER_ERROR"), None);
    let v = row_details_patch(&failed);
    assert_eq!(v["error_code"], "PROVIDER_ERROR");
    assert!(v.get("session_file").is_none());
}

#[test]
fn session_file_lookup_matches_child_id() {
    let mut p = SubagentPanel::new(None, false);
    p.rows
        .push(row("c1", "active", None, None, Some("D:/s/c1.jsonl")));
    assert_eq!(
        p.session_file_for_child("c1"),
        Some(std::path::PathBuf::from("D:/s/c1.jsonl"))
    );
    assert_eq!(p.session_file_for_child("nope"), None);
}
