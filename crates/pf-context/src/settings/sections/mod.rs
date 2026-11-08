//! section 注册表（001-1 §3：25 键的落点，S2 全表通电）。
//!
//! 分派函数（`build` / `read_legacy` / `write_legacy`）按 KEY 显式 match——
//! 键名即 `Section::KEY`、字段名即结构体字段；`in_file = false` 的段（dev_flags）
//! 不落 app-settings.json，源在别处（boot 层）。

mod appearance;
mod chat;
pub(crate) mod dev_flags;
mod index;
mod models;
mod sessions;
mod shell;
mod subagents;

pub use appearance::{Appearance, FontSpec};
pub use chat::{CHAT_WIDTH_DEFAULT, CHAT_WIDTH_MAX, CHAT_WIDTH_MIN, CHAT_WIDTH_STEP, Chat};
pub use dev_flags::DevFlags;
pub use index::Index;
pub use models::Models;
pub use sessions::Sessions;
pub use shell::{Shell, Updates};
pub use subagents::Subagents;

use crate::settings::section::{Section, deep_merge};
use serde_json::Value;
use std::any::Any;
use std::sync::Arc;

/// 注册表条目：`in_file = false` 的段不落 app-settings.json（boot-only，
/// 如 dev_flags 源在 dev-flags.json + env）；`exportable = true` 的段进
/// 跨进程投影 `ext-context.json`（001-2，白名单默认拒绝）。
pub(crate) struct SectionDef {
    pub key: &'static str,
    pub in_file: bool,
    pub exportable: bool,
}

/// 已注册 section（快照构建顺序）。
pub(crate) fn registered() -> &'static [SectionDef] {
    #[cfg(test)]
    return &[
        SectionDef {
            key: Subagents::KEY,
            in_file: true,
            exportable: true,
        },
        SectionDef {
            key: Appearance::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: Chat::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: Sessions::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: Index::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: Models::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: Updates::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: Shell::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: DevFlags::KEY,
            in_file: false,
            exportable: false,
        },
        SectionDef {
            key: probe::Probe::KEY,
            in_file: true,
            exportable: false,
        },
    ];
    #[cfg(not(test))]
    return &[
        SectionDef {
            key: Subagents::KEY,
            in_file: true,
            exportable: true,
        },
        SectionDef {
            key: Appearance::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: Chat::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: Sessions::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: Index::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: Models::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: Updates::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: Shell::KEY,
            in_file: true,
            exportable: false,
        },
        SectionDef {
            key: DevFlags::KEY,
            in_file: false,
            exportable: false,
        },
    ];
}

/// 测试专用的第二 section：「单段坏、其它段正常」的断言需要两段才能表达。
/// 不进公开 API。
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
        Appearance::KEY => build_one::<Appearance>(layers),
        Chat::KEY => build_one::<Chat>(layers),
        Sessions::KEY => build_one::<Sessions>(layers),
        Index::KEY => build_one::<Index>(layers),
        Models::KEY => build_one::<Models>(layers),
        Updates::KEY => build_one::<Updates>(layers),
        Shell::KEY => build_one::<Shell>(layers),
        DevFlags::KEY => build_one::<DevFlags>(layers),
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
        Appearance::KEY => of::<Appearance>(map),
        Chat::KEY => of::<Chat>(map),
        Sessions::KEY => of::<Sessions>(map),
        Index::KEY => of::<Index>(map),
        Models::KEY => of::<Models>(map),
        Updates::KEY => of::<Updates>(map),
        Shell::KEY => of::<Shell>(map),
        _ => None,
    }
}

/// section 写盘时的 legacy 平铺键回写（001-1 §3 写迁移：旧键保留一版）。
pub(crate) fn write_legacy(
    key: &str,
    section_json: &Value,
    map: &mut serde_json::Map<String, Value>,
) {
    macro_rules! via {
        ($t:ty) => {
            if let Ok(t) = serde_json::from_value::<$t>(section_json.clone()) {
                t.to_legacy(map);
            }
        };
    }
    match key {
        Subagents::KEY => via!(Subagents),
        Appearance::KEY => via!(Appearance),
        Chat::KEY => via!(Chat),
        Sessions::KEY => via!(Sessions),
        Index::KEY => via!(Index),
        Models::KEY => via!(Models),
        Updates::KEY => via!(Updates),
        Shell::KEY => via!(Shell),
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

    #[test]
    fn all_25_keys_have_legacy_mapping() {
        // 001-1 §3 全表：每个 v1 平铺键都能被对应 section 的 from_legacy 认领
        let mut map = serde_json::Map::new();
        for (k, v) in [
            ("theme", json!("rose")),
            ("icon_theme", json!("default")),
            ("session_font", json!({"family": "Inter", "size": 14})),
            ("panel_font", json!({"family": "Inter", "size": 13})),
            ("markdown_font", json!({"family": "Inter", "size": 15})),
            ("lang", json!(2)),
            ("chat_content_width", json!(920)),
            ("md_live_preview", json!(true)),
            ("send_mode", json!("steer")),
            ("show_thinking", json!(true)),
            ("git_markers", json!(true)),
            ("load_window_days", json!(14)),
            ("startup_restore", json!(false)),
            ("session_display_count", json!(5)),
            ("auto_collapse_hours", json!(72)),
            ("auto_clean_days", json!(30)),
            ("at_index_ttl_secs", json!(120)),
            ("at_index_max_projects", json!(20)),
            ("default_provider", json!("deepseek")),
            ("default_model", json!("deepseek-flash")),
            ("auto_update", json!(false)),
            ("autohide_icons", json!(false)),
            ("sound", json!(true)),
            ("subagent_capacity", json!(5)),
            ("subagent_enabled", json!(false)),
        ] {
            map.insert(k.into(), v);
        }
        let got: Vec<&str> = registered()
            .iter()
            .filter(|d| d.in_file)
            .filter_map(|d| read_legacy(d.key, &map).map(|_| d.key))
            .collect();
        assert_eq!(
            got,
            vec![
                "subagents",
                "appearance",
                "chat",
                "sessions",
                "index",
                "models",
                "updates",
                "shell",
            ],
            "25 键必须被 8 个 in-file section 全量认领"
        );
    }
}
