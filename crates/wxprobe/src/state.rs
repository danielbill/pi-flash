//! 探针状态：扫码会话、bot_token、长轮询游标、上次入站会话标识。
//!
//! 落盘位置跟 `pi_link::paths`（`PI_FLASH_DIR` 优先，见 `060-远程控制-微信.md` §3）。
//! 游标持久化遵循 ZCode 的 bugfix：**等本批消息全部处理完再写**，
//! 先写游标会让进程中途失败时跳过未处理消息（`weixinChannelRuntime.ts:155-163`）。

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

pub fn base_dir() -> PathBuf {
    pi_link::paths::dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn state_path() -> PathBuf {
    base_dir().join("wxprobe-state.json")
}

pub fn dump_dir() -> PathBuf {
    base_dir().join("wxprobe-dump")
}

pub fn load() -> Value {
    fs::read_to_string(state_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

pub fn save(state: &Value) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let body = serde_json::to_string_pretty(state).unwrap_or_else(|_| state.to_string());
    if let Err(e) = fs::write(&path, body) {
        eprintln!("[wxprobe] 写状态失败 {}: {e}", path.display());
    }
}

pub fn get_str(state: &Value, key: &str) -> Option<String> {
    state
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn set(state: &mut Value, key: &str, value: Option<&str>) {
    let obj = state.as_object_mut().expect("state 必须是对象");
    match value {
        Some(v) => {
            obj.insert(key.to_string(), json!(v));
        }
        None => {
            obj.remove(key);
        }
    }
}

/// P0 新生成的 client_id（对齐 ZCode `zcode-weixin-<uuid>` 形态）。
pub fn ensure_client_id(state: &mut Value) -> String {
    if let Some(v) = get_str(state, "client_id") {
        return v;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let id = format!("wxprobe-{:08x}-{:08x}", std::process::id(), (nanos & 0xFFFF_FFFF) as u32);
    set(state, "client_id", Some(&id));
    id
}
