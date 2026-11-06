use super::results::{group_hits, project_name};
use super::*;
use pi_link::sessions::SearchHit;
use std::time::SystemTime;

fn hit(path: &str, name: Option<&str>, ts: i64) -> SearchHit {
    SearchHit {
        session_path: std::path::PathBuf::from(path),
        project: Some(r"D:\ai_workspace\pi-flash".to_string()),
        session_name: name.map(str::to_string),
        preview: String::new(),
        modified: SystemTime::UNIX_EPOCH,
        ts: Some(ts),
        role: "user".into(),
        before: "aaa ".into(),
        match_text: "needle".into(),
        after: " bbb".into(),
    }
}

/// 同一个会话即使命中交错出现，也只能有一个组头
#[test]
fn interleaved_hits_collapse_into_one_group() {
    let hits = vec![
        hit("a.jsonl", Some("会话A"), 1),
        hit("b.jsonl", Some("会话B"), 2),
        hit("a.jsonl", None, 3),
    ];
    let groups = group_hits(&hits);
    assert_eq!(groups.len(), 2, "a/b/a 应归并成两个组");
    assert_eq!(groups[0].key, "a.jsonl");
    assert_eq!(groups[0].hits.len(), 2, "a 的两条命中归回同一组");
    assert_eq!(groups[1].key, "b.jsonl");
}

/// 项目名：末段路径；UUID 目录只留前 8 位
#[test]
fn project_name_takes_last_segment() {
    assert_eq!(
        project_name(r"D:\ai_workspace\pi-flash").as_deref(),
        Some("pi-flash")
    );
    assert_eq!(project_name("/home/u/work/").as_deref(), Some("work"));
    assert_eq!(
        project_name(r"C:\x\projects\c57695c6-bfcf-45fa-a54f-8540fb673f8b").as_deref(),
        Some("c57695c6")
    );
}

/// 相邻同会话的命中归一块，换会话才开新块（组头 = 会话名 + 时间）。
#[test]
fn groups_adjacent_hits_by_session() {
    let hits = vec![
        hit("a.jsonl", Some("会话A"), 1),
        hit("a.jsonl", None, 2),
        hit("b.jsonl", None, 3),
    ];
    let groups = group_hits(&hits);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].hits.len(), 2, "同会话两条命中进同一块");
    assert_eq!(groups[0].title.as_ref(), "会话A");
    assert_eq!(groups[1].hits.len(), 1);
    // 无名字无预览 → 回落到文件名（stem）
    assert_eq!(groups[1].title.as_ref(), "b");
    // 副标题 = 项目名 · 时间（全量搜索时用户要看清是哪个项目）
    assert!(
        groups[0].sub.starts_with("pi-flash · "),
        "副标题应带项目名，实测 {:?}",
        groups[0].sub
    );
}
