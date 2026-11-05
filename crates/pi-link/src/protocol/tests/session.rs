//! 会话状态/统计/树/扁平条目解析测试（含深树回归锁）。

use crate::protocol::*;
use serde_json::{Value, json};

#[test]
fn get_session_stats_parses() {
    let e = parse_line(
        r#"{"type":"response","command":"get_session_stats","success":true,"data":{"tokens":{"input":50000,"output":10000,"cacheRead":40000,"cacheWrite":5000,"total":105000},"cost":0.45,"contextUsage":{"tokens":60000,"contextWindow":200000,"percent":30}}}"#,
    )
    .unwrap();
    match e {
        Event::Response { command, data, .. } => {
            assert_eq!(command, "get_session_stats");
            let st = SessionStats::parse(&data.expect("data"));
            assert_eq!(st.tokens_total, 105000);
            assert_eq!(st.cache_write, 5000);
            assert!((st.cost - 0.45).abs() < 1e-9);
            assert_eq!(st.context_percent, Some(30.0));
            assert_eq!(st.summary(), "ctx 30% \u{b7} $0.45");
            // cacheRead / (input + cacheWrite + cacheRead)
            let hit = st.cache_hit_rate().expect("rate");
            assert!((hit - 40_000. / 95_000.).abs() < 1e-9);
        }
        other => panic!("wrong event: {other:?}"),
    }
}

#[test]
fn get_state_parses_snapshot() {
    let e = parse_line(
        r#"{"type":"response","command":"get_state","success":true,"data":{"model":{"id":"glm-5.3-flash","name":"GLM-5.3-Flash","provider":"glm","contextWindow":200000},"thinkingLevel":"high","isStreaming":false,"sessionId":"abc","messageCount":7,"pendingMessageCount":1}}"#,
    )
    .unwrap();
    match e {
        Event::Response { command, data, .. } => {
            assert_eq!(command, "get_state");
            let st = SessionState::parse(&data.expect("data"));
            assert_eq!(st.model_label().as_deref(), Some("GLM-5.3-Flash"));
            assert_eq!(st.thinking_level.as_deref(), Some("high"));
            assert!(!st.is_streaming);
            assert_eq!(st.message_count, 7);
            assert_eq!(st.pending_message_count, 1);
            assert!(st.session_name.is_none());
        }
        other => panic!("wrong event: {other:?}"),
    }
}

#[test]
fn parse_tree_response() {
    // Mirrors pi get_tree: entries wrap {entry:{id,parentId,type,message}, children:[]}
    let data = json!({
        "leafId": "c2",
        "tree": [{
            "entry": {"id":"a1","parentId":null,"type":"message",
                "message": {"role":"user","content":"fix the login bug"}},
            "children": [
                {"entry": {"id":"b1","parentId":"a1","type":"message",
                    "message": {"role":"assistant","content":"sure, on it"}},
                    "children": []},
                {"entry": {"id":"c1","parentId":"a1","type":"message",
                    "message": {"role":"user","content":"actually try tests first",
                        "extra": 1}},
                    "children": [
                        {"entry": {"id":"c2","parentId":"c1","type":"model_change"},
                         "children": []}
                    ]}
            ]
        }]
    });
    let (tree, leaf) = parse_tree(&data);
    assert_eq!(leaf.as_deref(), Some("c2"));
    assert_eq!(tree.len(), 1);
    let root = &tree[0];
    assert_eq!(root.id, "a1");
    assert_eq!(root.role.as_deref(), Some("user"));
    assert_eq!(root.text.as_deref(), Some("fix the login bug"));
    assert_eq!(root.children.len(), 2);
    // model_change entry has no message text
    let c2 = &root.children[1].children[0];
    assert_eq!(c2.entry_type, "model_change");
    assert!(c2.text.is_none());
    // forkable: assistant chain skips to first user message
    assert_eq!(root.children[0].forkable_entry_id(), None); // assistant leaf, no user below
    assert_eq!(root.forkable_entry_id(), Some("a1"));
}

/// 回归锁：pi get_tree 的会话树按 children 逐层嵌套（每层 entry+children
/// ≈ 4~5 层 JSON）。85 条消息的实测树深 95 → 整行超过 serde_json 默认
/// 128 层上限 → 整条 get_tree 响应被丢弃 → 所有用户消息拿不到 entry id
/// → 消息下的「新分支」按钮点不动。此处造一条 400 层深的链锁死行为。
#[test]
fn deep_tree_line_survives_parse_line() {
    // 400 层递归解析 debug 帧超 Windows 默认 2MB 测试线程栈（bead 0bk）：
    // 测试自抬栈，跑 `cargo test` 的人不需要配 RUST_MIN_STACK。
    let handle = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(deep_tree_body)
        .expect("spawn deep_tree_body thread");
    if let Err(payload) = handle.join() {
        std::panic::resume_unwind(payload);
    }
}

fn deep_tree_body() {
    // 从叶子往根拼：node -> {entry, children:[node]}
    let depth = 400;
    let mut node = String::from(
        r#"{"entry":{"id":"n0","parentId":null,"type":"message","message":{"role":"user","content":"hi"}},"children":[]}"#,
    );
    for i in 1..depth {
        node = format!(
            r#"{{"entry":{{"id":"n{i}","parentId":"n{}","type":"message","message":{{"role":"assistant","content":"a{i}"}}}},"children":[{node}]}}"#,
            i - 1
        );
    }
    let line = format!(
        r#"{{"id":"t1","type":"response","command":"get_tree","success":true,"data":{{"leafId":"n{leaf}","tree":[{node}]}}}}"#,
        leaf = depth - 1
    );
    // serde_json 的默认上限会在这里直接失败——用同款解析器做对照锁
    assert!(
        serde_json::from_str::<Value>(&line).is_err(),
        "fixture no longer exceeds the default recursion limit"
    );
    let Some(Event::Response { command, data, .. }) = crate::protocol::parse_line(&line) else {
        panic!("deep get_tree line was dropped by parse_line");
    };
    assert_eq!(command, "get_tree");
    let (tree, leaf) = parse_tree(data.as_ref().expect("data"));
    assert_eq!(leaf.as_deref(), Some(format!("n{}", depth - 1).as_str()));
    let mut n = 0;
    let mut cur = &tree[0];
    while !cur.children.is_empty() {
        n += 1;
        cur = &cur.children[0];
    }
    assert_eq!(n, depth - 1, "full chain survived the parse");
    assert_eq!(cur.role.as_deref(), Some("user"));
}

/// get_entries → active chain 的 user 锚点。这是「新分支」fork 的输入，
/// 必须与 pi-web sliceActiveBranch 同语义：沿 parentId 从 leaf 回溯，
/// 取 user message entry，最旧在前。
#[test]
fn active_user_ids_from_flat_entries() {
    let data = json!({
        "leafId": "c2",
        "entries": [
            {"id":"a1","parentId":null,"type":"message","message":{"role":"user","content":"first"}},
            {"id":"m1","parentId":"a1","type":"model_change","modelId":"x"},
            {"id":"b1","parentId":"m1","type":"message","message":{"role":"assistant","content":"ok"}},
            {"id":"c1","parentId":"b1","type":"message","message":{"role":"user","content":"second"}},
            {"id":"c2","parentId":"c1","type":"message","message":{"role":"assistant","content":"done"}},
            // sibling branch off c1 — must NOT appear on the active chain
            {"id":"c1b","parentId":"b1","type":"message","message":{"role":"user","content":"other branch"}}
        ]
    });
    let (entries, leaf) = parse_entries(&data);
    assert_eq!(leaf.as_deref(), Some("c2"));
    assert_eq!(
        active_user_entry_ids(&entries, leaf.as_deref()),
        vec!["a1", "c1"]
    );
    // no leaf → nothing is forkable (empty session)
    assert!(active_user_entry_ids(&entries, None).is_empty());
    // unknown leaf → nothing (not a panic)
    assert!(active_user_entry_ids(&entries, Some("nope")).is_empty());
}

/// parentId 环（坏文件）不能把调用方挂死
#[test]
fn active_user_ids_survive_parent_cycle() {
    let data = json!({
        "leafId": "x2",
        "entries": [
            {"id":"x1","parentId":"x2","type":"message","message":{"role":"user","content":"a"}},
            {"id":"x2","parentId":"x1","type":"message","message":{"role":"user","content":"b"}}
        ]
    });
    let (entries, leaf) = parse_entries(&data);
    let ids = active_user_entry_ids(&entries, leaf.as_deref());
    assert!(ids.len() <= entries.len());
}
