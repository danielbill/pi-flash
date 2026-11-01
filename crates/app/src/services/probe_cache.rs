//! 自定义 Provider 探测结果展示缓存（`~/.pi-flash/probe-cache.json`）。
//!
//! 2026-11-01 模型列表架构定稿：列表 = 展示层（内置 = pi 注册表，自定义 =
//! baseUrl 探测/手填），models.json 只存用户勾选。探测到的官方清单**只进
//! 本缓存做展示**，勾选哪个才把哪个物化进 models.json（见
//! `settings/models.rs` 勾选钩子）——不再整批合并进账本。

use std::collections::BTreeMap;
use std::path::Path;

use pi_link::protocol::ModelInfo;

/// 单商探测快照（手写读写，不依赖 ModelInfo 的 serde 实现）。
#[derive(Default, Clone)]
pub struct ProviderProbe {
    /// 最近一次探测的 unix 毫秒（0 = 未知）。
    pub fetched_at: u64,
    /// 探测到的模型（id/name/contextWindow；其余字段探测不关心）。
    pub models: Vec<ModelInfo>,
}

/// 全量缓存：provider → 探测快照（BTreeMap 保序稳定输出）。
pub type Cache = BTreeMap<String, ProviderProbe>;

/// 读缓存（无文件/坏文件/缺 provider 键 → 空）。
pub fn read() -> Cache {
    read_from(
        &pi_link::paths::probe_cache_file()
            .unwrap_or_else(|| std::path::PathBuf::from("<unavailable>")),
    )
}

/// 写缓存（原子写语义同 config::write_json）；返回是否写入。
pub fn write(cache: &Cache) -> bool {
    match pi_link::paths::probe_cache_file() {
        Some(p) => write_to(&p, cache),
        None => false,
    }
}

/// 整商替换（探测 Ok 时调用）；返回更新后的全量缓存。
pub fn replace_provider(cache: &mut Cache, provider: &str, models: Vec<ModelInfo>) {
    cache.insert(
        provider.to_string(),
        ProviderProbe {
            fetched_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            models,
        },
    );
}

/// 读显式路径（测试同实现）。
fn read_from(path: &Path) -> Cache {
    let Ok(value) = pi_link::config::read_json(path) else {
        return Cache::default();
    };
    let Some(obj) = value.as_object() else {
        return Cache::default();
    };
    let mut out = Cache::default();
    for (provider, entry) in obj {
        let models = entry
            .get("models")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        let id = m.get("id")?.as_str()?.to_string();
                        if id.is_empty() {
                            return None;
                        }
                        Some(ModelInfo {
                            id,
                            name: m
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            provider: provider.clone(),
                            context_window: m.get("contextWindow").and_then(|v| v.as_u64()),
                            max_tokens: None,
                            reasoning: None,
                            input: None,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.insert(
            provider.clone(),
            ProviderProbe {
                fetched_at: entry.get("fetchedAt").and_then(|v| v.as_u64()).unwrap_or(0),
                models,
            },
        );
    }
    out
}

/// 写显式路径。
fn write_to(path: &Path, cache: &Cache) -> bool {
    let mut obj = serde_json::Map::new();
    for (provider, probe) in cache {
        obj.insert(
            provider.clone(),
            serde_json::json!({
                "fetchedAt": probe.fetched_at,
                "models": probe
                    .models
                    .iter()
                    .map(|m| serde_json::json!({
                        "id": m.id,
                        "name": m.name,
                        "contextWindow": m.context_window,
                    }))
                    .collect::<Vec<_>>(),
            }),
        );
    }
    let _ = pi_link::paths::ensure_dir();
    pi_link::config::write_json(path, &serde_json::Value::Object(obj)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "pf-probe-cache-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ))
        .join("probe-cache.json")
    }

    fn model(id: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            name: id.into(),
            provider: String::new(),
            context_window: Some(128_000),
            max_tokens: None,
            reasoning: None,
            input: None,
        }
    }

    #[test]
    fn roundtrip_and_replace_provider() {
        let path = tmp("roundtrip");
        let mut cache = Cache::default();
        replace_provider(&mut cache, "ds", vec![model("glm-4.5"), model("glm-4.6")]);
        assert!(write_to(&path, &cache));
        let back = read_from(&path);
        let ds = back.get("ds").expect("ds entry");
        assert_eq!(ds.models.len(), 2);
        assert_eq!(ds.models[0].id, "glm-4.5");
        assert_eq!(ds.models[0].provider, "ds", "读回时 provider 由键补全");
        assert_eq!(ds.models[0].context_window, Some(128_000));
        assert!(ds.fetched_at > 0);
        // 整商替换：旧条目被新快照覆盖
        replace_provider(&mut cache, "ds", vec![model("glm-5.3")]);
        assert_eq!(cache.get("ds").unwrap().models.len(), 1);
        // 缺文件 → 空
        assert!(read_from(&path.with_file_name("nope.json")).is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn junk_and_empty_ids_are_skipped() {
        let path = tmp("junk");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"ds":{"fetchedAt":5,"models":[{"id":""},{"name":"no id"},{"id":"m1"}]}}"#,
        )
        .unwrap();
        let back = read_from(&path);
        assert_eq!(back.get("ds").unwrap().models.len(), 1);
        assert_eq!(back.get("ds").unwrap().models[0].id, "m1");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
