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

/// 老洪泛会话（pi-flash-9eck）：尾段塞满不可渲染的 subagent/descriptor
/// custom 条目，256KB 固定窗口会读空 → 打开卡「载入中」。倍增窗口必须把
/// 垃圾行前面的正常消息读出来。
#[test]
fn tail_window_grows_past_descriptor_flood() {
    let root = temp_root("tailflood");
    let mut body = String::new();
    for i in 0..12 {
        body.push_str(&format!(
            "{{\"type\":\"message\",\"id\":\"m{i}\",\"message\":{{\"role\":\"user\",\"content\":\"m{i}\"}}}}\n"
        ));
    }
    // 尾部灌 ~300KB descriptor 垃圾行（同扩展 manager.ts 曾有的落盘形状）：
    // 远超 256KB 初窗，且全是 non-renderable
    let pad = "x".repeat(128); // 撑行宽，控制行数即可灌体积
    let junk = format!(
        r#"{{"type":"custom","customType":"subagent/descriptor","id":"d","data":{{"childId":"sub-1","provider":"spawn","phase":"active","depth":1,"cwd":"/w"}},"timestamp":9,"pad":"{pad}"}}"#
    );
    for _ in 0..2400 {
        body.push_str(&junk);
        body.push('\n');
    }
    let path = write_session(&root, "D:\\proj", "flood", &body);
    let len = std::fs::metadata(&path).unwrap().len();
    assert!(len > 256 * 1024, "垃圾段必须超过初窗（实际 {len}B）");

    // 256KB 初窗全垃圾；扩窗后必须读到正常消息
    let msgs = read_tail_messages(&path, 256 * 1024, 100);
    assert!(
        msgs.len() >= 10,
        "扩窗后必须读到 ≥10 条 renderable（实际 {}）",
        msgs.len()
    );
    // 消息在最前面：窗口 max=100 时应含首条 m0 附近
    assert_eq!(
        msgs.first().unwrap()["message"]["content"]
            .as_str()
            .unwrap(),
        "m0",
        "扩窗到 2MB 后 12 条消息全部在窗内"
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

/// P0-1（047-内存削减）：count_message_entries 流式化后与整文件字节扫描
/// 的等价性边界——行首/行尾命中、单行多 needle、嵌套假阳性、垃圾行、
/// 空文件、无换行尾行。occurrence 计数（单行两个 needle = 2）。
#[test]
fn count_message_entries_streaming_edge_cases() {
    let dir = std::env::temp_dir().join("pi-flash-count-entries-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("count-entries.jsonl");

    // 空文件
    std::fs::write(&path, "").unwrap();
    assert_eq!(count_message_entries(&path), 0);

    let body = concat!(
        "\"type\":\"message\"\n",                        // 行首命中
        "{\"x\":1,\"type\":\"message\"}\n",              // 行尾命中
        "\"type\":\"message\" x \"type\":\"message\"\n", // 单行 2 次
        "{\"type\":\"custom\",\"id\":\"c\",\"data\":{\"type\":\"message\"}}\n", // 嵌套假阳性（计入上界）
        "not json\n",                                                           // 垃圾行
        "{\"type\":\"message\"}",                                               // 无换行尾行
    );
    std::fs::write(&path, body).unwrap();
    assert_eq!(count_message_entries(&path), 6);

    std::fs::remove_file(&path).ok();
}
