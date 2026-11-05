//! PF 插件域（090-4 v1 最小切片）：**纯声明式插件**的索引 / 台账 / 安装卸载。
//!
//! 数据流：内置索引（app 侧 include_str! 资产，唯一上架入口）→ 用户点安装 →
//! `npm pack <name>@<ver>` 拉 tarball → sha512 对账索引 integrity → 系统 tar
//! 解包到 `<数据目录>/plugins/<name>/<version>/` → 写台账 `plugins.json`。
//! 索引外的包一律拒装——「只能从市场获得」由这条校验链保证，不是 UI 隐藏。
//!
//! v1 明确不做：TS/sidecar 形态、远端索引、签名验签（090-5 L1 预留 integrity
//! 字段）、混淆、激活。npm 调用与宿主 plugin_updates 同款：内置 node 直跑
//! 内置 npm-cli.js（Windows 不裸 spawn npm，CVE-2024-27980）。

use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};

// ---------------------------------------------------------------------------
// 市场索引（app 侧从内置资产解析）
// ---------------------------------------------------------------------------

/// 索引条目展示名（与 npm 包名解耦，UI 一律显示这里）。
#[derive(Deserialize, Clone, Debug)]
pub struct IndexDisplay {
    pub zh: String,
    #[serde(default)]
    pub en: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct IndexEntry {
    pub name: String,
    pub version: String,
    /// SRI 格式：`sha512-<base64>`。安装时对真实 tarball 对账，不信任包自带值。
    pub integrity: String,
    /// 宿主 API 下限（`">=0.2"` 语义），app 侧用 semver 校验。
    #[serde(default)]
    pub min_pf: String,
    pub display: IndexDisplay,
    /// 一句话描述（列表卡片副标题；缺省 = 不渲染描述行）。
    #[serde(default)]
    pub desc: Option<IndexDisplay>,
    /// 发布人（v1 恒 "Pi-Flash"，第三方开放时为作者名）。
    #[serde(default)]
    pub publisher: String,
    /// 发布日期（ISO 字符串，直接展示）。
    #[serde(default)]
    pub published_at: String,
    /// 下载档：`"npm"`（默认，按 registry 约定隐式解析）| `"url"`（索引自描述路径，
    /// 直连拉取——私有渠道/内层 wasm 包用，090-3 §3.1 source 双档）。
    #[serde(default)]
    pub source: String,
    /// `source = "url"` 时的 tarball 直链（http/https）。
    #[serde(default)]
    pub url: String,
    /// 市场卡片预览图：包内资产相对路径（`assets/icon.svg`）。仅已安装时可
    /// 解析（`plugins/<pkg>/<icon>`，资产随包落盘）；未装/文件缺失回通用
    /// package 图标。pf-market/1 可选字段，缺省 = 按 status_bar 图标或回退。
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Deserialize, Clone, Default)]
pub struct MarketIndex {
    #[serde(default)]
    pub packages: Vec<IndexEntry>,
}

impl MarketIndex {
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("索引解析失败: {e}"))
    }

    pub fn entry(&self, name: &str) -> Option<&IndexEntry> {
        self.packages.iter().find(|p| p.name == name)
    }
}

// ---------------------------------------------------------------------------
// 台账（`<数据目录>/plugins.json`）
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LedgerEntry {
    pub name: String,
    pub version: String,
    pub integrity: String,
    #[serde(default)]
    pub installed_at: String,
    /// v1 恒为 "market"（唯一安装入口）。
    pub source: String,
    /// 090-4 v5：用户停用。停用的插件启动扫描跳过（不装载不绘制），运行态
    /// 由市场【已安装】页签的启停开关置位。缺省 false，旧台账直读兼容。
    #[serde(default)]
    pub disabled: bool,
}

/// 读台账；文件不存在/损坏 = 空台账（损坏不报错，下次安装覆盖写回）。
pub fn load_ledger() -> Vec<LedgerEntry> {
    let Some(f) = crate::paths::plugins_ledger_file() else {
        return Vec::new();
    };
    let Ok(s) = std::fs::read_to_string(f) else {
        return Vec::new();
    };
    serde_json::from_str(&s).unwrap_or_default()
}

pub(super) fn save_ledger(entries: &[LedgerEntry]) -> Result<(), String> {
    let f = crate::paths::plugins_ledger_file().ok_or("无法定位插件台账路径")?;
    if let Some(dir) = f.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    std::fs::write(f, s).map_err(|e| e.to_string())
}

pub(super) fn ledger_upsert(entry: LedgerEntry) -> Result<(), String> {
    let mut all = load_ledger();
    all.retain(|e| e.name != entry.name);
    all.push(entry);
    save_ledger(&all)
}

pub(super) fn ledger_remove(name: &str) -> Result<(), String> {
    let mut all = load_ledger();
    all.retain(|e| e.name != name);
    save_ledger(&all)
}

/// 某插件已装版本（未装 = None）。
pub fn installed_version(name: &str) -> Option<String> {
    load_ledger()
        .into_iter()
        .find(|e| e.name == name)
        .map(|e| e.version)
}

/// 启停落盘（090-4 v5）：只翻台账的 disabled 位；台账缺该包 = Err。
/// 停用语义在消费端——启动扫描跳过、运行态 entry.disabled 置位后
/// 状态栏 tab / 文件树插件菜单 / 分发全部不再出现该插件。
pub fn set_disabled(name: &str, disabled: bool) -> Result<(), String> {
    let mut all = load_ledger();
    let Some(entry) = all.iter_mut().find(|e| e.name == name) else {
        return Err(format!("台账中无插件 {name}"));
    };
    if entry.disabled == disabled {
        return Ok(());
    }
    entry.disabled = disabled;
    save_ledger(&all)
}

mod index;
mod install;
mod migrate;

pub use index::{dir_aliases, dir_name, index_url, verify_signed_index};
pub use install::{
    extract, install, install_prepare_hook, plugin_cache_for, sweep_orphans, uninstall,
};
pub use migrate::{migrate_data_dirs, prune_old_versions, prune_stale_versions};

#[cfg(test)]
mod dir_alias_tests;
#[cfg(test)]
mod install_prepare_tests;
#[cfg(test)]
mod migrate_data_tests;
#[cfg(test)]
mod signed_index_tests;
#[cfg(test)]
mod uninstall_tests;
