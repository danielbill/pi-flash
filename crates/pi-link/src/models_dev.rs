//! models.dev 模型目录（042 M2「填入模型信息」，对齐 pi-web
//! `lib/model-catalog.ts` + `app/api/models-config/catalog/route.ts`）。
//!
//! 用途：自定义模型编辑器里按模型 id 从 models.dev 的公开目录 best-effort
//! 回填空字段（显示名/推理/输入模态/上下文窗/最大输出/每百万价格）。请求
//! 形状、字段映射、匹配与共识定价规则逐条对照 pi-web；磁盘缓存 1h（pi-web
//! 是进程内存缓存，PF 桌面端沉成文件，重启不重拉）。

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const MODELS_DEV_URL: &str = "https://models.dev/api.json";
const CACHE_TTL_SECS: u64 = 60 * 60;
const FETCH_TIMEOUT_SECS: u64 = 15;

/// 目录条目（flatten 后的一行）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CatalogEntry {
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub provider_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_base_url: Option<String>,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    /// 过滤后的输入模态（只留 text/image；空 = 未提供）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub cost: Cost,
}

/// 每百万价格（缺项 = None）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Cost {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

/// 回填预设（只含目录能给的字段）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Preset {
    pub name: Option<String>,
    pub reasoning: Option<bool>,
    pub input: Option<Vec<String>>,
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
    pub cost: Option<Cost>,
}

/// 价格推荐（对齐 pi-web ModelCatalogPriceRecommendation；PF 侧只驱动
/// 提示文案，错误态折成一个原因枚举）。
#[derive(Debug, Clone, PartialEq)]
pub enum Price {
    Reliable {
        method: MatchMethod,
        cost: Cost,
        provider_id: Option<String>,
        provider_name: Option<String>,
        support: usize,
        total: usize,
    },
    Unreliable {
        reason: &'static str,
        support: usize,
        total: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMethod {
    Provider,
    BaseUrl,
    Consensus,
}

impl MatchMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::BaseUrl => "base-url",
            Self::Consensus => "consensus",
        }
    }
}

/// 匹配结果。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Recommendation {
    pub exact_matches: usize,
    pub preset: Preset,
    pub price: Option<Price>,
}

// ---- flatten ----------------------------------------------------------------

fn clean_str(v: Option<&Value>) -> Option<String> {
    v?.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn opt_pos_num(v: Option<&Value>) -> Option<u64> {
    v?.as_f64()
        .filter(|n| n.is_finite() && *n > 0.)
        .map(|n| n as u64)
}

fn opt_nonneg_f64(v: Option<&Value>) -> Option<f64> {
    v?.as_f64().filter(|n| n.is_finite() && *n >= 0.)
}

fn read_cost(v: Option<&Value>) -> Cost {
    let Some(map) = v.and_then(|v| v.as_object()) else {
        return Cost::default();
    };
    Cost {
        input: opt_nonneg_f64(map.get("input")),
        output: opt_nonneg_f64(map.get("output")),
        cache_read: opt_nonneg_f64(map.get("cache_read")),
        cache_write: opt_nonneg_f64(map.get("cache_write")),
    }
}

fn read_input_modalities(v: Option<&Value>) -> Option<Vec<String>> {
    let arr = v?.get("input")?.as_array()?;
    let mut out: Vec<String> = Vec::new();
    for item in arr {
        if let Some(s) = item.as_str() {
            let s = s.trim().to_lowercase();
            if (s == "text" || s == "image") && !out.contains(&s) {
                out.push(s);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// models.dev api.json → 扁平条目列表（pi-web flattenModelsDevCatalog 对齐）。
pub fn flatten(value: &Value) -> Vec<CatalogEntry> {
    let mut entries = Vec::new();
    let Some(providers) = value.as_object() else {
        return entries;
    };
    for (provider_id, raw_provider) in providers {
        let Some(models) = raw_provider.get("models").and_then(|m| m.as_object()) else {
            continue;
        };
        let provider_name =
            clean_str(raw_provider.get("name")).unwrap_or_else(|| provider_id.clone());
        let provider_base_url = clean_str(raw_provider.get("api"));
        for (fallback_id, raw_model) in models {
            let Some(raw_model) = raw_model.as_object() else {
                continue;
            };
            let id = clean_str(raw_model.get("id")).unwrap_or_else(|| fallback_id.clone());
            if id.is_empty() {
                continue;
            }
            let name = clean_str(raw_model.get("name")).unwrap_or_else(|| id.clone());
            entries.push(CatalogEntry {
                provider_id: provider_id.clone(),
                provider_name: provider_name.clone(),
                provider_base_url: provider_base_url.clone(),
                reasoning: raw_model.get("reasoning").and_then(|v| v.as_bool()),
                input: read_input_modalities(raw_model.get("modalities")),
                // models.dev 把窗口/输出上限放在 limit{context,output}
                context_window: raw_model.get("limit").and_then(|l| opt_pos_num(l.get("context"))),
                max_tokens: raw_model.get("limit").and_then(|l| opt_pos_num(l.get("output"))),
                cost: read_cost(raw_model.get("cost")),
                id,
                name,
            });
        }
    }
    entries
}

// ---- 匹配与推荐 --------------------------------------------------------------

fn normalize_provider(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

fn normalize_model_id(s: &str) -> String {
    s.trim().to_lowercase().trim_start_matches("models/").to_string()
}

fn hostname(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let host = rest.split(['/', ':', '?']).next()?;
    let host = host.trim_end_matches('.');
    (!host.is_empty()).then(|| host.to_lowercase())
}

fn host_matches(actual: &str, expected: &str) -> bool {
    actual == expected || actual.ends_with(&format!(".{expected}"))
}

/// 内置商的已知 API 域名（pi-web KNOWN_PROVIDER_HOSTS 对齐）。
fn known_provider_hosts(provider_id: &str) -> &'static [&'static str] {
    match normalize_provider(provider_id).as_str() {
        "anthropic" => &["api.anthropic.com"],
        "google" => &["generativelanguage.googleapis.com"],
        "openai" => &["api.openai.com"],
        "openrouter" => &["openrouter.ai"],
        _ => &[],
    }
}

fn provider_matches(entry: &CatalogEntry, provider_hint: &str) -> bool {
    let hint = normalize_provider(provider_hint);
    !hint.is_empty()
        && (normalize_provider(&entry.provider_id) == hint
            || normalize_provider(&entry.provider_name) == hint)
}

fn base_url_matches(entry: &CatalogEntry, base_url: &str) -> bool {
    let Some(actual) = hostname(base_url) else {
        return false;
    };
    if known_provider_hosts(&entry.provider_id)
        .iter()
        .any(|c| host_matches(&actual, c))
    {
        return true;
    }
    match entry.provider_base_url.as_deref().and_then(hostname) {
        Some(h) => host_matches(&actual, &h),
        None => false,
    }
}

fn exact_model_matches(entry: &CatalogEntry, query: &str) -> bool {
    let q = normalize_model_id(query);
    if q.is_empty() {
        return false;
    }
    let id = normalize_model_id(&entry.id);
    let full = format!("{}/{}", entry.provider_id.to_lowercase(), id);
    id == q || full == q
}

/// 众数（并列即无）。pi-web modeValue/modeNumber：赢家份额或并列规则见各处。
fn mode_value<T: PartialEq + Clone>(values: &[T]) -> Option<T> {
    let mut groups: Vec<(usize, T)> = Vec::new(); // (count, value)
    for v in values {
        if let Some((count, _)) = groups.iter_mut().find(|(_, g)| *g == *v) {
            *count += 1;
        } else {
            groups.push((1, v.clone()));
        }
    }
    groups.sort_by(|a, b| b.0.cmp(&a.0));
    let (top_n, top_v) = groups.first()?.clone();
    if groups.len() > 1 && groups[1].0 == top_n {
        return None; // 并列 → 无共识
    }
    Some(top_v)
}

/// 共识元数据（provider/baseUrl 都没匹配上时的兜底）。
fn consensus_metadata<'a>(entries: &[&'a CatalogEntry]) -> Preset {
    let total = entries.len();
    if total == 0 {
        return Preset::default();
    }
    let share_ok = |n: usize| n * 10 >= total * 6; // ≥60%
    let names = mode_value(
        &entries
            .iter()
            .map(|e| e.name.to_lowercase())
            .collect::<Vec<_>>(),
    )
    .map(|n| {
        entries
            .iter()
            .find(|e| e.name.to_lowercase() == n)
            .map(|e| e.name.clone())
            .unwrap_or(n)
    });
    if let Some(n) = &names {
        let support = entries.iter().filter(|e| &e.name.to_lowercase() == &n.to_lowercase()).count();
        if !share_ok(support) {
            return Preset::default();
        }
    }
    let reasoning = mode_value(
        &entries
            .iter()
            .filter_map(|e| e.reasoning)
            .collect::<Vec<_>>(),
    );
    let input = mode_value(
        &entries
            .iter()
            .filter_map(|e| e.input.clone())
            .collect::<Vec<_>>(),
    );
    let context_window = mode_value(
        &entries
            .iter()
            .filter_map(|e| e.context_window)
            .collect::<Vec<_>>(),
    );
    let max_tokens = mode_value(
        &entries
            .iter()
            .filter_map(|e| e.max_tokens)
            .collect::<Vec<_>>(),
    );
    Preset {
        name: names,
        reasoning,
        input,
        context_window,
        max_tokens,
        cost: None,
    }
}

fn price_from_entry(entry: &CatalogEntry, method: MatchMethod) -> Price {
    Price::Reliable {
        method,
        cost: Cost {
            input: entry.cost.input,
            output: entry.cost.output,
            cache_read: Some(entry.cost.cache_read.unwrap_or(0.)),
            cache_write: Some(entry.cost.cache_write.unwrap_or(0.)),
        },
        provider_id: Some(entry.provider_id.clone()),
        provider_name: Some(entry.provider_name.clone()),
        support: 1,
        total: 1,
    }
}

/// 共识定价：按 (input, output) 分组取最大组，≥60% 或 ≥5 家才算可靠。
fn consensus_price<'a>(entries: &[&'a CatalogEntry]) -> Price {
    let priced: Vec<&CatalogEntry> = entries
        .iter()
        .filter(|e| e.cost.input.is_some() && e.cost.output.is_some())
        .copied()
        .collect();
    if priced.is_empty() {
        return Price::Unreliable { reason: "no-valid-price", support: 0, total: 0 };
    }
    if priced.len() == 1 {
        return Price::Unreliable {
            reason: "insufficient-support",
            support: 1,
            total: 1,
        };
    }
    // 按 (input, output) 舍入到 6 位小数分组（f64 作 map 键不稳）
    let key = |e: &CatalogEntry| {
        (
            format!("{:.6}", e.cost.input.unwrap_or(0.)),
            format!("{:.6}", e.cost.output.unwrap_or(0.)),
        )
    };
    let mut groups: Vec<((String, String), usize)> = Vec::new();
    for e in &priced {
        let k = key(e);
        if let Some(g) = groups.iter_mut().find(|(gk, _)| *gk == k) {
            g.1 += 1;
        } else {
            groups.push((k, 1));
        }
    }
    groups.sort_by(|a, b| b.1.cmp(&a.1));
    let (win_key, win_n) = &groups[0];
    if groups.len() > 1 && groups[1].1 == *win_n {
        return Price::Unreliable {
            reason: "conflict",
            support: *win_n,
            total: priced.len(),
        };
    }
    let has_consensus = *win_n as f64 / priced.len() as f64 >= 0.6 || *win_n >= 5;
    if !has_consensus {
        return Price::Unreliable {
            reason: "conflict",
            support: *win_n,
            total: priced.len(),
        };
    }
    let winners: Vec<&CatalogEntry> =
        priced.iter().filter(|e| key(e) == *win_key).copied().collect();
    let cache_read = mode_value(
        &winners
            .iter()
            .map(|e| e.cost.cache_read.unwrap_or(0.))
            .collect::<Vec<_>>(),
    )
    .unwrap_or(0.);
    let cache_write = mode_value(
        &winners
            .iter()
            .map(|e| e.cost.cache_write.unwrap_or(0.))
            .collect::<Vec<_>>(),
    )
    .unwrap_or(0.);
    let win_e = winners[0];
    Price::Reliable {
        method: MatchMethod::Consensus,
        cost: Cost {
            input: win_e.cost.input,
            output: win_e.cost.output,
            cache_read: Some(cache_read),
            cache_write: Some(cache_write),
        },
        provider_id: None,
        provider_name: None,
        support: *win_n,
        total: priced.len(),
    }
}

/// 推荐（pi-web recommendModelCatalogPreset 对齐）：id 精确匹配 → provider
/// 提示优先取元数据，其次 baseUrl 匹配，再次跨 provider 共识；价格同序。
pub fn recommend(
    entries: &[CatalogEntry],
    query: &str,
    provider_hint: &str,
    base_url: &str,
) -> Recommendation {
    let exact: Vec<&CatalogEntry> = entries
        .iter()
        .filter(|e| exact_model_matches(e, query))
        .collect();
    if exact.is_empty() {
        return Recommendation {
            exact_matches: 0,
            preset: Preset::default(),
            price: Some(Price::Unreliable {
                reason: "no-exact-match",
                support: 0,
                total: 0,
            }),
        };
    }
    let provider_entry = exact.iter().find(|e| provider_matches(e, provider_hint));
    let base_entry = exact.iter().find(|e| base_url_matches(e, base_url));

    let pick = |e: &CatalogEntry, method: MatchMethod| -> (Preset, Price) {
        let preset = metadata_preset(e);
        let price = if e.cost.input.is_some() && e.cost.output.is_some() {
            price_from_entry(e, method)
        } else {
            consensus_price(&exact)
        };
        (preset, price)
    };
    let (preset, price) = if let Some(e) = provider_entry {
        pick(e, MatchMethod::Provider)
    } else if let Some(e) = base_entry {
        pick(e, MatchMethod::BaseUrl)
    } else {
        (consensus_metadata(&exact), consensus_price(&exact))
    };
    Recommendation {
        exact_matches: exact.len(),
        preset,
        price: Some(price),
    }
}

fn metadata_preset(e: &CatalogEntry) -> Preset {
    Preset {
        name: Some(e.name.clone()),
        reasoning: e.reasoning,
        input: e.input.clone(),
        context_window: e.context_window,
        max_tokens: e.max_tokens,
        cost: (e.cost.input.is_some() && e.cost.output.is_some()).then_some(e.cost),
    }
}

/// 按预设**覆盖**模型的目录派生字段（换了模型 ID 后的回填语义）：上下文/
/// 最大输出/能力/名称/价格都跟新 ID 走，旧值属于上一个模型、留着就是错的。
/// 返回实际改变的字段数。
pub fn fill_from_preset(model: &mut Value, preset: &Preset) -> usize {
    let mut applied = 0usize;
    let Some(map) = model.as_object_mut() else {
        return 0;
    };
    let mut put = |key: &str, v: Value, map: &mut serde_json::Map<String, Value>| {
        if map.get(key) != Some(&v) {
            map.insert(key.to_string(), v);
            applied += 1;
        }
    };
    if let Some(name) = preset.name.as_deref().filter(|n| !n.is_empty()) {
        put("name", Value::String(name.to_string()), map);
    }
    if let Some(reasoning) = preset.reasoning {
        put("reasoning", Value::Bool(reasoning), map);
    }
    if let Some(input) = preset.input.as_ref().filter(|i| !i.is_empty()) {
        put(
            "input",
            Value::Array(input.iter().map(|s| Value::String(s.clone())).collect()),
            map,
        );
    }
    if let Some(n) = preset.context_window {
        put("contextWindow", serde_json::json!(n), map);
    }
    if let Some(n) = preset.max_tokens {
        put("maxTokens", serde_json::json!(n), map);
    }
    if let Some(cost) = &preset.cost {
        // 四项全齐才整块写（pi 对扩展模型要求 cost 完整，缺项会让运行时崩）
        if let (Some(i), Some(o), Some(cr), Some(cw)) =
            (cost.input, cost.output, cost.cache_read, cost.cache_write)
        {
            put(
                "cost",
                serde_json::json!({"input": i, "output": o, "cacheRead": cr, "cacheWrite": cw}),
                map,
            );
        }
    }
    applied
}

// ---- 磁盘缓存 ----------------------------------------------------------------

/// 缓存文件（`~/.pi-flash/models-dev-cache.json`）。
pub fn cache_path() -> Option<PathBuf> {
    crate::paths::models_dev_cache_file()
}

#[derive(Serialize, Deserialize)]
struct CacheDoc {
    fetched_at: u64,
    entries: Vec<CatalogEntry>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_cache(path: &std::path::Path) -> Option<(Vec<CatalogEntry>, u64)> {
    let text = std::fs::read_to_string(path).ok()?;
    let doc: CacheDoc = serde_json::from_str(&text).ok()?;
    (!doc.entries.is_empty()).then_some((doc.entries, doc.fetched_at))
}

fn write_cache(path: &std::path::Path, entries: &[CatalogEntry]) {
    let doc = CacheDoc {
        fetched_at: now_secs(),
        entries: entries.to_vec(),
    };
    if let Ok(text) = serde_json::to_string(&doc) {
        let _ = std::fs::write(path, text);
    }
}

/// 拉目录（阻塞，供后台线程）：缓存命中（<1h）直接回；否则 GET models.dev
/// 并落缓存；网络失败回退陈旧缓存（pi-web loadCatalog 失败回旧 entries 的
/// 语义），都没有才报错。
pub fn load() -> Result<Vec<CatalogEntry>, String> {
    let path = cache_path().ok_or("pf data dir unavailable")?;
    if let Some((entries, at)) = read_cache(&path) {
        if now_secs().saturating_sub(at) < CACHE_TTL_SECS {
            return Ok(entries);
        }
    }
    match fetch_remote() {
        Ok(entries) if !entries.is_empty() => {
            write_cache(&path, &entries);
            Ok(entries)
        }
        Ok(_) => {
            if let Some((entries, _)) = read_cache(&path) {
                Ok(entries)
            } else {
                Err("models.dev returned an empty catalog".into())
            }
        }
        Err(e) => {
            if let Some((entries, _)) = read_cache(&path) {
                Ok(entries)
            } else {
                Err(e)
            }
        }
    }
}

/// 拉目录并**强制刷新**（阻塞，供后台线程·用户点【自动填参】时用）：
/// 先 GET models.dev 并落缓存；网络/解析失败回退陈旧缓存（离线也能回填），
/// 都没有才报错。与 [`load`] 的区别只有“不看 TTL、每次都真拉”。
pub fn load_fresh() -> Result<Vec<CatalogEntry>, String> {
    let path = cache_path().ok_or("pf data dir unavailable")?;
    match fetch_remote() {
        Ok(entries) if !entries.is_empty() => {
            write_cache(&path, &entries);
            Ok(entries)
        }
        Ok(_) => read_cache(&path)
            .map(|(entries, _)| entries)
            .ok_or_else(|| "models.dev returned an empty catalog".into()),
        Err(e) => read_cache(&path).map(|(entries, _)| entries).ok_or(e),
    }
}

fn fetch_remote() -> Result<Vec<CatalogEntry>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build();
    let resp = agent
        .get(MODELS_DEV_URL)
        .set("Accept", "application/json")
        .call()
        .map_err(|e| format!("models.dev fetch failed: {e}"))?;
    // 响应体限 32MB（目录全量文档，扁平计数不需要全文）
    let text = resp
        .into_string()
        .map_err(|e| format!("models.dev read failed: {e}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| format!("models.dev parse failed: {e}"))?;
    Ok(flatten(&value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flatten_maps_models_dev_shapes() {
        let doc = json!({
            "anthropic": {
                "name": "Anthropic", "api": "https://api.anthropic.com",
                "models": {
                    "claude-x": {
                        "id": "claude-x", "name": "Claude X", "reasoning": true,
                        "modalities": { "input": ["text", "image", "audio"] },
                        "limit": { "context": 200000, "output": 8192 },
                        "cost": { "input": 3.0, "output": 15.0, "cache_read": 0.3, "cache_write": 3.75 }
                    },
                    "no-id": { "name": "broken" },
                    "nested": { "id": "" }
                }
            },
            "no-models": { "name": "skip me" }
        });
        let entries = flatten(&doc);
        // pi-web 语义：id 缺失/空 → 回落到对象键（"no-id"/"nested" 都保留）
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1].id, "no-id");
        assert_eq!(entries[2].id, "nested");
        let e = &entries[0];
        assert_eq!(e.provider_id, "anthropic");
        assert_eq!(e.provider_name, "Anthropic");
        assert_eq!(e.provider_base_url.as_deref(), Some("https://api.anthropic.com"));
        assert_eq!(e.id, "claude-x");
        assert_eq!(e.name, "Claude X");
        assert_eq!(e.reasoning, Some(true));
        assert_eq!(e.input.as_deref(), Some(&["text".to_string(), "image".to_string()][..]));
        assert_eq!(e.context_window, Some(200000));
        assert_eq!(e.max_tokens, Some(8192));
        assert_eq!(e.cost.input, Some(3.0));
        assert_eq!(e.cost.cache_read, Some(0.3));
    }

    #[test]
    fn recommend_prefers_provider_then_baseurl_then_consensus() {
        let entries = vec![
            CatalogEntry {
                provider_id: "deepseek".into(),
                provider_name: "DeepSeek".into(),
                id: "ds-v3".into(),
                name: "DeepSeek V3".into(),
                reasoning: Some(true),
                input: Some(vec!["text".into()]),
                context_window: Some(128000),
                max_tokens: Some(8192),
                cost: Cost { input: Some(0.27), output: Some(1.1), cache_read: None, cache_write: None },
                provider_base_url: Some("https://api.deepseek.com".into()),
            },
            CatalogEntry {
                provider_id: "openrouter".into(),
                provider_name: "OpenRouter".into(),
                id: "ds-v3".into(),
                name: "DeepSeek V3 (OR)".into(),
                reasoning: Some(false),
                input: None,
                context_window: Some(64000),
                max_tokens: None,
                cost: Cost { input: Some(0.5), output: Some(2.0), cache_read: None, cache_write: None },
                provider_base_url: Some("https://openrouter.ai".into()),
            },
        ];
        // provider 提示命中 → 取该条元数据与价格
        let r = recommend(&entries, "ds-v3", "deepseek", "");
        assert_eq!(r.exact_matches, 2);
        assert_eq!(r.preset.name.as_deref(), Some("DeepSeek V3"));
        assert_eq!(r.preset.context_window, Some(128000));
        assert!(matches!(r.price, Some(Price::Reliable { method: MatchMethod::Provider, .. })));
        // baseUrl 命中（provider 名不匹配）
        let r = recommend(&entries, "ds-v3", "no-such", "https://api.deepseek.com/v1");
        assert_eq!(r.preset.name.as_deref(), Some("DeepSeek V3"));
        assert!(matches!(r.price, Some(Price::Reliable { method: MatchMethod::BaseUrl, .. })));
        // 都不匹配 → 共识元数据 + 共识价格（价格分组冲突 → unreliable）
        let r = recommend(&entries, "ds-v3", "no-such", "");
        assert_eq!(r.preset.name, None, "两组元数据无共识 → 不回填");
        assert!(matches!(r.price, Some(Price::Unreliable { reason: "conflict", .. })));
        // id 不存在
        let r = recommend(&entries, "gpt-9", "deepseek", "");
        assert_eq!(r.exact_matches, 0);
        assert!(matches!(r.price, Some(Price::Unreliable { reason: "no-exact-match", .. })));
    }

    #[test]
    fn full_id_and_models_prefix_match() {
        let entries = vec![CatalogEntry {
            provider_id: "google".into(),
            provider_name: "Google".into(),
            id: "gemini-flash".into(),
            name: "Gemini Flash".into(),
            reasoning: None,
            input: None,
            context_window: None,
            max_tokens: None,
            cost: Cost::default(),
            provider_base_url: None,
        }];
        assert_eq!(recommend(&entries, "google/gemini-flash", "", "").exact_matches, 1);
        assert_eq!(recommend(&entries, "models/gemini-flash", "", "").exact_matches, 1);
        assert_eq!(recommend(&entries, "GEMINI-FLASH", "", "").exact_matches, 1);
    }

    #[test]
    fn fill_from_preset_overwrites_stale_values() {
        // 换了 ID 的场景：旧上下文/旧能力/旧价格都属于上一个模型，必须被换掉
        let mut model = json!({
            "id": "deepseek-v4-pro",
            "name": "旧名字",
            "contextWindow": 1048576,
            "maxTokens": 393216,
            "input": ["text"],
            "cost": {"input": 0.28, "output": 0.42, "cacheRead": 0.028, "cacheWrite": 0.}
        });
        let preset = Preset {
            name: Some("DeepSeek V4 Pro".into()),
            reasoning: Some(true),
            input: Some(vec!["text".into(), "image".into()]),
            context_window: Some(128000),
            max_tokens: Some(8192),
            cost: Some(Cost {
                input: Some(0.27),
                output: Some(1.1),
                cache_read: Some(0.),
                cache_write: Some(0.),
            }),
        };
        let n = fill_from_preset(&mut model, &preset);
        // name + reasoning + input + contextWindow + maxTokens + cost = 6
        assert_eq!(n, 6);
        assert_eq!(model["name"], "DeepSeek V4 Pro");
        assert_eq!(model["reasoning"], true);
        assert_eq!(model["contextWindow"], 128000);
        assert_eq!(model["maxTokens"], 8192);
        assert_eq!(model["input"][1], "image");
        assert_eq!(model["cost"]["output"], 1.1);
        // 幂等：值已经一致 → 0
        assert_eq!(fill_from_preset(&mut model, &preset), 0);
    }

    #[test]
    fn base_url_matching_uses_known_hosts_and_suffix() {
        let e = CatalogEntry {
            provider_id: "openai".into(),
            provider_name: "OpenAI".into(),
            id: "gpt".into(),
            name: "GPT".into(),
            reasoning: None,
            input: None,
            context_window: None,
            max_tokens: None,
            cost: Cost::default(),
            provider_base_url: None,
        };
        assert!(base_url_matches(&e, "https://api.openai.com/v1"));
        assert!(base_url_matches(&e, "https://sub.api.openai.com/v1"));
        assert!(!base_url_matches(&e, "https://evil.openai.com.evil.io/v1"));
        assert!(!base_url_matches(&e, "https://api.groq.com/v1"));
    }

    #[test]
    fn consensus_price_needs_share_or_support() {
        // 3 家同价（100% ≥ 60%）→ reliable
        let mk = |input: f64, output: f64| CatalogEntry {
            provider_id: format!("p{input}{output}"),
            provider_name: "P".into(),
            id: "m".into(),
            name: "M".into(),
            reasoning: None,
            input: None,
            context_window: None,
            max_tokens: None,
            cost: Cost { input: Some(input), output: Some(output), cache_read: None, cache_write: None },
            provider_base_url: None,
        };
        let entries = vec![mk(1.0, 2.0), mk(1.0, 2.0), mk(1.0, 2.0)];
        assert!(matches!(
            consensus_price(&entries.iter().collect::<Vec<_>>()),
            Price::Reliable { method: MatchMethod::Consensus, .. }
        ));
        // 2 家同价 + 1 家异价（66% ≥ 60%）→ reliable
        let entries = vec![mk(1.0, 2.0), mk(1.0, 2.0), mk(9.0, 9.0)];
        assert!(matches!(
            consensus_price(&entries.iter().collect::<Vec<_>>()),
            Price::Reliable { .. }
        ));
        // 1 家有价 + 1 家无价 → insufficient-support
        let mut no_cost = mk(1.0, 2.0);
        no_cost.cost = Cost::default();
        let entries = vec![mk(1.0, 2.0), no_cost];
        assert!(matches!(
            consensus_price(&entries.iter().collect::<Vec<_>>()),
            Price::Unreliable { reason: "insufficient-support", support: 1, total: 1 }
        ));
    }
}
