use super::*;
use serde_json::json;
use std::time::SystemTime;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pf-auth-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("auth.json")
}

const MARKER: &str = "!'C:/pf/pif-secret.exe' get deepseek";

#[test]
fn marker_detection_covers_three_shapes() {
    assert!(is_pf_managed(&json!({"type":"api_key","key":MARKER})));
    assert!(!is_pf_managed(&json!({"type":"api_key","key":"sk-plain"})));
    assert!(!is_pf_managed(
        &json!({"type":"oauth","access":"a","refresh":"r","expires":1})
    ));
    assert_eq!(
        plaintext_key(&json!({"type":"api_key","key":"sk-plain"})),
        Some("sk-plain")
    );
    assert_eq!(plaintext_key(&json!({"type":"api_key","key":MARKER})), None);
}

#[test]
fn store_and_remove_marker_roundtrip() {
    let path = tmp("roundtrip");
    store_marker_at(&path, "deepseek", MARKER).unwrap();
    let cred = read_credential_at(&path, "deepseek").unwrap().unwrap();
    assert!(is_pf_managed(&cred));
    assert!(remove_marker_at(&path, "deepseek").unwrap());
    assert!(read_credential_at(&path, "deepseek").unwrap().is_none());
    // 二次删除：幂等
    assert!(!remove_marker_at(&path, "deepseek").unwrap());
}

#[test]
fn remove_refuses_user_plaintext_and_oauth_and_keeps_bytes() {
    let path = tmp("refuse");
    let original = json!({
        "deepseek": { "type": "api_key", "key": "sk-user-plain" },
        "anthropic": { "type": "oauth", "access": "a", "refresh": "r", "expires": 9 }
    });
    write_json_private(&path, &original).unwrap();
    let before = std::fs::read(&path).unwrap();

    assert!(!remove_marker_at(&path, "deepseek").unwrap());
    assert!(!remove_marker_at(&path, "anthropic").unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), before, "非标记条目不得改动");
}

#[test]
fn store_marker_preserves_other_entries() {
    let path = tmp("preserve");
    write_json_private(
        &path,
        &json!({
            "anthropic": { "type": "oauth", "access": "a", "refresh": "r", "expires": 9 },
            "glm": { "type": "api_key", "key": "!mine" }
        }),
    )
    .unwrap();
    store_marker_at(&path, "deepseek", MARKER).unwrap();
    let doc = read_json(&path).unwrap();
    assert_eq!(doc["anthropic"]["type"], "oauth");
    assert_eq!(doc["glm"]["key"], "!mine");
    assert!(is_pf_managed(&doc["deepseek"]));
}

#[test]
fn seal_plaintext_only_when_value_matches() {
    let path = tmp("seal");
    write_json_private(
        &path,
        &json!({ "deepseek": { "type": "api_key", "key": "sk-old" } }),
    )
    .unwrap();
    assert!(!seal_plaintext_at(&path, "deepseek", "sk-newer", MARKER).unwrap());
    assert!(seal_plaintext_at(&path, "deepseek", "sk-old", MARKER).unwrap());
    assert!(is_pf_managed(
        &read_credential_at(&path, "deepseek").unwrap().unwrap()
    ));
}

#[test]
fn broken_file_is_never_clobbered() {
    let path = tmp("broken");
    std::fs::write(&path, b"{ not json").unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(store_marker_at(&path, "deepseek", MARKER).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn kinds_reads_types() {
    let path = tmp("kinds");
    write_json_private(
        &path,
        &json!({
            "a": { "type": "api_key", "key": "x" },
            "b": { "type": "oauth", "access": "a", "refresh": "r", "expires": 1 }
        }),
    )
    .unwrap();
    let kinds = kinds_at(&path).unwrap();
    assert!(kinds.contains(&("a".into(), CredentialKind::ApiKey)));
    assert!(kinds.contains(&("b".into(), CredentialKind::OAuth)));
}

#[test]
fn marker_key_is_shell_safe() {
    // 路径形态（PATH 模式未就绪的回落）
    let key = marker_key_with(false, "deepseek");
    assert!(key.starts_with("!'"));
    assert!(key.ends_with("' get deepseek"));
    assert!(!key.contains('\\'), "路径反斜杠会被 shell 吞掉");
    // 裸命令形态（PATH 模式）：零路径，升级/搬家不失效
    assert_eq!(
        marker_key_with(true, "deepseek"),
        "!pif-secret get deepseek"
    );
    assert_eq!(marker_key_with(true, "zai"), "!pif-secret get zai");
}

#[test]
fn migrate_rewrites_path_markers_to_bare_form() {
    let path = tmp("migrate");
    let hand_fixed = "!'D:/softwares/node/pi-flash/payload-0.4.1/pif-secret.exe' get zai";
    write_json_private(
        &path,
        &json!({
            // PF 写的绝对路径形态 → 迁
            "deepseek": { "type": "api_key", "key": MARKER },
            // 0.4.1 事故里用户手改的形态 → 也迁
            "zai": { "type": "api_key", "key": hand_fixed },
            // 裸形态（已迁）→ 不动
            "glm": { "type": "api_key", "key": "!pif-secret get glm" },
            // oauth / 用户明文 / 用户 `!` 引用 → 一律不动
            "anthropic": { "type": "oauth", "access": "a", "refresh": "r", "expires": 9 },
            "mine": { "type": "api_key", "key": "sk-user-plain" },
            "other": { "type": "api_key", "key": "!my-cmd --flag" }
        }),
    )
    .unwrap();
    let n = migrate_markers_pathless_at(&path, true).unwrap();
    assert_eq!(n, 2, "只迁两条路径形态标记");
    let doc = read_json(&path).unwrap();
    assert_eq!(doc["deepseek"]["key"], "!pif-secret get deepseek");
    assert_eq!(doc["zai"]["key"], "!pif-secret get zai");
    assert_eq!(doc["glm"]["key"], "!pif-secret get glm");
    assert_eq!(doc["anthropic"]["type"], "oauth");
    assert_eq!(doc["mine"]["key"], "sk-user-plain");
    assert_eq!(doc["other"]["key"], "!my-cmd --flag");
    // 幂等：二跑零改动
    assert_eq!(migrate_markers_pathless_at(&path, true).unwrap(), 0);
}

#[test]
fn migrate_off_leaves_file_untouched() {
    let path = tmp("migrate-off");
    write_json_private(
        &path,
        &json!({ "deepseek": { "type": "api_key", "key": MARKER } }),
    )
    .unwrap();
    let before = std::fs::read(&path).unwrap();
    assert_eq!(migrate_markers_pathless_at(&path, false).unwrap(), 0);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "PATH 模式未就绪不得写"
    );
}

#[test]
fn migrate_refuses_unreadable_file() {
    let path = tmp("migrate-broken");
    std::fs::write(&path, "{not json").unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(
        migrate_markers_pathless_at(&path, true).is_err(),
        "不可读拒写"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before, "拒写 = 文件零改动");
}
