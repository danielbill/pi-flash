//! Startup catalogs read straight from disk — no pi process involved.
//!
//! 为什么需要这层：模型清单与命令清单过去只能来自活着的 pi 进程（RPC
//! `get_available_models` / `get_commands`），于是「新会话页」这种 lazy draft
//! （不 spawn 进程）拿到的是空清单 —— 模型 pill 回落「选择模型」、弹窗
//! 「no models match」、`/` 菜单没有命令。启动阶段改为从磁盘装配（见
//! `startup::load_globals`），进程答到之后只做**覆盖**。
//!
//! 磁盘源：
//! - `~/.pi/agent/models.json`       — 用户自定义 provider + 模型（name/contextWindow）
//! - `~/.pi/agent/models-store.json` — pi 缓存的远端 provider 目录（etag/TTL 由 pi 刷）
//! - `SKILL.md` 目录                  — `skill:<name>` 命令（`skill_commands`）
//!
//! 磁盘上**拿不到**的两类，由 app 侧自己的缓存补（crates/app/src/startup.rs）：
//! 包内注册的 provider 模型（pi-freeflow 的清单在 `src/models.ts` 里，是代码
//! 不是数据）与包内注册的扩展命令（同为 TS 代码）。
//!
//! pi 内置斜杠命令：pi 的 RPC `get_commands` 只返回扩展命令 + skill 命令
//! （1.0.0 实测 45 条 = 20 扩展 + 25 skill；1.1.0 复测 21 条 = 11 扩展 +
//! 10 skill；1.3.0 复测仍无内置），所以内置表是空常量；
//! **vendor/pi 升级时复核这一行**（PROBE: `get_commands`）。

use std::path::PathBuf;

use serde_json::Value;

use crate::protocol::{ModelInfo, SlashCommand};
use crate::skills::SkillEntry;

/// pi 内置斜杠命令表 (name, description)。钉 1.0.0/1.1.0/1.3.0 实测均为空，见模块头注。
pub const BUILTIN_COMMANDS: &[(&str, &str)] = &[];

/// 内置表 → `SlashCommand`。
pub fn builtin_commands() -> Vec<SlashCommand> {
    BUILTIN_COMMANDS
        .iter()
        .map(|(name, description)| SlashCommand {
            name: (*name).to_string(),
            description: (*description).to_string(),
        })
        .collect()
}

/// `models-store.json`（`provider → { models: [...] }`）→ 模型列表。
/// 字段与 `get_available_models` 同形（id/name/provider/contextWindow）。
pub fn parse_store(value: &Value) -> Vec<ModelInfo> {
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (provider, entry) in map {
        // models-store 形状 {"models":[...]}；provider-catalog 042 期的
        // 数组形状兼容（老文件自愈，免迁移）
        let empty: Vec<Value> = Vec::new();
        let models = entry
            .get("models")
            .and_then(Value::as_array)
            .or_else(|| entry.as_array())
            .unwrap_or(&empty);
        for m in models {
            let Some(id) = m
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            out.push(ModelInfo {
                id: id.to_string(),
                name: m
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                provider: m
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or(provider)
                    .to_string(),
                context_window: m.get("contextWindow").and_then(Value::as_u64),
                max_tokens: m.get("maxTokens").and_then(Value::as_u64),
                reasoning: m.get("reasoning").and_then(Value::as_bool),
                input: m.get("input").and_then(Value::as_array).map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                }),
            });
        }
    }
    out
}

/// `models.json`（`providers → { name, models: [...] }`）→ 模型列表。
pub fn parse_models_json(value: &Value) -> Vec<ModelInfo> {
    let Some(map) = value.get("providers").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (provider, entry) in map {
        let Some(models) = entry.get("models").and_then(Value::as_array) else {
            continue;
        };
        for m in models {
            let Some(id) = m
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            out.push(ModelInfo {
                id: id.to_string(),
                name: m
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                provider: provider.clone(),
                context_window: m.get("contextWindow").and_then(Value::as_u64),
                max_tokens: m.get("maxTokens").and_then(Value::as_u64),
                reasoning: m.get("reasoning").and_then(Value::as_bool),
                input: m.get("input").and_then(Value::as_array).map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                }),
            });
        }
    }
    out
}

/// `~/.pi/agent/models-store.json`（pi 的远端目录缓存）。
pub fn store_path() -> PathBuf {
    crate::config::agent_dir().join("models-store.json")
}

/// 磁盘模型目录 = `models.json`（用户/pi-web/PF 共同维护）+ `models-store.json`
///（pi 的远端目录缓存），按 `provider/id` 去重、保持 provider 出现顺序。
/// 任一文件缺失/坏掉都只是少一份。
pub fn disk_models() -> Vec<ModelInfo> {
    let mut out = Vec::new();
    // 042-1：PF 私有账本退役——自定义商就在 models.json（pi 的文件）
    if let Ok(v) = crate::models_json::read() {
        out.extend(parse_models_json(&v));
    }
    if let Ok(text) = std::fs::read_to_string(store_path()) {
        if let Ok(v) = crate::config::parse_lenient(&text) {
            out.extend(parse_store(&v));
        }
    }
    dedupe(out)
}

/// 按 `provider/id` 去重，保留首次出现（磁盘源优先级 = 调用顺序）。
pub fn dedupe(models: Vec<ModelInfo>) -> Vec<ModelInfo> {
    let mut seen = std::collections::HashSet::new();
    models
        .into_iter()
        .filter(|m| seen.insert(format!("{}/{}", m.provider, m.id)))
        .collect()
}

/// skill 列表 → `skill:<name>` 命令（pi 侧同款命名）。
pub fn skill_commands(skills: &[SkillEntry]) -> Vec<SlashCommand> {
    let mut out: Vec<SlashCommand> = Vec::new();
    for s in skills {
        let name = format!("skill:{}", s.name);
        if out.iter().any(|c| c.name == name) {
            continue;
        }
        out.push(SlashCommand {
            name,
            description: s.description.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_store_accepts_store_and_legacy_array_shapes() {
        // models-store 同形（save_catalog 现行写入）
        let store = json!({"ds": {"models": [{"id": "a", "provider": "ds", "name": "A"}]}});
        let got = parse_store(&store);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "a");
        // provider-catalog 042 期数组形状（写读不一致期的落盘，老文件自愈）
        let legacy = json!({"ds": [{"id": "b", "provider": "ds"}]});
        let got = parse_store(&legacy);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "b");
        assert_eq!(got[0].provider, "ds");
        // 非数组非对象条目跳过
        assert!(parse_store(&json!({"ds": "junk"})).is_empty());
    }

    #[test]
    fn store_parses_providers_and_context() {
        let v = json!({
            "openrouter": {
                "checkedAt": 1,
                "models": [
                    {"id": "nvidia/x:free", "name": "NVIDIA: X (free)", "provider": "openrouter",
                     "contextWindow": 256000},
                    {"name": "no id"},
                ]
            },
            "broken": {"models": "not an array"},
            "also-broken": 7
        });
        let ms = parse_store(&v);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].provider, "openrouter");
        assert_eq!(ms[0].name, "NVIDIA: X (free)");
        assert_eq!(ms[0].context_window, Some(256000));
        // provider 缺省时回落 map key
        let v2 = json!({"glm": {"models": [{"id": "glm-5", "name": "GLM 5"}]}});
        assert_eq!(parse_store(&v2)[0].provider, "glm");
    }

    #[test]
    fn models_json_parses_provider_keys() {
        let v = json!({
            "providers": {
                "glm": {"name": "GLM", "models": [{"id": "glm-5.3", "name": "GLM 5.3",
                                                    "contextWindow": 200000}]},
                "empty": {}
            }
        });
        let ms = parse_models_json(&v);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].provider, "glm");
        assert_eq!(ms[0].id, "glm-5.3");
        assert_eq!(ms[0].context_window, Some(200000));
    }

    #[test]
    fn dedupe_keeps_first_and_order() {
        let mk = |p: &str, id: &str, name: &str| ModelInfo {
            id: id.into(),
            name: name.into(),
            provider: p.into(),
            context_window: None,
        };
        let ms = dedupe(vec![
            mk("a", "1", "first"),
            mk("a", "1", "second"),
            mk("b", "1", "other provider"),
        ]);
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].name, "first");
        assert_eq!(ms[1].provider, "b");
    }

    #[test]
    fn skill_commands_use_pi_naming_and_dedupe() {
        let s = |name: &str| SkillEntry {
            name: name.into(),
            description: "d".into(),
            path: PathBuf::from("x/SKILL.md"),
            scope: crate::skills::SkillScope::Global,
            disable_invocation: false,
        };
        let cs = skill_commands(&[s("image-gen"), s("image-gen"), s("jev")]);
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[0].name, "skill:image-gen");
        assert_eq!(cs[1].name, "skill:jev");
    }

    /// pi 无内置斜杠命令（1.0.0/1.1.0/1.3.0 实测）；升级 vendor 时这条会提醒复核。
    #[test]
    fn builtin_table_is_empty() {
        assert!(builtin_commands().is_empty());
    }
}
