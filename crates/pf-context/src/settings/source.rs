//! 盘面读取层：`app-settings.json` → 原始 Map → 各 section 的 L1 层值。
//!
//! 缺失 ≠ 坏档：缺失是首启常态（全 Default）；坏档（非法 JSON / 非对象）
//! 由上层保上一份好快照并记 problem（001-0 §5）。

use serde_json::{Map, Value};
use std::path::Path;

/// 盘面文档状态。
pub(crate) enum Doc {
    /// 文件不存在（首启常态）
    Missing,
    /// 合法对象
    Ok(Map<String, Value>),
    /// 非法 JSON / 非对象（坏档）
    Broken,
}

pub(crate) fn read_doc(path: &Path) -> Doc {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Doc::Missing;
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Doc::Ok(map),
        _ => Doc::Broken,
    }
}

/// 一个 section 的 L1 层值：v2 section 键命中优先（哪怕形状怪——由 build
/// 报 problem 回 Default），否则按 v1 平铺键合成（001-1 §3 读迁移）。
pub(crate) fn layer1_for(key: &str, map: &Map<String, Value>) -> Option<Value> {
    if let Some(v) = map.get(key) {
        return Some(v.clone());
    }
    super::sections::read_legacy(key, map)
}
