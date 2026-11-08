//! app_settings 测试（itlq.9 自 app_settings.rs 外移，纯结构搬移）。

use crate::services::workspace::{load_map_from, save_map_to};
use serde_json::Value;
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
fn app_settings_roundtrip() {
    let path = tmpfile("app-settings.json");
    let mut obj = serde_json::Map::new();
    obj.insert("theme".into(), Value::String("mist".into()));
    obj.insert(
        "session_font".into(),
        serde_json::json!({"family": "Inter", "size": 14.5}),
    );
    obj.insert("lang".into(), Value::Number(1u64.into()));
    save_map_to(&path, &obj);
    let map = load_map_from(&path);
    let spec: pf_context::sections::FontSpec =
        serde_json::from_value(map.get("session_font").unwrap().clone()).unwrap();
    assert_eq!(spec.family, "Inter");
    assert_eq!(spec.size, 14.5);
    assert_eq!(map.get("theme").unwrap(), "mist");
    assert_eq!(
        map.get("lang").and_then(|v| v.as_u64()),
        Some(1),
        "lang/sound live in app_settings.json, not the workspace map"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
