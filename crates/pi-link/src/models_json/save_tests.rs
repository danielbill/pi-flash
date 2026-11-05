use super::*;
use serde_json::json;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pf-mj-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("models.json")
}

fn doc() -> Value {
    json!({"providers":{"glm":{"baseUrl":"https://x/v1","api":"openai-completions",
        "models":[{"id":"glm-5","contextWindow":128000,"maxTokens":8192}]}}})
}

#[test]
fn save_writes_and_is_idempotent() {
    let path = tmp("idem");
    save_at_locked(&path, &doc()).unwrap();
    let first = std::fs::read(&path).unwrap();
    save_at_locked(&path, &doc()).unwrap();
    assert_eq!(first, std::fs::read(&path).unwrap(), "重复保存必须字节一致");
    let back: Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(back["providers"]["glm"]["models"][0]["id"], "glm-5");
    // 缺省形状补齐（input/reasoning/cost）
    assert_eq!(back["providers"]["glm"]["models"][0]["input"][0], "text");
}

#[test]
fn broken_file_is_never_clobbered() {
    let path = tmp("broken");
    std::fs::write(&path, b"{ not json").unwrap();
    let before = std::fs::read(&path).unwrap();
    let err = save_at_locked(&path, &doc()).unwrap_err();
    assert!(err.contains("无法解析"), "错误信息应指明原因：{err}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn video_modality_is_rejected() {
    let path = tmp("video");
    let bad = json!({"providers":{"glm":{"models":[{"id":"m","input":["text","video"]}]}}});
    let err = save_at_locked(&path, &bad).unwrap_err();
    assert!(err.contains("非法模态"), "{err}");
    assert!(!path.exists(), "校验失败不得落盘");
}

#[test]
fn missing_id_is_rejected() {
    let path = tmp("noid");
    let bad = json!({"providers":{"glm":{"models":[{"name":"x"}]}}});
    assert!(save_at_locked(&path, &bad).is_err());
}

#[test]
fn missing_providers_key_is_rejected() {
    let path = tmp("noprov");
    assert!(save_at_locked(&path, &json!({})).is_err());
}
