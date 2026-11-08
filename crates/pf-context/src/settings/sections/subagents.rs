//! `subagents` section（001-1 §5）：容量 2..=10 默认 3、嵌套深度恒 1、模块开关。
//!
//! 这是 pf-context 通电的第一个 section；S2 起其余 24 键陆续迁入（001-1 §3）。

use serde::{Deserialize, Serialize};

use crate::settings::section::Section;

/// 子代理模块设置。clamp **只**发生在 [`Section::validate`]（UI / 扩展 /
/// 调用方不重复钳；扩展侧仅防御性钳）。
///
/// v1 平铺键：`subagent_capacity` / `subagent_enabled`（读迁移 + 写回一版，
/// 旧写入者/回滚版本兼容；S2 收编 app-settings 写入后随字段退役）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Subagents {
    /// 同帧并发子代理上限：2..=10，默认 3（2026-11-07 上限 5→10 用户定稿）。
    pub capacity: u64,
    /// 嵌套深度：恒 1（禁止递归委派，2026-11-07 用户拍板，UI 已移除控件）。
    /// 任何来源写不进别的值——`validate` 强制归一。
    pub max_depth: u64,
    /// 模块开关（关闭后 `agent` 工具直接拒绝；缺省开）。
    pub enabled: bool,
}

impl Default for Subagents {
    fn default() -> Self {
        Self {
            capacity: 3,
            max_depth: 1,
            enabled: true,
        }
    }
}

impl Section for Subagents {
    const KEY: &'static str = "subagents";

    fn validate(&mut self) {
        self.capacity = self.capacity.clamp(2, 10);
        self.max_depth = 1; // 恒 1：禁止套娃（001-1 §5）
    }

    fn from_legacy(map: &serde_json::Map<String, serde_json::Value>) -> Option<Self> {
        let capacity = map.get("subagent_capacity").and_then(|v| v.as_u64());
        let enabled = map.get("subagent_enabled").and_then(|v| v.as_bool());
        if capacity.is_none() && enabled.is_none() {
            return None;
        }
        let d = Self::default();
        Some(Self {
            capacity: capacity.unwrap_or(d.capacity),
            max_depth: d.max_depth,
            enabled: enabled.unwrap_or(d.enabled),
        })
    }

    fn to_legacy(&self, map: &mut serde_json::Map<String, serde_json::Value>) {
        map.insert(
            "subagent_capacity".into(),
            serde_json::Value::from(self.capacity),
        );
        map.insert(
            "subagent_enabled".into(),
            serde_json::Value::from(self.enabled),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_is_the_only_clamp_point() {
        let mut s = Subagents {
            capacity: 999,
            max_depth: 7,
            enabled: true,
        };
        s.validate();
        assert_eq!((s.capacity, s.max_depth), (10, 1));
        let mut s = Subagents {
            capacity: 0,
            ..Default::default()
        };
        s.validate();
        assert_eq!(s.capacity, 2);
    }

    #[test]
    fn legacy_round_trip() {
        let mut map = serde_json::Map::new();
        map.insert("subagent_capacity".into(), serde_json::Value::from(5));
        let s = Subagents::from_legacy(&map).unwrap();
        assert_eq!((s.capacity, s.enabled), (5, true));
        let mut map2 = serde_json::Map::new();
        s.to_legacy(&mut map2);
        assert_eq!(
            map2.get("subagent_capacity").and_then(|v| v.as_u64()),
            Some(5)
        );
        assert_eq!(
            map2.get("subagent_enabled").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn legacy_absent_is_none() {
        assert!(Subagents::from_legacy(&serde_json::Map::new()).is_none());
    }
}
