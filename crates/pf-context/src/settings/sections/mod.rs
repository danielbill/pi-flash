//! section 注册表（001-1 §3：25 键的落点。S1 只通电 `subagents`，S2 扩全表）。
//!
//! 分派函数（`build` / `read_legacy` / `write_legacy`）按 KEY 显式 match——
//! section 数量少时最直读；S2 扩表时若嫌长再收宏。

mod subagents;

pub use subagents::Subagents;

use crate::settings::section::{Section, deep_merge};
use serde_json::Value;
use std::any::Any;
use std::sync::Arc;

/// 已注册 section 键（快照构建顺序）。
pub(crate) fn registered() -> &'static [&'static str] {
    #[cfg(test)]
    return &[Subagents::KEY, probe::Probe::KEY];
    #[cfg(not(test))]
    return &[Subagents::KEY];
}

/// 测试专用的第二 section：S1 生产注册表只有一段，而「单段坏、其它段正常」
/// 的断言需要两段才能表达。不进公开 API。
#[cfg(test)]
pub(crate) mod probe {
    use serde::{Deserialize, Serialize};

    use crate::settings::section::Section;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
    #[serde(default)]
    pub struct Probe {
        pub value: u32,
    }

    impl Section for Probe {
        const KEY: &'static str = "z_probe";

        fn validate(&mut self) {
            self.value = self.value.min(99);
        }
    }
}

/// 各层（`layers[0..4]` = L0 缺省占位 / L1 文件 / L2 项目 / L3 会话）合并成
/// typed section，返回（typed Arc，validate 后的规范 JSON）。
/// 层值非法 → `Err`（调用方记 problem 并回退 Default）。
pub(crate) fn build(
    key: &str,
    layers: &[Option<Value>; 4],
) -> Result<(Arc<dyn Any + Send + Sync>, Value), String> {
    match key {
        Subagents::KEY => build_one::<Subagents>(layers),
        #[cfg(test)]
        probe::Probe::KEY => build_one::<probe::Probe>(layers),
        _ => Err(format!("未注册的 section: {key}")),
    }
}

fn build_one<T: Section>(
    layers: &[Option<Value>; 4],
) -> Result<(Arc<dyn Any + Send + Sync>, Value), String> {
    // L0：代码 Default（恒有值）
    let mut v = serde_json::to_value(T::default()).map_err(|e| e.to_string())?;
    for over in layers {
        if let Some(over) = over {
            deep_merge(&mut v, over);
        }
    }
    let mut t: T = serde_json::from_value(v).map_err(|e| format!("{} 解析失败: {e}", T::KEY))?;
    t.validate();
    let json = serde_json::to_value(&t).map_err(|e| e.to_string())?;
    Ok((Arc::new(t), json))
}

/// 全部 layer 缺省（= 纯 Default 构建，恒成功）。
pub(crate) fn build_default(key: &str) -> (Arc<dyn Any + Send + Sync>, Value) {
    build(key, &[None, None, None, None]).expect("纯 Default 恒可构建")
}

/// v1 平铺键 → section 规范 JSON（文件缺 v2 section 时的读迁移层，001-1 §3）。
pub(crate) fn read_legacy(key: &str, map: &serde_json::Map<String, Value>) -> Option<Value> {
    fn of<T: Section>(map: &serde_json::Map<String, Value>) -> Option<Value> {
        // legacy 合成后仍过 validate：存量文件可能有越界脏值
        let mut t = T::from_legacy(map)?;
        t.validate();
        serde_json::to_value(t).ok()
    }
    match key {
        Subagents::KEY => of::<Subagents>(map),
        #[cfg(test)]
        probe::Probe::KEY => None,
        _ => None,
    }
}

/// section 写盘时的 legacy 平铺键回写（001-1 §3 写迁移：旧键保留一版）。
pub(crate) fn write_legacy(
    key: &str,
    section_json: &Value,
    map: &mut serde_json::Map<String, Value>,
) {
    match key {
        Subagents::KEY => {
            if let Ok(t) = serde_json::from_value::<Subagents>(section_json.clone()) {
                t.to_legacy(map);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_merges_layers_in_order() {
        let layers = [
            None,
            Some(json!({"capacity": 5})),
            Some(json!({"capacity": 7})),
            Some(json!({"enabled": false})),
        ];
        let (_, json) = build(Subagents::KEY, &layers).unwrap();
        // L3 只覆盖自己声明的字段，其余保持下层值
        assert_eq!(json["capacity"], 7);
        assert_eq!(json["enabled"], false);
        assert_eq!(json["max_depth"], 1);
    }

    #[test]
    fn build_invalid_layer_is_err() {
        let layers = [None, Some(json!("garbage")), None, None];
        assert!(build(Subagents::KEY, &layers).is_err());
    }

    #[test]
    fn read_legacy_validates_stale_values() {
        let mut map = serde_json::Map::new();
        map.insert("subagent_capacity".into(), json!(999));
        let v = read_legacy(Subagents::KEY, &map).unwrap();
        assert_eq!(v["capacity"], 10); // 存量脏值被 validate 拦住
    }
}
