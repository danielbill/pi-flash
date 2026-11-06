//! PF 插件清单（02 §2；手写极简解析，不引 toml 库——设计定案）。
//!
//! 文件组织（090 超规拆分）：类型在本文件（[`PluginManifest`] + 8 贡献点 +
//! `PluginDock`）；解析在 [`parse`]（TomlVal/parse_val/parse_manifest）、
//! 启动扫描在 [`scan`]（scan_manifests/scan_root/失效包）、插件资产注册表在
//! [`assets`]（PLUGIN_ASSETS/DIRS）；tests 在 [`tests`]。090 重构自 mod.rs 拆出。

mod assets;
mod parse;
mod scan;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_scan;
mod val;

pub(crate) use assets::*;
pub(crate) use parse::*;
pub(crate) use scan::*;

use std::path::PathBuf;

// ---------------------------------------------------------------------------
// 清单类型（02 §2）
// ---------------------------------------------------------------------------

/// 本版 PF 支持的清单契约大版本（与插件仓 schema/plugin-contract.json 的
/// `manifest_schema` 对齐；加/删原语时一并升）
pub(crate) const SUPPORTED_MANIFEST_SCHEMA: u32 = 5;

#[derive(Debug, Clone)]
pub(crate) struct PluginManifest {
    /// 安装目录 = `<数据目录>/plugins/<pkg>/[<version>/]`
    pub dir: PathBuf,
    /// 顶层包目录 `<数据目录>/plugins/<pkg>/`——平铺 dev 布局 == `dir`，
    /// market 版本布局 == `dir` 的父目录。**用户数据只挂这里**（90-4：
    /// `plugins/<id>/data/` 无版本维度）；wasm / assets / .cache / 入口文件
    /// 仍随版本目录 `dir`。版本换代 data 不丢——`data_dir()` 唯一派生口。
    pub pkg_dir: PathBuf,
    /// 顶层包目录名（`plugins/<pkg>/`）——台账 name 经 `pf_plugins::dir_name`
    /// 转写的形态。**运行态条目 ↔ 台账条目对账靠它**（停用/卸载），版本层
    /// 目录名是版本号，不能当身份用（090-4 v5 踩过：停用形同虚设）。
    pub pkg: String,
    pub id: String,
    pub title_zh: String,
    pub title_en: Option<String>,
    pub version: String,
    pub status_bar: Option<StatusBarContrib>,
    /// topbar「菜单」三横浮层条目申请（契约 v2.5；props 同 status_bar）。
    /// 点击语义与 status_bar tab 同构（按 panels[].dock 解释形态）。
    pub topbar_menu: Option<TopbarMenuContrib>,
    /// 会话标签功能栏注册槽（契约 v3）：每个会话标签一个插件 icon，
    /// chrome 由插件导出 fn_bar(ctx, env) 给出；点击回环 command（env 带 session）。
    pub fn_bar: Option<FnBarContrib>,
    /// 非模态弹窗（契约 v3 popups）：内容由插件导出 render_popup(ctx) 给出；
    /// 开/关走 ctx.act.openPopup / closePopup。
    pub popups: Vec<PopupContrib>,
    pub file_menu: Vec<FileMenuContrib>,
    pub panels: Vec<PanelContrib>,
    /// 内层 wasm 插件（契约 inner_package）：`wasm = "plugin.wasm"` 声明即启用；
    /// 装载 = pf_plugin_host::PluginInstance（init 即验签门，090-5 L3）
    pub wasm: Option<String>,
    /// 分发态 JS **字节码**（`plugin.qbc`，由 `qjsc` 从同一份 main.js 编出）：
    /// 与源码包同一引擎、同一 ctx、同一渲染路径，落盘无明文源码。
    pub bytecode: Option<String>,
    /// 包内 pi 扩展文件（契约 v2.4/v3 contributes.pi_extensions，相对包根）：
    /// v3 镜像协议的 ext 挂载面（fnbar 点击 → 写会话清单 → 空闲重启挂载）。
    pub pi_extensions: Vec<String>,
    /// 执行域实例声明（契约 v4 contributes.service，90-6）：scope 本版仅 session。
    /// 宿主托管身份/通道/生命周期/存储；spawn 挂宿主自带 shim（Tier 2 验签门照旧）。
    pub service: Vec<ServiceContrib>,
    /// 认领前缀声明（契约 v2.4 contributes.sessions）：插件建线时经
    /// ctx.newDraft 显式给 prefix，此处声明是清单面对账位（缺段解析即拒）。
    pub sessions: Vec<SessionSlotContrib>,
}

#[derive(Debug, Clone)]
pub(crate) struct StatusBarContrib {
    /// 三来源之一：宿主内置名 / `assets:<名>` / `assets/` 相对路径
    pub icon: String,
    pub title_zh: String,
}

/// topbar「菜单」三横浮层条目（契约 v2.5；一个插件至多一条）。
#[derive(Debug, Clone)]
pub(crate) struct TopbarMenuContrib {
    pub icon: String,
    pub title_zh: String,
}

/// 执行域实例声明（契约 v4 contributes.service；90-6）。
#[derive(Debug, Clone)]
pub(crate) struct ServiceContrib {
    /// 插件内唯一 id（kebab-case）
    pub id: String,
    /// 本版仅 "session"
    pub scope: String,
    /// 执行面入口（相对包根；会话进程内运行）
    pub entry: String,
}

/// 会话功能栏注册槽（契约 v3 contributes.fn_bar；一个插件至多一条）。
#[derive(Debug, Clone)]
pub(crate) struct FnBarContrib {
    /// 点击回环命令 id（ctx.onCommand；回调 env 带 {session:{key}}）
    pub command: String,
}

/// 非模态弹窗（契约 v3 contributes.popups）。
#[derive(Debug, Clone)]
pub(crate) struct PopupContrib {
    pub id: String,
    pub title_zh: String,
    pub title_en: Option<String>,
    /// 尺寸档位 SM/MD/LG（MD≈500x500；实现值归宿主，插件不写像素）
    pub size: String,
    /// 逻辑入口（固定 main.js；wasm 面不读）
    pub entry: String,
}

/// 认领槽声明（契约 v2.4 contributes.sessions）。
#[derive(Debug, Clone)]
pub(crate) struct SessionSlotContrib {
    pub prefix: String,
}

#[derive(Debug, Clone)]
pub(crate) struct FileMenuContrib {
    pub id: String,
    pub title_zh: String,
    pub title_en: Option<String>,
    /// 宿主内置图标名（01 规范 §3.4）；缺省 puzzle（插件通用）
    pub icon: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PanelContrib {
    pub id: String,
    pub title_zh: String,
    /// 逻辑入口（02 §2；固定 main.js）
    pub entry: String,
    /// 展示形态（契约 v2.4）：dock 面板（缺省）或内容区预览页签
    pub dock: PluginDock,
}

/// 插件面板展示形态（契约 v2.4 `panels[].dock`）。平台语义命名，
/// 宿主不解释哪个插件用什么形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PluginDock {
    /// 左列 dock 面板（缺省，老插件零迁移）
    Panel,
    /// 内容区预览页签（单例、整页声明式树；认领会话线模式随会话槽交付）
    Tab,
}

impl PluginManifest {
    /// 插件私有数据目录 `<pkg_dir>/data`（90-4 §2：PF 建目录、插件读写、
    /// PF 不解析）。所有用户数据派生（plugin-ctx dataDir / JS ctx.dir /
    /// data/svc / claim.json）都从这里取，**不要**再 `dir.join("data")`——
    /// 那会把数据挂到版本目录，更新一次丢一次（090-10 迁移体验）。
    pub(crate) fn data_dir(&self) -> PathBuf {
        self.pkg_dir.join("data")
    }

    /// 声明为预览页签的面板（契约 v2.4 dock="tab"）。当前取第一个：
    /// 多 page 页签支持随认领会话线（W5）一起开。
    pub(crate) fn tab_page(&self) -> Option<(&str, &str)> {
        self.panels
            .iter()
            .find(|p| p.dock == PluginDock::Tab)
            .map(|p| (self.pkg.as_str(), p.id.as_str()))
    }
}

/// 当前语言取标题（i18n 0=简中 1=繁中 2=EN；繁中走 zh 文案）。
pub(crate) fn pick_lang(zh: &str, en: &Option<String>) -> String {
    match crate::i18n::lang_ix() {
        2 => en.clone().unwrap_or_else(|| zh.to_string()),
        _ => zh.to_string(),
    }
}
