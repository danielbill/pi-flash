//! 读 API 与叶链回归测试（自 sessions.rs 内嵌 tests / leaf_chain_tests 外移；
//! 只搬不改）。

use super::*;
use crate::sessions::group_name_for_cwd;
use crate::sessions::testing::{BODY, temp_root, write_session};

#[test]
fn stale_session_files_picks_old_jsonl_only() {
    // 052：只按 mtime 挑超期候选——老 jsonl 入选，新会话 / 非 jsonl /
    // 直接落在 root 的散文件都不入选
    let root = temp_root("stale");
    let old = write_session(&root, "D:\\proj-a", "old1", BODY);
    let fresh = write_session(&root, "D:\\proj-a", "new1", BODY);
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 86_400);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&old)
        .unwrap()
        .set_modified(past)
        .unwrap();
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 86_400);
    let hits = stale_session_files(&root, cutoff);
    assert!(
        hits.iter().any(|p| p == &old),
        "mtime 早于 cutoff 的会话入选"
    );
    assert!(!hits.iter().any(|p| p == &fresh), "新会话不入选");
    let group = root.join(group_name_for_cwd("D:\\proj-a"));
    std::fs::write(group.join("notes.txt"), "x").unwrap();
    std::fs::write(root.join("loose.jsonl"), "{}").unwrap();
    let hits2 = stale_session_files(&root, cutoff);
    assert_eq!(hits2.len(), hits.len(), "散文件/非 jsonl 不入选");
    assert!(
        hits2
            .iter()
            .all(|p| p.extension().is_some_and(|e| e == "jsonl")),
        "结果全是 jsonl"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tail_messages_window_and_cap() {
    let root = temp_root("tail");
    let mut body = String::new();
    for i in 0..5 {
        body.push_str(&format!(
            "{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"m{i}\"}}}}\n"
        ));
    }
    let path = write_session(&root, "D:\\proj", "t1", &body);
    let msgs = read_tail_messages(&path, 1024 * 1024, 3);
    assert_eq!(msgs.len(), 3, "capped at max");
    let last = msgs.last().unwrap()["message"]["content"].as_str().unwrap();
    assert_eq!(last, "m4");
    let first = msgs.first().unwrap()["message"]["content"]
        .as_str()
        .unwrap();
    assert_eq!(first, "m2", "oldest of the window dropped first");

    // tiny window starting mid-line: first partial line discarded
    let small = read_tail_messages(&path, 200, 10);
    assert!(small.len() <= 5);
    for v in &small {
        assert!(v["message"]["content"].is_string(), "no partial JSON rows");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tail_messages_expands_past_custom_flood() {
    // 044-2 实案：subagent 扩展以每条 assistant message 一条的速率把
    // `subagent/descriptor` custom 条目灌进父会话——真消息全被挤出 256KB
    // 尾窗（39MB 文件里 message 只占头部 ~100KB）。尾窗捞不到时窗口 ×4
    // 回溯再扫（4MB 封顶）：窗口内够得着的真消息必须捞到，秒开不落空态。
    let root = temp_root("tail-expand");
    let mut body = String::new();
    body.push_str("{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"early\"}}\n");
    body.push_str(
        "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":\"reply\"}}\n",
    );
    // >4MB 的 custom 洪泛：任何封顶窗口都只看到 custom
    let flood = "x".repeat(600);
    for _ in 0..8000 {
        body.push_str(&format!(
            "{{\"type\":\"custom\",\"customType\":\"subagent/descriptor\",\"data\":\"{flood}\"}}\n"
        ));
    }
    // 尾端再垫一条真消息（回溯应该捞到它，而不是更早的头部两条）
    body.push_str(
        "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"latest\"}}\n",
    );
    let path = write_session(&root, "D:\\proj", "t2", &body);
    let msgs = read_tail_messages(&path, 256 * 1024, 10);
    assert!(
        msgs.iter()
            .any(|v| v["message"]["content"].as_str() == Some("latest")),
        "custom 洪泛下的尾端真消息必须捞到（窗口回溯）"
    );
    assert!(
        !msgs
            .iter()
            .any(|v| v["message"]["content"].as_str() == Some("early")),
        "回溯按窗口截断，不回读全文件头部"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn lists_real_sessions_when_present() {
    // machine-dependent smoke test: only asserts invariants, never fails
    // when no sessions exist (e.g. fresh CI machine). Prints the scan
    // timing + index diagnostics used by the startup budget record.
    let t0 = std::time::Instant::now();
    let sessions = list_sessions(50);
    let elapsed = t0.elapsed();
    let (scans, rows) = scan_diagnostics();
    eprintln!(
        "scan: {} sessions in {elapsed:?} (files scanned: {scans}, index rows: {rows})",
        sessions.len()
    );
    for s in &sessions {
        assert!(!s.id.is_empty());
        assert!(s.path.exists());
    }
}

// 原 leaf_chain_tests 模块并入此处（itlq.6 外移）。
/// Synthetic session: linear messages, then a mis-parented custom entry
/// (plan-mode-state shape) that pi's own restore anchors the leaf at —
/// stranding the final turn. read_leaf_messages must anchor at the last
/// MESSAGE instead and recover the stranded turn.
#[test]
fn leaf_anchored_at_last_message_recovers_stranded_turn() {
    let dir = std::env::temp_dir().join("pi-flash-leaf-chain-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("leaf-chain.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session","version":3,"id":"hdr","cwd":"/w"}"#, "\n",
            r#"{"type":"message","id":"m1","parentId":"hdr","timestamp":1,"message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#, "\n",
            r#"{"type":"message","id":"m2","parentId":"m1","timestamp":2,"message":{"role":"assistant","content":[{"type":"text","text":"first"}]}}"#, "\n",
            r#"{"type":"message","id":"m3","parentId":"m2","timestamp":3,"message":{"role":"user","content":[{"type":"text","text":"again"}]}}"#, "\n",
            // mis-parented: pi's default leaf lands here (parent = m2, not m3)
            r#"{"type":"custom","customType":"plan-mode-state","id":"c1","parentId":"m2","timestamp":4,"data":{"enabled":false}}"#, "\n",
            // stranded turn
            r#"{"type":"message","id":"m4","parentId":"m3","timestamp":5,"message":{"role":"assistant","content":[{"type":"text","text":"stranded tail"}]}}"#, "\n",
        ),
    )
    .unwrap();

    assert_eq!(count_message_entries(&path), 4);
    let msgs = read_leaf_messages(&path, 100);
    let texts: Vec<String> = msgs
        .iter()
        .map(|v| {
            v["message"]["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert_eq!(texts, vec!["hi", "first", "again", "stranded tail"]);

    // max trims from the head, keeps the newest
    let trimmed = read_leaf_messages(&path, 2);
    let texts: Vec<String> = trimmed
        .iter()
        .map(|v| {
            v["message"]["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert_eq!(texts, vec!["again", "stranded tail"]);

    std::fs::remove_file(&path).ok();
}
