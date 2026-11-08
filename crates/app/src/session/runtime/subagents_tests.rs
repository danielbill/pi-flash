//! 派发账本回归：**两种调用者同一条路**（模型直调 / codemode 脚本里的
//! `tools.agent`）、被拒不落点、终态来源、快照重建幂等。
//!
//! 2026-11-08 报障现场：一轮 12 个调用里 3 个经 codemode 派出，transcript
//! 只多了一个 `codemode` 块，扫消息投影出来是 **0 个点**；下一轮直调 7 个
//! 才画出 7 个点。根因不是投影算错，而是**没有会话状态层**，渲染层只能靠
//! 扫 transcript 反推。

use super::*;
use crate::session::Role;
use crate::session::messages::Msg;
use pi_link::protocol::Block;

fn msg(role: Role, blocks: Vec<Block>) -> Msg {
    Msg {
        role,
        blocks,
        usage: None,
        entry_id: None,
        ts: Some(1),
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

/// codemode 脚本里的 `tools.agent` 派发：pi 的事件形状
/// （`NestedToolCallRunner`：id = `<父调用>/N`，另带 parentToolCallId）。
#[test]
fn nested_dispatch_makes_a_dot_like_a_direct_call() {
    let mut led = SubagentLedger::default();
    // 直调：模型自己发的 agent 调用
    assert!(led.start("t1", AGENT_TOOL, None, 0, None));
    // 嵌套：codemode 块 t2 内部派出 3 个
    for n in 1..=3 {
        let id = format!("t2/{n}");
        assert!(led.start(&id, AGENT_TOOL, Some("t2"), 0, None));
    }
    // 非派发工具不上账
    assert!(!led.start("t9", "bash", None, 0, None));
    assert_eq!(led.len(), 4);
    assert_eq!(led.turn_tones(), vec![vec![DotTone::Muted; 4]]);
}

/// 被 `CAPACITY_REACHED` 拒掉的调用：有 start 有 end，但**没有子代理**——
/// 点阵不能画（点 = 子代理数，不是调用数）。
#[test]
fn rejected_dispatch_leaves_no_dot() {
    let mut led = SubagentLedger::default();
    led.start("t1/1", AGENT_TOOL, Some("t1"), 0, None);
    let details = serde_json::json!({ "error_code": "CAPACITY_REACHED" });
    assert!(led.finish("t1/1", AGENT_TOOL, true, Some(&details), 0));
    assert!(
        led.turn_tones().iter().all(|t| t.is_empty()),
        "被拒不占点：{:?}",
        led.turn_tones()
    );
}

/// 终态上色：completed 青 / aborted·max_turns 橙 / error 红。
#[test]
fn finish_paints_the_terminal_tone() {
    let mut led = SubagentLedger::default();
    let cases = [
        ("a", "completed", DotTone::Notice),
        ("b", "aborted", DotTone::Warn),
        ("c", "max_turns", DotTone::Warn),
        ("d", "error", DotTone::Danger),
    ];
    for (id, stop, _) in cases {
        led.start(id, AGENT_TOOL, None, 0, None);
        let details = serde_json::json!({ "child_id": format!("c-{id}"), "stop_reason": stop });
        led.finish(id, AGENT_TOOL, stop == "error", Some(&details), 0);
    }
    assert_eq!(
        led.turn_tones()[0],
        cases.iter().map(|(_, _, tone)| *tone).collect::<Vec<_>>()
    );
}

/// 后台派发：toolResult 只是 ack（没有 stop_reason），终态走
/// `subagent-settled` 条目 / completion 消息 —— settle 按 child_id 归位。
#[test]
fn settle_by_child_id_fills_the_terminal_state() {
    let mut led = SubagentLedger::default();
    led.start("t1", AGENT_TOOL, None, 0, None);
    // ack：status=started + child_id，不算终态
    led.finish(
        "t1",
        AGENT_TOOL,
        false,
        Some(&serde_json::json!({ "child_id": "c1", "status": "started" })),
        0,
    );
    assert_eq!(led.turn_tones()[0], vec![DotTone::Muted], "ack 仍是运行中");

    assert!(led.settle("c1", Some("completed")));
    assert_eq!(led.turn_tones()[0], vec![DotTone::Notice]);
    // 幂等：同值再来不报变更（否则每帧 nav 重建）
    assert!(!led.settle("c1", Some("completed")));
    // 未知 child 不报变更
    assert!(!led.settle("nope", Some("completed")));
}

/// 派发轮序**定格**在派发那一刻：之后到达的消息（含新一轮 User）不会把
/// 既有记录挪走——旧投影实现的「第 2 轮的点画在第 3 轮卡上」由此根除。
#[test]
fn dispatch_turn_is_fixed_at_dispatch_time() {
    let mut led = SubagentLedger::default();
    led.start("t1", AGENT_TOOL, None, 1, None);
    // 后续轮次再派
    led.start("t2", AGENT_TOOL, None, 2, None);
    let tones = led.turn_tones();
    assert_eq!(tones.len(), 3, "第 0 轮无点也占位（序号不得塌缩）");
    assert!(tones[0].is_empty());
    assert_eq!(tones[1].len(), 1, "第 1 轮的点留在第 1 轮");
    assert_eq!(tones[2].len(), 1);
}

/// 快照重建：从 transcript 的 `agent` 块回放历史，且与随后到达的实时事件
/// 按 `toolCallId` 去重（不重复画点、不丢新点）。
#[test]
fn seed_from_transcript_dedupes_with_live_events() {
    let msgs = vec![
        msg(Role::User, Vec::new()),
        msg(
            Role::Assistant,
            vec![
                // 直调派发：transcript 里有块
                call("t1", AGENT_TOOL),
                // codemode：块名不是 agent（内层派发在 transcript 里不可见）
                call("t2", "codemode"),
            ],
        ),
    ];
    let mut led = SubagentLedger::default();
    led.reset_and_seed(&msgs);
    assert_eq!(led.len(), 1, "只有直调的块能回放");
    assert_eq!(led.turn_tones()[0], vec![DotTone::Muted]);

    // 重放同一条 start（快照与事件撞车）：不重复记账
    assert!(led.start("t1", AGENT_TOOL, None, 0, None));
    assert_eq!(led.len(), 1);
    assert_eq!(led.turn_tones()[0].len(), 1);

    // 嵌套派发在实时路径上补进来，落在同一轮
    assert!(led.start("t2/1", AGENT_TOOL, Some("t2"), 0, None));
    assert_eq!(led.len(), 2);
    assert_eq!(led.turn_tones()[0].len(), 2);
}

/// `tool_execution_update` 的 running 回填只补 child_id，不改终态。
#[test]
fn running_update_only_fills_child_id() {
    let mut led = SubagentLedger::default();
    led.start("t1", AGENT_TOOL, None, 0, None);
    assert!(led.touch(
        "t1",
        &serde_json::json!({ "child_id": "c1", "status": "running" })
    ));
    assert!(!led.touch(
        "t1",
        &serde_json::json!({ "child_id": "c1", "status": "running" })
    ));
    assert_eq!(led.turn_tones()[0], vec![DotTone::Muted], "running 不上色");
    // 有了 child_id 才收得到后台结算
    assert!(led.settle("c1", Some("max_turns")));
    assert_eq!(led.turn_tones()[0], vec![DotTone::Warn]);
}
