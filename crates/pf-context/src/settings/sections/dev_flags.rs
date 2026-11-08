//! `dev_flags` section（001-1 §3 末行）：开发旗标 boot-only 只读段。
//!
//! **不进 app-settings.json**（注册表 `in_file: false`）：事实源仍是
//! `<PF数据目录>/dev-flags.json` + `PI_FLASH_<键大写>` env（env 最高优先，
//! 语义全在 pi-link `dev_flags::enabled`）。open() 时一次性灌入 boot 层，
//! **boot-only = 改了需重启**（replacing 原 app_settings.rs 的 OnceLock 快照）。
//! 无人对它调 `set`（只读段），写通道永不触碰；full-upgrade 也按 `in_file`
//! 过滤跳过。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::settings::section::Section;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct DevFlags {
    /// 已知旗标键 → 生效值（键 = pi-link `dev_flags::KNOWN`）
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub flags: BTreeMap<String, bool>,
}

impl DevFlags {
    /// 旗标查询（缺键 = false，与原 OnceLock 快照同语义）。
    pub fn flag(&self, key: &str) -> bool {
        self.flags.get(key).copied().unwrap_or(false)
    }
}

impl Section for DevFlags {
    const KEY: &'static str = "dev_flags";

    fn validate(&mut self) {
        self.flags
            .retain(|k, _| pi_link::dev_flags::KNOWN.contains(&k.as_str()));
    }
}

/// open() 时构建 boot 层值：KNOWN 全键按 pi-link 语义解析
/// （env 覆盖 > dev-flags.json > 默认值；subagents 默认开）。
pub(crate) fn boot_value() -> serde_json::Value {
    let flags: BTreeMap<String, bool> = pi_link::dev_flags::KNOWN
        .iter()
        .map(|k| (k.to_string(), pi_link::dev_flags::enabled(k)))
        .collect();
    serde_json::to_value(DevFlags { flags }).expect("DevFlags 序列化")
}
