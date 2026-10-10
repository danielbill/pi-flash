//! 全量 provider 注册表（042 新增 Provider 弹窗数据源，对齐 pi-web
//! `lib/provider-listing*.ts`）。
//!
//! pi RPC 的 `get_available_models` 走 SDK 的 **available**（凭据过滤后）
//! 集合，而弹窗要的是 **registry 全量**（`getProviders()` + `getModels()`，
//! credential-blind）。auth 声明（`auth.oauth` / `auth.apiKey.login`）长在
//! pi-ai 的 JS 里，Rust 没法直读——所以起**一次性 node** 走 SDK 自己的 API
//! dump JSON（`ModelRuntime.create({ refreshOnCreate: false })`：不联网刷
//! 新，静态注册表 + 盘上目录缓存照常装配），结果落 `registry-cache.json`，
//! 以 [`crate::vendor::vendored_version`] 为缓存键，bump vendor 自动失效。
//!
//! 分组/过滤语义照抄 pi-web：
//! - API KEY 组 = 声明 `apiKey.login` 且当前凭据**不是** `api_key`（OAuth
//!   凭据的商也照列——pi-web 该组 configured=false 同款）
//! - 订阅服务组 = 声明 `oauth` 且当前凭据不是 `oauth`
//! - modelCount = 全量 registry 按 provider 计数，与启用开关无关
//! - OAuth 显示名 = 覆盖表 → `auth.oauth.name` → provider.name
//! - 组内**不排序**，保持 SDK `getProviders()` 原始顺序

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::paths;
use crate::vendor;

/// OAuth 显示名覆盖表（pi-web `OAUTH_DISPLAY_NAMES` 原样）。
const OAUTH_NAME_OVERRIDES: &[(&str, &str)] = &[
    ("openai-codex", "ChatGPT Plus/Pro"),
    ("github-copilot", "GitHub Copilot"),
];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RegistryProvider {
    pub id: String,
    pub name: String,
    #[serde(rename = "hasApiKeyLogin", default)]
    pub has_api_key_login: bool,
    #[serde(rename = "hasOAuth", default)]
    pub has_oauth: bool,
    #[serde(rename = "oauthName", default)]
    pub oauth_name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct RegistryDump {
    /// 生成本 dump 时的 vendored pi VERSION（缓存有效性键）。
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub providers: Vec<RegistryProvider>,
    /// provider id → 全量 registry 模型数。
    #[serde(default)]
    pub model_counts: BTreeMap<String, usize>,
    /// provider id → auth.json 凭据类型（"api_key" | "oauth"）。
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
}

impl RegistryDump {
    pub fn credential(&self, id: &str) -> Option<&str> {
        self.credentials.get(id).map(String::as_str)
    }

    /// 全量 registry 模型数（无记录 = 0）。
    pub fn model_count(&self, id: &str) -> usize {
        self.model_counts.get(id).copied().unwrap_or(0)
    }

    /// API KEY 组（picker 可列项）：声明 apiKey.login 且当前凭据不是
    /// `api_key`。保持 registry 顺序。
    pub fn api_key_providers(&self) -> Vec<&RegistryProvider> {
        self.providers
            .iter()
            .filter(|p| p.has_api_key_login && self.credential(&p.id) != Some("api_key"))
            .collect()
    }

    /// 订阅服务组（picker 可列项）：声明 oauth 且当前凭据不是 `oauth`。
    pub fn oauth_providers(&self) -> Vec<&RegistryProvider> {
        self.providers
            .iter()
            .filter(|p| p.has_oauth && self.credential(&p.id) != Some("oauth"))
            .collect()
    }

    /// OAuth 卡显示名：覆盖表 → `auth.oauth.name` → provider.name。
    pub fn oauth_display_name(p: &RegistryProvider) -> &str {
        if let Some((_, label)) = OAUTH_NAME_OVERRIDES.iter().find(|(id, _)| *id == p.id) {
            return label;
        }
        p.oauth_name.as_deref().unwrap_or(&p.name)
    }
}

/// `~/.pi-flash/registry-cache.json`。
fn cache_file() -> Option<std::path::PathBuf> {
    paths::registry_cache_file()
}

/// 读盘上缓存；vendor VERSION 不一致（或字段缺失）视为失效。
pub fn read_cache() -> Option<RegistryDump> {
    let path = cache_file()?;
    let text = std::fs::read_to_string(path).ok()?;
    let dump: RegistryDump = serde_json::from_str(&text).ok()?;
    if dump.version != vendored_version().unwrap_or_default() || dump.providers.is_empty() {
        return None;
    }
    Some(dump)
}

fn write_cache(dump: &RegistryDump) -> Result<(), String> {
    let path = cache_file().ok_or_else(|| "pi-flash 配置目录不可用".to_string())?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = serde_json::to_string_pretty(dump).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn vendored_version() -> Option<String> {
    vendor::vendored_version()
}

/// dump 脚本（写入配置目录后由 `node <script> <pkg-entry.js>` 执行）。
/// 只往 stdout 写一个 JSON；SDK 的告警走 stderr，不污染载荷。
const DUMP_SCRIPT: &str = r#"
import { pathToFileURL } from "node:url";
const entry = pathToFileURL(process.argv[2]).href;
const { ModelRuntime } = await import(entry);
const rt = await ModelRuntime.create({ refreshOnCreate: false });
const providers = rt.getProviders().map((p) => ({
    id: p.id,
    name: p.name,
    hasApiKeyLogin: Boolean(p.auth && p.auth.apiKey && p.auth.apiKey.login),
    hasOAuth: Boolean(p.auth && p.auth.oauth),
    oauthName: (p.auth && p.auth.oauth && p.auth.oauth.name) || null,
}));
const counts = {};
for (const m of rt.getModels()) counts[m.provider] = (counts[m.provider] || 0) + 1;
const credentials = {};
for (const c of await rt.listCredentials()) credentials[c.providerId] = c.type;
process.stdout.write(JSON.stringify({ providers, counts, credentials }));
"#;

/// 同步执行 dump 并落缓存——**只在后台线程调用**（阻塞一次 node 进程）。
pub fn dump_and_cache() -> Result<RegistryDump, String> {
    let dump = dump()?;
    write_cache(&dump)?;
    Ok(dump)
}

fn dump() -> Result<RegistryDump, String> {
    let entry = vendor::pkg_entry().ok_or_else(|| "vendored pi not found".to_string())?;
    let script =
        paths::registry_dump_script_file().ok_or_else(|| "pi-flash 配置目录不可用".to_string())?;
    if let Some(dir) = script.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&script, DUMP_SCRIPT).map_err(|e| e.to_string())?;

    let mut cmd = std::process::Command::new(vendor::node_bin());
    cmd.arg(&script).arg(&entry);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(if err.trim().is_empty() {
            format!("registry dump failed ({})", out.status)
        } else {
            err.lines().last().unwrap_or("registry dump failed").to_string()
        });
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut dump: RegistryDump =
        serde_json::from_str(stdout.trim()).map_err(|e| format!("registry dump 解析失败: {e}"))?;
    dump.version = vendored_version().unwrap_or_default();
    Ok(dump)
}

#[cfg(test)]
mod tests {
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
            id: "openai-codex".into(),
            name: "OpenAI Codex".into(),
            has_api_key_login: false,
            has_oauth: true,
            oauth_name: Some("Codex".into()),
        };
        d.providers.push(codex);
        let by_id = |id: &str| d.providers.iter().find(|p| p.id == id).unwrap();
        assert_eq!(RegistryDump::oauth_display_name(by_id("openai-codex")), "ChatGPT Plus/Pro");
        assert_eq!(RegistryDump::oauth_display_name(by_id("github-copilot")), "GitHub Copilot");
        assert_eq!(RegistryDump::oauth_display_name(by_id("anthropic")), "Claude Pro/Max");
        assert_eq!(RegistryDump::oauth_display_name(by_id("deepseek")), "DeepSeek");
    }

    #[test]
    fn model_count_defaults_to_zero() {
        let mut d = sample();
        assert_eq!(d.model_count("openrouter"), 0);
        d.model_counts.insert("openrouter".into(), 408);
        assert_eq!(d.model_count("openrouter"), 408);
    }
}
