//! 保存 apikey 后的通路探查（042 决策 3，PF 超出 pi-web 的优化）。
//!
//! pi-web 并无探查请求——模型列表来自 SDK 静态注册表，与 key 无关，保存
//! 只是刷新状态；但 051 的痛点是 key 错了要重启会话才发现。探查把失败提前
//! 到保存时刻：向该 provider 的列模型端点发一次轻量 GET，key 被拒/网络
//! 不可达当场可见。
//!
//! - 内置商端点表随 vendor pin 固化（与 [`crate::credentials::official_env_name`]
//!   同款看护），URL/鉴权样式对齐 pi-web `lib/model-discovery.ts` 的列模型
//!   请求形状。表外 provider 跳过探查（调用方直接呈现已配置态）。
//! - 自定义商按 models.json 的 `baseUrl` + `api` 走同一构造规则。
//! - 结果只驱动 UI 状态点，不回滚保存——可能是网络问题，key 留着。

use crate::credentials::{classify, pf_ref, StoreMode, SecretVault};
use std::time::Duration;

/// 列模型请求的鉴权样式（决定头形状）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    /// `Authorization: Bearer <key>`（openai 系）
    Bearer,
    /// `x-api-key` + `anthropic-version`（anthropic 系）
    Anthropic,
    /// `x-goog-api-key`（google-generative-ai 系）
    Google,
}

/// 探查结果三态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// 2xx：通路与 key 均有效。
    Ok { models: usize, latency_ms: u64 },
    /// 401/403：key 被拒（网络通）。
    AuthRejected { status: u16 },
    /// 超时/DNS/其他 HTTP 状态：不可达或端点不符。
    Unreachable { error: String },
}

/// 内置商探查端点表（vendor pin 1.1.0）。只收高置信条目——表外商跳过
/// 探查（UI 直接呈现已配置态），宁缺勿错报。
pub fn catalog_target(provider: &str) -> Option<(&'static str, Auth)> {
    const TABLE: &[(&str, &str, Auth)] = &[
        ("openai", "https://api.openai.com/v1/models", Auth::Bearer),
        ("deepseek", "https://api.deepseek.com/v1/models", Auth::Bearer),
        ("anthropic", "https://api.anthropic.com/v1/models", Auth::Anthropic),
        (
            "google",
            "https://generativelanguage.googleapis.com/v1beta/models",
            Auth::Google,
        ),
        ("openrouter", "https://openrouter.ai/api/v1/models", Auth::Bearer),
        ("groq", "https://api.groq.com/openai/v1/models", Auth::Bearer),
        ("cerebras", "https://api.cerebras.ai/v1/models", Auth::Bearer),
        ("mistral", "https://api.mistral.ai/v1/models", Auth::Bearer),
        ("xai", "https://api.x.ai/v1/models", Auth::Bearer),
        ("moonshotai", "https://api.moonshot.ai/v1/models", Auth::Bearer),
        ("moonshotai-cn", "https://api.moonshot.cn/v1/models", Auth::Bearer),
        ("together", "https://api.together.xyz/v1/models", Auth::Bearer),
        (
            "fireworks",
            "https://api.fireworks.ai/inference/v1/models",
            Auth::Bearer,
        ),
    ];
    TABLE
        .iter()
        .find(|(p, _, _)| *p == provider)
        .map(|(_, url, auth)| (*url, *auth))
}

/// 自定义商的列模型 URL：models.json 的 `baseUrl` + `api` 走同一构造规则
/// （对齐 pi-web buildModelsListUrl：按 api 补版本段，不重复补）。
pub fn custom_target(base_url: &str, api: &str) -> Option<(String, Auth)> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return None;
    }
    let (url, auth) = match api {
        "anthropic-messages" => (join_version(base, "v1"), Auth::Anthropic),
        "google-generative-ai" => (join_version(base, "v1beta"), Auth::Google),
        // openai-completions / openai-responses 共用 /models 列表端点
        _ => (format!("{base}/models"), Auth::Bearer),
    };
    Some((url, auth))
}

/// base 已带该版本段就只接 /models，否则补上版本段再接。
fn join_version(base: &str, version: &str) -> String {
    if base.ends_with(&format!("/{version}")) {
        format!("{base}/models")
    } else {
        format!("{base}/{version}/models")
    }
}

/// 目录 provider 的探查 key：pf-auth.json 条目（凭据库引用 → 解出；降级
/// 明文 → 原值）。无条目/解不出 → None（跳过探查）。
pub fn catalog_probe_key(provider: &str, vault: &dyn SecretVault) -> Option<String> {
    let entries = crate::pf_auth::entries_at(&crate::pf_auth::path()?).ok()?;
    for (p, key, _inject_as, mode) in entries {
        if p != provider {
            continue;
        }
        return match classify(&key) {
            StoreMode::Vault => pf_ref(&key).and_then(|v| vault.get(&v).ok().flatten()),
            StoreMode::File if mode == StoreMode::File.as_str() => Some(key),
            _ => None,
        };
    }
    None
}

/// 自定义 provider 的探查 key：models.json 的 `apiKey` 字段（`$PF_KEY_*`
/// 引用 → 凭据库解出；明文 → 原值；`!cmd` / 其他 `$VAR` 高级引用不探查）。
pub fn custom_probe_key(api_key: &str, vault: &dyn SecretVault) -> Option<String> {
    match classify(api_key) {
        StoreMode::Vault => pf_ref(api_key).and_then(|v| vault.get(&v).ok().flatten()),
        StoreMode::File => Some(api_key.to_string()),
        StoreMode::Ref => None,
    }
}

/// 发一次列模型 GET（阻塞，20s 超时）并按三态分类。2xx 响应体里的
/// `data[]`（openai/anthropic 形状）或 `models[]`（google 形状）计模型数，
/// 解不出按 0 计——探查只管通路，不接管目录。
pub fn probe(url: &str, auth: Auth, key: &str) -> ProbeOutcome {
    let started = std::time::Instant::now();
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build();
    let req = agent.get(url);
    let req = match auth {
        Auth::Bearer => req.set("Authorization", &format!("Bearer {key}")),
        Auth::Anthropic => req
            .set("x-api-key", key)
            .set("anthropic-version", "2023-06-01"),
        Auth::Google => req.set("x-goog-api-key", key),
    };
    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(status, _)) => {
            return if status == 401 || status == 403 {
                ProbeOutcome::AuthRejected { status }
            } else {
                ProbeOutcome::Unreachable { error: format!("HTTP {status}") }
            };
        }
        Err(e) => return ProbeOutcome::Unreachable { error: e.to_string() },
    };
    let latency_ms = started.elapsed().as_millis() as u64;
    let body = resp
        .into_string()
        .map(|s| {
            // 响应体截断到 1MB——列表端点可能带全文档，计数不需要
            s.chars().take(1024 * 1024).collect::<String>()
        })
        .unwrap_or_default();
    let models = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .map(|v| {
            v.get("data")
                .or_else(|| v.get("models"))
                .and_then(|a| a.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        })
        .unwrap_or(0);
    ProbeOutcome::Ok { models, latency_ms }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_table_covers_the_common_api_key_providers() {
        assert_eq!(
            catalog_target("openai"),
            Some(("https://api.openai.com/v1/models", Auth::Bearer))
        );
        assert_eq!(
            catalog_target("anthropic"),
            Some(("https://api.anthropic.com/v1/models", Auth::Anthropic))
        );
        assert!(catalog_target("zai-coding-cn").is_none(), "表外商跳过探查");
    }

    #[test]
    fn custom_urls_follow_the_api_shape() {
        assert_eq!(
            custom_target("https://gw.example.com/v1", "openai-completions"),
            Some(("https://gw.example.com/v1/models".into(), Auth::Bearer))
        );
        // 版本段不重复补
        assert_eq!(
            custom_target("https://gw.example.com/v1/", "anthropic-messages"),
            Some(("https://gw.example.com/v1/models".into(), Auth::Anthropic))
        );
        assert_eq!(
            custom_target("https://gw.example.com", "anthropic-messages"),
            Some(("https://gw.example.com/v1/models".into(), Auth::Anthropic))
        );
        assert_eq!(
            custom_target("https://gw.example.com/v1beta", "google-generative-ai"),
            Some(("https://gw.example.com/v1beta/models".into(), Auth::Google))
        );
        assert_eq!(custom_target("", "openai-completions"), None);
    }

    #[test]
    fn keys_resolve_through_the_vault_or_stay_plain() {
        let mut mem = std::collections::BTreeMap::new();
        mem.insert("PF_KEY_DEEPSEEK".to_string(), "sk-vault".to_string());
        let vault = crate::credentials::MemVault(std::sync::Mutex::new(mem));
        assert_eq!(catalog_probe_key("nosuch", &vault), None);
        assert_eq!(custom_probe_key("sk-plain", &vault), Some("sk-plain".into()));
        assert_eq!(custom_probe_key("$OTHER_VAR", &vault), None);
        assert_eq!(custom_probe_key("!cmd", &vault), None);
    }
}
