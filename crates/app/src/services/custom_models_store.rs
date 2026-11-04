//! 自定义 Provider 模型清单仓（`~/.pi-flash/custom-provider.json`）。
//!
//! 2026-11-01 定稿：自定义商的模型**完整定义**（自动拉取的、用户手填的、
//! 手调过参数的）全部存在这里；models.json 只放用户勾选（on）的子集
//! ——勾选 = 把定义原样复制进 models.json，取消 = 从 models.json 删掉，
//! 定义本身留在仓里，off/on 往返不丢参数。删除自定义 Provider 时整条目删。
//!
//! 这**不是** 051 账本的复活：pi 仍只读 models.json，本文件不参与注册、
//! 不注入、不碰凭据——只是模型清单的参数仓。
//!
//! 形状：`{ "<provider>": { "fetchedAt": ms, "models": { "<id>": <定义 JSON> } } }`
//! （定义 = models.json `models[]` 同款条目，id 键冗余保留在对象里）。

use std::collections::BTreeMap;
use std::path::Path;

use pi_link::protocol::ModelInfo;
use serde_json::Value;

/// 单商条目。
#[derive(Default, Clone)]
pub struct ProviderEntry {
    /// 最近一次探测的 unix 毫秒（0 = 未探测过/未知）。
    pub fetched_at: u64,
    /// id → 完整模型定义（BTreeMap 保序稳定输出）。
    pub models: BTreeMap<String, Value>,
}

/// 全量仓：provider → 条目。
pub type Store = BTreeMap<String, ProviderEntry>;

pub fn read() -> Store {
    read_from(
        &pi_link::paths::custom_provider_file()
            .unwrap_or_else(|| std::path::PathBuf::from("<unavailable>")),
    )
}

pub fn write(store: &Store) -> bool {
    match pi_link::paths::custom_provider_file() {
        Some(p) => write_to(&p, store),
        None => false,
    }
}

/// 探测结果并入（**只补缺，不覆盖**）：新 id 按探测定义新建；已有 id 只补
/// 它没有的键（手调过的参数原样保留；老探测缺的 maxTokens/input 等随新探测
/// 补上——0.3.2 之前探测只取三个键，存量条目靠这一步补齐）。
pub fn merge_probed(store: &mut Store, provider: &str, probed: &[ModelInfo]) {
    let entry = store.entry(provider.to_string()).or_default();
    entry.fetched_at = now_ms();
    for m in probed {
        if m.id.is_empty() {
            continue;
        }
        let def = definition_from_info(m);
        match entry.models.get_mut(&m.id) {
            None => {
                entry.models.insert(m.id.clone(), def);
            }
            Some(existing) => {
                if let (Some(dst), Some(src)) = (existing.as_object_mut(), def.as_object()) {
                    for (k, v) in src {
                        dst.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                }
            }
        }
    }
}

/// 写入/覆盖一条定义（手填保存 / 模型详情保存）。
pub fn upsert_model(store: &mut Store, provider: &str, id: &str, definition: Value) {
    let entry = store.entry(provider.to_string()).or_default();
    entry.models.insert(id.to_string(), definition);
}

/// 删一条定义（模型详情【移除】= 删除该模型）。
pub fn remove_model(store: &mut Store, provider: &str, id: &str) -> bool {
    store
        .get_mut(provider)
        .map(|e| e.models.remove(id).is_some())
        .unwrap_or(false)
}

/// 取一条定义（勾选 on 时复制进 models.json 用）。
pub fn definition<'a>(store: &'a Store, provider: &str, id: &str) -> Option<&'a Value> {
    store.get(provider)?.models.get(id)
}

/// 该商的展示条目（罗列用；按仓内顺序）。
pub fn listing(store: &Store, provider: &str) -> Vec<ModelInfo> {
    store
        .get(provider)
        .map(|e| {
            e.models
                .values()
                .map(|v| info_from_definition(provider, v))
                .collect()
        })
        .unwrap_or_default()
}

/// ModelInfo → 定义（探测给多少写多少：id/name/contextWindow/maxTokens/
/// input/reasoning；没给的键不写，落盘时走 ensure_model_defaults 缺省）。
fn definition_from_info(m: &ModelInfo) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), Value::String(m.id.clone()));
    if !m.name.is_empty() {
        obj.insert("name".into(), Value::String(m.name.clone()));
    }
    if let Some(cw) = m.context_window {
        obj.insert("contextWindow".into(), serde_json::json!(cw));
    }
    if let Some(mt) = m.max_tokens {
        obj.insert("maxTokens".into(), serde_json::json!(mt));
    }
    if let Some(input) = m.input.as_ref().filter(|v| !v.is_empty()) {
        obj.insert("input".into(), serde_json::json!(input));
    }
    if let Some(r) = m.reasoning {
        obj.insert("reasoning".into(), serde_json::json!(r));
    }
    Value::Object(obj)
}

/// 定义 → 展示条目。
pub fn info_from_definition(provider: &str, v: &Value) -> ModelInfo {
    ModelInfo {
        id: v
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        name: v
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        provider: provider.to_string(),
        context_window: v.get("contextWindow").and_then(|x| x.as_u64()),
        max_tokens: v.get("maxTokens").and_then(|x| x.as_u64()),
        reasoning: v.get("reasoning").and_then(|x| x.as_bool()),
        input: v.get("input").and_then(|x| x.as_array()).map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        }),
        cost: v.get("cost").and_then(pi_link::protocol::CostInfo::parse),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn read_from(path: &Path) -> Store {
    let Ok(value) = pi_link::config::read_json(path) else {
        return Store::default();
    };
    let Some(obj) = value.as_object() else {
        return Store::default();
    };
    let mut out = Store::default();
    for (provider, entry) in obj {
        let mut models = BTreeMap::new();
        if let Some(map) = entry.get("models").and_then(|v| v.as_object()) {
            for (id, def) in map {
                if id.is_empty() || !def.is_object() {
                    continue;
                }
                let mut def = def.clone();
                // id 键冗余保证（定义复制进 models.json 时必须带 id）
                if let Some(o) = def.as_object_mut() {
                    o.insert("id".into(), Value::String(id.clone()));
                }
                models.insert(id.clone(), def);
            }
        }
        out.insert(
            provider.clone(),
            ProviderEntry {
                fetched_at: entry.get("fetchedAt").and_then(|v| v.as_u64()).unwrap_or(0),
                models,
            },
        );
    }
    out
}

fn write_to(path: &Path, store: &Store) -> bool {
    let mut obj = serde_json::Map::new();
    for (provider, entry) in store {
        let models: serde_json::Map<String, Value> = entry
            .models
            .iter()
            .map(|(id, def)| (id.clone(), def.clone()))
            .collect();
        obj.insert(
            provider.clone(),
            serde_json::json!({ "fetchedAt": entry.fetched_at, "models": models }),
        );
    }
    let _ = pi_link::paths::ensure_dir();
    pi_link::config::write_json(path, &Value::Object(obj)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pf-custom-store-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("custom-provider.json")
    }

    fn info(id: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            name: format!("N-{id}"),
            provider: String::new(),
            context_window: Some(128_000),
            max_tokens: Some(4096),
            reasoning: None,
            input: None,
            cost: None,
        }
    }

    #[test]
    fn probe_merge_fills_missing_keeps_edited() {
        let mut st = Store::default();
        // 用户手调过的定义（带 maxTokens/cost）先在
        upsert_model(
            &mut st,
            "ds",
            "glm-5.3-flash",
            serde_json::json!({"id":"glm-5.3-flash","maxTokens":65536,"cost":{"input":1}}),
        );
        merge_probed(&mut st, "ds", &[info("glm-5.3-flash"), info("glm-4.6")]);
        let e = st.get("ds").unwrap();
        assert_eq!(e.models.len(), 2);
        // 已有 id：手调的键原样保留（探测的 4096 不覆盖 65536）……
        assert_eq!(e.models["glm-5.3-flash"]["maxTokens"], 65536);
        assert_eq!(e.models["glm-5.3-flash"]["cost"]["input"], 1);
        // ……但它缺的键随探测补上（存量条目补 name/contextWindow）
        assert_eq!(e.models["glm-5.3-flash"]["name"], "N-glm-5.3-flash");
        assert_eq!(e.models["glm-5.3-flash"]["contextWindow"], 128_000);
        // 缺的按探测新建（探测给了最大输出就带上）
        assert_eq!(e.models["glm-4.6"]["name"], "N-glm-4.6");
        assert_eq!(e.models["glm-4.6"]["contextWindow"], 128_000);
        assert_eq!(e.models["glm-4.6"]["maxTokens"], 4096);
        // 探测没给的键不写（input/reasoning 交给落盘缺省）
        assert!(e.models["glm-4.6"].get("input").is_none());
        assert!(e.models["glm-4.6"].get("reasoning").is_none());
        assert!(e.fetched_at > 0);
    }

    #[test]
    fn probe_modalities_and_reasoning_land_in_definition() {
        let mut st = Store::default();
        let mut m = info("deepseek-flash");
        m.input = Some(vec!["text".into(), "image".into()]);
        m.reasoning = Some(false);
        merge_probed(&mut st, "ds", &[m]);
        let d = definition(&st, "ds", "deepseek-flash").unwrap();
        assert_eq!(d["input"], serde_json::json!(["text", "image"]));
        assert_eq!(d["reasoning"], serde_json::json!(false));
        // 展示条目派生同样带上
        let l = listing(&st, "ds");
        assert_eq!(l[0].reasoning, Some(false));
        assert_eq!(l[0].input.as_ref().map(|v| v.len()), Some(2));
    }

    #[test]
    fn roundtrip_keeps_full_definitions() {
        let path = tmp("rt");
        let mut st = Store::default();
        upsert_model(
            &mut st,
            "ds",
            "m1",
            serde_json::json!({"id":"m1","name":"M1","maxTokens":4096,"reasoning":true,
                "input":["text","image"],"cost":{"input":0.5,"output":1.5}}),
        );
        assert!(write_to(&path, &st));
        let back = read_from(&path);
        let d = definition(&back, "ds", "m1").unwrap();
        assert_eq!(d["maxTokens"], 4096);
        assert_eq!(d["cost"]["output"], 1.5);
        assert_eq!(d["input"][1], "image");
        // 罗列条目派生
        let l = listing(&back, "ds");
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].max_tokens, Some(4096));
        assert_eq!(l[0].provider, "ds");
        // 删除
        assert!(remove_model(&mut st, "ds", "m1"));
        assert!(!remove_model(&mut st, "ds", "m1"));
        assert!(read_from(&path.with_file_name("nope.json")).is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
