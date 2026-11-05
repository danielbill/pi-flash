use super::*;

fn sample() -> RegistryDump {
    serde_json::from_str(
        r#"{
            "version": "1.1.0",
            "providers": [
                { "id": "anthropic", "name": "Anthropic", "hasApiKeyLogin": true,
                  "hasOAuth": true, "oauthName": "Claude Pro/Max" },
                { "id": "deepseek", "name": "DeepSeek", "hasApiKeyLogin": true },
                { "id": "github-copilot", "name": "GitHub Copilot", "hasOAuth": true }
            ],
            "counts": { "anthropic": 2, "deepseek": 1 },
            "models": [
                { "id": "claude-opus-4-8", "name": "Claude Opus 4.8",
                  "provider": "anthropic", "contextWindow": 200000 },
                { "id": "claude-sonnet-4-6", "provider": "anthropic" },
                { "id": "deepseek-v4-pro", "name": "DeepSeek V4 Pro",
                  "provider": "deepseek", "contextWindow": 128000 }
            ]
        }"#,
    )
    .unwrap()
}

#[test]
fn parse_is_case_shaped_like_dump_script() {
    let d = sample();
    assert_eq!(d.providers.len(), 3);
    assert!(d.providers[0].has_api_key_login);
    assert!(d.providers[0].has_oauth);
    assert_eq!(d.providers[0].oauth_name.as_deref(), Some("Claude Pro/Max"));
    assert!(!d.providers[1].has_oauth);
}

#[test]
fn models_parse_with_defaults_and_models_for_filters_by_provider() {
    let d = sample();
    assert_eq!(d.models.len(), 3);
    // 缺 name 的条目回退空串、缺 contextWindow 回退 None（serde default）
    let sonnet = d
        .models
        .iter()
        .find(|m| m.id == "claude-sonnet-4-6")
        .unwrap();
    assert_eq!(sonnet.name, "");
    assert_eq!(sonnet.context_window, None);
    let ids: Vec<_> = d.models_for("anthropic").map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["claude-opus-4-8", "claude-sonnet-4-6"]);
    // 无条目的 provider → 空
    assert_eq!(d.models_for("github-copilot").count(), 0);
}

#[test]
fn models_list_target_derives_from_sdk_base_url() {
    // 带 baseUrl/api 的 provider：机械拼装端点（zai-coding-cn 实测形状）
    let d: RegistryDump = serde_json::from_str(
        r#"{
            "version": "1.4.0",
            "providers": [
                { "id": "zai-coding-cn", "name": "Z.AI Coding CN",
                  "baseUrl": "https://open.bigmodel.cn/api/coding/paas/v4",
                  "api": "openai-completions" },
                { "id": "minimax-cn", "name": "MiniMax CN",
                  "baseUrl": "https://api.minimaxi.com/anthropic",
                  "api": "anthropic-messages" },
                { "id": "amazon-bedrock", "name": "Bedrock" }
            ]
        }"#,
    )
    .unwrap();
    // models_list_target：有 baseUrl → 端点；无 baseUrl（bedrock 类）→ None
    let t = models_list_target(&d, "zai-coding-cn").unwrap();
    assert!(t.0.ends_with("/v4/models"));
    assert_eq!(t.1, crate::probe::Auth::Bearer);
    let t2 = models_list_target(&d, "minimax-cn").unwrap();
    assert!(t2.0.ends_with("/anthropic/v1/models?limit=1000"));
    assert_eq!(t2.1, crate::probe::Auth::Anthropic);
    assert!(models_list_target(&d, "amazon-bedrock").is_none());
    assert!(models_list_target(&d, "no-such").is_none());
}

#[test]
fn cache_without_any_base_url_is_invalid_format() {
    // 2026-11-01 前的缓存 providers 全员无 baseUrl → read_cache 视为
    // 旧格式失效（重 dump 自愈）。这里断言判据本身
    let old: RegistryDump = serde_json::from_str(
        r#"{ "version": "1.4.0", "staticOnly": true,
            "providers": [ { "id": "zai", "name": "ZAI" } ],
            "models": [ { "id": "m", "provider": "zai" } ] }"#,
    )
    .unwrap();
    assert!(old.providers.iter().all(|p| p.base_url.is_none()));
    let new_: RegistryDump = serde_json::from_str(
        r#"{ "version": "1.4.0", "staticOnly": true,
            "providers": [ { "id": "zai", "name": "ZAI", "baseUrl": "https://x" } ],
            "models": [ { "id": "m", "provider": "zai" } ] }"#,
    )
    .unwrap();
    assert!(!new_.providers.iter().all(|p| p.base_url.is_none()));
}

#[test]
fn old_cache_without_models_key_parses_to_empty() {
    // 旧格式缓存（本改动之前落盘）没有 models 键：serde default 解析成功
    // 但 models 为空——read_cache 按失效处理，重 dump 补全
    let d: RegistryDump = serde_json::from_str(
        r#"{ "version": "1.1.0", "providers": [ { "id": "zai", "name": "ZAI" } ],
            "counts": { "zai": 7 } }"#,
    )
    .unwrap();
    assert_eq!(d.providers.len(), 1);
    assert!(d.models.is_empty());
}

#[test]
fn groups_filter_by_credential_type() {
    let mut d = sample();
    d.credentials.insert("anthropic".into(), "oauth".into());
    d.credentials.insert("deepseek".into(), "api_key".into());
    // anthropic 有 oauth 凭据 → 订阅服务组隐藏，API KEY 组照列
    assert!(d.oauth_providers().iter().all(|p| p.id != "anthropic"));
    assert!(d.api_key_providers().iter().any(|p| p.id == "anthropic"));
    // deepseek 有 api_key 凭据 → API KEY 组隐藏
    assert!(d.api_key_providers().iter().all(|p| p.id != "deepseek"));
    assert!(d.oauth_providers().iter().all(|p| p.id != "deepseek"));
}

#[test]
fn oauth_display_name_overrides_then_oauth_name_then_name() {
    let mut d = sample();
    let codex = RegistryProvider {
        base_url: None,
        api: None,
        id: "openai-codex".into(),
        name: "OpenAI Codex".into(),
        has_api_key_login: false,
        has_oauth: true,
        oauth_name: Some("Codex".into()),
    };
    d.providers.push(codex);
    let by_id = |id: &str| d.providers.iter().find(|p| p.id == id).unwrap();
    assert_eq!(
        RegistryDump::oauth_display_name(by_id("openai-codex")),
        "ChatGPT Plus/Pro"
    );
    assert_eq!(
        RegistryDump::oauth_display_name(by_id("github-copilot")),
        "GitHub Copilot"
    );
    assert_eq!(
        RegistryDump::oauth_display_name(by_id("anthropic")),
        "Claude Pro/Max"
    );
    assert_eq!(
        RegistryDump::oauth_display_name(by_id("deepseek")),
        "DeepSeek"
    );
}

#[test]
fn model_count_defaults_to_zero() {
    let mut d = sample();
    assert_eq!(d.model_count("openrouter"), 0);
    d.model_counts.insert("openrouter".into(), 408);
    assert_eq!(d.model_count("openrouter"), 408);
}

#[test]
fn source_requires_models_json_entry() {
    let d = sample();
    // 注册表内的商恒为内置（models.json 覆盖条目不改变归属）
    assert_eq!(d.source("deepseek", true), ProviderSource::Builtin);
    assert_eq!(d.source("deepseek", false), ProviderSource::Builtin);
    // models.json 有条目 = 自定义
    assert_eq!(d.source("ds", true), ProviderSource::Custom);
    // 不在注册表**且** models.json 无条目 = 扩展注册的 provider
    //（pi-freeflow 等）：非自定义，但归「内置及插件」组出列
    assert_eq!(d.source("freeflow", false), ProviderSource::Builtin);
    assert!(d.is_extension_provider("freeflow", false));
    assert!(!d.is_extension_provider("freeflow", true));
    assert!(!d.is_extension_provider("deepseek", false));
}
