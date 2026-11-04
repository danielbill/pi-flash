//! models.dev 仓库单文件 TOML 价格源（042-2「模型价格管理」）。
//!
//! 数据源 = `sst/models.dev` 仓库按模型一个 TOML（models.dev 站点的源数据），
//! 4KB/个、免鉴权；官方价以**波谷 base** 落盘（文件头注释明示 base = off-peak，
//! 高峰用 `cost.multipliers` 分时表达——pi 账本无时段分档，读即丢）。
//! 双通道：raw.githubusercontent.com 主 + cdn.jsdelivr.net 兜底（CN 可达）。
//!
//! 解析链（[`resolve`]）：TOML → OpenRouter `resolve_many`（现有链下沉）
//! → Err（调用方维持 0）。缓存 `~/.pi-flash/model-price-cache.json`
//! （TTL 7 天，网络失败回陈旧）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::model_ref::{query_candidates, slugify};
use crate::openrouter::{self, Entry, FillError};

const RAW_BASE: &str = "https://raw.githubusercontent.com/sst/models.dev/dev/providers";
const JSDELIVR_BASE: &str = "https://cdn.jsdelivr.net/gh/sst/models.dev@dev/providers";
const FETCH_TIMEOUT_SECS: u64 = 8;
const CACHE_TTL_SECS: u64 = 60 * 60 * 24 * 7;

/// 清单仓模型定义里的 PF 私有来源键（models.json 投影由 normalize 剥除）。
pub const SOURCE_KEY: &str = "priceSource";
pub const SOURCE_MODELS_DEV: &str = "models-dev";
pub const SOURCE_OPENROUTER: &str = "openrouter";
pub const SOURCE_MANUAL: &str = "manual";

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
}

// ---- provider 目录猜测（{dir} 梯子）------------------------------------------

/// 从 baseUrl 提取 host 核心段：去 scheme、丢弃 `api.`/`open.` 等服务前缀，
/// 取剩余首个标签。`https://api.deepseek.com/v1` → `deepseek`。
fn host_core(base_url: &str) -> Option<String> {
    let no_scheme = base_url.split_once("://").map(|x| x.1).unwrap_or(base_url);
    let host = no_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let mut labels = host.split('.').filter(|l| !l.is_empty());
    if let Some(first) = labels.next() {
        if ["api", "open", "openapi", "gateway", "dashscope-api"]
            .iter()
            .any(|s| first.eq_ignore_ascii_case(s))
        {
            // 丢弃服务前缀，取下一个标签
        } else {
            return Some(first.to_ascii_lowercase());
        }
    }
    labels.next().map(|c| c.to_ascii_lowercase())
}

/// host 核心段 → models.dev 仓库目录名候选（实测目录清单 2026-11-04；
/// 猜错只是 404，不当权威绑定）。
fn alias_dirs(core: &str) -> Vec<&'static str> {
    match core {
        "deepseek" => vec!["deepseek"],
        "bigmodel" | "zhipu" | "zhipuai" => vec!["zhipuai", "zai"],
        "moonshot" | "kimi" => vec!["moonshotai", "moonshotai-cn"],
        "siliconflow" => vec!["siliconflow", "siliconflow-cn"],
        "aliyuncs" | "dashscope" => vec!["alibaba", "alibaba-cn"],
        "openai" => vec!["openai"],
        "anthropic" => vec!["anthropic"],
        "google" | "gemini" => vec!["google"],
        "xai" => vec!["xai"],
        "mistral" => vec!["mistral"],
        "minimax" => vec!["minimax", "minimax-cn"],
        "stepfun" => vec!["stepfun-ai"],
        _ => Vec::new(),
    }
}

/// `{dir}` 候选列表：host 核心段本身 + 别名目录 + 商名 slug，去重保序。
pub fn provider_dir_candidates(base_url: &str, provider: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |d: &str| {
        if !d.is_empty() && !out.iter().any(|x| x == d) {
            out.push(d.to_string());
        }
    };
    if let Some(core) = host_core(base_url) {
        push(&core);
        for a in alias_dirs(&core) {
            push(a);
        }
    }
    let pslug = slugify(provider);
    if !pslug.is_empty() {
        push(&pslug);
    }
    out
}

// ---- TOML 拉取与解析 ----------------------------------------------------------

fn fetch_toml_text(dir: &str, slug: &str) -> Result<String, FillError> {
    let path = format!("{dir}/models/{slug}.toml");
    let mut last = FillError::NoMatch;
    for base in [RAW_BASE, JSDELIVR_BASE] {
        let url = format!("{base}/{path}");
        match agent().get(&url).set("Accept", "text/plain").call() {
            Ok(resp) => {
                return resp
                    .into_string()
                    .map_err(|e| FillError::Net(format!("models.dev read failed: {e}")));
            }
            // HTTP 404 = 该目录无此文件（候选失配，不算网络错）→ 换下一候选
            Err(ureq::Error::Status(404, _)) => {
                return Err(FillError::NoMatch);
            }
            Err(e) => last = FillError::Net(format!("models.dev fetch failed: {e}")),
        }
    }
    Err(last)
}

#[derive(Debug, Deserialize)]
struct TomlDoc {
    schema: Option<i64>,
    limit: Option<TomlLimit>,
    cost: Option<TomlCost>,
    capabilities: Option<TomlCapabilities>,
}
#[derive(Debug, Deserialize)]
struct TomlLimit {
    context: Option<u64>,
    output: Option<u64>,
}
#[derive(Debug, Deserialize)]
struct TomlCost {
    text: Option<TomlCostText>,
}
#[derive(Debug, Deserialize)]
struct TomlCostText {
    input: Option<TomlCostInput>,
    output: Option<TomlCostOutput>,
}
#[derive(Debug, Deserialize)]
struct TomlCostInput {
    price: Option<f64>,
    cache: Option<TomlCache>,
}
#[derive(Debug, Deserialize)]
struct TomlCache {
    read: Option<f64>,
    write: Option<f64>,
}
#[derive(Debug, Deserialize)]
struct TomlCostOutput {
    price: Option<f64>,
}
#[derive(Debug, Deserialize)]
struct TomlCapabilities {
    reasoning: Option<TomlReasoning>,
}
#[derive(Debug, Deserialize)]
struct TomlReasoning {
    supported: Option<bool>,
}

/// schema = 2 才认：cost 四键（$/MTok，USD）；limit 顺带入 Entry（仍只进
/// 表单）；`multipliers`（高峰分时）读取即丢。失配/缺价 → None（走降级）。
fn parse_toml(text: &str) -> Option<Entry> {
    let doc: TomlDoc = toml::from_str(text).ok()?;
    if doc.schema != Some(2) {
        return None;
    }
    let text_cost = doc.cost?.text?;
    let input = text_cost.input?;
    let output = text_cost.output?;
    let cost = openrouter::Cost {
        input: input.price,
        output: output.price,
        cache_read: input.cache.as_ref().and_then(|c| c.read),
        cache_write: input.cache.as_ref().and_then(|c| c.write),
    };
    if cost.input.is_none() && cost.output.is_none() {
        return None;
    }
    Some(Entry {
        id: String::new(),
        name: String::new(),
        reasoning: doc
            .capabilities
            .as_ref()
            .and_then(|c| c.reasoning.as_ref())
            .and_then(|r| r.supported)
            .unwrap_or(false),
        input: Vec::new(),
        context_window: doc.limit.as_ref().and_then(|l| l.context),
        max_tokens: doc.limit.as_ref().and_then(|l| l.output),
        cost,
        alias: false,
        alias_target: None,
        source: SOURCE_MODELS_DEV.into(),
    })
}

// ---- 缓存 ---------------------------------------------------------------------

pub fn cache_path() -> Option<PathBuf> {
    crate::paths::model_price_cache_file()
}

#[derive(Serialize, Deserialize, Default)]
struct CacheDoc {
    entries: HashMap<String, CachedEntry>,
}
#[derive(Serialize, Deserialize, Clone)]
struct CachedEntry {
    fetched_at: u64,
    entry: Entry,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_cache(path: &std::path::Path) -> CacheDoc {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_cache_entry(path: &std::path::Path, key: &str, entry: &Entry) {
    let mut doc = read_cache(path);
    doc.entries.insert(
        key.to_string(),
        CachedEntry {
            fetched_at: now_secs(),
            entry: entry.clone(),
        },
    );
    if let Ok(text) = serde_json::to_string(&doc) {
        let _ = std::fs::write(path, text);
    }
}

// ---- 解析链 -------------------------------------------------------------------

/// 042-2 解析链（阻塞，供后台线程）：① TOML 双通道 → ② OpenRouter →
/// Err（调用方维持 0）。命中带 `Entry.source`（models-dev / openrouter）。
pub fn resolve(
    provider: &str,
    base_url: &str,
    id: &str,
    name: &str,
) -> Result<Entry, FillError> {
    let queries = query_candidates(id, name);
    let dirs = provider_dir_candidates(base_url, provider);
    let cache = cache_path();
    let mut net_failed = false;
    // 缓存（TTL 内）先于网络：任意 (dir, slug) 命中即回
    if let Some(path) = &cache {
        let doc = read_cache(path);
        for dir in &dirs {
            for q in &queries {
                let key = format!("{dir}/{q}");
                if let Some(c) = doc.entries.get(&key) {
                    if now_secs().saturating_sub(c.fetched_at) < CACHE_TTL_SECS {
                        return Ok(c.entry.clone());
                    }
                }
            }
        }
    }
    'ladder: for dir in &dirs {
        for q in &queries {
            match fetch_toml_text(dir, q) {
                Ok(text) => {
                    if let Some(mut e) = parse_toml(&text) {
                        e.id = id.to_string();
                        if let Some(path) = &cache {
                            write_cache_entry(path, &format!("{dir}/{q}"), &e);
                        }
                        return Ok(e);
                    }
                }
                Err(FillError::NoMatch) => {}
                Err(_) => {
                    net_failed = true;
                    // 双通道都网络挂 → 该目录剩余候选不必再试
                    continue 'ladder;
                }
            }
        }
    }
    // 陈旧缓存兜底（离线也能填参）
    if net_failed {
        if let Some(path) = &cache {
            let doc = read_cache(path);
            if let Some(c) = doc.entries.values().next() {
                return Ok(c.entry.clone());
            }
        }
    }
    // ② OpenRouter（现有链下沉）
    openrouter::resolve_many(provider, base_url, &queries).map(|mut e| {
        e.source = SOURCE_OPENROUTER.into();
        e
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEEPSEEK_FLASH_TOML: &str = include_str!("fixtures/deepseek-flash.toml");

    #[test]
    fn parse_schema2_offpeak_base() {
        let e = parse_toml(DEEPSEEK_FLASH_TOML).expect("schema 2 fixture parses");
        assert_eq!(e.cost.input, Some(0.15));
        assert_eq!(e.cost.output, Some(0.6));
        assert_eq!(e.cost.cache_read, Some(0.003));
        assert_eq!(e.cost.cache_write, None);
        assert_eq!(e.context_window, Some(1_048_576));
        assert_eq!(e.max_tokens, Some(393_216));
        assert!(e.reasoning);
        assert_eq!(e.source, SOURCE_MODELS_DEV);
    }

    #[test]
    fn parse_bails_on_unknown_schema_or_garbage() {
        assert!(parse_toml("schema = 1\n").is_none());
        assert!(parse_toml("not toml {{{").is_none());
        assert!(parse_toml("schema = 2\n[cost.text.input]\nprice = 1\n").is_none()); // 缺 output
    }

    #[test]
    fn host_core_strips_service_prefixes() {
        assert_eq!(
            host_core("https://api.deepseek.com/v1").as_deref(),
            Some("deepseek")
        );
        assert_eq!(
            host_core("https://open.bigmodel.cn/api/paas/v4").as_deref(),
            Some("bigmodel")
        );
        assert_eq!(
            host_core("http://127.0.0.1:8080/v1").as_deref(),
            Some("127")
        );
        assert_eq!(host_core(""), None);
    }

    #[test]
    fn dir_ladder_core_then_alias_then_slug() {
        let dirs = provider_dir_candidates("https://api.deepseek.com/v1", "my-ds");
        assert_eq!(dirs, vec!["deepseek", "my-ds"]);
        let dirs = provider_dir_candidates("https://open.bigmodel.cn/api/paas/v4", "zhipu");
        assert_eq!(dirs, vec!["bigmodel", "zhipuai", "zai", "zhipu"]);
        let dirs = provider_dir_candidates("https://dashscope.aliyuncs.com/compatible-mode/v1", "x");
        assert_eq!(dirs, vec!["dashscope", "alibaba", "alibaba-cn", "x"]);
    }

    #[test]
    fn cache_roundtrip_and_expiry_keying() {
        let dir = std::env::temp_dir().join(format!("pf-mprice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cache.json");
        let e = parse_toml(DEEPSEEK_FLASH_TOML).unwrap();
        write_cache_entry(&path, "deepseek/deepseek-flash", &e);
        let doc = read_cache(&path);
        assert!(doc.entries.contains_key("deepseek/deepseek-flash"));
        assert_eq!(doc.entries["deepseek/deepseek-flash"].entry.cost.input, Some(0.15));
        std::fs::remove_dir_all(&dir).ok();
    }
}
