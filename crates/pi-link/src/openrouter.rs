//! OpenRouter 模型目录（「自动填参」数据源，替代 models.dev 的 5MB 整包）。
//!
//! 实测（2026-10-27，本机直连）：单模型 `GET /api/v1/model/{author}/{slug}`
//! 2.4KB/0.9s 免鉴权；全量 `GET /api/v1/models` 783KB/3.7s。而 models.dev
//! api.json 5MB 在同一路由拉不完（<80KB/s 被 QoS）。字段映射：context_length
//! → contextWindow、top_provider.max_completion_tokens → maxTokens、
//! architecture.input_modalities（只留 text/image）→ input、supported_parameters
//! 含 reasoning* → reasoning、pricing.*（$/token ×1e6）→ cost（$/MTok）。
//! 解析顺序：query 自带 author → 单模型直查；否则按 provider 名/baseUrl host
//! 推断 author 试单模型；都不中退全量列表反查。全量走 1h 磁盘缓存，网络失败
//! 回陈旧缓存（离线也能回填）。

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const OR_BASE: &str = "https://openrouter.ai/api/v1";
const CACHE_TTL_SECS: u64 = 60 * 60;
const FETCH_TIMEOUT_SECS: u64 = 20;

/// 目录条目（OpenRouter 一个模型；价格为每百万 USD）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Entry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub cost: Cost,
    /// 「最新版」别名通配条目（"~author/…-latest"）：参数齐全但名称泛化
    /// （"… Flash Latest"），回填时名称不落账本（保留用户/测速落的具体名）。
    #[serde(default)]
    pub alias: bool,
    /// 别名指向的具体型号 id（"deepseek/deepseek-v4.1-flash-1017"）。命中
    /// 别名后顺着它落到具体条目——名称与参数都以具体型号为准。
    #[serde(default)]
    pub alias_target: Option<String>,
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

/// 解析失败原因（NoMatch 由 UI 翻译成本地文案）。
#[derive(Debug, Clone, PartialEq)]
pub enum FillError {
    NoMatch,
    Net(String),
}

// ---- 解析 --------------------------------------------------------------------

fn opt_pos_num(v: Option<&Value>) -> Option<u64> {
    v?.as_f64()
        .filter(|n| n.is_finite() && *n > 0.)
        .map(|n| n as u64)
}

/// pricing 字段是字符串数字（"0.0000000699"）或数字；$/token → $/百万。
fn price_per_mtok(v: Option<&Value>) -> Option<f64> {
    let n = match v? {
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        Value::Number(n) => n.as_f64()?,
        _ => return None,
    };
    (n.is_finite() && n >= 0.).then(|| n * 1_000_000.)
}

/// OpenRouter 单模型 JSON → 条目。
pub fn parse_entry(v: &Value) -> Option<Entry> {
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_lowercase();
    let mut name = v
        .get("name")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&id)
        .to_string();
    // "Z.ai: GLM 5.3" → "GLM 5.3"（剥厂商前缀）
    if let Some((_, rest)) = name.split_once(": ") {
        if !rest.trim().is_empty() {
            name = rest.trim().to_string();
        }
    }
    let params = v.get("supported_parameters").and_then(|x| x.as_array());
    let reasoning = params.is_some_and(|a| {
        a.iter().any(|p| {
            p.as_str().is_some_and(|s| {
                matches!(s, "reasoning" | "include_reasoning" | "reasoning_effort")
            })
        })
    });
    let mut input: Vec<String> = v
        .get("architecture")
        .and_then(|a| a.get("input_modalities"))
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str())
                .map(|s| s.trim().to_lowercase())
                .filter(|s| s == "text" || s == "image")
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    input.dedup();
    // 注意：**不**给「目录没给模态」的情况兜 {"text"}——留空表示未知，
    // 回填时保留模型原有能力（否则自动填参会把已勾的「图片」冲掉）
    let top = v.get("top_provider");
    let context_window = opt_pos_num(v.get("context_length")).or(opt_pos_num(
        top.as_ref().and_then(|t| t.get("context_length")),
    ));
    let max_tokens = opt_pos_num(top.as_ref().and_then(|t| t.get("max_completion_tokens")));
    let p = v.get("pricing");
    let cost = Cost {
        input: price_per_mtok(p.as_ref().and_then(|x| x.get("prompt"))),
        output: price_per_mtok(p.as_ref().and_then(|x| x.get("completion"))),
        cache_read: price_per_mtok(p.as_ref().and_then(|x| x.get("internal_cache_read"))),
        cache_write: price_per_mtok(p.as_ref().and_then(|x| x.get("internal_cache_write"))),
    };
    let alias = is_alias(&id);
    let alias_target = v
        .get("alias_target")
        .and_then(|t| t.get("slug"))
        .and_then(|s| s.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase);
    Some(Entry {
        id,
        name,
        reasoning,
        input,
        context_window,
        max_tokens,
        cost,
        alias,
        alias_target,
    })
}

fn parse_list(v: &Value) -> Vec<Entry> {
    v.get("data")
        .and_then(|d| d.as_array())
        .map(|a| a.iter().filter_map(parse_entry).collect())
        .unwrap_or_default()
}

// ---- author 推断 --------------------------------------------------------------

fn normalize_provider(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

fn hostname(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let host = rest.split(['/', ':', '?']).next()?;
    let host = host.trim_end_matches('.');
    (!host.is_empty()).then(|| host.to_lowercase())
}

/// provider 名 / baseUrl host → OpenRouter author（命中不了就空，走全量反查）。
fn author_hints(provider: &str, base_url: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |a: &str| {
        if !a.is_empty() && !out.iter().any(|x| x == a) {
            out.push(a.to_string());
        }
    };
    let by_name = |p: &str| match p {
        "zhipu" | "zhipuai" | "bigmodel" | "zai" => Some("z-ai"),
        "deepseek" => Some("deepseek"),
        "moonshot" | "kimi" => Some("moonshotai"),
        "qwen" | "alibaba" | "dashscope" => Some("qwen"),
        "openai" => Some("openai"),
        "anthropic" => Some("anthropic"),
        "google" | "gemini" => Some("google"),
        "xai" | "grok" => Some("x-ai"),
        "mistral" => Some("mistralai"),
        "minimax" => Some("minimax"),
        "stepfun" => Some("stepfun-ai"),
        "bytedance" | "doubao" => Some("bytedance"),
        "baidu" => Some("baidu"),
        "tencent" => Some("tencent"),
        "xiaomi" => Some("xiaomi"),
        "nvidia" => Some("nvidia"),
        "perplexity" => Some("perplexity"),
        _ => None,
    };
    if let Some(a) = by_name(&normalize_provider(provider)) {
        push(a);
    }
    if let Some(h) = hostname(base_url) {
        let by_host: Option<&str> = if h.ends_with("bigmodel.cn") {
            Some("z-ai")
        } else if h.ends_with("deepseek.com") {
            Some("deepseek")
        } else if h.ends_with("moonshot.cn") || h.ends_with("moonshot.ai") {
            Some("moonshotai")
        } else if h.ends_with("aliyuncs.com") {
            Some("qwen")
        } else if h.ends_with("openai.com") {
            Some("openai")
        } else if h.ends_with("anthropic.com") {
            Some("anthropic")
        } else if h.ends_with("googleapis.com") {
            Some("google")
        } else if h.ends_with("x.ai") {
            Some("x-ai")
        } else if h.ends_with("mistral.ai") {
            Some("mistralai")
        } else {
            None
        };
        if let Some(a) = by_host {
            push(a);
        }
    }
    out
}

// ---- 匹配 --------------------------------------------------------------------

fn author_of(id: &str) -> &str {
    id.split('/').next().unwrap_or(id)
}

fn slug_of(id: &str) -> &str {
    id.rsplit('/').next().unwrap_or(id)
}

/// 去变体后缀（"deepseek-chat:free" → "deepseek-chat"）。
fn base_slug(slug: &str) -> &str {
    slug.split(':').next().unwrap_or(slug)
}

/// 别名通配条目（"~deepseek/deepseek-flash-latest"）：OpenRouter 的「最新
/// 版」目录项，id 带前导 `~`、slug 以 "-latest" 结尾，参数齐全（ctx /
/// maxTokens / 能力）但名称泛化——厂商自家命名（如 DeepSeek 的
/// deepseek-flash）在目录里常常只有这个形态，具体型号是另一个 id。
fn is_alias(id: &str) -> bool {
    id.starts_with('~') && slug_of(id).ends_with("-latest")
}

/// 别名 slug 剥 "-latest"（"deepseek-flash-latest" → "deepseek-flash"）。
fn alias_base_slug(slug: &str) -> Option<&str> {
    let base = slug.strip_suffix("-latest")?;
    (!base.is_empty()).then_some(base)
}

fn normalize_query(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .trim_start_matches("models/")
        .to_string()
}

/// 裸 id / 全 id → 目录条目。author 命中提示的候选优先；同级里无变体
/// （不带 ":free" 等）优先；**具体型号优先于别名通配**——别名只在没有任何
/// 具体命中时兜底（参数齐全，填得全）。
fn pick<'a>(entries: &'a [Entry], q: &str, hints: &[String]) -> Option<&'a Entry> {
    if q.is_empty() {
        return None;
    }
    if let Some(e) = entries.iter().find(|e| e.id == q) {
        return Some(e);
    }
    let q_slug = q.rsplit('/').next().unwrap_or(q);
    let q_base = base_slug(q_slug);
    let cand: Vec<&Entry> = entries
        .iter()
        .filter(|e| {
            let s = slug_of(&e.id);
            s == q_slug || base_slug(s) == q_base || alias_base_slug(s) == Some(q_base)
        })
        .collect();
    if cand.is_empty() {
        return None;
    }
    // 具体型号走原四级优先序；全部候选都是别名时同一套序兜底
    let concrete: Vec<&Entry> = cand.iter().copied().filter(|e| !is_alias(&e.id)).collect();
    let tiered = |pool: &[&'a Entry]| -> Option<&'a Entry> {
        if let Some(e) = pool
            .iter()
            .find(|e| !slug_of(&e.id).contains(':') && hints.iter().any(|h| h == &author_of(&e.id)))
        {
            return Some(e);
        }
        if let Some(e) = pool.iter().find(|e| !slug_of(&e.id).contains(':')) {
            return Some(e);
        }
        if let Some(e) = pool
            .iter()
            .find(|e| hints.iter().any(|h| h == &author_of(&e.id)))
        {
            return Some(e);
        }
        pool.first().copied()
    };
    if !concrete.is_empty() {
        tiered(&concrete)
    } else {
        tiered(&cand)
    }
}

// ---- 网络 --------------------------------------------------------------------

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
}

/// 单模型直查（2KB 级）。HTTP 404 = 该 author 下无此 slug（不算网络错）。
fn fetch_model(author: &str, slug: &str) -> Result<Entry, FillError> {
    let resp = agent()
        .get(&format!("{OR_BASE}/model/{author}/{slug}"))
        .set("Accept", "application/json")
        .call()
        .map_err(|e| FillError::Net(format!("openrouter fetch failed: {e}")))?;
    let text = resp
        .into_string()
        .map_err(|e| FillError::Net(format!("openrouter read failed: {e}")))?;
    let v: Value = serde_json::from_str(&text)
        .map_err(|e| FillError::Net(format!("openrouter parse failed: {e}")))?;
    parse_entry(v.get("data").unwrap_or(&Value::Null)).ok_or(FillError::NoMatch)
}

fn fetch_full() -> Result<Vec<Entry>, FillError> {
    let resp = agent()
        .get(&format!("{OR_BASE}/models"))
        .set("Accept", "application/json")
        .call()
        .map_err(|e| FillError::Net(format!("openrouter fetch failed: {e}")))?;
    let text = resp
        .into_string()
        .map_err(|e| FillError::Net(format!("openrouter read failed: {e}")))?;
    let v: Value = serde_json::from_str(&text)
        .map_err(|e| FillError::Net(format!("openrouter parse failed: {e}")))?;
    let entries = parse_list(&v);
    (!entries.is_empty())
        .then_some(entries)
        .ok_or(FillError::Net(
            "openrouter returned an empty catalog".into(),
        ))
}

// ---- 磁盘缓存（全量列表，1h TTL）----------------------------------------------

pub fn cache_path() -> Option<PathBuf> {
    crate::paths::openrouter_cache_file()
}

#[derive(Serialize, Deserialize)]
struct CacheDoc {
    fetched_at: u64,
    entries: Vec<Entry>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_cache(path: &std::path::Path) -> Option<(Vec<Entry>, u64)> {
    let text = std::fs::read_to_string(path).ok()?;
    let doc: CacheDoc = serde_json::from_str(&text).ok()?;
    (!doc.entries.is_empty()).then_some((doc.entries, doc.fetched_at))
}

fn write_cache(path: &std::path::Path, entries: &[Entry]) {
    let doc = CacheDoc {
        fetched_at: now_secs(),
        entries: entries.to_vec(),
    };
    if let Ok(text) = serde_json::to_string(&doc) {
        let _ = std::fs::write(path, text);
    }
}

/// 全量列表：缓存命中（<1h）直接回；否则拉取落缓存；网络失败回陈旧缓存。
fn load_full() -> Result<Vec<Entry>, FillError> {
    let path = cache_path().ok_or(FillError::Net("pf data dir unavailable".into()))?;
    if let Some((entries, at)) = read_cache(&path) {
        if now_secs().saturating_sub(at) < CACHE_TTL_SECS {
            return Ok(entries);
        }
    }
    match fetch_full() {
        Ok(entries) => {
            write_cache(&path, &entries);
            Ok(entries)
        }
        Err(e) => read_cache(&path).map(|(entries, _)| entries).ok_or(e),
    }
}

/// 别名命中 → 顺着 alias_target 取**具体型号**条目（名称/参数都以具体为
/// 准，泛化的「…Latest」标签不进账本）；目标取不到再回别名本身。
fn concrete_of_alias(e: &Entry) -> Option<Entry> {
    let target = e.alias_target.as_deref()?;
    let (a, s) = target.split_once('/')?;
    fetch_model(a, s).ok()
}

/// 解析一个模型（阻塞，供后台线程）：query 自带 author → 单模型直查；
/// author 可从 provider 名/baseUrl 推断 → 仍单模型直查；否则全量列表反查
/// （783KB，走缓存）。命中别名通配条目时一律顺着 alias_target 落到具体型号。
pub fn resolve(provider: &str, base_url: &str, query: &str) -> Result<Entry, FillError> {
    let q = normalize_query(query);
    if q.is_empty() {
        return Err(FillError::NoMatch);
    }
    let hints = author_hints(provider, base_url);
    // 1) query 自带 author（"deepseek/deepseek-v4-pro"）
    if let Some((a, s)) = q.split_once('/') {
        if !a.is_empty() && !s.is_empty() {
            if let Ok(e) = fetch_model(a, s) {
                return Ok(concrete_of_alias(&e).unwrap_or(e));
            }
        }
    }
    // 2) author 推断命中 → 裸 id 当 slug 试；再试 "-latest" 别名形态（厂商
    //    自家命名常只有别名条目，其 author 是 "~author"——实测不带 ~ 404）。
    //    404 不算错，落全量反查兜底
    for a in &hints {
        if let Ok(e) = fetch_model(a, &q) {
            return Ok(concrete_of_alias(&e).unwrap_or(e));
        }
        if let Ok(e) = fetch_model(&format!("~{a}"), &format!("{q}-latest")) {
            return Ok(concrete_of_alias(&e).unwrap_or(e));
        }
    }
    // 3) 全量反查：别名命中 → 目录内直接找 alias_target 指向的具体条目
    //    （零额外请求），找不到再单查，最后回别名本身
    let entries = load_full()?;
    let e = pick(&entries, &q, &hints)
        .cloned()
        .ok_or(FillError::NoMatch)?;
    if is_alias(&e.id) {
        if let Some(target) = e.alias_target.as_deref() {
            if let Some(concrete) = entries.iter().find(|x| x.id == target) {
                return Ok(concrete.clone());
            }
        }
        return Ok(concrete_of_alias(&e).unwrap_or(e));
    }
    Ok(e)
}

// ---- 回填 --------------------------------------------------------------------

/// 把条目**覆盖**写进 models.json 模型对象。页面可见字段（上下文/最大输出/
/// 能力 input/推理）一律以目录为准；页面没有的（名称/价格）只填空、不覆盖
/// 已有值。返回实际改变的字段数。
///
/// 注意 reasoning 必须显式写 bool：ensure_model_defaults 对缺键补 true，
/// 只删键的话「关推理」会被悄悄改回「开」。
pub fn fill_from_entry(model: &mut Value, e: &Entry) -> usize {
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
    if let Some(n) = e.context_window {
        put("contextWindow", serde_json::json!(n), map);
    }
    if let Some(n) = e.max_tokens {
        put("maxTokens", serde_json::json!(n), map);
    }
    if !e.input.is_empty() {
        put(
            "input",
            Value::Array(e.input.iter().map(|s| Value::String(s.clone())).collect()),
            map,
        );
    }
    put("reasoning", Value::Bool(e.reasoning), map);
    // 名称：只认**具体型号**条目（e 为别名时不写，泛化标签不进账本）。
    // 现名若为旧版本写坏的「…Latest」泛化标签，换成具体型号名（自愈）
    let name_now = map
        .get("name")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let name_generic = name_now.to_lowercase().ends_with("latest");
    if !e.name.is_empty() && !e.alias && (name_now.is_empty() || name_generic) {
        put("name", Value::String(e.name.clone()), map);
    }
    if let Some(cost) = map
        .entry("cost")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
    {
        for (k, v) in [
            ("input", e.cost.input),
            ("output", e.cost.output),
            ("cacheRead", e.cost.cache_read),
            ("cacheWrite", e.cost.cache_write),
        ] {
            if let Some(val) = v {
                if cost.get(k).and_then(|x| x.as_f64()).is_none() {
                    cost.insert(k.to_string(), serde_json::json!(val));
                    applied += 1;
                }
            }
        }
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn glm_entry() -> Value {
        // 2026-10-27 实测响应节选
        json!({
            "id": "z-ai/glm-5.3",
            "canonical_slug": "z-ai/glm-5.3-20260816",
            "name": "Z.ai: GLM 5.3",
            "context_length": 1048576,
            "top_provider": {"context_length": 1048576, "max_completion_tokens": 943718},
            "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]},
            "supported_parameters": ["include_reasoning", "reasoning", "reasoning_effort"],
            "pricing": {"prompt": "0.0000000699", "completion": "0.0000014"}
        })
    }

    #[test]
    fn parse_maps_openrouter_shape() {
        let e = parse_entry(&glm_entry()).unwrap();
        assert_eq!(e.id, "z-ai/glm-5.3");
        assert_eq!(e.name, "GLM 5.3");
        assert!(e.reasoning);
        assert_eq!(e.input, vec!["text".to_string()]);
        assert_eq!(e.context_window, Some(1048576));
        assert_eq!(e.max_tokens, Some(943718));
        let c = e.cost;
        assert!((c.input.unwrap() - 0.0699).abs() < 1e-9);
        assert!((c.output.unwrap() - 1.4).abs() < 1e-9);
        assert_eq!(c.cache_read, None);
    }

    #[test]
    fn pick_prefers_hint_author_then_non_variant() {
        let mk = |id: &str| Entry {
            id: id.into(),
            name: id.into(),
            ..Default::default()
        };
        let entries = vec![
            mk("deepseek/deepseek-chat:free"),
            mk("deepseek/deepseek-chat"),
            mk("other/deepseek-chat"),
        ];
        let hints = vec!["deepseek".to_string()];
        // 提示 author 命中且无变体
        assert_eq!(
            pick(&entries, "deepseek-chat", &hints).unwrap().id,
            "deepseek/deepseek-chat"
        );
        // 无提示 → 非变体优先
        assert_eq!(
            pick(&entries, "deepseek-chat", &[]).unwrap().id,
            "deepseek/deepseek-chat"
        );
        // 全 id 精确
        assert_eq!(
            pick(&entries, "other/deepseek-chat", &[]).unwrap().id,
            "other/deepseek-chat"
        );
        // 变体查询精确命中变体
        assert_eq!(
            pick(&entries, "deepseek/deepseek-chat:free", &[])
                .unwrap()
                .id,
            "deepseek/deepseek-chat:free"
        );
        assert!(pick(&entries, "gpt-9", &[]).is_none());
    }

    #[test]
    fn pick_resolves_latest_alias_with_concrete_preference() {
        let mk = |id: &str| Entry {
            id: id.into(),
            name: id.into(),
            ..Default::default()
        };
        let entries = vec![
            mk("deepseek/deepseek-v4-flash"),
            mk("~deepseek/deepseek-flash-latest"),
            mk("deepseek/deepseek-v4.1-flash"),
        ];
        // 厂商自家命名（deepseek-flash）目录里只有别名通配 → 兜底命中别名，
        // 别名条目参数齐全（ctx/maxTokens 由单测外的 parse 实测覆盖）
        assert_eq!(
            pick(&entries, "deepseek-flash", &[]).unwrap().id,
            "~deepseek/deepseek-flash-latest"
        );
        // 具体型号存在时优先具体，别名不抢
        assert_eq!(
            pick(&entries, "deepseek-v4-flash", &[]).unwrap().id,
            "deepseek/deepseek-v4-flash"
        );
        assert_eq!(
            pick(&entries, "deepseek-v4.1-flash", &[]).unwrap().id,
            "deepseek/deepseek-v4.1-flash"
        );
        // 别名 id 显式写出 → 精确命中
        assert_eq!(
            pick(&entries, "~deepseek/deepseek-flash-latest", &[])
                .unwrap()
                .id,
            "~deepseek/deepseek-flash-latest"
        );
    }

    #[test]
    fn fill_overwrites_visible_fills_blank_invisible() {
        let e = parse_entry(&glm_entry()).unwrap();
        let mut model = json!({
            "id": "glm-5.3",
            "name": "手动起的名",
            "contextWindow": 4096,
            "maxTokens": 4096,
            "input": ["text", "image"],
            "reasoning": false
        });
        let n = fill_from_entry(&mut model, &e);
        // 页面可见全被覆盖：ctx/maxTokens/input/reasoning（false→true）= 4；
        // 名称已有不覆盖；cost 原缺 → 补 input/output 两键 = 2
        assert_eq!(n, 6);
        assert_eq!(model["contextWindow"], 1048576);
        assert_eq!(model["maxTokens"], 943718);
        assert_eq!(model["input"][0], "text");
        assert_eq!(model["reasoning"], true);
        assert_eq!(model["name"], "手动起的名");
        assert!(model["cost"]["input"].as_f64().unwrap() > 0.0);
        // 幂等
        assert_eq!(fill_from_entry(&mut model, &e), 0);
        // 关推理：显式 false 落盘，不会被幂等改回 true
        let off = Entry {
            reasoning: false,
            ..e.clone()
        };
        assert_eq!(fill_from_entry(&mut model, &off), 1);
        assert_eq!(model["reasoning"], false);
    }

    #[test]
    fn author_hints_from_provider_and_host() {
        assert_eq!(
            author_hints("my-ds", "https://api.deepseek.com/v1"),
            vec!["deepseek"]
        );
        assert_eq!(
            author_hints("zhipu", "https://open.bigmodel.cn/api/paas/v4"),
            vec!["z-ai"]
        );
        assert!(author_hints("whatever", "https://example.com/v1").is_empty());
    }

    #[test]
    fn fill_skips_generic_name_for_alias_entries() {
        let mut e = parse_entry(&glm_entry()).unwrap();
        e.alias = true;
        let mut model = json!({ "id": "deepseek-flash" });
        fill_from_entry(&mut model, &e);
        // 别名名称泛化（"… Flash Latest"）不落账本；ctx/maxTokens 照填
        assert!(model.get("name").is_none());
        assert_eq!(model["contextWindow"], 1048576);
        assert_eq!(model["maxTokens"], 943718);
    }

    #[test]
    fn fill_heals_generic_name_with_concrete() {
        // 旧版本守卫缺失期把泛化标签写进了账本：具体型号条目回填时换回来
        let e = Entry {
            id: "deepseek/deepseek-v4.1-flash-1017".into(),
            name: "DeepSeek V4.1 Flash 1017".into(),
            reasoning: true,
            input: vec!["text".into()],
            context_window: Some(1048576),
            max_tokens: Some(393216),
            cost: Cost::default(),
            alias: false,
            alias_target: None,
        };
        let mut model = json!({ "id": "deepseek-flash", "name": "DeepSeek Flash Latest" });
        fill_from_entry(&mut model, &e);
        assert_eq!(model["name"], "DeepSeek V4.1 Flash 1017");
        // 具体名不被冲掉
        let mut model2 = json!({ "id": "deepseek-flash", "name": "DeepSeek-V4.1-Flash" });
        fill_from_entry(&mut model2, &e);
        assert_eq!(model2["name"], "DeepSeek-V4.1-Flash");
    }

    #[test]
    fn parse_captures_alias_target() {
        let v = json!({
            "id": "~deepseek/deepseek-flash-latest",
            "name": "DeepSeek: DeepSeek Flash Latest",
            "context_length": 1048576,
            "architecture": {"input_modalities": ["text", "image"]},
            "supported_parameters": ["reasoning"],
            "alias_target": {"name": "DeepSeek: DeepSeek V4.1 Flash 1017",
                             "slug": "deepseek/deepseek-v4.1-flash-1017"}
        });
        let e = parse_entry(&v).unwrap();
        assert!(e.alias);
        assert_eq!(
            e.alias_target.as_deref(),
            Some("deepseek/deepseek-v4.1-flash-1017")
        );
        // 具体型号无 alias_target
        let c = parse_entry(&glm_entry()).unwrap();
        assert!(!c.alias);
        assert_eq!(c.alias_target, None);
    }
}
