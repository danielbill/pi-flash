//! 全量 provider 注册表（042 新增 Provider 弹窗数据源，对齐 pi-web
//! `lib/provider-listing*.ts`）。
//!
//! pi RPC 的 `get_available_models` 走 SDK 的 **available**（凭据过滤后）
//! 集合，而弹窗要的是 **registry 全量**（`getProviders()` + `getModels()`，
//! credential-blind）。auth 声明（`auth.oauth` / `auth.apiKey.login`）长在
//! pi-ai 的 JS 里，Rust 没法直读——所以起**一次性 node** 走 SDK 自己的 API
//! dump JSON（`ModelRuntime.create({ refreshOnCreate: false })`：不联网刷
//! 新，静态注册表 + 盘上目录缓存照常装配；credentials 传 `ReadOnlyAuthStorage`
//! ——默认存储的 `listCredentials` 会等 auth.json 的 proper-lockfile 锁，
//! 孤儿锁耗满 30s stale 死线才放行，曾致弹窗"读取注册表"挂几十秒），
//! 结果落 `registry-cache.json`，
//! 以 [`crate::vendor::vendored_version`] 为缓存键，bump vendor 自动失效。
//! dump 在 `PI_CODING_AGENT_DIR` 指向的**空沙箱**里跑——`ModelRuntime` 会把
//! agent 目录 models.json 的自定义商装进 `getProviders()`，归属判据会被
//! models.json 污染；载荷带 `staticOnly` 标记，旧非静态缓存一律失效。
//!
//! 分组/过滤语义照抄 pi-web：
//! - API KEY 组 = 声明 `apiKey.login` 且当前凭据**不是** `api_key`（OAuth
//!   凭据的商也照列——pi-web 该组 configured=false 同款）
//! - 订阅服务组 = 声明 `oauth` 且当前凭据不是 `oauth`
//! - modelCount = 全量 registry 按 provider 计数，与启用开关无关
//! - OAuth 显示名 = 覆盖表 → `auth.oauth.name` → provider.name
//! - 组内**不排序**，保持 SDK `getProviders()` 原始顺序
//!
//! dump 同时携带全量模型条目（`models`，credential-blind 静态目录）——
//! 表外内置商（无探查端点）保存 key 后可用模型区的保底数据源，见
//! [`RegistryDump::models`] 字段注。

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
    /// SDK 注册表里的baseUrl（provider 对象直带；2026-11-01 起提取——
    /// 内置商「获取模型」端点由此派生，替代 13 条手写白名单表）。
    #[serde(rename = "baseUrl", default)]
    pub base_url: Option<String>,
    /// 该商模型的协议形状（取首模型的 api；模型级覆盖以模型条目为准）。
    #[serde(default)]
    pub api: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct RegistryDump {
    /// 生成本 dump 时的 vendored pi VERSION（缓存有效性键）。
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub providers: Vec<RegistryProvider>,
    /// provider id → 全量 registry 模型数（dump 脚本输出的键名是 `counts`）。
    #[serde(default, rename = "counts")]
    pub model_counts: BTreeMap<String, usize>,
    /// provider id → auth.json 凭据类型（"api_key" | "oauth"）。
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
    /// 全量 registry 模型条目（静态目录，credential-blind）。表外内置商
    /// （无探查端点）可用模型区的保底数据源——只喂设置页详情
    /// （`merge_registry_models`），**不并入 `catalog_for`/`Globals.models`**
    /// （会话 picker 非虚拟化，全量灌入踩性能铁律）。
    #[serde(default)]
    pub models: Vec<crate::protocol::ModelInfo>,
    /// dump 是否为**纯静态注册表**（PI_CODING_AGENT_DIR 指向空目录生成）。
    /// 旧 dump 在用户 agent 目录里跑，`ModelRuntime` 会把 models.json 的自定义
    /// 商也装进 getProviders()——归属判据「在不在注册表」被 models.json 污染
    /// 且随缓存时机漂移。无此标记的缓存一律失效重 dump。
    #[serde(default, rename = "staticOnly")]
    pub static_only: bool,
}

/// Provider 归属（042-1）：判据是**在不在 pi 的 SDK 注册表**，而不是「谁写的
/// 文件」。`models.json` 里出现内置商条目（走代理只改 baseUrl、或 upsert 模型）
/// 是合法的覆盖写法，不能因此把它当自定义商。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProviderSource {
    Builtin,
    Custom,
}

/// 无注册表（首启 dump 未就绪 / 失败）时的过渡判据：`models.json` 有条目 →
/// 自定义。注册表就绪后以 [`RegistryDump::source`] 为准（自愈）。
pub fn source_without_registry(in_models_json: bool) -> ProviderSource {
    if in_models_json {
        ProviderSource::Custom
    } else {
        ProviderSource::Builtin
    }
}

impl RegistryDump {
    /// 该 id 是否 pi 内置注册表里的商。
    pub fn has_provider(&self, id: &str) -> bool {
        self.providers.iter().any(|p| p.id == id)
    }

    /// 归属判定（注册表就绪时）。dump 为**纯静态注册表**（agent 目录置空的
    /// 沙箱里生成，不含 models.json / 扩展内容），所以：`in_models_json` =
    /// models.json 里有条目 → 自定义（内置商的 models.json 覆盖条目不算，
    /// 见 [`Self::has_provider`]）；不在静态注册表且 models.json 无条目 →
    /// 扩展注册的 provider（pi-freeflow 等，归「内置及插件」组，不是自定义）。
    pub fn source(&self, id: &str, in_models_json: bool) -> ProviderSource {
        if self.has_provider(id) || !in_models_json {
            ProviderSource::Builtin
        } else {
            ProviderSource::Custom
        }
    }

    /// 扩展注册的 provider（注册表就绪时）：不在静态注册表且 models.json
    /// 无条目。与 [`Self::source`] 的 Builtin 分支互补——归属上不是内置也
    /// 不是自定义，设置页归「内置及插件」组、无凭据也出列（否则插件带的
    /// 模型用户看不见）。
    pub fn is_extension_provider(&self, id: &str, in_models_json: bool) -> bool {
        !self.has_provider(id) && !in_models_json
    }

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

    /// 指定 provider 的静态模型条目（registry 顺序）。线性扫过滤——1599
    /// 条微秒级，消费方（设置页详情）非每帧热路径，无需分组索引。
    pub fn models_for<'a>(
        &'a self,
        provider: &'a str,
    ) -> impl Iterator<Item = &'a crate::protocol::ModelInfo> + 'a {
        self.models.iter().filter(move |m| m.provider == provider)
    }
}

/// `~/.pi-flash/registry-cache.json`。
fn cache_file() -> Option<std::path::PathBuf> {
    paths::registry_cache_file()
}

/// 内置商「获取模型」端点（SDK 派生）：该商的 (baseUrl, api) → 机械拼装。
/// 无 baseUrl / 空 base 的商（云专属鉴权类）→ None。
pub fn models_list_target(
    dump: &RegistryDump,
    provider: &str,
) -> Option<(String, crate::probe::Auth)> {
    let p = dump.providers.iter().find(|p| p.id == provider)?;
    let base = p.base_url.as_deref()?.trim();
    if base.is_empty() {
        return None;
    }
    crate::probe::models_list_url(base, p.api.as_deref().unwrap_or(""))
}

/// 读盘上缓存；vendor VERSION 不一致（或字段缺失）视为失效。旧格式缓存
/// （无 `models` 键或非静态 dump——无 `staticOnly` 标记，serde default 为
/// false）同样失效——重 dump 补全。
pub fn read_cache() -> Option<RegistryDump> {
    let path = cache_file()?;
    let text = std::fs::read_to_string(path).ok()?;
    let dump: RegistryDump = serde_json::from_str(&text).ok()?;
    if dump.version != vendored_version().unwrap_or_default()
        || !dump.static_only
        || dump.providers.is_empty()
        || dump.models.is_empty()
        // 2026-11-01 前的缓存不带 baseUrl（内置商拉取端点派生源）→ 重 dump
        || dump.providers.iter().all(|p| p.base_url.is_none())
        // 模型条目不带 maxTokens/reasoning/input（模型详情只读视图回填用）→ 重 dump
        // 2026-11-04 前的缓存不带 cost（042-2 价格回显用）→ 重 dump
        || dump.models.iter().all(|m| m.max_tokens.is_none())
        || dump.models.iter().all(|m| m.cost.is_none())
    {
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
// credentials 必须传只读存储：默认 AuthStorage.list 走 proper-lockfile 等
// auth.json 锁，孤儿锁要耗满 30s stale 死线才放行（弹窗"几十秒"根因）。
// 只读版裸读文件零锁，且本 dump 本来只消费 providerId→type 元数据。
const { ReadOnlyAuthStorage } = await import(new URL("./core/auth-storage.js", entry));
const rt = await ModelRuntime.create({
    refreshOnCreate: false,
    credentials: new ReadOnlyAuthStorage(),
});
const allModels = rt.getModels();
const firstApi = {};
for (const m of allModels) if (!firstApi[m.provider]) firstApi[m.provider] = m.api ?? null;
const providers = rt.getProviders().map((p) => ({
    id: p.id,
    name: p.name,
    hasApiKeyLogin: Boolean(p.auth && p.auth.apiKey && p.auth.apiKey.login),
    hasOAuth: Boolean(p.auth && p.auth.oauth),
    oauthName: (p.auth && p.auth.oauth && p.auth.oauth.name) || null,
    baseUrl: p.baseUrl ?? null,
    api: firstApi[p.id] ?? null,
}));
const counts = {};
for (const m of allModels) counts[m.provider] = (counts[m.provider] || 0) + 1;
const models = allModels.map((m) => ({
    id: m.id,
    name: m.name ?? m.id,
    provider: m.provider,
    contextWindow: m.contextWindow ?? null,
    maxTokens: m.maxTokens ?? null,
    reasoning: m.reasoning ?? null,
    input: Array.isArray(m.input) ? m.input : null,
    cost: m.cost ?? null,
}));
const credentials = {};
for (const c of await rt.listCredentials()) credentials[c.providerId] = c.type;
process.stdout.write(JSON.stringify({ providers, counts, credentials, models, staticOnly: true }));
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
    // 纯静态注册表：ModelRuntime 会把 agent 目录里 models.json 的自定义商
    // 也装进 getProviders()——归属判据「在不在注册表」就没了立足点（自定义
    // 商 ds 曾随 dump 时机混进内置组）。指向空目录 = dump 只含 SDK 静态
    // 注册表；目录留盘不清（%TEMP% 卫生交给系统，删除走回收站的规矩不碰它）
    let sandbox =
        std::env::temp_dir().join(format!("pi-flash-registry-dump-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&sandbox);
    cmd.env("PI_CODING_AGENT_DIR", &sandbox);
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
            err.lines()
                .last()
                .unwrap_or("registry dump failed")
                .to_string()
        });
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut dump: RegistryDump =
        serde_json::from_str(stdout.trim()).map_err(|e| format!("registry dump 解析失败: {e}"))?;
    // 旧版 SDK（≤1.0.0）refreshOnCreate:false 下 getModels() 返回空——计数
    // 或模型条目任一为空 = dump 无效，按失败处理（不落缓存，下次弹窗重试）
    if !dump.providers.is_empty() && (dump.model_counts.is_empty() || dump.models.is_empty()) {
        return Err("registry dump 缺少模型清单/计数（vendor SDK 过旧）".to_string());
    }
    dump.version = vendored_version().unwrap_or_default();
    Ok(dump)
}

#[cfg(test)]
mod tests;
