//! `index` section（001-1 §3）：@ 文件索引缓存参数（015/031 @ 补全）。

use serde::{Deserialize, Serialize};

use crate::settings::section::Section;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct Index {
    /// @ 文件索引缓存有效期（秒，钳 1..=3600）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_index_ttl_secs: Option<u64>,
    /// 缓存的最多项目数（每 cwd 一份，钳 1..=100）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_index_max_projects: Option<u64>,
}

impl Section for Index {
    const KEY: &'static str = "index";

    fn validate(&mut self) {
        if let Some(v) = self.at_index_ttl_secs.as_mut() {
            *v = (*v).clamp(1, 3600);
        }
        if let Some(v) = self.at_index_max_projects.as_mut() {
            *v = (*v).clamp(1, 100);
        }
    }

    fn from_legacy(map: &serde_json::Map<String, serde_json::Value>) -> Option<Self> {
        let ttl = map.get("at_index_ttl_secs").and_then(|v| v.as_u64());
        let max_projects = map.get("at_index_max_projects").and_then(|v| v.as_u64());
        if ttl.is_none() && max_projects.is_none() {
            return None;
        }
        Some(Self {
            at_index_ttl_secs: ttl,
            at_index_max_projects: max_projects,
        })
    }

    fn to_legacy(&self, map: &mut serde_json::Map<String, serde_json::Value>) {
        if let Some(v) = self.at_index_ttl_secs {
            map.insert("at_index_ttl_secs".into(), serde_json::Value::from(v));
        }
        if let Some(v) = self.at_index_max_projects {
            map.insert("at_index_max_projects".into(), serde_json::Value::from(v));
        }
    }
}
