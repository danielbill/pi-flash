//! 跨进程投影 `ext-context.json`（001-2）。
//!
//! 只装**扩展需要**的段：注册表 `exportable` 白名单 + 导出时二次过滤（不依赖
//! section 自觉，默认拒绝）。顶层 `{version, generated_at, <段名>...}`，扩展
//! 按段名直取。**PF 是唯一写者**，扩展禁止回写。payload（段内容）哈希不变则
//! 整体跳过——`generated_at` 只在内容真变时前进，扩展才能拿它判断「读到的是
//! 不是旧档」。
//!
//! 触发点：flush 有实写后 / 外部重载成功后 / open 初次（后台线程）。debounce
//! 由写通道的 50ms coalesce 天然承担。

use super::{Snapshot, sections};
use std::path::Path;

/// 投影文件名（PF 数据目录下）。
pub(crate) const PROJECTION_FILENAME: &str = "ext-context.json";
/// 投影格式版本：扩展据此决定回退行为（不匹配 = 视为不可用，001-2 §4）。
pub(crate) const PROJECTION_VERSION: u64 = 1;

/// 投影键名禁入片段（001-2 §6：api key / token / oauth / 秘密类永不进投影；
/// 对导出 JSON 的**键名**递归扫描做硬闸，命中 = 拒绝导出并告警）。
const FORBIDDEN_KEY_FRAGMENTS: [&str; 6] = ["api", "token", "key", "secret", "oauth", "password"];

/// 组装 + 落盘投影。返回是否真的写了（payload 哈希与上次一致 = 跳过）。
/// `last_payload` 由调用方持有（0 = 尚未导出过）。
pub(crate) fn export(snap: &Snapshot, dir: &Path, last_payload: &mut u64) -> Result<bool, String> {
    let mut payload = serde_json::Map::new();
    for def in sections::registered() {
        if !def.exportable {
            continue;
        }
        if let Some(json) = snap.section_json.get(def.key) {
            payload.insert(def.key.to_string(), json.clone());
        }
    }
    let serialized = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    let hash = super::content_hash(serialized.as_bytes());
    if *last_payload == hash && *last_payload != 0 {
        return Ok(false); // 段内容未变：不重写，generated_at 保持上次变更时刻
    }
    guard_no_sensitive(&payload)?;

    let mut doc = serde_json::Map::new();
    doc.insert(
        "version".into(),
        serde_json::Value::from(PROJECTION_VERSION),
    );
    doc.insert(
        "generated_at".into(),
        serde_json::Value::String(
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ),
    );
    for (k, v) in payload {
        doc.insert(k, v);
    }
    pi_link::config::write_json(
        &dir.join(PROJECTION_FILENAME),
        &serde_json::Value::Object(doc),
    )?;
    *last_payload = hash;
    Ok(true)
}

/// 敏感键硬闸（001-2 §6 + S3 验收：扫描导出 JSON 不含 api/token/key 等
/// 敏感键——白名单导出默认拒绝，这里再兜一层底）。
fn guard_no_sensitive(payload: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    fn scan(prefix: &str, v: &serde_json::Value) -> Result<(), String> {
        match v {
            serde_json::Value::Object(map) => {
                for (k, sub) in map {
                    let lk = k.to_ascii_lowercase();
                    if FORBIDDEN_KEY_FRAGMENTS.iter().any(|f| lk.contains(f)) {
                        return Err(format!("投影含敏感键 `{prefix}{k}`，拒绝导出（001-2 §6）"));
                    }
                    scan(&format!("{prefix}{k}."), sub)?;
                }
                Ok(())
            }
            serde_json::Value::Array(items) => items
                .iter()
                .enumerate()
                .try_for_each(|(i, v)| scan(&format!("{prefix}[{i}]"), v)),
            _ => Ok(()),
        }
    }
    scan("", &serde_json::Value::Object(payload.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::sections::Subagents;
    use serde_json::json;

    #[test]
    fn whitelist_filters_non_exportable_sections() {
        let dir = crate::tests_support::tmp_dir("export-wl");
        let store = crate::tests_support::store_at(&dir);
        store.set(|c: &mut crate::settings::sections::Chat| c.git_markers = Some(true));
        store.flush().unwrap();
        let snap = store.snapshot_clone();
        let mut last = 0;
        export(&snap, &dir, &mut last).unwrap();
        let doc = crate::tests_support::read_json_file(&dir.join(PROJECTION_FILENAME));
        assert_eq!(doc["version"], 1);
        assert!(doc["generated_at"].is_string());
        assert_eq!(doc["subagents"]["capacity"], 3, "exportable 段进投影");
        assert!(doc.get("chat").is_none(), "未声明 exportable 的段不出现");
        assert!(doc.get("appearance").is_none());
        assert!(doc.get("dev_flags").is_none(), "boot-only 段永不进投影");
    }

    #[test]
    fn same_payload_skips_write_keeps_generated_at() {
        let dir = crate::tests_support::tmp_dir("export-skip");
        let store = crate::tests_support::store_at(&dir);
        let snap = store.snapshot_clone();
        let mut last = 0;
        assert!(export(&snap, &dir, &mut last).unwrap(), "首导出写盘");
        let first_at =
            crate::tests_support::read_json_file(&dir.join(PROJECTION_FILENAME))["generated_at"]
                .clone();
        assert!(!export(&snap, &dir, &mut last).unwrap(), "同 payload 跳过");
        // 内容变化（容量改）→ 重写（generated_at 粒度是秒，同秒内不比较值）
        store.set(|s: &mut Subagents| s.capacity = 6);
        let snap2 = store.snapshot_clone();
        assert!(export(&snap2, &dir, &mut last).unwrap());
        let doc = crate::tests_support::read_json_file(&dir.join(PROJECTION_FILENAME));
        assert_eq!(doc["subagents"]["capacity"], 6);
        assert!(doc["generated_at"].is_string());
        let _ = first_at;
    }

    #[test]
    fn sensitive_keys_rejected() {
        let mut payload = serde_json::Map::new();
        payload.insert("subagents".into(), json!({"capacity": 3}));
        assert!(guard_no_sensitive(&payload).is_ok());
        payload.insert("models".into(), json!({"api_key": "sk-xxx"}));
        assert!(guard_no_sensitive(&payload).is_err(), "api 键命中硬闸");
        let mut payload = serde_json::Map::new();
        payload.insert("auth".into(), json!({"oauth_token": "x"}));
        assert!(guard_no_sensitive(&payload).is_err());
    }
}
