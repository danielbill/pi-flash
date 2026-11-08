//! `models` section（001-1 §3）：默认模型指针（042）。只存指针——
//! models.json / auth.json 是 pi 的文件，永不进 context（001-0 §6）。

use serde::{Deserialize, Serialize};

use crate::settings::section::Section;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct Models {
    /// 042 默认 Provider（设置-模型页五角星；两键齐才生效）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
}

impl Section for Models {
    const KEY: &'static str = "models";

    fn validate(&mut self) {}

    fn from_legacy(map: &serde_json::Map<String, serde_json::Value>) -> Option<Self> {
        let provider = map
            .get("default_provider")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let model = map
            .get("default_model")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if provider.is_none() && model.is_none() {
            return None;
        }
        Some(Self {
            default_provider: provider,
            default_model: model,
        })
    }

    fn to_legacy(&self, map: &mut serde_json::Map<String, serde_json::Value>) {
        if let Some(v) = &self.default_provider {
            map.insert(
                "default_provider".into(),
                serde_json::Value::String(v.clone()),
            );
        }
        if let Some(v) = &self.default_model {
            map.insert("default_model".into(), serde_json::Value::String(v.clone()));
        }
    }
}
