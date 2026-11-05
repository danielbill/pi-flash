use super::*;

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("cat-{tag}-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn desc(child: &str, phase: &str, extra: &str) -> String {
    format!(
        "{{\"type\":\"custom\",\"customType\":\"subagent/descriptor\",\"data\":{{\"version\":1,\"childId\":\"{child}\",\"provider\":\"spawn\",\"cwd\":\"/w\",\"depth\":1,\"phase\":\"{phase}\",\"startedAt\":\"2026-10-19T10:00:00.000Z\",\"updatedAt\":\"2026-10-19T10:00:10.000Z\"{extra}}},\"id\":\"e1\",\"parentId\":\"root\",\"timestamp\":\"2026-10-19T10:00:00.000Z\"}}\n"
    )
}

#[test]
fn reads_descriptors_and_skips_others() {
    let f = tmp("read");
    let mut body = String::from("{\"type\":\"session\",\"id\":\"x\"}\n");
    body.push_str(&desc("sub-1", "active", ",\"label\":\"arch-review\""));
    body.push_str(&desc("sub-2", "provisioning", ""));
    std::fs::write(&f, body).unwrap();
    let scan = read_catalog(&f, 0);
    assert_eq!(scan.rows.len(), 2);
    assert_eq!(scan.next_offset, std::fs::metadata(&f).unwrap().len());
    let r1 = scan.rows.iter().find(|r| r.child_id == "sub-1").unwrap();
    assert_eq!(r1.display_label(), "arch-review");
    assert!(r1.in_flight());
    assert_eq!(r1.status_kind(), StatusKind::Running);
    assert_eq!(r1.provider, "spawn");
    assert_eq!(r1.depth, 1);
    let _ = std::fs::remove_file(&f);
}

#[test]
fn last_entry_wins_for_same_child() {
    let f = tmp("lastwins");
    let mut body = desc("sub-1", "active", "");
    body.push_str(&desc("sub-1", "completed", ",\"stopReason\":\"completed\""));
    std::fs::write(&f, body).unwrap();
    let scan = read_catalog(&f, 0);
    assert_eq!(scan.rows.len(), 1);
    assert_eq!(scan.rows[0].phase, "completed");
    assert_eq!(scan.rows[0].status_kind(), StatusKind::Done);
    let _ = std::fs::remove_file(&f);
}

#[test]
fn incremental_read_only_new_bytes_and_merges() {
    let f = tmp("incr");
    std::fs::write(&f, desc("sub-1", "active", "")).unwrap();
    let first = read_catalog(&f, 0);
    assert_eq!(first.rows.len(), 1);
    // 追加一条更新 + 一条新的
    let mut more = desc("sub-1", "completed", ",\"stopReason\":\"completed\"");
    more.push_str(&desc(
        "sub-2",
        "failed",
        ",\"stopReason\":\"error\",\"errorCode\":\"CAPACITY_REACHED\"",
    ));
    let mut fh = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
    use std::io::Write;
    fh.write_all(more.as_bytes()).unwrap();
    drop(fh);
    let second = read_catalog(&f, first.next_offset);
    assert_eq!(second.rows.len(), 2, "块内同 childId 末条覆盖后应为 2");
    let mut host = first.rows.clone();
    merge_rows(&mut host, second.rows);
    assert_eq!(host.len(), 2);
    let s1 = host.iter().find(|r| r.child_id == "sub-1").unwrap();
    assert_eq!(s1.phase, "completed", "merge 必须用末条覆盖");
    let s2 = host.iter().find(|r| r.child_id == "sub-2").unwrap();
    assert_eq!(s2.error_code.as_deref(), Some("CAPACITY_REACHED"));
    assert_eq!(s2.status_kind(), StatusKind::Error);
    let _ = std::fs::remove_file(&f);
}

#[test]
fn partial_tail_line_does_not_advance_offset() {
    let f = tmp("partial");
    let full = desc("sub-1", "active", "");
    std::fs::write(&f, &full).unwrap();
    let scan = read_catalog(&f, 0);
    assert_eq!(scan.next_offset, full.len() as u64);
    // 再写一个半行（模拟面板在扩展 append 中途读到）
    let partial = &desc("sub-2", "active", "");
    let half = &partial[..partial.len() / 2];
    let mut fh = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
    use std::io::Write;
    fh.write_all(half.as_bytes()).unwrap();
    drop(fh);
    let second = read_catalog(&f, scan.next_offset);
    assert!(second.rows.is_empty(), "半行不得解析");
    assert_eq!(second.next_offset, scan.next_offset, "半行不得推进 offset");
    // 补完后重读同一 offset 能拿到它
    let rest = &partial[partial.len() / 2..];
    let mut fh = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
    fh.write_all(rest.as_bytes()).unwrap();
    drop(fh);
    let third = read_catalog(&f, second.next_offset);
    assert_eq!(third.rows.len(), 1);
    assert_eq!(third.rows[0].child_id, "sub-2");
    let _ = std::fs::remove_file(&f);
}

#[test]
fn missing_file_and_shrunk_file_fail_closed() {
    let gone = std::env::temp_dir().join("cat-nope-does-not-exist.jsonl");
    let scan = read_catalog(&gone, 0);
    assert!(scan.rows.is_empty());
    assert_eq!(scan.next_offset, 0);

    let f = tmp("shrink");
    std::fs::write(&f, desc("sub-1", "active", "")).unwrap();
    let first = read_catalog(&f, 0);
    assert_eq!(first.rows.len(), 1);
    // 文件被截断（换会话）：offset 大于新长度 → 归零重扫
    std::fs::write(&f, desc("sub-9", "active", "")).unwrap();
    let second = read_catalog(&f, first.next_offset + 1000);
    assert_eq!(second.rows.len(), 1, "截断后必须归零重扫");
    assert_eq!(second.rows[0].child_id, "sub-9");
    let _ = std::fs::remove_file(&f);
}

#[test]
fn usage_total_prefers_total_then_sums_parts() {
    let f = tmp("usage");
    // 带 total：直接用
    let with_total = "{\"type\":\"custom\",\"customType\":\"subagent/descriptor\",\"data\":{\"childId\":\"a\",\"provider\":\"spawn\",\"phase\":\"completed\",\"usage\":{\"input\":100,\"output\":50,\"total\":23700}}}
";
    // 无 total：字段求和；空对象 / 缺字段：None（老 descriptor）
    let no_total = "{\"type\":\"custom\",\"customType\":\"subagent/descriptor\",\"data\":{\"childId\":\"b\",\"provider\":\"spawn\",\"phase\":\"completed\",\"usage\":{\"input\":100,\"output\":50,\"cacheRead\":3}}}
";
    let plain = desc("c", "completed", "");
    std::fs::write(&f, format!("{with_total}{no_total}{plain}")).unwrap();
    let scan = read_catalog(&f, 0);
    assert_eq!(scan.rows[0].usage_total, Some(23700));
    assert_eq!(scan.rows[1].usage_total, Some(153));
    assert_eq!(
        scan.rows[2].usage_total, None,
        "老 descriptor 无 usage → None"
    );
    let _ = std::fs::remove_file(&f);
}

#[test]
fn stats_fields_parse_or_fall_back_to_none() {
    let f = tmp("stats");
    let with_stats = "{\"type\":\"custom\",\"customType\":\"subagent/descriptor\",\"data\":{\"childId\":\"a\",\"provider\":\"spawn\",\"phase\":\"active\",\"stats\":{\"turns\":3,\"agentMessages\":2,\"toolCalls\":5}}}\n";
    // stats 缺字段 → 该顶 None；无 stats（老 descriptor）→ 三顶全 None
    let partial = "{\"type\":\"custom\",\"customType\":\"subagent/descriptor\",\"data\":{\"childId\":\"b\",\"provider\":\"spawn\",\"phase\":\"active\",\"stats\":{\"turns\":1}}}\n";
    let plain = desc("c", "completed", "");
    std::fs::write(&f, format!("{with_stats}{partial}{plain}")).unwrap();
    let scan = read_catalog(&f, 0);
    assert_eq!(scan.rows[0].turns, Some(3));
    assert_eq!(scan.rows[0].agent_messages, Some(2));
    assert_eq!(scan.rows[0].tool_calls, Some(5));
    assert_eq!(scan.rows[1].turns, Some(1));
    assert_eq!(scan.rows[1].agent_messages, None);
    assert_eq!(scan.rows[2].turns, None, "老 descriptor 无 stats → None");
    let _ = std::fs::remove_file(&f);
}

#[test]
fn malformed_and_wrong_type_lines_are_ignored() {
    let f = tmp("malformed");
    let mut body = String::from("not json at all\n");
    body.push_str(
        "{\"type\":\"custom\",\"customType\":\"subagent/child\",\"data\":{\"childId\":\"x\"}}\n",
    );
    body.push_str("{\"type\":\"custom\",\"customType\":\"subagent/descriptor\",\"data\":{\"childId\":\"\",\"provider\":\"spawn\",\"phase\":\"active\"}}\n");
    body.push_str("{\"type\":\"custom\",\"customType\":\"subagent/descriptor\",\"data\":{\"childId\":\"ok\",\"provider\":\"\",\"phase\":\"active\"}}\n");
    body.push_str(&desc("good", "active", ""));
    std::fs::write(&f, body).unwrap();
    let scan = read_catalog(&f, 0);
    assert_eq!(scan.rows.len(), 1, "只有字段齐全的 descriptor 才算数");
    assert_eq!(scan.rows[0].child_id, "good");
    let _ = std::fs::remove_file(&f);
}

#[test]
fn duration_and_status_kinds() {
    let f = tmp("dur");
    let mut body = desc("sub-1", "active", "");
    body.push_str(&desc("sub-2", "completed", ",\"stopReason\":\"completed\""));
    body.push_str(&desc("sub-3", "failed", ",\"stopReason\":\"max_turns\""));
    body.push_str(&desc("sub-4", "interrupted", ""));
    std::fs::write(&f, body).unwrap();
    let rows = read_catalog(&f, 0).rows;
    // startedAt=10:00:00 updatedAt=10:00:10 → 10s
    assert_eq!(row_duration_secs(&rows[0]), Some(10));
    assert_eq!(rows[0].status_kind(), StatusKind::Running);
    assert_eq!(rows[1].status_kind(), StatusKind::Done);
    assert_eq!(rows[2].status_kind(), StatusKind::MaxTurns);
    assert_eq!(rows[3].status_kind(), StatusKind::Aborted);
    let _ = std::fs::remove_file(&f);
}

#[test]
fn parse_iso_handles_offsets_and_bad_input() {
    assert_eq!(parse_iso_secs("2026-10-19T00:00:00.000Z"), Some(1792368000));
    assert_eq!(
        parse_iso_secs("2026-10-19T00:00:00+08:00"),
        Some(1792368000 - 8 * 3600)
    );
    assert_eq!(parse_iso_secs("garbage"), None);
    assert_eq!(parse_iso_secs(""), None);
    assert_eq!(
        parse_iso_secs("2026-13-99T99:99:99Z").is_some(),
        true,
        "只做算术，不校验日历合法性"
    );
}
