//! `updates` / `shell` sections（001-1 §3）：自动更新（081）与界面杂项
//! （部分操作隐藏、提示音）。字段少，两个 section 同文件。

use serde::{Deserialize, Serialize};

use crate::settings::section::Section;

/// 081 自动更新（默认 on——off 时只提示新版本号不下载）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct Updates {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_update: Option<bool>,
}

impl Section for Updates {
    const KEY: &'static str = "updates";

    fn validate(&mut self) {}

    fn from_legacy(map: &serde_json::Map<String, serde_json::Value>) -> Option<Self> {
        map.get("auto_update")
            .and_then(|v| v.as_bool())
            .map(|auto_update| Self {
                auto_update: Some(auto_update),
            })
    }

    fn to_legacy(&self, map: &mut serde_json::Map<String, serde_json::Value>) {
        if let Some(v) = self.auto_update {
            map.insert("auto_update".into(), serde_json::Value::from(v));
        }
    }
}

/// 界面杂项：部分操作隐藏（dock 两操作栏 icon 常态透明）+ 提示音。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct Shell {
    /// 默认 on（不常用 icon 常态透明、hover 才显影）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autohide_icons: Option<bool>,
    /// 通知提示音（缺省走 workspace 记忆回退，见 ws_state::load_sound_pref）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sound: Option<bool>,
}

impl Section for Shell {
    const KEY: &'static str = "shell";

    fn validate(&mut self) {}

    fn from_legacy(map: &serde_json::Map<String, serde_json::Value>) -> Option<Self> {
        let autohide_icons = map.get("autohide_icons").and_then(|v| v.as_bool());
        let sound = map.get("sound").and_then(|v| v.as_bool());
        if autohide_icons.is_none() && sound.is_none() {
            return None;
        }
        Some(Self {
            autohide_icons,
            sound,
        })
    }

    fn to_legacy(&self, map: &mut serde_json::Map<String, serde_json::Value>) {
        if let Some(v) = self.autohide_icons {
            map.insert("autohide_icons".into(), serde_json::Value::from(v));
        }
        if let Some(v) = self.sound {
            map.insert("sound".into(), serde_json::Value::from(v));
        }
    }
}
