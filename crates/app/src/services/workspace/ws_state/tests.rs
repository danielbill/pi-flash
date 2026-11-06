//! ws_state 测试（itlq.9 自 ws_state.rs 外移，纯结构搬移）：purge 内核 /
//! 窗口布局 / 文件树状态 / dock 映射 / tab 带存档。

use super::*;
use crate::services::workspace::load_map_from;
use std::path::PathBuf;

fn tmpfile(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pi-flash-ws-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}
#[test]
fn purge_app_memory_drops_poisoned_keys_only() {
    let root = Path::new("D:/softwares/pi-flash");
    let mut map = serde_json::Map::new();
    map.insert(
        WS_LAST_KEY.into(),
        Value::String("d:\\softwares\\pi-flash\\payload".into()),
    );
    map.insert(
        "d:\\softwares\\pi-flash\\payload".into(),
        Value::String("sess.jsonl".into()),
    );
    map.insert(
        "d:\\ai_workspace\\pi_work".into(),
        Value::String("keep.jsonl".into()),
    );
    map.insert("__window".into(), serde_json::json!({"max": true}));
    assert_eq!(purge_app_paths_in_map(&mut map, Some(root)), 2);
    assert!(!map.contains_key(WS_LAST_KEY), "脏 __last 删键");
    assert!(!map.contains_key("d:\\softwares\\pi-flash\\payload"));
    assert_eq!(map.get("d:\\ai_workspace\\pi_work").unwrap(), "keep.jsonl");
    assert!(map.contains_key("__window"), "非路径键不动");
    assert_eq!(purge_app_paths_in_map(&mut map, None), 0);
    assert_eq!(purge_app_paths_in_map(&mut map, Some(root)), 0, "幂等");
}
#[test]
fn window_dock_state_roundtrip_via_map() {
    let path = tmpfile("layout.json");
    let mut map = serde_json::Map::new();
    map.insert(
        WS_WINDOW_KEY.into(),
        serde_json::json!({"x": 10.0, "y": 20.0, "w": 1180.0, "h": 760.0, "max": true}),
    );
    map.insert(
        "__ui".into(),
        serde_json::json!({"panel": "git", "slp_w": 300.0, "panes_hidden": false,
                            "list_mode": "grouped", "sort_mode": "time", "collapsed": []}),
    );
    save_map_to(&path, &map);
    let loaded = load_map_from(&path);
    let w = loaded.get(WS_WINDOW_KEY).unwrap();
    assert_eq!(w.get("max").unwrap(), &Value::Bool(true));
    assert_eq!(w.get("w").unwrap(), &serde_json::json!(1180.0));
    let d = loaded.get("__ui").unwrap();
    assert_eq!(d.get("panel").unwrap(), "git");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
#[test]
fn tree_state_rel_abs_roundtrip_and_prune() {
    let base = tmpfile("tree").parent().unwrap().to_path_buf();
    let cwd = base.join("proj");
    std::fs::create_dir_all(cwd.join("src").join("deep")).unwrap();
    std::fs::create_dir_all(cwd.join("docs")).unwrap();

    let expanded: std::collections::HashSet<PathBuf> = [
        cwd.clone(),
        cwd.join("src"),
        cwd.join("src").join("deep"),
        cwd.join("docs"),
        base.join("other"),
    ]
    .into_iter()
    .collect();
    let rel = tree_rel_paths(&cwd, &expanded);
    assert!(rel.iter().any(|p| p.is_empty()), "根存空串");
    assert!(rel.iter().any(|p| p == "src"), "相对路径");
    assert!(!rel.iter().any(|p| p.contains("other")), "项目外丢弃");

    let back = tree_abs_paths(&cwd, &rel);
    assert!(back.contains(&cwd) && back.contains(&cwd.join("src")));
    assert!(back.contains(&cwd.join("src").join("deep")));

    std::fs::remove_dir_all(cwd.join("docs")).unwrap();
    let pruned = tree_abs_paths(
        &cwd,
        &["docs".to_string(), "src".to_string(), "..".to_string()],
    );
    assert_eq!(pruned.len(), 2, "只剩根与 src");
    assert!(pruned.contains(&cwd));
    assert!(!pruned.contains(&cwd.join("docs")));
    let _ = std::fs::remove_dir_all(&base);
}
#[test]
fn tree_state_default_is_not_stored() {
    let base = tmpfile("tree-store").parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&base).unwrap();
    let cwd = base.join("proj");
    std::fs::create_dir_all(&cwd).unwrap();
    let cwd_s = cwd.to_string_lossy().to_string();

    let mut map = serde_json::Map::new();
    tree_state_into_map(
        &mut map,
        &cwd_s,
        &TreeState {
            expanded: vec![String::new()],
            pin: false,
        },
    );
    assert!(tree_state_from(&map, &cwd_s).is_none(), "默认态不占槽位");

    tree_state_into_map(
        &mut map,
        &cwd_s,
        &TreeState {
            expanded: vec![String::new(), "src".into()],
            pin: true,
        },
    );
    let st = tree_state_from(&map, &cwd_s).expect("有内容应可读回");
    assert_eq!(st.expanded, vec![String::new(), "src".to_string()]);
    assert!(st.pin);
    let _ = std::fs::remove_dir_all(&base);
}
#[test]
fn dock_max_mapping_roundtrips() {
    for (sess_min, files_min) in [(false, false), (true, false), (false, true)] {
        let dm = dock_max_from_min(sess_min, files_min);
        assert_eq!(min_from_dock_max(dm), (sess_min, files_min), "dm={dm}");
    }
    assert_eq!(dock_max_from_min(true, false), "files", "文件树最大化");
    assert_eq!(dock_max_from_min(false, true), "sessions", "会话列表最大化");
    assert_eq!(min_from_dock_max("garbage"), (false, false), "坏值回落常态");
}
#[test]
fn tabs_state_roundtrip_and_prune() {
    let state = TabsState {
        tabs: vec![
            SavedTab {
                kind: "file".into(),
                path: "D:\\a\\b.rs".into(),
            },
            SavedTab {
                kind: "term".into(),
                path: "D:\\a".into(),
            },
        ],
        active: Some(1),
        chat: false,
    };
    let v = tabs_state_to_value(&state).unwrap();
    assert_eq!(tabs_state_from_value(&v).unwrap(), state);

    // 越界 active 被读侧滤掉
    let v = tabs_state_to_value(&TabsState {
        active: Some(9),
        ..state.clone()
    })
    .unwrap();
    assert_eq!(
        tabs_state_from_value(&v).unwrap().active,
        None,
        "越界下标不采信"
    );

    // 空带 = None（写侧据此移除槽位）
    assert!(tabs_state_to_value(&TabsState::default()).is_none());

    // 非法条目读侧丢弃（kind 不识 / 空 path）
    let v = serde_json::json!({ "tabs": [
        {"k": "file", "p": "D:\\x.rs"},
        {"k": "market", "p": ""},
        {"k": "term", "p": ""},
    ], "active": 0, "chat": true });
    let back = tabs_state_from_value(&v).unwrap();
    assert_eq!(back.tabs.len(), 1, "非法条目丢弃");

    // 旧版 per-project 嵌套形态（无顶层 tabs）= 不识别
    let legacy = serde_json::json!({ "d:\\a": { "tabs": [
        {"k": "file", "p": "D:\\x.rs"}], "active": 0, "chat": true }});
    assert!(tabs_state_from_value(&legacy).is_none(), "旧形态不采信");
}
