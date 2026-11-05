//! 扫描器/索引/增量计数/改名回归测试（自 sessions.rs 内嵌 tests 外移；
//! 只搬不改）。

use super::*;
use crate::sessions::read::PARTIAL_PREVIEW_CHARS;
use crate::sessions::testing::{BODY, temp_root, write_session};
use serde_json::json;

#[test]
fn scan_parses_summary_name_and_count() {
    let root = temp_root("scan");
    let path = write_session(&root, "D:\\proj", "abc", BODY);
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    let e = scan_file(&path, mtime, 42, None).expect("scan ok");
    assert_eq!(e.id, "abc");
    assert_eq!(e.cwd, "D:\\proj");
    assert_eq!(e.preview, "hello 世界");
    assert_eq!(e.name.as_deref(), Some("renamed"));
    assert_eq!(e.message_count, 3);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn rename_survives_growth_beyond_old_tail_window() {
    // 回归：改名条目被 1MB+ 后续聊天推出旧版尾窗上限后，侧栏标题会
    // "改回去"（回落首条用户消息预览）。名字现随计数流全量扫描，
    // 与条目在文件中的位置无关。
    let root = temp_root("rename-growth");
    let pad = json!({"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"padpadpadpadpadpadpadpadpadpad"}]}})
        .to_string();
    let mut body = String::from(BODY);
    for _ in 0..12_000 {
        body.push_str(&pad);
        body.push('\n');
    }
    assert!(body.len() > 1_048_576, "必须真超过旧 1MB 尾窗上限");
    let path = write_session(&root, "D:\\proj-g", "g1", &body);
    let meta = std::fs::metadata(&path).unwrap();
    let e = scan_file(&path, meta.modified().unwrap(), meta.len(), None).expect("scan ok");
    assert_eq!(e.name.as_deref(), Some("renamed"));
    assert_eq!(e.message_count, 3 + 12_000);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn incremental_rescan_tracks_new_rename_and_keeps_old_name() {
    let root = temp_root("rename-incr");
    let path = write_session(&root, "D:\\proj-r", "r1", BODY);
    let meta = std::fs::metadata(&path).unwrap();
    let e1 = scan_file(&path, meta.modified().unwrap(), meta.len(), None).expect("scan ok");
    assert_eq!(e1.name.as_deref(), Some("renamed"));
    use std::io::Write;
    // 追加新改名 + 一条消息 → 增量续读后缀，名字更新
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(
            concat!(
                "{\"type\":\"session_info\",\"name\":\"renamed-2\"}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"more\"}}\n"
            )
            .as_bytes(),
        )
        .unwrap();
    let meta = std::fs::metadata(&path).unwrap();
    let e2 = scan_file(&path, meta.modified().unwrap(), meta.len(), Some(&e1)).expect("scan ok");
    assert_eq!(e2.name.as_deref(), Some("renamed-2"));
    assert_eq!(e2.message_count, e1.message_count + 1);
    // 再追加一条普通消息（无改名）→ 沿用旧名，不回落预览
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(
            b"{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"even more\"}}\n",
        )
        .unwrap();
    let meta = std::fs::metadata(&path).unwrap();
    let e3 = scan_file(&path, meta.modified().unwrap(), meta.len(), Some(&e2)).expect("scan ok");
    assert_eq!(e3.name.as_deref(), Some("renamed-2"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn needle_substring_in_other_entries_never_shadows_rename() {
    // 改名之后的行里出现 `"type":"session_info"` 裸子串（嵌套结构，
    // 比如讨论本仓源码的会话），不得冒充改名条目——解析校验 type 后
    // 才认。旧尾窗 rfind 版会把子串所在行当条目行，名字丢失。
    let root = temp_root("rename-shadow");
    let body = concat!(
        "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n",
        "{\"type\":\"session_info\",\"name\":\"real name\"}\n",
        "{\"type\":\"custom\",\"customType\":\"x\",\"details\":{\"type\":\"session_info\"}}\n",
    );
    let path = write_session(&root, "D:\\proj-s2", "s2", body);
    let meta = std::fs::metadata(&path).unwrap();
    let e = scan_file(&path, meta.modified().unwrap(), meta.len(), None).expect("scan ok");
    assert_eq!(e.name.as_deref(), Some("real name"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn scan_salvages_preview_when_first_user_line_truncated() {
    // Real-world shape: small header/meta lines, a fat system-prompt
    // line, then a first user message so big the 256KB prefix cuts its
    // line — full parse fails, the degraded path must still surface
    // the leading text (capped at PARTIAL_PREVIEW_CHARS).
    let root = temp_root("scan-trunc");
    let big = "甲".repeat(200_000); // 600KB utf8 >> PREFIX_BYTES
    let body = format!(
        concat!(
            "{{\"type\":\"model_change\",\"id\":\"m\"}}\n",
            "{{\"type\":\"message\",\"id\":\"s\",\"message\":{{\"role\":\"system\",\"content\":\"{}\"}}}}\n",
            "{{\"type\":\"message\",\"id\":\"u1\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"text\",\"text\":\"{}\"}}]}}}}\n",
        ),
        "sys ".repeat(30_000), // ~120KB system line
        big,
    );
    let path = write_session(&root, "D:\\proj-trunc", "trunc1", &body);
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    let e = scan_file(&path, mtime, body.len() as u64, None).expect("scan ok");
    let want: String = "甲".repeat(PARTIAL_PREVIEW_CHARS.min(40)); // 50 chars = 150B > 120B display cap → re-truncated with …
    assert_eq!(e.preview, format!("{want}…"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn partial_line_preview_rejects_assistant_and_bad_anchors() {
    // assistant line quoting the literal must not yield a preview
    let a = "{\"type\":\"message\",\"id\":\"a\",\"message\":{\"role\":\"assistant\",\"content\":\"see \\\"role\\\":\\\"user\\\" docs and \\\"text\\\":\\\"spoiled\"";
    assert_eq!(preview_from_partial_line(a), None);
    // role marker after content (inside quoted text) is rejected
    let late = "{\"message\":{\"content\":\"junk\"},\"role\":\"user\"}";
    assert_eq!(preview_from_partial_line(late), None);
    // proper string-content user line, escapes decoded, \n folded;
    // result capped at exactly 50 chars
    let ok = "{\"message\":{\"role\":\"user\",\"content\":\"line1\\nline2 \\\"quoted\\\" rest of a fairly long first message";
    assert_eq!(
        preview_from_partial_line(ok).as_deref(),
        Some("line1 line2 \"quoted\" rest of a fairly long first m")
    );
    // empty text → None (caller keeps scanning later lines)
    let empty = "{\"message\":{\"role\":\"user\",\"content\":\"\"}";
    assert_eq!(preview_from_partial_line(empty), None);
}

#[test]
fn list_for_cwd_targets_one_group() {
    let root = temp_root("groups");
    write_session(&root, "D:\\proj-a", "a1", BODY);
    write_session(&root, "D:\\proj-b", "b1", BODY);
    let mut s = Scanner::open(&root);
    let a = s.list_for_cwd("D:\\proj-a", 10);
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].id, "a1");
    assert_eq!(s.list_for_cwd("D:\\missing", 10).len(), 0);
    assert_eq!(s.list(10).len(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

/// 行路径恒为 on-disk 形态：调用方 cwd 大小写漂移（projects.json 里的
/// 小写 `d:` vs 盘上的 `D:`）不得带进 SessionInfo.path——否则与 pi 自报
/// 路径 / 通知点集合互相失配（行转圈、完成点全哑的真机回归根因）。
#[test]
fn list_for_cwd_paths_are_on_disk_form() {
    let root = temp_root("canon-list");
    write_session(&root, "D:\\proj-canon", "c1", BODY);
    let mut s = Scanner::open(&root);
    let rows = s.list_for_cwd("d:\\proj-canon", 10);
    assert_eq!(rows.len(), 1);
    // write_session 用大写 cwd 建 group；小写 cwd 拉列表也必须产大写形态
    assert_eq!(
        rows[0].path,
        root.join("--D--proj-canon--")
            .join(rows[0].path.file_name().unwrap())
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn list_skips_child_sessions_but_keeps_forks() {
    // 045：子代理会话靠自体内的 `subagent/child` marker 排除；pi 原生 fork
    // 分支（header 带 parentSession 但无 marker）是用户会话，必须上行。
    // 回归背景：曾用 parentSession 判定，把用户的 fork 置顶会话从侧栏全杀。
    let root = temp_root("children");
    let parent = write_session(&root, "D:\\proj-c", "parent1", BODY);
    let group = parent.parent().unwrap().to_path_buf();

    // fork 分支：header 带 parentSession，无 marker。
    let fork = group.join("2026-09-26T00-00-02-000Z_fork1.jsonl");
    std::fs::write(
        &fork,
        format!(
            "{{\"type\":\"session\",\"version\":3,\"id\":\"fork1\",\"cwd\":\"D:\\\\proj-c\",\"parentSession\":\"{}\"}}\n{}",
            parent.display().to_string().replace('\\', "\\\\"),
            BODY
        ),
    )
    .unwrap();

    // subagent 子会话：header 带 parentSession + 自体 marker entry。
    let child = group.join("2026-09-26T00-00-01-000Z_child1.jsonl");
    std::fs::write(
        &child,
        format!(
            "{{\"type\":\"session\",\"version\":3,\"id\":\"child1\",\"cwd\":\"D:\\\\proj-c\",\"parentSession\":\"{}\"}}\n{{\"type\":\"custom\",\"customType\":\"subagent/child\",\"id\":\"m1\",\"data\":{{\"provider\":\"spawn\",\"depth\":1}}}}\n{}",
            parent.display().to_string().replace('\\', "\\\\"),
            BODY
        ),
    )
    .unwrap();

    let mut s = Scanner::open(&root);
    let mut listed = s.list_for_cwd("D:\\proj-c", 10);
    listed.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(
        listed.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        vec!["fork1", "parent1"],
        "fork 上行、子代理被排除: {listed:?}"
    );

    // 子会话文件本身仍可解析，且 marker 被识别。
    let mtime = std::fs::metadata(&child).unwrap().modified().unwrap();
    let entry = scan_file(
        &child,
        mtime,
        std::fs::metadata(&child).unwrap().len(),
        None,
    )
    .expect("child scan ok");
    assert!(entry.is_child(), "marker 应被识别为子会话");
    let fmtime = std::fs::metadata(&fork).unwrap().modified().unwrap();
    let fork_entry = scan_file(&fork, fmtime, std::fs::metadata(&fork).unwrap().len(), None)
        .expect("fork scan ok");
    assert!(!fork_entry.is_child(), "fork 无 marker，不是子会话");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn fingerprint_index_hits_and_invalidates() {
    let root = temp_root("index");
    let path = write_session(&root, "D:\\proj", "x1", BODY);
    let mut s = Scanner::open(&root);
    assert_eq!(s.list(10).len(), 1);
    assert_eq!(s.scans, 1, "first list scans the file");
    assert_eq!(s.list(10).len(), 1);
    assert_eq!(s.scans, 1, "second list is index-served");

    // fingerprint change (size differs) forces a rescan — append keeps
    // the header line intact
    let mut content = std::fs::read_to_string(&path).unwrap();
    content.push_str("{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"z\"}}\n");
    std::fs::write(&path, content).unwrap();
    let infos = s.list(10);
    assert_eq!(s.scans, 2, "appended line invalidates the entry");
    assert_eq!(infos[0].message_count, 4);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn incremental_count_survives_append_and_partial_tail() {
    // 052 后活跃会话文件每轮重扫（用户每发一条消息 ListDirty 一次），
    // 计数必须只读新增后缀——全量流读几百 MB 会话文件发生在主线程就是
    // 「发送一条消息窗口就冻死」的实绩。半行（pi 正在写）不计、不推进。
    use std::io::Write as _;
    let root = temp_root("inccount");
    let path = write_session(&root, r"D:\proj", "ic1", BODY);
    let mut s = Scanner::open(&root);
    let first = s.list(10);
    let base_count = first[0].message_count;
    let base_bytes = s.entries.get(&path).unwrap().count_bytes;
    assert_eq!(base_count, 3);
    assert!(base_bytes > 0, "首扫即记录计数终点");

    // 追加 2 条 + 一条未写完的半行（无换行结尾）
    let mut full = String::new();
    for i in 0..2 {
        full.push_str(&format!(
            "{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"m{i}\"}}}}
"
        ));
    }
    let partial = "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"half";
    let app = format!("{full}{partial}");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(app.as_bytes())
        .unwrap();

    let second = s.list(10);
    assert_eq!(
        second[0].message_count,
        base_count + 2,
        "完整行计入，半行不计"
    );
    let mid_bytes = s.entries.get(&path).unwrap().count_bytes;
    assert_eq!(
        mid_bytes,
        base_bytes + full.len() as u64,
        "计数终点停在半行之前"
    );

    // 半行写完 → 下次扫描补计这一条
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(
            "\"}}
"
            .as_bytes(),
        )
        .unwrap();
    let third = s.list(10);
    assert_eq!(third[0].message_count, base_count + 3, "补齐的行下次补计");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn persisted_index_roundtrip() {
    let root = temp_root("persist");
    write_session(&root, "D:\\proj", "p1", BODY);
    let idx = root.join("index.json");
    {
        let mut a = Scanner::open_persisted(&root, &idx);
        a.list(10);
        a.save_index();
        assert_eq!(a.scans, 1);
    }
    let mut b = Scanner::open_persisted(&root, &idx);
    assert_eq!(b.list(10).len(), 1);
    assert_eq!(b.scans, 0, "warm scanner stats only, no file scans");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn index_version_bump_forces_full_rescan() {
    let root = temp_root("idx-ver");
    let path = write_session(&root, "D:\\proj", "v1", BODY);
    let idx = root.join("index.json");
    // hand-written v1 index (previous schema) with a stale empty preview
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    let dur = mtime.duration_since(UNIX_EPOCH).unwrap();
    let stale = serde_json::json!({
        "version": 1,
        "entries": {
            path.to_string_lossy(): {
                "mtime_sec": dur.as_secs(), "mtime_nsec": dur.subsec_nanos(),
                "size": std::fs::metadata(&path).unwrap().len(),
                "id": "v1", "cwd": "D:\\proj", "preview": "",
                "name": null, "message_count": 3,
            }
        }
    });
    crate::config::write_json(&idx, &stale).unwrap();
    let mut b = Scanner::open_persisted(&root, &idx);
    let list = b.list(10);
    assert_eq!(b.scans, 1, "v1 cache discarded → file rescanned");
    assert_eq!(list[0].preview, "hello 世界", "fresh scan, not stale cache");
    b.save_index();
    let saved = crate::config::read_json(&idx).unwrap();
    assert_eq!(saved["version"].as_u64(), Some(INDEX_VERSION));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn prune_drops_deleted_files() {
    let root = temp_root("prune");
    let path = write_session(&root, "D:\\proj", "d1", BODY);
    let mut s = Scanner::open(&root);
    assert_eq!(s.list(10).len(), 1);
    assert_eq!(s.entries.len(), 1);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(s.list(10).len(), 0);
    assert!(s.entries.is_empty(), "dead row pruned from the index");
    let _ = std::fs::remove_dir_all(&root);
}

// 原 name_tests 模块并入此处（itlq.6 外移）。
#[test]
fn parses_session_info_name_from_real_file_tail() {
    // regression for the rfind mid-line bug; machine-dependent by
    // design (the renamed session lives on the dev machine) — CI and
    // fresh checkouts skip silently. Fixture coverage for the same
    // parse path lives in scan_parses_summary_name_and_count.
    let home = match std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        Ok(h) => h,
        Err(_) => {
            eprintln!("no home dir, skipping");
            return;
        }
    };
    let p = Path::new(&home)
        .join(".pi/agent/sessions/--D--ai_workspace-pi_work--")
        .join("2026-09-25T06-12-27-503Z_01a0d731-9aee-71ef-982e-cfb077b412de.jsonl");
    let Ok(meta) = std::fs::metadata(&p) else {
        eprintln!("file missing, skipping");
        return;
    };
    let e = scan_file(&p, meta.modified().unwrap(), meta.len(), None).expect("session parsed");
    eprintln!(
        "parsed name={:?} preview={} count={}",
        e.name, e.preview, e.message_count
    );
    assert!(e.name.is_some(), "session_info name must be parsed");
}
