//! 导航第三行点阵 + 「一个子会话只有一个状态」的回归：轮序归属、首帧
//! 即终态、completion 归一幂等。
//!
//! 2026-11-08 用户定夺的设计判据：子会话完成后状态**一次性**归一，重载
//! 后不存在"先灰点、后青点"的两段式渲染——装载时终态就已折进卡片。

use super::*;
use crate::session::Role;
use crate::session::messages::Msg;

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

fn msg(role: Role, ts: i64, blocks: Vec<Block>) -> Msg {
    Msg {
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

fn user(ts: i64) -> Msg {
    msg(Role::User, ts, Vec::new())
}

fn assistant(ts: i64, blocks: Vec<Block>) -> Msg {
    msg(Role::Assistant, ts, blocks)
}

/// 派发 result details（扩展 tool.ts）：只有 child_id + status=started——
/// 与磁盘上落地的形状一致（回灌只改内存，不回写盘）。
fn dispatch(id: &str, child: &str) -> Block {
    let mut b = call(id, "agent");
    if let Block::ToolCall { details, .. } = &mut b {
        *details = Some(serde_json::json!({ "child_id": child, "status": "started" }));
    }
    b
}

/// `subagent-completion` custom 消息（扩展 onCompleted 落盘）。
fn completion(child: &str, stop: &str) -> Msg {
    let mut m = msg(Role::Custom, 9, Vec::new());
    m.custom_type = Some(crate::session::messages::COMPLETION_CUSTOM.to_string());
    m.details = Some(serde_json::json!({
        "childId": child,
        "stopReason": stop,
        "session_file": "C:/x/child.jsonl",
    }));
    m
}

/// 2026-11-08 报障回归：第 2 轮派的 3 个子代理被画在第 3 轮卡上。
#[test]
fn turn_agents_land_on_their_own_turn() {
    let msgs = vec![
        user(1),
        assistant(2, vec![call("a1", "bash")]),
        // 第 2 条 User 消息 = 第 2 轮，三个子代理在这一轮派发
        user(3),
        assistant(
            4,
            vec![
                call("s1", "agent"),
                call("s2", "agent"),
                call("s3", "agent"),
            ],
        ),
        // 第 3 轮没有子代理
        user(5),
        assistant(6, Vec::new()),
    ];
    let rows = project_turn_agents(&msgs);
    assert_eq!(rows.len(), 3, "槽位数 = 轮数");
    assert!(rows[0].is_empty(), "第 1 轮无 agent 调用");
    assert_eq!(rows[1].len(), 3, "三个子代理属第 2 轮，不是第 3 轮");
    assert!(rows[2].is_empty(), "第 3 轮不得蹭到上一轮的点");
}

/// 末轮无后继 User 消息：旧实现的尾部补 push 落到无卡片的下标，第三行不显。
#[test]
fn last_turn_agents_stay_on_the_last_turn() {
    let msgs = vec![
        user(1),
        assistant(2, vec![call("a1", "agent")]),
        user(3),
        assistant(4, vec![call("b1", "agent"), call("b2", "agent")]),
    ];
    let rows = project_turn_agents(&msgs);
    assert_eq!(rows.len(), 2, "不得多出无卡片的幻影槽位");
    assert_eq!(rows[0].len(), 1);
    assert_eq!(rows[1].len(), 2, "末轮的点必须留在末轮槽位");
}

/// 单轮会话：旧实现整行不显（点全落在幻影下标上）。
#[test]
fn single_turn_session_still_shows_its_dots() {
    let msgs = vec![user(1), assistant(2, vec![call("a1", "agent")])];
    let rows = project_turn_agents(&msgs);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 1);
}

/// 首条 User 之前的 assistant（系统提示 / 回放残片）不归任何轮，也不能把
/// 后续轮次序号顶位。
#[test]
fn assistant_before_first_user_does_not_shift_turns() {
    let msgs = vec![
        assistant(1, vec![call("pre", "agent")]),
        user(2),
        assistant(3, vec![call("a1", "agent")]),
    ];
    let rows = project_turn_agents(&msgs);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 1, "只有第 1 轮自己派的那一个");
}

/// 非 agent 调用不上第三行。
#[test]
fn non_agent_calls_are_ignored() {
    let msgs = vec![user(1), assistant(2, vec![call("t1", "bash")])];
    let rows = project_turn_agents(&msgs);
    assert!(rows.iter().all(|r| r.is_empty()));
}

/// 装载即终态：磁盘形状（派发卡 status=started + completion 消息）经一次
/// 归一后，第一帧投影出来就是青/红点——不存在先灰后青。
#[test]
fn completion_normalizes_before_first_projection() {
    let mut msgs = vec![
        user(1),
        assistant(2, vec![dispatch("t1", "c1"), dispatch("t2", "c2")]),
        completion("c1", "completed"),
        completion("c2", "error"),
    ];
    let before = project_turn_agents(&msgs);
    assert_eq!(before[0], vec![DotTone::Muted, DotTone::Muted], "磁盘口径");

    assert!(
        crate::session::messages::apply_completion_messages(&mut msgs),
        "应当有变更"
    );
    assert_eq!(
        project_turn_agents(&msgs)[0],
        vec![DotTone::Notice, DotTone::Danger],
        "归一后首帧即终态，不再有灰点阶段"
    );
    // 卡片同源：spinner 也随之停（result_arrived + 终态 stop_reason）
    let Block::ToolCall {
        details,
        result_arrived,
        ..
    } = &msgs[1].blocks[0]
    else {
        unreachable!()
    };
    assert!(result_arrived);
    let d = details.as_ref().unwrap();
    assert_eq!(d["stop_reason"], "completed");
    assert_eq!(d["status"], "completed");
}

/// 归一幂等：装载与摄入路径都会调用，值不变就不该报变更（否则每帧 notify）。
#[test]
fn completion_normalization_is_idempotent() {
    let mut msgs = vec![
        user(1),
        assistant(2, vec![dispatch("t1", "c1")]),
        completion("c1", "completed"),
    ];
    assert!(crate::session::messages::apply_completion_messages(
        &mut msgs
    ));
    assert!(
        !crate::session::messages::apply_completion_messages(&mut msgs),
        "第二次应用不得再产生变更"
    );
}
