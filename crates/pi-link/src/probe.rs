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

use crate::credentials::{SecretVault, StoreMode, classify, pf_ref};
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
#[derive(Debug, Clone, PartialEq)]
pub enum ProbeOutcome {
    /// 2xx：通路与 key 均有效；`catalog` = 响应体里 best-effort 解出的模型
    /// 目录（字段缺失尽量回填，解不出为空）。
    Ok {
        models: usize,
        latency_ms: u64,
        catalog: Vec<crate::protocol::ModelInfo>,
    },
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
        (
            "deepseek",
            "https://api.deepseek.com/v1/models",
            Auth::Bearer,
        ),
        (
            "anthropic",
            "https://api.anthropic.com/v1/models",
            Auth::Anthropic,
        ),
        (
            "google",
            "https://generativelanguage.googleapis.com/v1beta/models",
            Auth::Google,
        ),
        (
            "openrouter",
            "https://openrouter.ai/api/v1/models",
            Auth::Bearer,
        ),
        (
            "groq",
            "https://api.groq.com/openai/v1/models",
            Auth::Bearer,
        ),
        (
            "cerebras",
            "https://api.cerebras.ai/v1/models",
            Auth::Bearer,
        ),
        ("mistral", "https://api.mistral.ai/v1/models", Auth::Bearer),
        ("xai", "https://api.x.ai/v1/models", Auth::Bearer),
        (
            "moonshotai",
            "https://api.moonshot.ai/v1/models",
            Auth::Bearer,
        ),
        (
            "moonshotai-cn",
            "https://api.moonshot.cn/v1/models",
            Auth::Bearer,
        ),
        (
            "together",
            "https://api.together.xyz/v1/models",
            Auth::Bearer,
        ),
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

/// 自定义商的列模型 URL：`baseUrl` + `api`（042-1 改版：api 由 [`infer_api`]
/// 从 baseUrl 推断，UI 不再让用户选）按同一构造规则（对齐 pi-web
/// buildModelsListUrl：按 api 补版本段，不重复补）。Anthropic 兼容端点的
/// messages 连通兑底见 [`anthropic_compat_urls`] + [`probe_anthropic_compat`]。
pub fn custom_target(base_url: &str, api: &str) -> Option<(String, Auth)> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return None;
    }
    let (url, auth) = match api {
        "anthropic-messages" => (join_version(base, "v1"), Auth::Anthropic),
        "google-generative-ai" => (join_version(base, "v1beta"), Auth::Google),
        // openai 系（completions/responses）：base 末段已是版本号（/v1、/v4…
        // GLM coding 端点即 /v4）则直接接 /models，否则补 /v1（通用自定义商）
        _ => if base_has_version_segment(base) {
            (format!("{base}/models"), Auth::Bearer)
        } else {
            (format!("{base}/v1/models"), Auth::Bearer)
        },
    };
    Some((url, auth))
}

/// base 是否已含 openai 版本段：末段为 /v<数字>（v1/v4/v1beta 不算——那是
/// google 段）或路径中含 /v1/。决定接 /models 还是补 /v1。
fn base_has_version_segment(base: &str) -> bool {
    let last = base.rsplit('/').next().unwrap_or("");
    let tail_is_version = last.len() >= 2
        && last.starts_with('v')
        && last[1..].bytes().all(|b| b.is_ascii_digit());
    tail_is_version || base.contains("/v1/")
}

/// baseUrl → pi wire 协议形状（042-1 改版：Provider = baseUrl + apiKey，
/// API 形状用户不选，PF 推断后随条目落盘——pi 侧 modelFromJson /
/// extensionModelFromDefinition 对缺 api 硬 throw，绝不留空）。
/// `*/anthropic` / 含 `/anthropic/` → anthropic-messages（DeepSeek /anthropic、
/// GLM、Kimi 等兼容端点）；`*/v1beta` → google-generative-ai；其余 →
/// openai-completions（最大公约数）。openai-responses 与 completions 在 URL
/// 上不可区分，一律默认 completions，误判走模型级 API 覆盖逃生门。
pub fn infer_api(base_url: &str) -> &'static str {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/anthropic") || base.contains("/anthropic/") {
        "anthropic-messages"
    } else if base.ends_with("/v1beta") || base.contains("/v1beta/") {
        "google-generative-ai"
    } else {
        "openai-completions"
    }
}

/// base 已带该版本段就只接 /models，否则补上版本段再接。
fn join_version(base: &str, version: &str) -> String {
    if base.ends_with(&format!("/{version}")) {
        format!("{base}/models")
    } else {
        format!("{base}/{version}/models")
    }
}

/// base 指向 Anthropic 兼容端点（[`infer_api`] 同款判定：带 `/anthropic` 段）
/// → 返回 (models_url, messages_url)，供 [`probe_anthropic_compat`]
/// 目录优先 + 连通兑底。
pub fn anthropic_compat_urls(base_url: &str) -> Option<(String, String)> {
    let base = base_url.trim().trim_end_matches('/');
    let implies = base.ends_with("/anthropic") || base.contains("/anthropic/");
    (!base.is_empty() && implies)
        .then(|| (format!("{base}/v1/models"), format!("{base}/v1/messages")))
}

/// Anthropic 兼容端点探查：先 GET /v1/models（Anthropic 官方列模型端点，
/// DeepSeek /anthropic 等网关多有实现——无 key 探测 401 而非 404）；
/// 拉到目录即成功。401/403 直接判鉴权拒绝（key 坏，不必再打）；其余
/// （404 / 空目录 / 网络）回退 POST /v1/messages 判连通——Messages-only
/// 代理通 = 空目录，上层 PassedEmpty 提示手动 +模型。
pub fn probe_anthropic_compat(models_url: &str, messages_url: &str, key: &str) -> ProbeOutcome {
    let first = probe(models_url, Auth::Anthropic, key);
    let got_catalog = matches!(&first, ProbeOutcome::Ok { catalog, .. } if !catalog.is_empty());
    if got_catalog || matches!(first, ProbeOutcome::AuthRejected { .. }) {
        return first;
    }
    probe(messages_url, Auth::Anthropic, key)
}

/// 目录 provider 的探查 key（042-1）：auth.json 条目 —— PF 标记条目解钥匙库，
/// 用户手写的明文取原值；`!cmd` / `$VAR` 高级引用不探查。无条目 → None。
pub fn catalog_probe_key(provider: &str) -> Option<String> {
    crate::auth_json::resolve_secret(provider)
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
/// `data[]`（openai/anthropic 形状）或 `models[]`（google 形状）解析模型
/// 目录：id 缺失回退 `name`（google 形状剥 `models/` 前缀）、显示名取
/// `displayName`/`display_name`、上下文窗口取 `context_length`/
/// `context_window`/`inputTokenLimit`；解不出按空目录计。
pub fn probe(url: &str, auth: Auth, key: &str) -> ProbeOutcome {
    let started = std::time::Instant::now();
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build();
    // DeepSeek Anthropic 兼容代理 /v1/messages 只接受 POST，测速用 POST 判连通
    let is_msg = url.contains("/messages");
    let req = if is_msg { agent.post(url) } else { agent.get(url) };
    let req = match auth {
        Auth::Bearer => req.set("Authorization", &format!("Bearer {key}")),
        Auth::Anthropic => {
            let mut r = req.set("x-api-key", key).set("anthropic-version", "2023-06-01");
            if url.contains("/messages") { r = r.set("Content-Type", "application/json"); }
            r
        }
        Auth::Google => req.set("x-goog-api-key", key),
    };
    let resp_result: Result<ureq::Response, ureq::Error> = if is_msg {
        req.send_string(r#"{"model":"deepseek-chat","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}"#)
    } else {
        req.call()
    };
    let resp = match resp_result {
        Ok(r) => r,
        Err(ureq::Error::Status(status, _)) => {
            return if status == 401 || status == 403 {
                ProbeOutcome::AuthRejected { status }
            } else {
                ProbeOutcome::Unreachable {
                    error: format!("HTTP {status}"),
                }
            };
        }
        Err(e) => {
            return ProbeOutcome::Unreachable { error: e.to_string() };
        }
    };
    let latency_ms = started.elapsed().as_millis() as u64;
    let body = resp
        .into_string()
        .map(|s| {
            // 响应体截断到 1MB——列表端点可能带全文档，计数不需要
            s.chars().take(1024 * 1024).collect::<String>()
        })
        .unwrap_or_default();
    let entries = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| {
            v.get("data")
                .or_else(|| v.get("models"))
                .and_then(|a| a.as_array())
                .cloned()
        })
        .unwrap_or_default();
    let models = entries.len();
    let catalog = entries.iter().filter_map(parse_entry).collect();
    ProbeOutcome::Ok {
        models,
        latency_ms,
        catalog,
    }
}

/// 单条模型目录项的 best-effort 解析（各家 /models 字段形状不一）。
fn parse_entry(e: &serde_json::Value) -> Option<crate::protocol::ModelInfo> {
    let raw_id = e
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| e.get("name").and_then(|v| v.as_str()))?;
    let id = raw_id.strip_prefix("models/").unwrap_or(raw_id);
    if id.is_empty() {
        return None;
    }
    let display = e
        .get("displayName")
        .and_then(|v| v.as_str())
        .or_else(|| e.get("display_name").and_then(|v| v.as_str()))
        .or_else(|| e.get("name").and_then(|v| v.as_str()))
        .unwrap_or("");
    let name = if display.is_empty() || display == raw_id {
        String::new()
    } else {
        display.to_string()
    };
    let context_window = e
        .get("context_length")
        .and_then(|v| v.as_u64())
        .or_else(|| e.get("context_window").and_then(|v| v.as_u64()))
        .or_else(|| e.get("inputTokenLimit").and_then(|v| v.as_u64()));
    Some(crate::protocol::ModelInfo {
        id: id.to_string(),
        name,
        provider: String::new(),
        context_window,
    })
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
        // 末段版本号任意 /vN 都直接接 /models（GLM coding /v4 实测样本）
        assert_eq!(
            custom_target("https://open.bigmodel.cn/api/coding/paas/v4", "openai-completions"),
            Some((
                "https://open.bigmodel.cn/api/coding/paas/v4/models".into(),
                Auth::Bearer
            ))
        );
        // /v1beta 不是 openai 版本段（google 专用），不误判
        assert_eq!(
            custom_target("https://g.example.com/v1beta", "openai-completions"),
            Some(("https://g.example.com/v1beta/v1/models".into(), Auth::Bearer))
        );
    }

    #[test]
    fn anthropic_compat_urls_match_the_base_condition() {
        let (models, messages) =
            anthropic_compat_urls("https://api.deepseek.com/anthropic").unwrap();
        assert_eq!(models, "https://api.deepseek.com/anthropic/v1/models");
        assert_eq!(messages, "https://api.deepseek.com/anthropic/v1/messages");
        // 尾斜杠归一 + 段中匹配（custom_target 同条件）
        assert!(anthropic_compat_urls("https://x.com/anthropic/").is_some());
        assert!(anthropic_compat_urls("https://x.com/api/anthropic/v2").is_some());
        // 纯 openai 形 base 不适用
        assert!(anthropic_compat_urls("https://api.deepseek.com").is_none());
        assert!(anthropic_compat_urls("https://api.deepseek.com/v1").is_none());
        assert_eq!(anthropic_compat_urls(""), None);
    }

    #[test]
    fn infer_api_routes_by_base_shape() {
        assert_eq!(
            infer_api("https://api.deepseek.com/anthropic"),
            "anthropic-messages"
        );
        assert_eq!(
            infer_api("https://gw.example.com/api/anthropic/"),
            "anthropic-messages"
        );
        assert_eq!(
            infer_api("https://generativelanguage.googleapis.com/v1beta"),
            "google-generative-ai"
        );
        assert_eq!(infer_api("https://api.openai.com/v1"), "openai-completions");
        assert_eq!(infer_api("https://api.deepseek.com"), "openai-completions");
        assert_eq!(infer_api(""), "openai-completions");
        // 推断值必须被 custom_target 原样消化（形状闭环，目录 URL 不落 messages）
        for base in [
            "https://api.deepseek.com/anthropic",
            "https://x.example.com/v1",
            "https://g.example.com/v1beta",
        ] {
            let (url, _) = custom_target(base, infer_api(base)).unwrap();
            assert!(url.starts_with(base.trim_end_matches('/')), "{url}");
            assert!(url.ends_with("/models"), "{url}");
        }
    }

    #[test]
    fn keys_resolve_through_the_vault_or_stay_plain() {
        let mut mem = std::collections::BTreeMap::new();
        mem.insert("PF_KEY_DEEPSEEK".to_string(), "sk-vault".to_string());
        let vault = crate::credentials::MemVault(std::sync::Mutex::new(mem));
        // 目录商 key 自 042-1 起走 auth.json（本机无标注条目 → None）
        assert_eq!(catalog_probe_key("nosuch"), None);
        assert_eq!(
            custom_probe_key("sk-plain", &vault),
            Some("sk-plain".into())
        );
        assert_eq!(custom_probe_key("$OTHER_VAR", &vault), None);
        assert_eq!(custom_probe_key("!cmd", &vault), None);
    }

    #[test]
    fn entry_parse_maps_each_provider_shape() {
        // openrouter：name/id/context_length
        let m = parse_entry(&serde_json::json!({
            "id": "deepseek/deepseek-r1", "name": "DeepSeek R1", "context_length": 128000
        }))
        .unwrap();
        assert_eq!(m.id, "deepseek/deepseek-r1");
        assert_eq!(m.name, "DeepSeek R1");
        assert_eq!(m.context_window, Some(128000));
        // google：name 带 models/ 前缀 + displayName + inputTokenLimit
        let m = parse_entry(&serde_json::json!({
            "name": "models/gemini-2.0-flash", "displayName": "Gemini 2.0 Flash",
            "inputTokenLimit": 1048576
        }))
        .unwrap();
        assert_eq!(m.id, "gemini-2.0-flash");
        assert_eq!(m.name, "Gemini 2.0 Flash");
        assert_eq!(m.context_window, Some(1048576));
        // openai：只有 id（name=owned_by 字符串时回落空显示名）
        let m = parse_entry(&serde_json::json!({ "id": "gpt-4o", "object": "model" })).unwrap();
        assert_eq!(m.id, "gpt-4o");
        assert_eq!(m.name, "");
        assert_eq!(m.context_window, None);
        assert!(parse_entry(&serde_json::json!({ "object": "model" })).is_none());
        assert_eq!(
            parse_entry(&serde_json::json!({ "id": "x", "name": "x" }))
                .unwrap()
                .name,
            ""
        );
    }
}
