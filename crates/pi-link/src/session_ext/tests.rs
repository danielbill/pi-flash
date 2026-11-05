use super::*;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pi-flash-sessext-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn roundtrip_per_conversation() {
    let p = tmp("roundtrip.json");
    let _ = std::fs::remove_file(&p);
    write_for(&p, "C:/s/a.jsonl", &["npm:x".into(), "npm:y".into()]).unwrap();
    write_for(&p, "C:/s/b.jsonl", &["npm:z".into()]).unwrap();
    assert_eq!(read_for(&p, "C:/s/a.jsonl"), vec!["npm:x", "npm:y"]);
    assert_eq!(read_for(&p, "C:/s/b.jsonl"), vec!["npm:z"]);
    assert!(read_for(&p, "missing").is_empty());
}

#[test]
fn empty_list_removes_entry_and_missing_file_is_empty() {
    let p = tmp("empty.json");
    let _ = std::fs::remove_file(&p);
    assert!(read(&p).is_empty(), "文件缺失 = 空表");
    write_for(&p, "k", &["npm:a".into()]).unwrap();
    write_for(&p, "k", &[]).unwrap();
    assert!(read(&p).is_empty(), "空清单不落空数组");
    assert!(remove_for(&p, "nope").is_ok());
}

#[test]
fn key_normalization_matches_both_windows_spellings() {
    let p = tmp("norm.json");
    let _ = std::fs::remove_file(&p);
    // 写入 pi 的反斜杠形态（get_state.sessionFile），读出用 app 的正斜杠形态
    let win = format!("C:\\Users\\bi_da\\.pi\\agent\\sessions\\x.jsonl");
    write_for(&p, &win, &["npm:a".into()]).unwrap();
    assert_eq!(
        read_for(&p, "C:/Users/bi_da/.pi/agent/sessions/x.jsonl"),
        vec!["npm:a"]
    );
    // 反向：写正斜杠、读出反斜杠 + 盘符小写
    write_for(&p, "C:/tmp/y.jsonl", &["npm:b".into()]).unwrap();
    let win2 = format!("c:\\tmp\\y.jsonl");
    assert_eq!(read_for(&p, &win2), vec!["npm:b"]);
}

/// 插件挂载通道（真机 2026-10-31：勾 2 显 [3]）——scheme 源以勾选为准，
/// 路径型条目跨勾选保留；路径判定与平台绝对路径口径一致。
#[test]
fn mount_paths_survive_user_pick_rewrites() {
    let mount = std::env::temp_dir()
        .join("plugins/demo-plugin/pi/index.mjs")
        .to_string_lossy()
        .into_owned();
    assert!(is_mount_path(&mount), "绝对路径 = 挂载条目");
    assert!(!is_mount_path("npm:pi-freeflow"));
    assert!(!is_mount_path("git:https://github.com/u/r"));

    let existing = vec!["npm:old".into(), mount.clone()];
    // 用户取消勾了 npm:old、新勾 npm:new——old 该走就走，mount 必须留下
    let merged = with_mounts_preserved(&existing, &["npm:new".to_string()]);
    assert_eq!(merged, vec!["npm:new".to_string(), mount.clone()]);

    // 幂等：以合并结果为 existing 再合并不增生
    let again = with_mounts_preserved(&merged, &["npm:new".to_string()]);
    assert_eq!(again, merged);
}

#[test]
fn write_preserving_mounts_roundtrip() {
    let p = tmp("preserve.json");
    let _ = std::fs::remove_file(&p);
    let mount = std::env::temp_dir()
        .join("plugins/demo-plugin/pi/index.mjs")
        .to_string_lossy()
        .into_owned();
    // 先按插件挂载通道的形态落一份（历史 attach 曾整表覆写：台账只剩路径）
    write_for(&p, "k", &[mount.clone()]).unwrap();
    // 用户勾选落盘：mount 保留，npm 勾选进来
    write_for_preserving_mounts(&p, "k", &["npm:pi-freeflow".into()]).unwrap();
    assert_eq!(
        read_for(&p, "k"),
        vec!["npm:pi-freeflow".to_string(), mount]
    );
}

#[test]
fn broken_file_degrades_to_empty() {
    let p = tmp("broken.json");
    std::fs::write(&p, "{ not json").unwrap();
    assert!(read(&p).is_empty());
}
