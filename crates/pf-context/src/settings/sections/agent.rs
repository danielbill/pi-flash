//! `agent` section（001-4 内部工具化）：agent 面能力政策。
//!
//! 首个字段 = 自我调试开关（设置-更多【自我调试】，默认开）：控制
//! `pfctx_manager` 工具是否注入 agent 会话（spawn 门控 + 切换重绑，见
//! extensions/pfctx-manager）。v1 无 v1 平铺键（新参数，无存量迁移）。

use serde::{Deserialize, Serialize};

use crate::settings::section::Section;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Agent {
    /// 自我调试：允许 agent 经 pfctx_manager 工具动态修改 PF 设置（默认开）。
    pub self_debug: bool,
}

impl Default for Agent {
    fn default() -> Self {
        Self { self_debug: true }
    }
}

impl Section for Agent {
    const KEY: &'static str = "agent";

    fn validate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_on() {
        assert!(Agent::default().self_debug);
    }

    #[test]
    fn partial_json_fills_default() {
        let a: Agent = serde_json::from_str("{}").unwrap();
        assert!(a.self_debug);
        let a: Agent = serde_json::from_str(r#"{"self_debug": false}"#).unwrap();
        assert!(!a.self_debug);
    }
}
