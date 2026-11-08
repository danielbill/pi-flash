//! `sessions` section（001-1 §3）：加载窗口 / 启动恢复 / 展示数 / 折叠阈值 /
//! 自动清理。`session_display_count` → `display_count`（去冗余前缀），
//! `startup_restore` v1 键名与字段名一致直迁。

use serde::{Deserialize, Serialize};

use crate::settings::section::Section;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct Sessions {
    /// v54/v60 启动装载时间窗（档位 7/14/30，档位归一在读侧 getter）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_window_days: Option<u64>,
    /// v54 启动恢复最后会话（缺省 on）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_restore: Option<bool>,
    /// 其他页: 各项目默认显示会话数（psp 初始页大小，钳 3..=10）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_count: Option<u64>,
    /// 启动自动折叠阈值（小时；档位 24/72/168，档位归一在读侧 getter）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_collapse_hours: Option<u64>,
    /// 052 会话自动清理天数（0 = 关，钳 0..=100）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_clean_days: Option<u64>,
}

impl Section for Sessions {
    const KEY: &'static str = "sessions";

    fn validate(&mut self) {
        if let Some(v) = self.display_count.as_mut() {
            *v = (*v).clamp(3, 10);
        }
        if let Some(v) = self.auto_clean_days.as_mut() {
            *v = (*v).min(100);
        }
    }

    fn from_legacy(map: &serde_json::Map<String, serde_json::Value>) -> Option<Self> {
        let load_window_days = map.get("load_window_days").and_then(|v| v.as_u64());
        let startup_restore = map.get("startup_restore").and_then(|v| v.as_bool());
        let display_count = map.get("session_display_count").and_then(|v| v.as_u64());
        let auto_collapse_hours = map.get("auto_collapse_hours").and_then(|v| v.as_u64());
        let auto_clean_days = map.get("auto_clean_days").and_then(|v| v.as_u64());
        if load_window_days.is_none()
            && startup_restore.is_none()
            && display_count.is_none()
            && auto_collapse_hours.is_none()
            && auto_clean_days.is_none()
        {
            return None;
        }
        Some(Self {
            load_window_days,
            startup_restore,
            display_count,
            auto_collapse_hours,
            auto_clean_days,
        })
    }

    fn to_legacy(&self, map: &mut serde_json::Map<String, serde_json::Value>) {
        let mut insert = |k: &str, v: Option<serde_json::Value>| {
            if let Some(v) = v {
                map.insert(k.to_string(), v);
            }
        };
        insert(
            "load_window_days",
            self.load_window_days.map(serde_json::Value::from),
        );
        insert(
            "startup_restore",
            self.startup_restore.map(serde_json::Value::from),
        );
        insert(
            "session_display_count",
            self.display_count.map(serde_json::Value::from),
        );
        insert(
            "auto_collapse_hours",
            self.auto_collapse_hours.map(serde_json::Value::from),
        );
        insert(
            "auto_clean_days",
            self.auto_clean_days.map(serde_json::Value::from),
        );
    }
}
