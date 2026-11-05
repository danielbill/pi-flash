//! `~/.pi/agent/models.json` — pi 的 provider/模型清单。042-1 起 **PF 会写它**
//!（推翻 051 的「只读」铁律）：整份草稿保存 + **不可读拒写** + proper-lockfile
//! 同款锁，与 pi / pi-web 共用写者约定。用户手写的内容同样受保护——读不出
//! 来就永不覆盖。
//!
//! ```json
//! { "providers": { "glm": { "baseUrl": "...", "api": "openai-completions",
//!     "apiKey": "ENV_OR_KEY", "models": [ { "id": "glm-5.3", "reasoning": true } ] } } }
//! ```

use std::path::PathBuf;

use serde_json::Value;

use crate::config::{parse_lenient, read_json};

pub fn path() -> PathBuf {
    crate::config::agent_dir().join("models.json")
}

pub fn read() -> Result<Value, String> {
    read_json(&path())
}

/// 指定路径读（测试注入 / spawn 注入扫描用）。无文件 → 空对象。
pub fn read_at(path: &std::path::Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    read_json(path)
}

pub fn providers(value: &Value) -> Vec<(String, &Value)> {
    value
        .get("providers")
        .and_then(|p| p.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v)).collect())
        .unwrap_or_default()
}

pub fn provider_entry<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.get("providers")?.get(name)
}

pub fn provider_models<'a>(entry: &'a Value) -> Vec<&'a Value> {
    entry
        .get("models")
        .and_then(|m| m.as_array())
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

/// 自定义模型条目补 pi 运行时必需的缺省形状。实测 042-1（扩展注册模型不
/// 走 models.dev 缺省合并）：缺 input → 聊天报 `reading 'includes'`、
/// 缺 reasoning → `reading 'tiers'`、缺 cost 同崩——表现即「会话无反应」。
/// reasoning 缺省 true：pi 的 getSupportedThinkingLevels 对 falsy 只给
/// ["off"] 且菜单锁死（「推理强度强行 off」根因）；不可推理的模型走模型
/// 编辑器勾选关闭。已有值一律保留，幂等。
pub fn ensure_model_defaults(model: &mut Value) {
    let Some(map) = model.as_object_mut() else {
        return;
    };
    map.entry("input")
        .or_insert_with(|| serde_json::json!(["text"]));
    map.entry("reasoning").or_insert(serde_json::json!(true));
    let cost = map.entry("cost").or_insert_with(
        || serde_json::json!({"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}),
    );
    // 已有 cost 但缺键（自动填参只拿到 input/output）→ 补齐；非对象 → 重置
    match cost.as_object_mut() {
        Some(obj) => complete_cost(obj),
        None => {
            *cost = serde_json::json!({"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0})
        }
    }
}

/// pi `models[]` 的 cost 是 ModelCostSchema——**四键全必填**（只有
/// modelOverrides 才是 Partial）。缺任一键 pi 对**整份** models.json 拒载：
/// 自定义商全部消失、`--model 该商/模型` 直接 "not found" 退出（2026-11-02
/// 真机：my-glm 填参后只剩 input/output → 发送空转）。没给的补 0，已有值不动。
pub fn complete_cost(cost: &mut serde_json::Map<String, Value>) {
    for k in ["input", "output", "cacheRead", "cacheWrite"] {
        if cost.get(k).and_then(|x| x.as_f64()).is_none() {
            cost.insert(k.to_string(), serde_json::json!(0));
        }
    }
}

/// 整份文档按 pi 形状修复（= 落盘前的 normalize）：有改动返回 true。启动时
/// 对存量 models.json 跑一次——老版本写出的残缺 cost 在这里自愈，不必等用户
/// 再保存一次。
pub fn repair(doc: &mut Value) -> bool {
    let before = doc.clone();
    normalize(doc);
    *doc != before
}

/// Ensure `providers` exists and return the provider object for `name`,
/// creating an empty entry when missing.
fn provider_entry_mut<'a>(value: &'a mut Value, name: &str) -> &'a mut Value {
    let obj = value
        .as_object_mut()
        .expect("models.json document is an object");
    let providers = obj
        .entry("providers")
        .or_insert_with(|| Value::Object(Default::default()));
    providers
        .as_object_mut()
        .expect("providers is an object")
        .entry(name.to_string())
        .or_insert_with(|| Value::Object(Default::default()))
}

fn remove_provider_entry(value: &mut Value, name: &str) -> bool {
    value
        .get_mut("providers")
        .and_then(|p| p.as_object_mut())
        .map(|obj| obj.remove(name).is_some())
        .unwrap_or(false)
}

/// Create or replace a whole provider entry (custom-provider create / field
/// edits that hand back a full object). Returns true when it replaced one.
pub fn upsert_provider(value: &mut Value, name: &str, entry: Value) -> bool {
    let replaced = value.get("providers").and_then(|p| p.get(name)).is_some();
    let slot = provider_entry_mut(value, name);
    *slot = entry;
    replaced
}

/// Rename a provider key, preserving its position in the object.
pub fn rename_provider(value: &mut Value, from: &str, to: &str) -> Result<(), String> {
    if from == to {
        return Ok(());
    }
    let obj = value
        .get_mut("providers")
        .and_then(|p| p.as_object_mut())
        .ok_or_else(|| "models.json has no providers".to_string())?;
    if !obj.contains_key(from) {
        return Err(format!("no provider named {from}"));
    }
    if obj.contains_key(to) {
        return Err(format!("provider {to} already exists"));
    }
    // serde_json objects preserve insertion order; rebuild the map in the
    // original order with the key swapped in place.
    let entries: Vec<(String, Value)> = obj
        .iter()
        .map(|(k, v)| {
            let key = if k == from { to.to_string() } else { k.clone() };
            (key, v.clone())
        })
        .collect();
    *obj = entries.into_iter().collect();
    Ok(())
}

pub fn remove_provider(value: &mut Value, name: &str) -> bool {
    remove_provider_entry(value, name)
}

/// Overwrite one model entry (Save applies the editor fields).
pub fn update_model(value: &mut Value, provider: &str, index: usize, model: Value) {
    let entry = provider_entry_mut(value, provider);
    if let Some(arr) = entry
        .as_object_mut()
        .and_then(|o| o.get_mut("models"))
        .and_then(|m| m.as_array_mut())
    {
        if index < arr.len() {
            arr[index] = model;
        }
    }
}

/// Append a full model entry（【+模型】草稿行的保存落盘）。+模型不再预写
/// `{id:""}` 占位——validate 对空 id 硬拒绝，预写必失败且把整次写盘连带
/// 炸掉（P1-1）；草稿行保存时在此追加。返回新条目下标。
pub fn push_model(value: &mut Value, provider: &str, model: Value) -> usize {
    let entry = provider_entry_mut(value, provider);
    let obj = entry.as_object_mut().expect("provider entry is an object");
    let arr = obj
        .entry("models")
        .or_insert_with(|| Value::Array(Vec::new()));
    let arr = arr.as_array_mut().expect("models is an array");
    arr.push(model);
    arr.len() - 1
}

pub fn remove_model(value: &mut Value, provider: &str, index: usize) -> bool {
    let entry = provider_entry_mut(value, provider);
    entry
        .as_object_mut()
        .and_then(|o| o.get_mut("models"))
        .and_then(|m| m.as_array_mut())
        .map(|arr| (index < arr.len()).then(|| arr.remove(index)).is_some())
        .unwrap_or(false)
}

/// 勾选物化（042-1，自定义商专用；**纯内存变换**，落盘由调用方负责）。
/// `enable` → 把 `id` 以完整定义追加进 `providers.<p>.models[]`：定义优先
/// 原样取 `store_def`（清单仓，手调参数 off/on 往返不丢），仓里没有才按展示
/// 条目（`shown_name` / `shown_ctx`）最小构造；两条路都从商骨架兜底
/// api/baseUrl（pi `modelFromJson` 对缺 api/baseUrl 硬 throw）再
/// [`ensure_model_defaults`]。`!enable` → 按 id 移除，`models[]` 空则删键
///（骨架 baseUrl/api/name 保留，pi 不 throw）。已在 / 已不在 → 幂等 no-op；
/// 商不在账本（内置/扩展）→ no-op。返回是否改动了 `doc`。
///
/// 0.3.2 教训：此前两条 enable 分支各自 push，仓命中分支 push 完直接
/// `return Ok(())`，根本没走到落盘——勾选只改了内存克隆，pi 永远不认识该
/// 模型（对话出错 + 详情页因账本无此 id 落只读视图）。合并成单一出口。
pub fn materialize_model(
    doc: &mut Value,
    provider: &str,
    id: &str,
    enable: bool,
    store_def: Option<&Value>,
    shown_name: Option<&str>,
    shown_ctx: Option<u64>,
) -> bool {
    let Some(entry) = provider_entry(doc, provider) else {
        return false;
    };
    let skeleton = |key: &str| {
        entry
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let (skeleton_api, skeleton_base) = (skeleton("api"), skeleton("baseUrl"));
    let existing_ix = provider_models(entry)
        .iter()
        .position(|m| m.get("id").and_then(|v| v.as_str()) == Some(id));
    if !enable {
        let Some(ix) = existing_ix else {
            return false;
        };
        remove_model(doc, provider, ix);
        if let Some(o) = provider_entry_mut(doc, provider).as_object_mut() {
            if o.get("models")
                .and_then(|m| m.as_array())
                .is_some_and(|a| a.is_empty())
            {
                o.remove("models");
            }
        }
        return true;
    }
    if existing_ix.is_some() {
        return false;
    }
    let mut model = match store_def {
        Some(d) if d.is_object() => d.clone(),
        _ => {
            let mut obj = serde_json::Map::new();
            if let Some(n) = shown_name.filter(|n| !n.is_empty()) {
                obj.insert("name".into(), Value::String(n.to_string()));
            }
            if let Some(cw) = shown_ctx {
                obj.insert("contextWindow".into(), serde_json::json!(cw));
            }
            Value::Object(obj)
        }
    };
    if let Some(o) = model.as_object_mut() {
        o.insert("id".into(), Value::String(id.to_string()));
        for (key, fallback) in [("api", skeleton_api), ("baseUrl", skeleton_base)] {
            let missing = o.get(key).and_then(|v| v.as_str()).unwrap_or("").is_empty();
            if missing {
                if let Some(v) = fallback {
                    o.insert(key.into(), Value::String(v));
                }
            }
        }
    }
    ensure_model_defaults(&mut model);
    push_model(doc, provider, model);
    true
}

/// Lenient re-parse used when loading the file into the editor buffer.
pub fn parse(text: &str) -> Result<Value, String> {
    parse_lenient(text)
}

// ---------------------------------------------------------------------------
// 写盘（042-1）
// ---------------------------------------------------------------------------

/// pi `MODEL_INPUT_MODALITIES` 只认这两项；多写会让 pi 对**整份文件**的
/// schema 校验失败（自定义 provider 全部消失）。
const ALLOWED_MODALITIES: &[&str] = &["text", "image"];

fn validate_model_at(provider: &str, ix: usize, model: &Value) -> Result<(), String> {
    let obj = model
        .as_object()
        .ok_or_else(|| format!("{provider}.models[{ix}] 不是对象"))?;
    match obj.get("id").and_then(Value::as_str).map(str::trim) {
        Some(id) if !id.is_empty() => {}
        _ => return Err(format!("{provider}.models[{ix}] 缺少 id")),
    }
    if let Some(input) = obj.get("input") {
        let arr = input
            .as_array()
            .ok_or_else(|| format!("{provider}.models[{ix}].input 不是数组"))?;
        for v in arr {
            let s = v.as_str().unwrap_or("");
            if !ALLOWED_MODALITIES.contains(&s) {
                return Err(format!(
                    "{provider}.models[{ix}].input 含非法模态 {s:?}（只允许 text/image）"
                ));
            }
        }
    }
    for key in ["contextWindow", "maxTokens"] {
        if let Some(v) = obj.get(key) {
            if !v.as_f64().is_some_and(|n| n > 0.0) {
                return Err(format!("{provider}.models[{ix}].{key} 必须是正数"));
            }
        }
    }
    if let Some(cost) = obj.get("cost") {
        let rates = cost
            .as_object()
            .ok_or_else(|| format!("{provider}.models[{ix}].cost 不是对象"))?;
        for (k, v) in rates {
            if !v.is_number() {
                return Err(format!("{provider}.models[{ix}].cost.{k} 必须是数字"));
            }
        }
        // pi ModelCostSchema 四键全必填——缺一个整份拒载（normalize 已补齐，
        // 这里是最后一道闸：绝不写出 pi 不认的文件）
        for k in ["input", "output", "cacheRead", "cacheWrite"] {
            if !rates.contains_key(k) {
                return Err(format!(
                    "{provider}.models[{ix}].cost 缺少 {k}（pi 四键必填）"
                ));
            }
        }
    }
    Ok(())
}

/// 写盘前校验（pi schema 的子集）：结构错就 Err，不落盘。
pub fn validate(doc: &Value) -> Result<(), String> {
    let obj = doc.as_object().ok_or("models.json 顶层必须是对象")?;
    let providers = obj
        .get("providers")
        .ok_or("models.json 缺少 providers")?
        .as_object()
        .ok_or("providers 必须是对象")?;
    for (name, entry) in providers {
        let p = entry
            .as_object()
            .ok_or_else(|| format!("provider {name} 不是对象"))?;
        if let Some(models) = p.get("models") {
            let arr = models
                .as_array()
                .ok_or_else(|| format!("provider {name} 的 models 不是数组"))?;
            for (ix, model) in arr.iter().enumerate() {
                validate_model_at(name, ix, model)?;
            }
        }
    }
    Ok(())
}

/// 落盘前补齐模型缺省形状（input/reasoning/cost），并剥除 PF 私有键，幂等。
fn normalize(doc: &mut Value) {
    let Some(providers) = doc.get_mut("providers").and_then(|p| p.as_object_mut()) else {
        return;
    };
    for entry in providers.values_mut() {
        let Some(models) = entry.get_mut("models").and_then(|m| m.as_array_mut()) else {
            continue;
        };
        for model in models.iter_mut() {
            // PF 私有元数据（priceSource，042-2）只住清单仓，永不进 pi 账本
            if let Some(o) = model.as_object_mut() {
                o.remove(crate::model_price::SOURCE_KEY);
            }
            ensure_model_defaults(model);
        }
    }
}

pub fn save(doc: &Value) -> Result<(), String> {
    save_at_locked(&path(), doc)
}

/// 锁内整份写。不可读（用户写坏/手工带错）时**拒绝写入**——pi 的 schema
/// 是整份校验，覆盖一次就等于删掉用户的全部自定义 provider。
pub fn save_at_locked(path: &std::path::Path, doc: &Value) -> Result<(), String> {
    let mut doc = doc.clone();
    normalize(&mut doc);
    validate(&doc)?;
    let _guard = crate::fs_lock::lock(path, std::time::Duration::from_secs(10))?;
    if path.exists() {
        let text = std::fs::read(path).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&text).into_owned();
        let text = text.trim_start_matches('\u{feff}');
        if parse(text).is_err() {
            return Err("models.json 无法解析，已拒绝写入（请先修好该文件）".to_string());
        }
    }
    crate::config::write_json_private(path, &doc)
}

#[cfg(test)]
mod save_tests;
#[cfg(test)]
mod tests;
