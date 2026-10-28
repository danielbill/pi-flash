//! PF 插件宿主接线（beads 360；契约 docs/PF组件规范/01+02，设计 090-4）。
//!
//! 职责（02 §6 责任清单）：①启动扫 `<数据目录>/plugins/*/pf-plugin.toml`
//! （极简解析，坏清单跳过记日志）；②状态栏 tab（status_bar.rs 消费）；
//! ③文件树右键【插件 → …】（psp_overlays.rs 消费）；④面板渲染（panel.rs，
//! 组件树先硬编码快捷面板这棵，解析器后续抽象）；⑤ctx/懒加载/异常隔离
//! （本文件，引擎 = `pf_plugin_host::js::JsPlugin`）。
//!
//! 时序（02 §4）：清单启动读（spawn_plugin_scan，后台），ctx 首用时
//! 建；插件异常 → toast + 会话级禁用，不影响宿主。

mod broadcast;
pub(crate) mod panel;
mod wasm_hooks;

pub(crate) use wasm_hooks::spawn_precompile_installed;
use wasm_hooks::{
    ensure_wasm_hooks, has_official_proof, host_proof_material, install_prepare,
    verify_wasm_plugins,
};

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

use gpui::{AppContext, MouseButton, SharedString, div, prelude::*, px, rgb};
use pf_plugin_host::js::{JsAction, JsHost, JsPlugin};
use pf_plugin_host::{PluginAction, PluginInstance};
use serde_json::{Value, json};

use crate::Chat;
use crate::ui::tooltip::WithTip;

// ---------------------------------------------------------------------------
// 清单（02 §2；手写极简解析，不引 toml 库——设计定案）
// ---------------------------------------------------------------------------

/// 本版 PF 支持的清单契约大版本（与插件仓 schema/plugin-contract.json 的
/// `manifest_schema` 对齐；加/删原语时一并升）
pub(crate) const SUPPORTED_MANIFEST_SCHEMA: u32 = 3;

#[derive(Debug, Clone)]
pub(crate) struct PluginManifest {
    /// 安装目录 = `<数据目录>/plugins/<pkg>/[<version>/]`
    pub dir: PathBuf,
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
    /// 包内 pi 扩展文件（契约 v2.4 contributes.pi_extensions，相对包根）。
    /// 认领会话 spawn 时以 `-e` 挂入，插件逻辑在会话进程内运行。
    /// **Tier 2 门**：仅官方验签包可携带（安装链执行，见 002 §W5）。
    pub pi_extensions: Vec<String>,
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

// ---- 值形态 ----

#[derive(Debug, Clone)]
enum TomlVal {
    Str(String),
    /// `{ zh = "…", en = "…" }`
    I18n {
        zh: String,
        en: Option<String>,
    },
    /// 裸字面量（数字 / bool）
    Raw(String),
}

impl TomlVal {
    fn as_str(&self, what: &str, line: usize) -> Result<String, String> {
        match self {
            TomlVal::Str(s) => Ok(s.clone()),
            _ => Err(format!("pf-plugin.toml 行 {}：{what} 需为字符串", line + 1)),
        }
    }
    fn as_i18n(&self, what: &str, line: usize) -> Result<(String, Option<String>), String> {
        match self {
            TomlVal::I18n { zh, en } => Ok((zh.clone(), en.clone())),
            TomlVal::Str(s) => Ok((s.clone(), None)), // 宽松：纯字符串当 zh
            _ => Err(format!(
                "pf-plugin.toml 行 {}：{what} 需为文案对象",
                line + 1
            )),
        }
    }
    fn as_raw(&self, what: &str, line: usize) -> Result<String, String> {
        match self {
            TomlVal::Raw(s) => Ok(s.clone()),
            _ => Err(format!("pf-plugin.toml 行 {}：{what} 非法取值", line + 1)),
        }
    }
}

/// check_arch 的 fn 行数统计是括号裸计数：正文里的字符面量 `'{'`/`'}'`
/// 会把计数带偏（parse_val 曾被计成 652 行）。集中在此，正文不出现裸花括号。
const LBRACE: char = '{';
const RBRACE: char = '}';

/// 解析一行值：`"…"` / `{ zh = "…", en = "…" }` / 裸字面量。
fn parse_val(v: &str, line: usize) -> Result<TomlVal, String> {
    let v = v.trim();
    if let Some(rest) = v.strip_prefix('"') {
        let end = rest
            .rfind('"')
            .ok_or(format!("pf-plugin.toml 行 {}：字符串未闭合", line + 1))?;
        if rest[end + 1..].trim() != "" {
            return Err(format!(
                "pf-plugin.toml 行 {}：字符串后有多余内容",
                line + 1
            ));
        }
        return Ok(TomlVal::Str(
            rest[..end].replace("\\\"", "\"").replace("\\\\", "\\"),
        ));
    }
    if v.starts_with(LBRACE) {
        let inner = v
            .strip_prefix(LBRACE)
            .and_then(|s| s.strip_suffix(RBRACE))
            .ok_or(format!("pf-plugin.toml 行 {}：文案对象未闭合", line + 1))?;
        let mut zh = None;
        let mut en = None;
        // 极简扫描：逐个 `key = "value"` 对（值内不含引号——清单文案域够用）
        let mut rest = inner.trim();
        while !rest.is_empty() {
            let eq = rest
                .find('=')
                .ok_or(format!("pf-plugin.toml 行 {}：文案对象缺 =", line + 1))?;
            let key = rest[..eq].trim().trim_matches(',').to_string();
            let after = rest[eq + 1..].trim();
            if !after.starts_with('"') {
                return Err(format!(
                    "pf-plugin.toml 行 {}：文案对象值需为字符串",
                    line + 1
                ));
            }
            let close = after[1..].find('"').ok_or(format!(
                "pf-plugin.toml 行 {}：文案对象字符串未闭合",
                line + 1
            ))?;
            let val = after[1..1 + close].to_string();
            match key.as_str() {
                "zh" => zh = Some(val),
                "en" => en = Some(val),
                other => {
                    return Err(format!(
                        "pf-plugin.toml 行 {}：未知文案键 {other}",
                        line + 1
                    ));
                }
            }
            rest = after[1 + close + 1..].trim_start_matches(',');
            rest = rest.trim();
        }
        let Some(zh) = zh else {
            return Err(format!(
                "pf-plugin.toml 行 {}：文案对象缺 zh（U7）",
                line + 1
            ));
        };
        return Ok(TomlVal::I18n { zh, en });
    }
    if v.is_empty() || v.contains('"') || v.contains(LBRACE) {
        return Err(format!("pf-plugin.toml 行 {}：无法解析的值", line + 1));
    }
    Ok(TomlVal::Raw(v.to_string()))
}

fn is_kebab(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.chars().last().is_some_and(|c| c.is_ascii_alphanumeric())
}

/// 剥行内注释：引号外的 `#` 起截断（02 §2 示例清单自带行内注释，必须支持；
/// 引号内的 `#` 是文案内容，保留）。
fn strip_inline_comment(line: &str) -> &str {
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_str => escaped = true,
            '"' => in_str = !in_str,
            '#' if !in_str => return line[..i].trim_end(),
            _ => {}
        }
    }
    line
}

/// 解析 + 校验单个插件目录的 pf-plugin.toml（02 §2 校验规则）。
fn parse_manifest(dir: &Path) -> Result<PluginManifest, String> {
    let path = dir.join("pf-plugin.toml");
    let src =
        std::fs::read_to_string(&path).map_err(|e| format!("{} 不可读: {e}", path.display()))?;

    let mut m = PluginManifest {
        dir: dir.to_path_buf(),
        pkg: String::new(), // 扫描期回填（顶层包目录名）
        id: String::new(),
        title_zh: String::new(),
        title_en: None,
        version: String::new(),
        status_bar: None,
        topbar_menu: None,
        fn_bar: None,
        popups: Vec::new(),
        file_menu: Vec::new(),
        panels: Vec::new(),
        pi_extensions: Vec::new(),
        sessions: Vec::new(),
        wasm: None,
        bytecode: None,
    };
    let mut section = String::new();
    for (n, raw) in src.lines().enumerate() {
        let line = strip_inline_comment(raw.trim());
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("[[") {
            let name = rest
                .strip_suffix("]]")
                .ok_or(format!("pf-plugin.toml 行 {}：段头未闭合", n + 1))?;
            section = match name.trim() {
                "contributes.status_bar" => {
                    if m.status_bar
                        .replace(StatusBarContrib {
                            icon: String::new(),
                            title_zh: String::new(),
                        })
                        .is_some()
                    {
                        return Err("pf-plugin.toml：status_bar 只能声明一个".into());
                    }
                    "status_bar".to_string()
                }
                "contributes.file_menu" => {
                    m.file_menu.push(FileMenuContrib {
                        id: String::new(),
                        title_zh: String::new(),
                        title_en: None,
                        icon: "puzzle".into(),
                    });
                    "file_menu".to_string()
                }
                "contributes.topbar_menu" => {
                    if m.topbar_menu
                        .replace(TopbarMenuContrib {
                            icon: String::new(),
                            title_zh: String::new(),
                        })
                        .is_some()
                    {
                        return Err("pf-plugin.toml：topbar_menu 只能声明一个".into());
                    }
                    "topbar_menu".to_string()
                }
                "contributes.fn_bar" => {
                    if m.fn_bar
                        .replace(FnBarContrib {
                            command: String::new(),
                        })
                        .is_some()
                    {
                        return Err("pf-plugin.toml：fn_bar 只能声明一个".into());
                    }
                    "fn_bar".to_string()
                }
                "contributes.popups" => {
                    m.popups.push(PopupContrib {
                        id: String::new(),
                        title_zh: String::new(),
                        title_en: None,
                        size: "MD".into(),
                        entry: "main.js".into(),
                    });
                    "popups".to_string()
                }
                "contributes.panels" => {
                    m.panels.push(PanelContrib {
                        id: String::new(),
                        title_zh: String::new(),
                        entry: "main.js".into(),
                        dock: PluginDock::Panel,
                    });
                    "panels".to_string()
                }
                "contributes.pi_extensions" => {
                    m.pi_extensions.push(String::new());
                    "pi_extensions".to_string()
                }
                "contributes.sessions" => {
                    m.sessions.push(SessionSlotContrib {
                        prefix: String::new(),
                    });
                    "sessions".to_string()
                }
                other => return Err(format!("pf-plugin.toml 行 {}：未知段 [[{other}]]", n + 1)),
            };
            continue;
        }
        if line.starts_with('[') {
            return Err(format!(
                "pf-plugin.toml 行 {}：不支持 [table] 段（用 [[contributes.*]]）",
                n + 1
            ));
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(format!("pf-plugin.toml 行 {}：缺 =", n + 1));
        };
        let k = k.trim();
        let val = parse_val(v, n)?;
        match (section.as_str(), k) {
            ("", "schema") => {
                // 契约大版本协商（schema/plugin-contract.json 的 manifest_schema）：
                // 高于宿主支持的版本一律拒装，**不降级运行**
                let got: u32 = val
                    .as_raw("schema", n)?
                    .parse()
                    .map_err(|_| format!("pf-plugin.toml 行 {}：schema 必须是整数", n + 1))?;
                if got != SUPPORTED_MANIFEST_SCHEMA {
                    return Err(format!(
                        "pf-plugin.toml：清单 schema {got} 不被本版 PF 支持（只支持 {SUPPORTED_MANIFEST_SCHEMA}）"
                    ));
                }
            }
            ("", "id") => m.id = val.as_str("id", n)?,
            ("", "version") => m.version = val.as_str("version", n)?,
            ("", "wasm") => m.wasm = Some(val.as_str("wasm", n)?),
            ("", "bytecode") => m.bytecode = Some(val.as_str("bytecode", n)?),
            ("", "title") => {
                let (zh, en) = val.as_i18n("title", n)?;
                m.title_zh = zh;
                m.title_en = en;
            }
            ("status_bar", "icon") => {
                m.status_bar.as_mut().unwrap().icon = val.as_str("icon", n)?
            }
            ("status_bar", "title") => {
                m.status_bar.as_mut().unwrap().title_zh = val.as_i18n("title", n)?.0
            }
            ("topbar_menu", "icon") => {
                m.topbar_menu.as_mut().unwrap().icon = val.as_str("icon", n)?
            }
            ("topbar_menu", "title") => {
                m.topbar_menu.as_mut().unwrap().title_zh = val.as_i18n("title", n)?.0
            }
            ("fn_bar", "command") => {
                m.fn_bar.as_mut().unwrap().command = val.as_str("command", n)?
            }
            ("popups", "id") => m.popups.last_mut().unwrap().id = val.as_str("id", n)?,
            ("popups", "title") => {
                let (zh, en) = val.as_i18n("title", n)?;
                let p = m.popups.last_mut().unwrap();
                p.title_zh = zh;
                p.title_en = en;
            }
            ("popups", "size") => {
                let s = val.as_str("size", n)?;
                if !matches!(s.as_str(), "SM" | "MD" | "LG") {
                    return Err(format!(
                        "pf-plugin.toml 行 {}：popups.size 只认 \"SM\" | \"MD\" | \"LG\"，得到 {s}",
                        n + 1
                    ));
                }
                m.popups.last_mut().unwrap().size = s;
            }
            ("popups", "entry") => m.popups.last_mut().unwrap().entry = val.as_str("entry", n)?,
            ("file_menu", "id") => m.file_menu.last_mut().unwrap().id = val.as_str("id", n)?,
            ("file_menu", "title") => {
                let (zh, en) = val.as_i18n("title", n)?;
                let fm = m.file_menu.last_mut().unwrap();
                fm.title_zh = zh;
                fm.title_en = en;
            }
            ("file_menu", "icon") => {
                m.file_menu.last_mut().unwrap().icon = val.as_str("icon", n)?
            }
            ("panels", "id") => m.panels.last_mut().unwrap().id = val.as_str("id", n)?,
            ("panels", "title") => {
                m.panels.last_mut().unwrap().title_zh = val.as_i18n("title", n)?.0
            }
            ("panels", "layout") => {
                return Err(format!(
                    "pf-plugin.toml 行 {}：字段 layout 已废弃（契约 v2，见 02 §2）",
                    n + 1
                ));
            }
            ("panels", "store") => {
                return Err(format!(
                    "pf-plugin.toml 行 {}：字段 store 已废弃，数据由插件自管（ctx.dir，见 02 §3）",
                    n + 1
                ));
            }
            ("panels", "entry") => m.panels.last_mut().unwrap().entry = val.as_str("entry", n)?,
            ("panels", "dock") => {
                m.panels.last_mut().unwrap().dock = match val.as_str("dock", n)?.as_str() {
                    "panel" => PluginDock::Panel,
                    "tab" => PluginDock::Tab,
                    other => {
                        return Err(format!(
                            "pf-plugin.toml 行 {}：dock 只认 \"panel\" | \"tab\"，得到 {other}",
                            n + 1
                        ));
                    }
                };
            }
            ("pi_extensions", "path") => {
                *m.pi_extensions.last_mut().unwrap() = val.as_str("path", n)?;
            }
            ("sessions", "prefix") => {
                *m.sessions.last_mut().unwrap() = SessionSlotContrib {
                    prefix: val.as_str("prefix", n)?,
                };
            }
            (sec, key) => return Err(format!("pf-plugin.toml 行 {}：{sec} 段未知键 {key}", n + 1)),
        }
    }

    // ---- 校验 ----
    if m.id.is_empty() {
        return Err("pf-plugin.toml：缺 id".into());
    }
    if !is_kebab(&m.id) {
        return Err(format!("pf-plugin.toml：id「{}」非 kebab-case", m.id));
    }
    // 目录名对账（02 §6 布局两档）：平铺 dev = `<id>/`；market 安装 =
    // `@<scope>__<id>/<version>/`。版本层取父目录名；scope 前缀剥掉后对账。
    let mut base = dir.file_name().and_then(|s| s.to_str()).unwrap_or_default();
    if base.starts_with(|c: char| c.is_ascii_digit())
        && let Some(p) = dir
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
    {
        base = p; // 版本目录 → 上钻到插件目录
    }
    let expected = base.rsplit("__").next().unwrap_or(base);
    if m.id != expected {
        return Err(format!("pf-plugin.toml：id「{}」≠ 目录名「{base}」", m.id));
    }
    if m.title_zh.is_empty() {
        return Err("pf-plugin.toml：缺 title.zh（U7）".into());
    }
    if m.version.is_empty() {
        return Err("pf-plugin.toml：缺 version".into());
    }
    // 内层 wasm 插件（090-5 L3）无 contributes 也合法——验签在 wasm 内
    if m.wasm.is_none()
        && m.bytecode.is_none()
        && m.status_bar.is_none()
        && m.file_menu.is_empty()
        && m.panels.is_empty()
        && m.fn_bar.is_none()
        && m.popups.is_empty()
    {
        return Err("纯库包（没申请任何进驻面），不加载".into());
    }
    if let Some(sb) = &m.status_bar {
        if sb.icon.is_empty() {
            return Err("pf-plugin.toml：status_bar 缺 icon".into());
        }
        if sb.icon.starts_with("assets/") && !dir.join(&sb.icon).is_file() {
            return Err(format!(
                "pf-plugin.toml：status_bar.icon「{}」不存在（M6）",
                sb.icon
            ));
        }
    }
    let mut panel_ids = std::collections::HashSet::new();
    for p in &m.panels {
        if !is_kebab(&p.id) {
            return Err(format!(
                "pf-plugin.toml：panel id「{}」非 kebab-case（M5）",
                p.id
            ));
        }
        if !panel_ids.insert(p.id.clone()) {
            return Err(format!("pf-plugin.toml：panel id「{}」重复（M5）", p.id));
        }
        if p.title_zh.is_empty() {
            return Err(format!(
                "pf-plugin.toml：panel「{}」缺 title.zh（M5）",
                p.id
            ));
        }
        // 内层 wasm 包面板无 entry 文件（渲染面 = wasm render），跳过文件校验
        if m.wasm.is_none() && m.bytecode.is_none() && !dir.join(&p.entry).is_file() {
            return Err(format!(
                "pf-plugin.toml：entry「{}」不存在（02 §2）",
                p.entry
            ));
        }
    }
    for fm in &m.file_menu {
        if !is_kebab(&fm.id) || fm.title_zh.is_empty() {
            return Err(format!(
                "pf-plugin.toml：file_menu「{}」缺 id/title.zh",
                fm.id
            ));
        }
    }
    // 契约 v3：fn_bar / popups 校验
    if let Some(fb) = &m.fn_bar {
        if !is_kebab(&fb.command) {
            return Err(format!(
                "pf-plugin.toml：fn_bar.command「{}」非 kebab-case",
                fb.command
            ));
        }
    }
    let mut popup_ids = std::collections::HashSet::new();
    for p in &m.popups {
        if !is_kebab(&p.id) {
            return Err(format!("pf-plugin.toml：popup id「{}」非 kebab-case", p.id));
        }
        if !popup_ids.insert(p.id.clone()) {
            return Err(format!("pf-plugin.toml：popup id「{}」重复", p.id));
        }
        // title 可空：空 = 不画标题栏（老微信弹窗无标题栏，用户 2026-10-28 定夺）
        if m.wasm.is_none() && m.bytecode.is_none() && !dir.join(&p.entry).is_file() {
            return Err(format!(
                "pf-plugin.toml：popups entry「{}」不存在（02 §2）",
                p.entry
            ));
        }
    }
    Ok(m)
}

/// 扫描 `<数据目录>/plugins/*/` 只出清单（后台线程可调——PluginEntry 持
/// 非 Send 的 JsPlugin，条目装配必须在主线程做）；坏清单 eprintln 跳过。
///
/// **装载白名单 = 台账**（唯一安装真相）：只有台账里登记且未停用的包才装载，
/// 磁盘上多出来的目录（手拷 / 卸载残留 / 历史野包）一律不装载——否则就会
/// 「市场说未安装、左面板却挂着它」（090-4 v5b 踩过）。
pub(crate) fn scan_manifests() -> Vec<PluginManifest> {
    let Some(root) = pi_link::paths::plugins_dir() else {
        return Vec::new();
    };
    // 台账包名 → 目录别名（`@scope__name` 与平铺 `name` 都认）
    let enabled: std::collections::HashSet<String> = pi_link::pf_plugins::load_ledger()
        .into_iter()
        .filter(|l| !l.disabled)
        .flat_map(|l| pi_link::pf_plugins::dir_aliases(&l.name))
        .collect();
    let mut out = Vec::new();
    let mut candidates = scan_candidates(&root);
    candidates.sort_by(|a, b| a.1.cmp(&b.1));
    for (pkg, dir) in candidates {
        // 准入/停用判定必须看**顶层包目录名**：版本层目录名是版本号，拿它比
        // 包名永远比不中（停用过滤失效 → 重启后照旧装载，090-4 v5 踩过）
        if !enabled.contains(&pkg) {
            continue; // 未安装（台账无此包）或已停用，不装载不绘制
        }
        match parse_manifest(&dir) {
            Ok(mut m) => {
                // 内层 wasm 插件无 contributes 也合法（验签在 wasm 内，090-5 L3）
                if m.wasm.is_none()
                    && m.bytecode.is_none()
                    && m.status_bar.is_none()
                    && m.file_menu.is_empty()
                    && m.panels.is_empty()
                    && m.fn_bar.is_none()
                    && m.popups.is_empty()
                {
                    continue; // 纯库包
                }
                m.pkg = pkg;
                register_plugin_dir(&m.id, &dir);
                out.push(m);
            }
            Err(e) => eprintln!("[pf-plugin] 跳过 {}：{e}", dir.display()),
        }
    }
    out
}

/// 扫插件根目录出 `(顶层包目录名, 清单所在目录)` 候选。
///
/// 布局两档：`plugins/<pkg>/pf-plugin.toml`（dev 手工平铺）与
/// `plugins/<pkg>/<version>/pf-plugin.toml`（market 安装链落盘，带版本层）。
/// 顶层没有清单就下钻一层收版本目录。**顶层包目录名才是可对账的身份**，
/// 下钻出来的版本目录只是装载位置。
fn scan_candidates(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    rd.flatten()
        .flat_map(|e| {
            let d = e.path();
            if !d.is_dir() {
                return Vec::new();
            }
            let Some(pkg) = d.file_name().map(|f| f.to_string_lossy().into_owned()) else {
                return Vec::new();
            };
            if d.join("pf-plugin.toml").is_file() {
                return vec![(pkg, d)];
            }
            std::fs::read_dir(&d)
                .map(|rd| {
                    rd.flatten()
                        .map(|v| v.path())
                        .filter(|v| v.is_dir() && v.join("pf-plugin.toml").is_file())
                        .map(|v| (pkg.clone(), v))
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 插件资产（状态栏 icon）：gpui AssetSource 走磁盘，内存缓存
// ---------------------------------------------------------------------------

static PLUGIN_ASSETS: OnceLock<Mutex<HashMap<String, Option<Vec<u8>>>>> = OnceLock::new();

/// 插件 id → 实际安装目录（扫描时登记）。market 版本层布局下资产不在
/// `plugins/<id>/` 平铺位，解析走这里。
static PLUGIN_DIRS: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();

fn register_plugin_dir(id: &str, dir: &Path) {
    if let Ok(mut c) = PLUGIN_DIRS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        c.insert(id.to_string(), dir.to_path_buf());
    }
}

/// 卸载后清插件 id 的进程级残留：目录登记 + 资产缓存（否则同名重装读到
/// 旧字节 / 旧路径，图标永远是死链）。
pub(crate) fn plugin_forget(id: &str) {
    if let Ok(mut c) = PLUGIN_DIRS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        c.remove(id);
    }
    if let Ok(mut c) = PLUGIN_ASSETS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        let prefix = format!("plugins/{id}/");
        c.retain(|k, _| !k.starts_with(&prefix));
    }
}

/// `rel` = `<slug>/<assets 相对路径>`（assets.rs 的 `plugins/` 前缀段）。
pub(crate) fn plugin_asset(rel: &str) -> Option<Vec<u8>> {
    let cache = PLUGIN_ASSETS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(hit) = cache.lock() {
        if let Some(hit) = hit.get(rel) {
            return hit.clone();
        }
    }
    // 平铺位优先（dev 手工拷贝）；未中则查扫描登记的实际目录（版本层布局）
    let mut bytes = pi_link::paths::plugins_dir().and_then(|d| std::fs::read(d.join(rel)).ok());
    if bytes.is_none()
        && let Some((id, rest)) = rel.split_once('/')
        && let Some(dir) = PLUGIN_DIRS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .ok()
            .and_then(|c| c.get(id).cloned())
    {
        bytes = std::fs::read(dir.join(rest)).ok();
    }
    if let Some(b) = &bytes {
        eprintln!("[pf-plugin] asset {rel} -> {} bytes", b.len());
    } else {
        // 未找到不缓存：扫描登记晚于首帧渲染时，下帧重查即中
        eprintln!("[pf-plugin] asset {rel} -> 未找到");
        return None;
    }
    if let Ok(mut c) = cache.lock() {
        c.insert(rel.to_string(), bytes.clone());
    }
    bytes
}

// ---------------------------------------------------------------------------
// Chat 侧状态 + ctx 宿主实现
// ---------------------------------------------------------------------------

/// 单个插件条目：清单 + 懒加载运行时 + 面板行缓存。
pub(crate) struct PluginEntry {
    pub manifest: PluginManifest,
    /// 会话级禁用（插件逻辑抛异常后置位；02 §4）
    pub disabled: bool,
    /// QuickJS 运行时（懒建；主线程独占）
    pub js: Option<JsPlugin>,
    /// 内层 wasm 实例（加密包；懒装载——首次开面板时主线程 `load_cached`
    /// 毫秒级实例化，cwasm 缓存由启动预热。验签门在启动后台线程已过一次，
    /// 这里 init 再跑一遍是全新实例的自拒门，语义不变）
    pub wasm: Option<PluginInstance>,
    /// wasm 面板键盘焦点（点击面板时抢入；键盘回灌只在其聚焦时收）
    pub wasm_focus: Option<gpui::FocusHandle>,
    /// 宿主能力载体（与 js 同生；fire 前清 actions）
    pub host: Option<Rc<RefCell<ChatHost>>>,
    /// 最近一次 render 的组件树（01 规范；渲染器消费。v2 无 rows）
    pub body: Option<serde_json::Value>,
    pub tree_loaded: bool,
    /// 右键菜单弹层（None = 收起；面板渲染时消费）
    pub cell_menu: Option<crate::pf_plugin::panel::CellMenuState>,
    /// 分区比例（宿主 UI 状态，内存态；重启回等分）
    pub split_ratios: std::cell::RefCell<Vec<f32>>,
    /// 正在拖动分隔线的瞬时状态
    pub split_drag: std::cell::RefCell<Option<crate::pf_plugin::panel::SplitDrag>>,
    /// 悬停中的分隔线序号（§5.12 统一分隔线：hover 与拖拽同款 4px accent。
    /// hover 走字段而非 gpui hover 样式——后者改布局属性不可靠，且与其他
    /// 两处分隔线会成两套逻辑）
    pub split_hover: std::cell::RefCell<Option<usize>>,
    /// 面板滚动（左列格子容器；跨帧存续）
    /// 面板滚动（要求⑤：上下两分栏各自管理；跨帧存续）
    /// 分栏滚动句柄 + psp 滚动条状态（要求⑤：每 group 一对；渲染期按序
    /// 懒扩容——渲染期 &self 内部可变性，先例 at_filter_cache/bubble_scrolls。
    /// 滚动条 = psp_scrollbar：hover 常显 / 3s 淡出 / 无轨道底，平时隐形，
    /// 面板静止即纯居中排版）
    pub section_scrolls: std::cell::RefCell<Vec<gpui::ScrollHandle>>,
}

impl PluginEntry {
    // ---- 能力面取数（8tb.2）：同一签名，载体分发收在实例调用这一处；
    // 调用方（fn_bar chrome / 徽标 / 弹窗）按清单贡献项取数，不按载体分支。
    // JS 动作入 host 队列随返回带走；wasm 动作走 pump（OpenPopup 等意图）。

    /// 契约 v3 fn_bar chrome（JS fn_bar / wasm fn-bar，同形 JSON）。
    /// 返回 (chrome，期间动作)。chrome = None = 本会话无（含未导出/未实现）。
    fn hook_fn_bar(&mut self, env: &str) -> (Result<Option<String>, String>, Vec<JsAction>) {
        if let Some(inst) = self.wasm.as_mut() {
            // chrome 是纯取数面：wasm 动作不在 chrome 期放行（与 JS 同口径，
            // JS 侧例外只有徽标声明，wasm 面无此动作）
            let r = inst.fn_bar_json(env);
            let _ = inst.pump_actions();
            return (r, Vec::new());
        }
        let Some(js) = self.js.as_ref() else {
            return (Ok(None), Vec::new());
        };
        let host = self.host.as_ref().expect("js 载体必有 host").clone();
        host.borrow_mut().actions.clear();
        let r = js.fn_bar_json(env);
        let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
        (r, drained)
    }

    /// PF 钩子 session_row 徽标（JS session_list_render / wasm session-row）。
    fn hook_session_row(&mut self, env: &str) -> (Result<Option<String>, String>, Vec<JsAction>) {
        if let Some(inst) = self.wasm.as_mut() {
            let r = inst.session_row_json(env);
            let _ = inst.pump_actions();
            return (r, Vec::new());
        }
        let Some(js) = self.js.as_ref() else {
            return (Ok(None), Vec::new());
        };
        let host = self.host.as_ref().expect("js 载体必有 host").clone();
        host.borrow_mut().actions.clear();
        let r = js.session_row_json(env);
        let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
        (r, drained)
    }

    /// 契约 v3 弹窗体（JS render_popup / wasm render-popup；wasm 未导出时
    /// 退回 v2 render 面）。返回 (体 JSON，期间动作)。
    fn hook_render_popup(&mut self) -> (Result<String, String>, Vec<JsAction>, Vec<PluginAction>) {
        if let Some(inst) = self.wasm.as_mut() {
            let r = inst.render_popup_json();
            let acts = inst.pump_actions();
            return (r, Vec::new(), acts);
        }
        let Some(js) = self.js.as_ref() else {
            return (Ok("null".to_string()), Vec::new(), Vec::new());
        };
        let host = self.host.as_ref().expect("js 载体必有 host").clone();
        host.borrow_mut().actions.clear();
        let r = js.render_popup_json();
        let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
        (r, drained, Vec::new())
    }
}

/// 会话列表徽标（契约 v3 sessions.setBadge）：落时间位置，owner = 插件 id。
#[derive(Debug, Clone)]
/// PF 钩子 session_list_render 的缓存条目（徽标是渲染结果，缓存只省 JS 调用）。
pub(crate) struct RowCache {
    pub pkg: String,
    pub session_key: String,
    /// None = 本行无徽标（负缓存）
    pub badge: Option<(String, String)>,
    pub at: std::time::Instant,
}

pub(crate) struct PluginBadge {
    pub owner: String,
    pub session: String,
    pub icon: String,
    pub tone: String,
}

/// 弹窗打开态（契约 v3 popups）：一次一个（开新的关旧的）。
#[derive(Debug, Clone)]
pub(crate) struct PluginPopupOpen {
    pub ix: usize,
    pub id: String,
    pub title: String,
    pub body: Option<serde_json::Value>,
    pub tree_loaded: bool,
}

/// fn_bar chrome 缓存（契约 v3）：功能栏每帧渲染，QuickJS 调用不进每帧
/// ——1s TTL + 会话键变更即失效。wasm 面（wit v3 前）不参与。
#[derive(Debug, Clone)]
pub(crate) struct FnBarCache {
    pub pkg: String,
    pub session_key: String,
    pub icon: String,
    pub tone: String,
    pub tip: String,
    pub at: std::time::Instant,
}

/// Chat 上的插件簇（与 aux/ws 等并列的最小簇）。
#[derive(Default)]
pub(crate) struct PluginUi {
    pub plugins: Vec<PluginEntry>,
    /// 组件状态广播的上次已发快照（None = 从未发过/面板刚开，下一拍必发）。
    /// 类型 = broadcast::Snap：(project, model)。见 `broadcast.rs`。
    pub broadcast_last: Option<(Option<String>, Option<(String, String)>)>,
    /// 契约 v3 session 广播的上次已发会话键（差分守门）。
    pub session_last: Option<Option<String>>,
    /// 认领槽：插件实例 × 进行中会话（002 wx_session_id 的泛化；一个插件
    /// 同时只有一条进行中线，新建即换绑）。见 `Claim`。
    pub claims: Vec<Claim>,
    /// 打开中的插件弹窗（契约 v3；None = 无）。
    pub popup: Option<PluginPopupOpen>,
    /// 弹窗轮询代数：每次开/关自增，旧轮询循环见代数不符自行退出
    /// （重复点击不堆叠 1s 轮询）。
    pub popup_gen: u64,
    /// fn_bar chrome 缓存（键 = pkg + session_key，TTL 见 FnBarCache 用法）。
    pub fn_bar_cache: Vec<FnBarCache>,
    /// 会话列表徽标（旧命令式路径，保留字段供存量包；新路径 = session_list_render）
    pub badges: Vec<PluginBadge>,
    /// PF 钩子 session_list_render 的渲染缓存（pkg + 会话键，1s TTL）
    pub row_cache: Vec<RowCache>,
}

/// 插件认领槽记录（匿名归属：宿主只认识 "某插件 × 某 key"）。
#[derive(Debug, Clone)]
pub(crate) struct Claim {
    pub plugin_id: String,
    pub prefix: String,
    pub key: String,
}

/// 认领会话名：`{prefix}YYMMDDHHMMSSmmm`（与 wx 线同款 15 位时间戳口径，
/// 002 §5）。纯函数便于单测。
pub(crate) fn claim_session_name(prefix: &str) -> String {
    format!("{prefix}{}", chrono::Local::now().format("%y%m%d%H%M%S%3f"))
}

/// JsHost 实现（02 §3 提问类同步、动作类入队）。
/// 纯数据，不持 Chat——动作由 Chat 方法在 JS 返回后消费。
pub(crate) struct ChatHost {
    /// 插件私有目录（`plugins/<id>/data/`）——插件自管，宿主不解析内容
    dir: PathBuf,
    pub actions: Vec<JsAction>,
}

impl ChatHost {
    /// `name` 只许是本目录下的文件名：拒绝分隔符、`..`、隐藏文件（02 §3 越界拒绝）
    fn safe_path(&self, name: &str) -> Option<PathBuf> {
        let bad = name.is_empty()
            || name.contains('/')
            || name.contains('\\')
            || name.contains("..")
            || name.starts_with('.');
        if bad {
            eprintln!("[pf-plugin] fs 越界拒绝: {name}");
            return None;
        }
        Some(self.dir.join(name))
    }
}

impl JsHost for ChatHost {
    fn host_proof(&mut self) -> String {
        // 090-5 L3：官方构建由 build.rs 注入（PF_BUILD_PROOF → 常量 PF_HOST_PROOF）；
        // 无注入构建 option_env! 为 None → 占位符（fail-closed，与 fork 自建同款）。
        let proof = option_env!("PF_HOST_PROOF").unwrap_or("PROOF_PLACEHOLDER");
        let kid = option_env!("PF_HOST_PROOF_KID").unwrap_or("");
        serde_json::json!({ "kid": kid, "proof": proof }).to_string()
    }
    fn is_executive(&mut self, path: &str) -> bool {
        crate::services::script_exec::exec_kind(Path::new(path)).is_some()
    }
    fn file_icon(&mut self, path: &str) -> String {
        crate::services::file_icons::get_icon(Path::new(path)).to_string()
    }
    fn dir(&mut self) -> String {
        self.dir.to_string_lossy().into_owned()
    }
    fn fs_read(&mut self, name: &str) -> String {
        // 缺失/不可读 → ""（02 §3：兜底是插件的责任，不是宿主的）
        self.safe_path(name)
            .and_then(|f| std::fs::read_to_string(f).ok())
            .unwrap_or_default()
    }
    fn fs_write(&mut self, name: &str, data: &str) {
        let Some(path) = self.safe_path(name) else {
            return;
        };
        // 原子写 = 临时文件 + rename（插件管内容，原子性归宿主）
        let tmp = path.with_extension("tmp");
        if let Err(e) = (|| -> std::io::Result<()> {
            if let Some(p) = tmp.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::write(&tmp, data.as_bytes())?;
            std::fs::rename(&tmp, &path)
        })() {
            eprintln!("[pf-plugin] fs 写入失败（{}）: {e}", path.display());
        }
    }
    fn fs_list(&mut self) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(&self.dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_file())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out
    }
    fn queue(&mut self, action: JsAction) {
        self.actions.push(action);
    }
}

// ---------------------------------------------------------------------------
// Chat 方法（scan 落位 / 状态栏 tab / 面板打开 / 右键回调 / 动作分派）
// ---------------------------------------------------------------------------

impl Chat {
    /// 启动扫描落位（spawn_plugin_scan 回调）。
    pub(crate) fn plugins_set(&mut self, plugins: Vec<PluginEntry>, cx: &mut gpui::Context<Self>) {
        self.plugins.plugins = plugins;
        cx.notify();
    }

    /// 状态栏插件 tab 点击：左列切到该插件面板（懒加载链 02 §4：
    /// 建 ctx → onPanelOpen → render → 行缓存）。重复点击 = 重新激活（刷新）。
    pub(crate) fn open_plugin_dock(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        // 已停用的插件不开面板（tab 本身已隐藏，这里只是兼作硬门）
        if self.plugins.plugins.get(ix).is_none_or(|e| e.disabled) {
            return;
        }
        self.layout.dock_panel = crate::DockPanel::Plugin(ix);
        // 广播「面板打开首拍」语义：清快照 → 下一拍必发当前状态
        broadcast::reset(&mut self.plugins);
        self.plugin_refresh(ix, cx);
    }

    /// 该插件的（第一个）面板是否声明为内容区预览页签（契约 v2.4 dock）。
    pub(crate) fn plugin_dock_is_tab(&self, ix: usize) -> bool {
        self.plugins
            .plugins
            .get(ix)
            .is_some_and(|e| e.manifest.tab_page().is_some())
    }

    /// 某会话 key 是否被任一插件认领（过滤/last_open 排除的唯一判据）。
    pub(crate) fn plugin_claimed(&self, key: &str) -> bool {
        self.plugins.claims.iter().any(|c| c.key == key)
    }

    /// 新建插件认领会话（契约 v2.4 sessions；002「设置会话」步骤的泛化）。
    /// 名字 = 前缀 + 15 位时间戳（唯一性归宿主）；过滤清单 owner = 插件 id
    /// （hidden_sessions 天然 owner 泛化，消费方零改动）；包内 pi 扩展挂到
    /// 该会话的 session_ext（spawn 时 `-e` 装载）。cwd = 插件自持的工作
    /// 目录（002 模块独立：与桌面 cwd 互不联动，错误项目问题无从发生）。
    /// 建完经 broadcast "claim" 回推 key（插件存指针）。
    pub(crate) fn plugin_new_draft(
        &mut self,
        ix: usize,
        prefix: &str,
        cwd: &str,
        cx: &mut gpui::Context<Self>,
    ) -> Result<String, String> {
        if prefix.is_empty() {
            return Err("prefix 不能为空".into());
        }
        let cwd_path = std::path::PathBuf::from(cwd);
        if !cwd_path.is_dir() {
            return Err(format!("cwd 不是目录：{cwd}"));
        }
        let Some(entry) = self.plugins.plugins.get(ix) else {
            return Err("插件不存在".into());
        };
        if entry.disabled {
            return Err("插件已停用".into());
        }
        // 清单对账（契约 v2.4 `[[contributes.sessions]] prefix`）：声明过槽位的
        // 插件，实际认领前缀不在声明里就留痕——不阻断（运行期真相是
        // ctx.newDraft 传入的 prefix，声明只是清单面的可审计位）。
        let declared = &entry.manifest.sessions;
        if !declared.is_empty() && !declared.iter().any(|s| s.prefix == prefix) {
            let list: Vec<&str> = declared.iter().map(|s| s.prefix.as_str()).collect();
            eprintln!(
                "[pf-plugin] 认领前缀与清单声明不一致（plugin={}，declared=[{}]，actual={prefix}）",
                entry.manifest.id,
                list.join(", ")
            );
        }
        // 先取值再动 self（借用分离）：pi 扩展绝对路径 + 插件 id + 形态门
        let pi_exts: Vec<String> = entry
            .manifest
            .pi_extensions
            .iter()
            .map(|rel| entry.manifest.dir.join(rel).to_string_lossy().into_owned())
            .collect();
        let plugin_id = entry.manifest.id.clone();
        let is_wasm = entry.manifest.wasm.is_some();
        let pkg_dir = entry.manifest.dir.clone();
        let name = claim_session_name(prefix);
        let key = format!("draft-{}", self.pool.draft_seq);
        self.pool.draft_seq += 1;
        let rt = cx.new(|_| {
            let mut r =
                crate::session::runtime::SessionRuntime::new(key.clone(), cwd_path.clone(), None);
            r.default_model = crate::services::workspace::default_model_pref();
            r.status = crate::i18n::tr("session.new").to_string();
            r
        });
        self.pool.runtimes.insert(key.clone(), rt.clone());
        self.subscribe_runtime(&rt, cx);
        // 包内 pi 扩展挂到该会话（spawn 时 `-e`；路径 = 包根 + 相对路径）。
        // **Tier 2 门**：pi ext 是会话进程内全权代码，仅官方验签包（wasm 形态，
        // init 即验签门）可携带；JS 形态声明了也不挂（fail-closed + 告警）。
        // 同时写 claim.json 到插件 data 目录：ext 在 session_start 自读交接
        //（ext 自定位：__dirname/../data/，与宿主 ChatHost dir 同源）。
        if !pi_exts.is_empty() {
            if !is_wasm {
                eprintln!(
                    "[pf-plugin] Tier2 拒绝：非验签包（JS 形态）声明 pi_extensions，不挂载（{plugin_id}）"
                );
            } else if let Some(store) = pi_link::session_ext::store_path() {
                if let Err(e) = pi_link::session_ext::write_for(&store, &key, &pi_exts) {
                    eprintln!("[pf-plugin] pi 扩展挂载失败（{key}）: {e}");
                }
            }
        }
        // 认领槽替换（一个插件同时只有一条进行中线）+ 过滤清单（owner = 插件 id）
        let prefix = prefix.to_string();
        self.plugins.claims.retain(|c| c.plugin_id != plugin_id);
        self.plugins.claims.push(Claim {
            plugin_id: plugin_id.clone(),
            prefix: prefix.clone(),
            key: key.clone(),
        });
        pi_link::hidden_sessions::set_owned(&plugin_id, Some(&name), None, Some(&prefix));
        // 结果回推（契约 broadcast_events.claim）：插件存指针（wx_session_id 泛化）
        // 交接文件：ext 在 session_start 自读（data 目录 = 包根/data/，
        // 与宿主 ChatHost dir 同源；卸载随目录删，更新随包走）
        if !pi_exts.is_empty() {
            let claim_file = Some(pkg_dir.join("data").join("claim.json"));
            if let Some(f) = claim_file {
                if let Some(p) = f.parent() {
                    let _ = std::fs::create_dir_all(p);
                }
                if let Err(e) = std::fs::write(
                    &f,
                    json!({"key": key, "name": name, "cwd": cwd, "prefix": prefix, "plugin_id": plugin_id})
                        .to_string(),
                ) {
                    eprintln!("[pf-plugin] claim.json 写入失败: {e}");
                }
            }
        }
        let payload = json!({"key": key, "name": name, "cwd": cwd}).to_string();
        self.plugin_broadcast_fire(ix, broadcast::CLAIM, &payload, cx);
        Ok(key)
    }

    /// 打开插件预览页签（契约 v2.4 `dock = "tab"`）：单例（同 pkg+page
    /// 已开 → 激活）、整页声明式树。懒加载链与 dock 同源（plugin_refresh），
    /// 广播首拍同步复位。入口 = 状态栏点击（清单 dock 归宿主解释，
    /// 插件不感知形态）。
    pub(crate) fn open_plugin_tab(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        // 已停用的插件不开页签（同 dock 硬门）
        let Some(entry) = self.plugins.plugins.get(ix) else {
            return;
        };
        if entry.disabled {
            return;
        };
        let Some((pkg, page)) = entry
            .manifest
            .tab_page()
            .map(|(p, g)| (p.to_string(), g.to_string()))
        else {
            return;
        };
        // 单例：同 pkg+page 已开 → 激活既有页签（不重排，082 顺序稳定）
        let tix = self.ws.panel_tabs.iter().position(
            |t| matches!(t, crate::PanelTab::Plugin { pkg: p, page: g } if p == &pkg && g == &page),
        );
        let tix = match tix {
            Some(tix) => tix,
            None => {
                self.ws
                    .panel_tabs
                    .push(crate::PanelTab::Plugin { pkg, page });
                self.ws.panel_tabs.len() - 1
            }
        };
        self.activate_panel_tab(tix, cx);
        self.set_content_view(crate::ContentView::Plugin);
        // 广播首拍 + 懒加载链（与 dock 同源）
        broadcast::reset(&mut self.plugins);
        self.plugin_refresh(ix, cx);
    }

    /// 台账包名 → 运行态条目下标。对账走清单的**顶层包目录名**
    /// （市场布局 `@scope__name` 与 dev 平铺 `name` 两种别名都认）——
    /// 拿 `manifest.dir.file_name()` 比是版本号，永远比不中。
    pub(crate) fn plugin_ix_by_pkg(&self, name: &str) -> Option<usize> {
        let aliases = pi_link::pf_plugins::dir_aliases(name);
        self.plugins
            .plugins
            .iter()
            .position(|e| aliases.iter().any(|a| *a == e.manifest.pkg))
    }

    /// 运行态就地撤下（停用 / 卸载共用）：丢 JS 运行时与组件树缓存、收起右键
    /// 菜单；**左 dock 正显示它就退回默认面板**。
    ///
    /// 不撤组件树就是「停用了左面板还在照旧渲染」：`plugin_panel_view` 只认
    /// `tree_loaded` + 缓存树，不看 disabled 位（090-4 v5 踩过）。
    pub(crate) fn plugin_evict(&mut self, ix: usize) {
        let docked = self.layout.dock_panel == crate::DockPanel::Plugin(ix);
        // 认领槽随停用一井撤：过滤清单同步摘除（会话文件保留，靠前缀仍藏得住）
        if let Some(entry) = self.plugins.plugins.get(ix) {
            let id = entry.manifest.id.clone();
            self.plugins.claims.retain(|c| c.plugin_id != id);
            pi_link::hidden_sessions::clear_owned(&id);
        }
        if let Some(entry) = self.plugins.plugins.get_mut(ix) {
            entry.disabled = true;
            entry.js = None;
            entry.wasm = None;
            entry.wasm_focus = None;
            entry.host = None;
            entry.body = None;
            entry.tree_loaded = false;
            entry.cell_menu = None;
            entry.section_scrolls.borrow_mut().clear();
        }
        if docked {
            self.layout.dock_panel = crate::DockPanel::Default;
        }
    }

    /// 运行态（重）装载单个插件：撤旧条目 + 按新清单重建（安装 / 更新 / 启用
    /// 共用）。返回 false = 清单为空（调用方决定报错文案）。
    ///
    /// **不整体替换 `plugins.plugins`**：那会清掉其他插件的 JS 运行态，还会让
    /// dock 存的 `Plugin(ix)` 位置错位。
    pub(crate) fn plugin_replace_by_pkg(
        &mut self,
        name: &str,
        manifests: Vec<PluginManifest>,
    ) -> bool {
        if let Some(ix) = self.plugin_ix_by_pkg(name) {
            self.plugin_evict(ix);
            self.plugins.plugins.remove(ix);
            // 下标位移：插件面板存的是位置，任何一条被摘都退默认
            if matches!(self.layout.dock_panel, crate::DockPanel::Plugin(_)) {
                self.layout.dock_panel = crate::DockPanel::Default;
            }
        }
        if manifests.is_empty() {
            return false;
        }
        self.plugins.plugins.extend(plugin_entries(manifests));
        true
    }

    /// 后台按台账包名重扫清单（只出这一个包）→ 主线程 `plugin_replace_by_pkg`
    /// 落位。安装完 / 启用即生效，不靠重启；扫不到就走市场页头段错误行。
    pub(crate) fn reload_plugin_by_pkg(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        let pkg = name.clone();
        let scan = cx.background_executor().spawn(async move {
            let aliases = pi_link::pf_plugins::dir_aliases(&pkg);
            scan_manifests()
                .into_iter()
                .filter(|m| aliases.iter().any(|a| *a == m.pkg))
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let manifests = scan.await;
            let _ = this.update(cx, |c, cx| {
                if !c.plugin_replace_by_pkg(&name, manifests) {
                    let msg = crate::i18n::tf("market.pf.enable_failed", &[("name", name.clone())]);
                    c.aux.market.pf.error = Some(msg);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 懒加载就绪：runtime 已建或现建；禁用/失败返回 false（已 toast）。
    fn plugin_ready(&mut self, ix: usize, cx: &mut gpui::Context<Self>) -> bool {
        let Some(entry) = self.plugins.plugins.get_mut(ix) else {
            return false;
        };
        if entry.disabled {
            return false;
        }
        if entry.manifest.wasm.is_some() {
            if entry.wasm.is_some() {
                return true;
            }
            return self.plugin_wasm_ready(ix, cx);
        }
        if entry.js.is_some() {
            return true;
        }
        // 首用建 ctx：entry 取第一个 panel 声明（v1 单面板）；私有目录 data/
        let panel = entry.manifest.panels.first().cloned();
        let entry_rel = panel
            .as_ref()
            .map(|p| p.entry.clone())
            .unwrap_or_else(|| "main.js".into());
        let dir = entry.manifest.dir.join("data");
        let _ = std::fs::create_dir_all(&dir); // 插件自管目录：PF 只建，不解析
        let dir_str = dir.to_string_lossy().into_owned();
        let host = Rc::new(RefCell::new(ChatHost {
            dir,
            actions: Vec::new(),
        }));
        // 分发态字节码包：entry 换成 plugin.qbc，加载器走 QuickJS 字节码
        let bytecode_rel = entry.manifest.bytecode.clone();
        let entry_path = entry
            .manifest
            .dir
            .join(bytecode_rel.as_deref().unwrap_or(&entry_rel));
        let id = entry.manifest.id.clone();
        // 正版门（分发态）：字节码包只在官方构建里加载（非官方 = 占位证明 → 拒载）
        if bytecode_rel.is_some() && !has_official_proof() {
            entry.disabled = true;
            self.set_status(format!("插件 {id} 拒载：需要官方构建的 PF"), cx);
            return false;
        }
        let loaded = match bytecode_rel {
            Some(_) => JsPlugin::load_bytecode(&entry_path, &dir_str, host.clone()),
            None => JsPlugin::load(&entry_path, &dir_str, host.clone()),
        };
        match loaded {
            Ok(js) => {
                let mut failed = None;
                if let Err(e) = js.activate() {
                    failed = Some(format!("插件 {id} 出错：{e}"));
                }
                if failed.is_none() {
                    entry.js = Some(js);
                    entry.host = Some(host);
                    return true;
                }
                entry.disabled = true;
                self.set_status(failed.unwrap(), cx);
                false
            }
            Err(e) => {
                entry.disabled = true;
                self.set_status(format!("插件 {id} 出错：{e}"), cx);
                false
            }
        }
    }

    /// 内层 wasm 插件懒装载（加密包渲染面）：主线程 `load_cached` —— cwasm
    /// 缓存命中 = mmap 反序列化 + 实例化，毫秒级（启动预热已把编译搬走），
    /// 首次开面板是用户显式动作，这个量级无感。失败 = 会话级禁用（与 JS 同款）。
    fn plugin_wasm_ready(&mut self, ix: usize, cx: &mut gpui::Context<Self>) -> bool {
        ensure_wasm_hooks(cx);
        self.plugin_sync_quick_pins();
        let Some(entry) = self.plugins.plugins.get_mut(ix) else {
            return false;
        };
        let id = entry.manifest.id.clone();
        let wasm_path = entry
            .manifest
            .dir
            .join(entry.manifest.wasm.as_deref().unwrap_or("plugin.wasm"));
        // 编译产物缓存绑在插件安装目录（卸载即清、整目录拷走跟着走）。
        // 安装时已预编译过 → 这里命中是常态（mmap 反序列化，毫秒级）。
        let cache = pi_link::pf_plugins::plugin_cache_for(&entry.manifest.dir);
        // 缓存未命中（手工摆包/旧版安装）：编译秒级重活不许进主线程 →
        // 转后台装载，装好回主线程填实例+渲染；此刻返回 false = 「…」
        let cached = pf_plugin_host::PluginInstance::has_cached_artifact(&wasm_path, &cache);
        if !cached {
            cx.spawn(async move |this, cx| {
                let res = cx
                    .background_executor()
                    .spawn(async move {
                        let inst = PluginInstance::load_cached(&wasm_path, &cache);
                        inst.and_then(|mut i| i.init(&id).map(|()| i))
                    })
                    .await;
                let _ = this.update(cx, |c, cx| {
                    let Some(entry) = c.plugins.plugins.get_mut(ix) else {
                        return;
                    };
                    match res {
                        Ok(inst) => {
                            entry.wasm = Some(inst);
                            entry.wasm_focus = Some(cx.focus_handle());
                            c.plugin_wasm_render(ix, cx);
                        }
                        Err(e) => {
                            entry.disabled = true;
                            let id = entry.manifest.id.clone();
                            c.set_status(format!("插件 {id} 出错：{e}"), cx);
                        }
                    }
                });
            })
            .detach();
            return false;
        }
        let mut inst = match PluginInstance::load_cached(&wasm_path, &cache) {
            Ok(i) => i,
            Err(e) => {
                entry.disabled = true;
                self.set_status(format!("插件 {id} 出错：{e}"), cx);
                return false;
            }
        };
        if let Err(e) = inst.init(&id) {
            entry.disabled = true;
            self.set_status(format!("插件 {id} 拒载：{e}"), cx);
            return false;
        }
        entry.wasm = Some(inst);
        entry.wasm_focus = Some(cx.focus_handle());
        true
    }

    /// 后台验签门拒载的包 → 会话级禁用（tab 隐藏、面板点不开；090-5 L3 fail-closed）。
    fn plugin_mark_rejected(&mut self, ids: &[String]) {
        for e in &mut self.plugins.plugins {
            if e.manifest.wasm.is_some() && ids.contains(&e.manifest.id) {
                e.disabled = true;
            }
        }
    }

    /// onPanelOpen + render → rows 缓存（02 §4 懒加载链）。
    fn plugin_refresh(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        if !self.plugin_ready(ix, cx) {
            return;
        }
        // 内层 wasm 插件：render() → Paint 缓存（画布路径，与 JS 组件树分叉）。
        // 不需要 panel-open 事件：render 每次都现读插件自己的存储。
        if self.plugins.plugins[ix].wasm.is_some() {
            self.plugin_wasm_render(ix, cx);
            // 契约 v3：弹窗开着且属于本插件 → 体跟着新 render
            if self.plugins.popup.as_ref().map(|p| p.ix) == Some(ix) {
                let body = self.plugins.plugins.get(ix).and_then(|e| e.body.clone());
                if let Some(p) = self.plugins.popup.as_mut() {
                    p.body = body;
                    p.tree_loaded = p.body.is_some();
                }
                cx.notify();
            }
            return;
        }
        let (result, actions) = {
            let entry = &mut self.plugins.plugins[ix];
            let host = entry.host.as_ref().unwrap().clone();
            host.borrow_mut().actions.clear();
            let js = entry.js.as_ref().unwrap();
            let r = js.fire_panel_open().and_then(|()| js.render_json());
            let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
            (r, drained)
        };
        for a in actions {
            // 徽标是渲染的伴随声明（同 fn_bar）：render_popup 里也会同步徽标，
            // 一律丢弃会让列表徽标彻底打不上（真机 2026-10-28）。
            match a {
                JsAction::SetBadge {
                    session,
                    icon,
                    tone,
                } => {
                    if !session.is_empty() {
                        let owner = self.plugins.plugins[ix].manifest.id.clone();
                        self.plugins.badges.retain(|b| b.owner != owner);
                        self.plugins.badges.push(PluginBadge {
                            owner,
                            session,
                            icon,
                            tone,
                        });
                        cx.notify();
                    }
                }
                JsAction::ClearBadge { session } => {
                    let owner = self.plugins.plugins[ix].manifest.id.clone();
                    let before = self.plugins.badges.len();
                    if session.is_empty() {
                        self.plugins.badges.retain(|b| b.owner != owner);
                    } else {
                        self.plugins
                            .badges
                            .retain(|b| !(b.owner == owner && b.session == session));
                    }
                    if self.plugins.badges.len() != before {
                        cx.notify();
                    }
                }
                other => eprintln!("[pf-plugin] render/onPanelOpen 期间的动作被忽略: {other:?}"),
            }
        }
        match result {
            Ok(json) => {
                let entry = &mut self.plugins.plugins[ix];
                let v: serde_json::Value =
                    serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                // v2：render 直接返回组件树（根节点），没有 { body, rows } 外壳
                entry.body = Some(v);
                entry.tree_loaded = true;
                // 契约 v3：弹窗开着且属于本插件 → 内容跟着新态（refresh-qr 等回环后弹窗即新态）
                self.plugin_popup_reload(ix, cx);
                cx.notify();
            }
            Err(e) => self.plugin_broken(ix, e, cx),
        }
    }

    /// 右键菜单点击：onMenu(id, {path, fileIcon}) → 动作分派 → 面板开着则重渲染。
    pub(crate) fn plugin_menu_fire(
        &mut self,
        ix: usize,
        menu_id: &str,
        path: &Path,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.plugin_ready(ix, cx) {
            return;
        }
        // 内层 wasm 插件：同一事件通道（on-event kind=menu）
        if self.plugins.plugins[ix].wasm.is_some() {
            let p = path.to_string_lossy().into_owned();
            self.plugin_wasm_event(ix, "menu", menu_id, &p, window, cx);
            return;
        }
        let (result, actions) = {
            let entry = &mut self.plugins.plugins[ix];
            let host = entry.host.as_ref().unwrap().clone();
            host.borrow_mut().actions.clear();
            let fi = crate::services::file_icons::get_icon(path).to_string();
            let js = entry.js.as_ref().unwrap();
            let r = js.fire_menu(menu_id, &path.to_string_lossy(), &fi);
            let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
            (r, drained)
        };
        match result {
            Ok(()) => self.plugin_dispatch(ix, actions, window, cx),
            Err(e) => self.plugin_broken(ix, e, cx),
        }
    }

    /// 组件菜单项回调（01 §4.2：不带 action 的 menu 项 → 插件改自己的数据）。
    /// 返回后由 `plugin_dispatch` 统一 `plugin_refresh` 重渲染（02 §3 重渲染规则）。
    pub(crate) fn plugin_command_fire(
        &mut self,
        ix: usize,
        command_id: &str,
        path: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.plugin_ready(ix, cx) {
            return;
        }
        if self.plugins.plugins[ix].wasm.is_some() {
            self.plugin_wasm_event(ix, "command", command_id, path, window, cx);
            return;
        }
        let (result, actions) = {
            let entry = &mut self.plugins.plugins[ix];
            let host = entry.host.as_ref().unwrap().clone();
            host.borrow_mut().actions.clear();
            let js = entry.js.as_ref().unwrap();
            let r = js.fire_command(command_id, path);
            let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
            (r, drained)
        };
        match result {
            Ok(()) => self.plugin_dispatch(ix, actions, window, cx),
            Err(e) => self.plugin_broken(ix, e, cx),
        }
    }

    /// 契约 v3 fn_bar：会话功能栏插件 icon 批量取数（带 1s TTL 缓存——
    /// 功能栏每帧渲染，调用不进每帧）。按能力面取（清单 fn_bar 声明 +
    /// 插件导出，载体无关）；停用/坏插件不出。
    pub(crate) fn plugin_fn_bar_chrome(
        &mut self,
        session_key: &str,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<(usize, SharedString, String, String)> {
        const TTL: std::time::Duration = std::time::Duration::from_millis(1000);
        let mut out = Vec::new();
        let mut miss: Vec<usize> = Vec::new();
        for (ix, e) in self.plugins.plugins.iter().enumerate() {
            if e.manifest.fn_bar.is_none() || e.disabled {
                continue;
            }
            let hit = self.plugins.fn_bar_cache.iter().find(|c| {
                c.pkg == e.manifest.pkg && c.session_key == session_key && c.at.elapsed() < TTL
            });
            match hit {
                Some(c) => {
                    let path = contrib_icon_path(&e.manifest.id, &c.icon);
                    out.push((ix, path, c.tone.clone(), c.tip.clone()));
                }
                None => miss.push(ix),
            }
        }
        for ix in miss {
            // 未就绪的插件不阻塞功能栏（懒加载链；首拍拿不到下一拍再试）
            if !self.plugin_ready(ix, cx) {
                continue;
            }
            let pkg = self.plugins.plugins[ix].manifest.pkg.clone();
            let env = serde_json::json!({ "session": { "key": session_key } }).to_string();
            let (result, actions) = {
                let entry = &mut self.plugins.plugins[ix];
                entry.hook_fn_bar(&env)
            };
            // chrome 取数期间的动作丢弃（fn_bar 是纯 chrome，不该有副作用）。
            // 例外：会话列表徽标（SetBadge/ClearBadge）就是 chrome 的伴随声明——
            // 不放行的话，已绑会话的列表 icon 永远打不上（002 时间位徽标）。
            for a in &actions {
                match a {
                    JsAction::SetBadge {
                        session,
                        icon,
                        tone,
                    } => {
                        if !session.is_empty() {
                            let owner = self.plugins.plugins[ix].manifest.id.clone();
                            self.plugins.badges.retain(|b| b.owner != owner); // 一插件一条
                            self.plugins.badges.push(PluginBadge {
                                owner,
                                session: session.clone(),
                                icon: icon.clone(),
                                tone: tone.clone(),
                            });
                        }
                    }
                    JsAction::ClearBadge { session } => {
                        let owner = self.plugins.plugins[ix].manifest.id.clone();
                        if session.is_empty() {
                            self.plugins.badges.retain(|b| b.owner != owner);
                        } else {
                            self.plugins
                                .badges
                                .retain(|b| !(b.owner == owner && b.session == *session));
                        }
                    }
                    _ => eprintln!("[pf-plugin] fn_bar 期间的动作被忽略: {a:?}"),
                }
            }
            match result {
                Ok(Some(json)) => {
                    let v: serde_json::Value =
                        serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                    let icon = v
                        .get("icon")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let tone = v
                        .get("tone")
                        .and_then(Value::as_str)
                        .unwrap_or("text.faint")
                        .to_string();
                    // tip 是文案对象 {zh,en} 或纯字符串，统一解成本语言串
                    let tip = match v.get("tip") {
                        Some(Value::Object(_)) => {
                            let zh = v["tip"]["zh"].as_str().unwrap_or("").to_string();
                            let en = v["tip"]["en"].as_str().map(|s| s.to_string());
                            pick_lang(&zh, &en)
                        }
                        Some(Value::String(s)) => s.clone(),
                        _ => String::new(),
                    };
                    self.plugins
                        .fn_bar_cache
                        .retain(|c| !(c.pkg == pkg && c.session_key == session_key));
                    self.plugins.fn_bar_cache.push(FnBarCache {
                        pkg: pkg.clone(),
                        session_key: session_key.to_string(),
                        icon: icon.clone(),
                        tone: tone.clone(),
                        tip: tip.clone(),
                        at: std::time::Instant::now(),
                    });
                    if !icon.is_empty() {
                        let path = contrib_icon_path(&self.plugins.plugins[ix].manifest.id, &icon);
                        out.push((ix, path, tone, tip));
                    }
                }
                // 未导出 fn_bar：负缓存一轮（TTL 内不再问）
                Ok(None) => self.plugins.fn_bar_cache.push(FnBarCache {
                    pkg: pkg.clone(),
                    session_key: session_key.to_string(),
                    icon: String::new(),
                    tone: String::new(),
                    tip: String::new(),
                    at: std::time::Instant::now(),
                }),
                Err(e) => self.plugin_broken(ix, e, cx),
            }
        }
        out
    }

    /// fn_bar icon 点击：command 回环（env 带 {session:{key}}，契约 v3）。
    /// 绑定目标 = 当前会话：先把包内 pi 扩展写进该会话扩展清单（出码/收发
    /// 本体在 ext，004 §5.2），进程空闲即重启挂载，再回环命令。
    pub(crate) fn plugin_fn_bar_click(
        &mut self,
        ix: usize,
        session_key: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        // 两套键各归其位：ext 挂载清单按 **PF 会话键**（active_key，与
        // spawn 读侧一致）；插件命令 env 按 **pi sessionId**（与 ext
        // getSessionId() 同源，004 §7-1）
        let pf_key = self.pool.active_key.clone();
        self.plugin_attach_pi_exts(ix, &pf_key, cx);
        let Some(cmd) = self
            .plugins
            .plugins
            .get(ix)
            .and_then(|e| e.manifest.fn_bar.as_ref())
            .map(|f| f.command.clone())
        else {
            return;
        };
        let env = serde_json::json!({ "session": { "key": session_key } }).to_string();
        self.plugin_command_fire_env(ix, &cmd, &env, window, cx);
    }

    /// 包内 pi 扩展 → 当前会话扩展清单（契约 v3：绑定目标 = 用户当前普通
    /// 会话，不再走建线，挂载改在绑定时完成）。`pf_session_key` = PF 会话键
    /// （active_key，session-ext.json 的键）；Tier 2 门：market 验签包才可
    /// 携带（安装链已验）；dev 手拷包（台账 integrity 空）告警放行。
    /// 会话进程空闲 → 立即重启挂载；忙 → 清单已写，下次 spawn 生效。
    fn plugin_attach_pi_exts(
        &mut self,
        ix: usize,
        pf_session_key: &str,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(entry) = self.plugins.plugins.get(ix) else {
            return;
        };
        if entry.manifest.pi_extensions.is_empty() || pf_session_key.is_empty() {
            return;
        }
        // Tier 2 门口径：台账 integrity 非空 = market 验签包（安装链已验）；
        // 空 = dev 手拷（link:dev），告警放行——市场分发必须走验签链
        let verified = pi_link::pf_plugins::load_ledger().iter().any(|l| {
            pi_link::pf_plugins::dir_aliases(&l.name)
                .iter()
                .any(|a| *a == entry.manifest.pkg)
                && !l.integrity.is_empty()
        });
        if !verified {
            eprintln!(
                "[pf-plugin] dev 手拷包携带 pi_extensions，告警放行（{}；市场分发必须走验签链）",
                entry.manifest.id
            );
        }
        let pi_exts: Vec<String> = entry
            .manifest
            .pi_extensions
            .iter()
            .map(|rel| entry.manifest.dir.join(rel).to_string_lossy().into_owned())
            .collect();
        let missing = pi_link::session_ext::store_path()
            .map(|store| {
                let have = pi_link::session_ext::read_for(&store, pf_session_key);
                pi_exts.iter().any(|p| !have.iter().any(|h| h == p))
            })
            .unwrap_or(true);
        if !missing {
            return; // 清单已含，无需重启
        }
        if let Some(store) = pi_link::session_ext::store_path() {
            if let Err(e) = pi_link::session_ext::write_for(&store, pf_session_key, &pi_exts) {
                eprintln!("[pf-plugin] pi 扩展挂载失败（{pf_session_key}）: {e}");
                return;
            }
        }
        eprintln!(
            "[pf-plugin] pi 扩展已入会话清单（{} → {}），空闲重启挂载",
            entry.manifest.id, pf_session_key
        );
        // 空闲会话立即重启挂载（忙 = 有任务在跑，不动，下次 spawn 生效）
        if let Some(rt) = self.pool.runtimes.get(pf_session_key).cloned() {
            let busy = rt.read(cx).agent_running;
            if !busy {
                rt.update(cx, |r, cx| {
                    // 内存清单同步（custom 档 spawn 走 ext_sources，其他档走
                    // session-ext.json 统一追加——两路都要有这份路径）
                    for p in &pi_exts {
                        if !r.ext_sources.iter().any(|e| e == p) {
                            r.ext_sources.push(p.clone());
                        }
                    }
                    r.shutdown_process();
                    r.ensure_spawned(cx);
                });
            } else {
                self.set_status("微信机器人扩展将在当前任务完成后生效".to_string(), cx);
            }
        }
    }

    /// 命令回调（env 负载形态，契约 v3）：与 plugin_command_fire 同链，
    /// JS 走 fire_command_env，wasm 走 on-event（env 串放 path 位）。
    pub(crate) fn plugin_command_fire_env(
        &mut self,
        ix: usize,
        command_id: &str,
        env_json: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.plugin_ready(ix, cx) {
            return;
        }
        // 命令回环后插件状态已变（绑定指针/开关/出码）：chrome 缓存作废重建
        // ——两载体同时机（8tb.2：wasm 命令回环原先不清，导致扫码后 icon 滞灰）
        self.plugins.fn_bar_cache.clear();
        if self.plugins.plugins[ix].wasm.is_some() {
            self.plugin_wasm_event(ix, "command", command_id, env_json, window, cx);
            return;
        }
        let (result, actions) = {
            let entry = &mut self.plugins.plugins[ix];
            let host = entry.host.as_ref().unwrap().clone();
            host.borrow_mut().actions.clear();
            let js = entry.js.as_ref().unwrap();
            let r = js.fire_command_env(command_id, env_json);
            let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
            (r, drained)
        };
        match result {
            Ok(()) => self.plugin_dispatch(ix, actions, window, cx),
            Err(e) => self.plugin_broken(ix, e, cx),
        }
    }

    /// 弹窗打开（契约 v3）：内容 = render_popup(ctx)（wasm = render()）；
    /// 一次一个（开新的关旧的）。同插件重复 open = 重取内容。
    pub(crate) fn plugin_popup_open(
        &mut self,
        ix: usize,
        popup_id: Option<&str>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.plugin_ready(ix, cx) {
            return;
        }
        // 代数自增：杀掉上一轮 poll（重复 open 不堆叠 1s 轮询循环）
        self.plugins.popup_gen += 1;
        let (id, title) = {
            let entry = &self.plugins.plugins[ix];
            let pc = match popup_id {
                Some(id) => entry.manifest.popups.iter().find(|p| p.id == id),
                None => entry.manifest.popups.first(),
            };
            let Some(pc) = pc else {
                self.set_status(
                    format!(
                        "插件 {} 没有弹窗 {}",
                        entry.manifest.id,
                        popup_id.unwrap_or("")
                    ),
                    cx,
                );
                return;
            };
            (pc.id.clone(), pick_lang(&pc.title_zh, &pc.title_en))
        };
        // 能力面取体（8tb.2）：JS render_popup / wasm render-popup（未导出
        // 退回 render 面）同一取数口，解析与开窗逻辑不分支。
        let (result, js_actions, wasm_actions) = {
            let entry = &mut self.plugins.plugins[ix];
            entry.hook_render_popup()
        };
        match result {
            Ok(json) => {
                let body = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                // 契约约定：render_popup 返回 null = 此刻无弹窗内容（如绑定
                // 已成功）→ 不开/自动关
                if body.is_null() {
                    self.plugin_popup_close(cx);
                    return;
                }
                self.plugins.popup = Some(PluginPopupOpen {
                    ix,
                    id,
                    title,
                    tree_loaded: true,
                    body: Some(body),
                });
                // bind 回调里 openPopup 之外若还带了别的动作，照走；
                // wasm 意图（open-popup）同落点
                self.plugin_dispatch(ix, js_actions, window, cx);
                if !wasm_actions.is_empty() {
                    self.plugin_wasm_dispatch(ix, wasm_actions, window, cx);
                }
            }
            Err(e) => self.plugin_broken(ix, e, cx),
        }
        cx.notify();
        let popup_gen = self.plugins.popup_gen;
        self.plugin_popup_poll(ix, popup_gen, cx);
    }

    /// 弹窗关闭（随手关：点外/×；act.closePopup 同路）。
    pub(crate) fn plugin_popup_close(&mut self, cx: &mut gpui::Context<Self>) {
        eprintln!("[pf-plugin] popup close (trace)");
        if self.plugins.popup.take().is_some() {
            self.plugins.popup_gen += 1;
            // chrome 缓存作废：扫码成功 → 弹窗自动收，此刻功能栏 icon 必须立刻
            // 灰→绿。不失效就要等下一个会话事件（真机：滞后到微信来消息才亮）
            self.plugins.fn_bar_cache.clear();
            cx.notify();
        }
    }

    /// 弹窗打开期间的低频重取（1s，旧版宿主泵采样 parity）：ext 写镜像
    /// （过期白遮/新码/开关态）无回调链可触发，轮询是弹窗场景唯一时效保障；
    /// 弹窗关闭即停。停用/坏插件自动断流。
    fn plugin_popup_poll(&mut self, ix: usize, popup_gen: u64, cx: &mut gpui::Context<Self>) {
        eprintln!("[pf-plugin] popup poll 启动 ix={ix} gen={popup_gen}");
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(1000))
                    .await;
                let mut stop = true;
                let _ = this.update(cx, |c, cx| match c.plugins.popup.as_ref() {
                    Some(p) if p.ix == ix && p.tree_loaded && c.plugins.popup_gen == popup_gen => {
                        stop = false;
                        c.plugin_popup_reload(ix, cx);
                    }
                    _ => {}
                });
                if stop {
                    break;
                }
            }
        })
        .detach();
    }

    /// 弹窗内容重取（回调链返回后的重渲染规则：弹窗开着就跟着新态）。
    /// 能力面取体（8tb.2）：JS/wasm 同一取数口，解析不分支。
    fn plugin_popup_reload(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        eprintln!("[pf-plugin] popup reload trace ix={ix}");
        if self.plugins.popup.as_ref().map(|p| p.ix) != Some(ix) {
            return;
        }
        // 防御：js/wasm 可能已被异常隔离（plugin_broken）或重扫描清掉——
        // 弹窗场景绝不打崩宿主，收弹窗即可
        if !self.plugin_ready(ix, cx) {
            self.plugin_popup_close(cx);
            return;
        }
        let (result, actions, wasm_actions) = {
            let entry = &mut self.plugins.plugins[ix];
            entry.hook_render_popup()
        };
        // wasm 意图：本面无 window，只放行 Toast，其余丢弃（broadcast 同口径；
        // 弹窗体不该再开弹窗）
        for a in wasm_actions {
            match a {
                PluginAction::Toast(m) => self.set_status(m, cx),
                other => eprintln!("[pf-plugin] popup 重取期间 wasm 动作被忽略: {:?}", other),
            }
        }
        // 重取期间的动作：徽标是渲染的伴随声明，必须落地（否则列表永远没徽标）；
        // 其余动作忽略（与 render 面同口径）。
        for a in actions {
            match a {
                JsAction::SetBadge {
                    session,
                    icon,
                    tone,
                } => {
                    if !session.is_empty() {
                        let owner = self.plugins.plugins[ix].manifest.id.clone();
                        self.plugins.badges.retain(|b| b.owner != owner);
                        self.plugins.badges.push(PluginBadge {
                            owner,
                            session,
                            icon,
                            tone,
                        });
                        cx.notify();
                    }
                }
                JsAction::ClearBadge { session } => {
                    let owner = self.plugins.plugins[ix].manifest.id.clone();
                    let before = self.plugins.badges.len();
                    if session.is_empty() {
                        self.plugins.badges.retain(|b| b.owner != owner);
                    } else {
                        self.plugins
                            .badges
                            .retain(|b| !(b.owner == owner && b.session == session));
                    }
                    if self.plugins.badges.len() != before {
                        cx.notify();
                    }
                }
                other => eprintln!("[pf-plugin] popup 重取期间的动作被忽略: {other:?}"),
            }
        }
        match result {
            Ok(json) => {
                let body: serde_json::Value =
                    serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                // null = 插件判定弹窗该收了（如绑定成功瞬间）→ 自动关
                if body.is_null() {
                    self.plugin_popup_close(cx);
                    return;
                }
                if let Some(p) = self.plugins.popup.as_mut() {
                    p.body = Some(body);
                    p.tree_loaded = true;
                }
                cx.notify();
            }
            Err(e) => self.plugin_broken(ix, e, cx),
        }
    }

    /// 动作分派（02 §3 动作类 → 宿主实现；run 目标不可执行 → toast 报错）。
    pub(crate) fn plugin_dispatch(
        &mut self,
        ix: usize,
        actions: Vec<JsAction>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        for action in actions {
            match action {
                JsAction::Run(p) => {
                    let path = PathBuf::from(&p);
                    match crate::services::script_exec::resolve(&path, self.ws.cwd.as_deref()) {
                        Some(plan) => self.open_script_terminal(path, plan, window, cx),
                        None => self.set_status(format!("无法执行：{}", p), cx),
                    }
                }
                JsAction::Edit(p) => self.open_file_tab(PathBuf::from(&p), cx),
                // 契约 v3 sessions.setBadge / clearBadge：会话列表时间位徽标
                JsAction::SetBadge {
                    session,
                    icon,
                    tone,
                } => {
                    if !session.is_empty() {
                        let owner = self
                            .plugins
                            .plugins
                            .get(ix)
                            .map(|e| e.manifest.id.clone())
                            .unwrap_or_default();
                        // 一个插件只允许一条徽标（002：只绑一个会话）——换绑即覆盖，
                        // 否则列表里会残留旧会话的绿标
                        self.plugins.badges.retain(|b| b.owner != owner);
                        self.plugins.badges.push(PluginBadge {
                            owner,
                            session,
                            icon,
                            tone,
                        });
                        cx.notify();
                    }
                }
                JsAction::ClearBadge { session } => {
                    let owner = self
                        .plugins
                        .plugins
                        .get(ix)
                        .map(|e| e.manifest.id.clone())
                        .unwrap_or_default();
                    let before = self.plugins.badges.len();
                    if session.is_empty() {
                        self.plugins.badges.retain(|b| b.owner != owner);
                    } else {
                        self.plugins
                            .badges
                            .retain(|b| !(b.owner == owner && b.session == session));
                    }
                    if self.plugins.badges.len() != before {
                        cx.notify();
                    }
                }
                JsAction::RevealInExplorer(p) => {
                    crate::services::shell_open::reveal_in_explorer(Path::new(&p))
                }
                JsAction::Toast(m) => self.set_status(m, cx),
                JsAction::OpenUrl(u) => crate::services::shell_open::open_url(&u),
                JsAction::OpenFolder(p) => {
                    crate::services::shell_open::open_in_explorer(std::path::Path::new(&p))
                }
                JsAction::NewSession => self.new_session(cx),
                // 意图调用：新建插件认领会话（契约 v2.4 sessions）。同步无返回值，
                // 建线后经 broadcast "claim" 事件回推 key（动作队列模型）
                JsAction::NewDraft { prefix, cwd } => {
                    if let Err(e) = self.plugin_new_draft(ix, &prefix, &cwd, cx) {
                        self.set_status(format!("newDraft 失败：{e}"), cx);
                    }
                }
                // 只预填 + 聚焦：发不发由用户按发送键决定（插件不代发）
                JsAction::AskPi(text) => match self.composer.composer.clone() {
                    Some(input) => {
                        let fh = input.update(cx, |i, cx| {
                            i.set_value(&text, cx);
                            i.focus_handle_in(cx)
                        });
                        fh.focus(window);
                    }
                    None => self.set_status(text, cx),
                },
                // 契约 v3 popups：开/关插件弹窗（非模态，宿主管窗体）
                JsAction::OpenPopup(id) => self.plugin_popup_open(ix, Some(&id), window, cx),
                JsAction::ClosePopup(id) => {
                    if self.plugins.popup.as_ref().map(|p| (p.ix, p.id.as_str()))
                        == Some((ix, id.as_str()))
                    {
                        self.plugin_popup_close(cx);
                    }
                }
            }
        }
        // 插件数据可能被 onMenu/onCommand 改过：无条件重 render（独立窗口
        // 靠 observe(Chat) 自动重渲，02 §4）
        self.plugin_refresh(ix, cx);
    }

    // -------------------------------------------------------------------
    // 内层 wasm 插件（加密包）：**与 JS 插件同一条渲染管线**——`render()` 返回
    // 01 规范组件树 JSON，宿主 `panel::resolve` 统一解释；事件走 on-event。
    // wasm 不另立渲染模型（Paint 直绘已废）。
    // -------------------------------------------------------------------

    /// wasm `render()` → 组件树（与 JS 同一 schema，`panel::resolve` 统一渲染）。
    fn plugin_wasm_render(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        let result = {
            let entry = &mut self.plugins.plugins[ix];
            entry
                .wasm
                .as_mut()
                .map(|i| i.render_json())
                .unwrap_or_else(|| Ok(String::new()))
        };
        match result {
            Ok(json) => {
                let v: serde_json::Value =
                    serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                let entry = &mut self.plugins.plugins[ix];
                entry.body = Some(v);
                entry.tree_loaded = true;
                cx.notify();
            }
            Err(e) => self.plugin_broken(ix, format!("{e}"), cx),
        }
    }

    /// 组件状态广播派发（契约 v2.4 broadcast_events）。纯数据回调：
    /// 窗口相关动作（run/edit/终端）在无窗口面不可达即丢弃，Toast 照走；
    /// 处理完照重渲染规则刷新面板（wasm = render 回灌）。
    /// PF 钩子 page_actived：把「当前页会话 key」推给每个 JS 插件
    /// （PF 独有概念，pi 进程没有“当前页”；插件据此记住身份，禁止再猜）。
    pub(crate) fn plugin_page_active(
        &mut self,
        session_key: &str,
        cx: &mut gpui::Context<Self>,
    ) -> usize {
        if session_key.is_empty() {
            return 0;
        }
        let env = serde_json::json!({ "session": { "key": session_key } }).to_string();
        let mut notified = 0usize;
        let n = self.plugins.plugins.len();
        for ix in 0..n {
            // 能力面（8tb.2）：PF 钩子全员派发，载体无关——JS 未导出由胶水
            // 回 "0" 记日志；wasm 走事件通道 kind="page-active"（wit v3；
            // 旧 wasm 不认识即忽略，无害）
            let ok = {
                let e = &self.plugins.plugins[ix];
                !e.disabled
            };
            if !ok || !self.plugin_ready(ix, cx) {
                continue;
            }
            let is_wasm = self.plugins.plugins[ix].wasm.is_some();
            let r: Result<String, String> = if is_wasm {
                let (res, acts) = {
                    let entry = &mut self.plugins.plugins[ix];
                    let Some(inst) = entry.wasm.as_mut() else {
                        continue;
                    };
                    let res = inst.on_event("page-active", "", &env);
                    let acts = inst.pump_actions();
                    (res, acts)
                };
                // 无 window 面：Toast 照走，其余丢弃（broadcast 同口径；
                // page-active 只作废 chrome/徽标缓存，不触发面板重渲）
                for a in acts {
                    match a {
                        PluginAction::Toast(m) => self.set_status(m, cx),
                        other => {
                            eprintln!("[pf-plugin] page-active 期间动作被忽略: {other:?}")
                        }
                    }
                }
                res.map(|()| "1".to_string()).map_err(|e| e.to_string())
            } else {
                let entry = &self.plugins.plugins[ix];
                let host = entry.host.as_ref().unwrap().clone();
                host.borrow_mut().actions.clear();
                let r = entry.js.as_ref().unwrap().page_active(&env);
                host.borrow_mut().actions.clear();
                r
            };
            match r {
                Err(e) => self.plugin_broken(ix, e, cx),
                Ok(hit) => {
                    if hit == "1" {
                        notified += 1;
                    } else {
                        eprintln!("[pf-plugin] page_actived 未导出（ix={ix}）");
                    }
                }
            }
        }
        self.plugins.row_cache.clear();
        self.plugins.fn_bar_cache.clear();
        eprintln!(
            "[pf-plugin] page_actived 派发 key={} 插件数={} 通知={}",
            session_key, n, notified
        );
        notified
    }

    pub(crate) fn plugin_broadcast_fire(
        &mut self,
        ix: usize,
        name: &str,
        payload: &str,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.plugin_ready(ix, cx) {
            return;
        }
        if self.plugins.plugins[ix].wasm.is_some() {
            let (res, actions) = {
                let entry = &mut self.plugins.plugins[ix];
                let Some(inst) = entry.wasm.as_mut() else {
                    return;
                };
                (
                    inst.on_event("broadcast", name, payload),
                    inst.pump_actions(),
                )
            };
            match res {
                Ok(()) => {
                    for a in actions {
                        match a {
                            PluginAction::Toast(m) => self.set_status(m, cx),
                            // 意图调用可达（newDraft 无窗口依赖）；窗口类动作
                            // 丢弃（broadcast 纯数据回调口径，与 JS 分支一致）
                            PluginAction::NewDraft { prefix, cwd } => {
                                if let Err(e) = self.plugin_new_draft(ix, &prefix, &cwd, cx) {
                                    self.set_status(format!("newDraft 失败：{e}"), cx);
                                }
                            }
                            _ => {}
                        }
                    }
                    self.plugin_wasm_render(ix, cx);
                }
                Err(e) => self.plugin_broken(ix, format!("{e}"), cx),
            }
            return;
        }
        let (result, actions) = {
            let entry = &mut self.plugins.plugins[ix];
            let host = entry.host.as_ref().unwrap().clone();
            host.borrow_mut().actions.clear();
            let js = entry.js.as_ref().unwrap();
            let r = js.fire_broadcast(name, payload);
            let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
            (r, drained)
        };
        match result {
            Ok(()) => {
                for a in actions {
                    if let JsAction::Toast(m) = a {
                        self.set_status(m, cx);
                    }
                }
                self.plugin_refresh(ix, cx);
            }
            Err(e) => self.plugin_broken(ix, e, cx),
        }
    }

    /// wasm 面板事件回灌（`contributes.*` 回调统一走这里 → on-event）。
    /// 事件处理完必重渲染（插件数据可能改了）。
    pub(crate) fn plugin_wasm_event(
        &mut self,
        ix: usize,
        kind: &str,
        id: &str,
        path: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.plugin_sync_quick_pins();
        let (res, actions) = {
            let entry = &mut self.plugins.plugins[ix];
            let Some(inst) = entry.wasm.as_mut() else {
                return;
            };
            (inst.on_event(kind, id, path), inst.pump_actions())
        };
        match res {
            Ok(()) => {
                self.plugin_wasm_dispatch(ix, actions, window, cx);
                self.plugin_wasm_render(ix, cx);
            }
            Err(e) => self.plugin_broken(ix, format!("{e}"), cx),
        }
    }

    /// wasm `PluginAction` → 宿主实现（与 JS 动作同款落点，复用实现不复制逻辑）。
    fn plugin_wasm_dispatch(
        &mut self,
        ix: usize,
        actions: Vec<PluginAction>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        for a in actions {
            match a {
                PluginAction::Toast(m) => self.set_status(m, cx),
                PluginAction::OpenFile(p) => self.open_file_tab(PathBuf::from(&p), cx),
                // 意图调用（契约 v2.4 sessions）：与 JS 分支同款落点——建线
                // 后经 broadcast "claim" 回推，失败经状态栏透传
                PluginAction::NewDraft { prefix, cwd } => {
                    if let Err(e) = self.plugin_new_draft(ix, &prefix, &cwd, cx) {
                        self.set_status(format!("newDraft 失败：{e}"), cx);
                    }
                }
                PluginAction::RunScript(p) => {
                    let path = PathBuf::from(&p);
                    match crate::services::script_exec::resolve(&path, self.ws.cwd.as_deref()) {
                        Some(plan) => self.open_script_terminal(path, plan, window, cx),
                        None => self.set_status(format!("无法执行：{p}"), cx),
                    }
                }
                // 契约 v3 act.openPopup：与 JS JsAction::OpenPopup 同落点
                PluginAction::OpenPopup(id) => self.plugin_popup_open(ix, Some(&id), window, cx),
            }
        }
        // 动作可能改了插件数据（如 toggle pin）：同步快照给后续 render
        self.plugin_sync_quick_pins();
    }

    /// 快照当前项目收藏 → host 钩子（fn 指针拿不到 Chat，走 thread_local 桥）。
    fn plugin_sync_quick_pins(&self) {
        let pins: Vec<String> = self
            .ws
            .pinned
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        QUICK_PIN_SNAP.with(|c| *c.borrow_mut() = pins);
    }

    /// 插件异常隔离（02 §4）：捕获 → toast → 会话级禁用，不影响宿主。
    fn plugin_broken(&mut self, ix: usize, err: String, cx: &mut gpui::Context<Self>) {
        eprintln!("[pf-plugin] 插件熔断 ix={ix}: {err}");
        let Some(entry) = self.plugins.plugins.get_mut(ix) else {
            return;
        };
        entry.disabled = true;
        entry.js = None;
        entry.wasm = None;
        entry.wasm_focus = None;
        entry.host = None;
        let id = entry.manifest.id.clone();
        self.set_status(format!("插件 {id} 出错：{err}"), cx);
    }
}

/// 清单 → 运行条目（disabled 一律 false 起步：停用已在扫描层被台账过滤；
/// 运行中的禁用/复用走原地翻 entry.disabled 位，090-4 v5）。
pub(crate) fn plugin_entries(manifests: Vec<PluginManifest>) -> Vec<PluginEntry> {
    manifests
        .into_iter()
        .map(|manifest| PluginEntry {
            manifest,
            disabled: false,
            js: None,
            wasm: None,
            wasm_focus: None,
            host: None,
            tree_loaded: false,
            cell_menu: None,
            split_ratios: std::cell::RefCell::new(Vec::new()),
            split_drag: std::cell::RefCell::new(None),
            split_hover: std::cell::RefCell::new(None),
            section_scrolls: std::cell::RefCell::new(Vec::new()),
            body: None,
        })
        .collect()
}

/// 启动扫描（spawn_boot_tasks 调；首帧后异步落位，微秒级/个）。
///
/// 两段式，**主线程零重活**：
/// ① 清单扫描（后台线程）→ JS 插件条目立即落位（状态栏 tab / 右键【插件】可见）；
/// ② 内层 wasm 插件验签门（090-5 L3）：**揭幕后**才在后台线程发起，且走
///    编译产物缓存（`load_cached`，命中 = mmap 反序列化 → 毫秒级）。
///
/// **验签结论不向用户展示**（正版校验/激活/拒载一类提示一律不出）——插件
/// 能不能用看它自己的面板表现，启动不播报；结论只进 stderr 供开发查。
pub(crate) fn spawn_plugin_scan(cx: &mut gpui::Context<Chat>) {
    // 组件状态广播（契约 v2.4）：observe Chat 自身，差分派发给订阅插件
    broadcast::attach(cx);
    // 安装尾声钩子：wasm → cwasm 预编译压在安装那一次做完（pi-link 不依赖
    // wasmtime，故由 app 注入）。纯函数指针，零开销。
    pi_link::pf_plugins::install_prepare_hook(install_prepare);
    // 宿主证明钩子**启动即注入**（进程级，线程无关）：晚注入会让先加载的
    // wasm 插件读到 fallback 占位符 → 误判「fork/无证明构建」而拒载。
    pf_plugin_host::install_proof_hook(host_proof_material);
    // 路径派生能力（与 JS 插件 ctx.fileIcon / ctx.isExecutive 同一套服务）
    pf_plugin_host::install_path_hooks(
        |p| crate::services::file_icons::get_icon(Path::new(p)).to_string(),
        |p| crate::services::script_exec::exec_kind(Path::new(p)).is_some(),
    );
    cx.spawn(async move |this, cx| {
        let manifests = cx
            .background_executor()
            .spawn(async move {
                // 先清无主残留（卸载时被占用留下的 .cache 等），再扫
                pi_link::pf_plugins::sweep_orphans();
                scan_manifests()
            })
            .await;
        let (wasm, js): (Vec<_>, Vec<_>) = manifests.into_iter().partition(|m| m.wasm.is_some());
        // ① 全部落位（JS+wasm）：状态栏 tab 立即可见；wasm 实例本身仍是懒装载
        //   （首次开面板才建，见 plugin_wasm_ready），落位只是清单/入口
        let _ = this.update(cx, |c, cx| {
            let mut entries = plugin_entries(js);
            entries.extend(plugin_entries(wasm.clone()));
            c.plugins_set(entries, cx);
        });
        if wasm.is_empty() {
            return;
        }
        // ② 等揭幕：验签是秒级 CPU 重活，不和启动抢核（揭幕前只有启动页）
        loop {
            if this.update(cx, |c, _| c.pool.booted).unwrap_or(true) {
                break;
            }
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
        }
        // ③ 验签门：拒载的包置 disabled（tab 隐藏，面板点不开）；通过的无事
        //   （实例是懒装载的，这里只做安全把关 + stderr 播报）
        let rejected = cx
            .background_executor()
            .spawn(async move { verify_wasm_plugins(wasm) })
            .await;
        if !rejected.is_empty() {
            let _ = this.update(cx, |c, _| c.plugin_mark_rejected(&rejected));
        }
    })
    .detach();
}

// ---------------------------------------------------------------------------
// 内层 wasm 插件：宿主能力钩子桥（fn 指针拿不到 Chat/文本系统 → thread_local）
// ---------------------------------------------------------------------------

thread_local! {
    /// 当前项目收藏快照（插件 `quick-pins` import 的数据源；fn 指针拿不到
    /// Chat，每次 wasm 面板装载/动作后由 `plugin_sync_quick_pins` 重灌）
    static QUICK_PIN_SNAP: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// gpui 文本系统（`measure-text` import 用；`Arc`，首装后全程存活）
    static TEXT_SYSTEM: std::cell::RefCell<Option<std::sync::Arc<gpui::TextSystem>>> =
        const { std::cell::RefCell::new(None) };
}

fn quick_pin_snap() -> Vec<String> {
    QUICK_PIN_SNAP.with(|c| c.borrow().clone())
}

/// 插件声明的图标引用 → 可渲染资产路径（内置名 / `assets:x.svg` / 完整路径）。
/// 与面板内 `button.icon` 同一套规则，供清单 `file_menu.icon` 等复用。
pub(crate) fn plugin_icon_asset(plugin_id: &str, icon: &str) -> Option<String> {
    if icon.is_empty() {
        return None;
    }
    if icon.contains('/') {
        return Some(icon.to_string());
    }
    if let Some(name) = icon.strip_prefix("assets:") {
        return Some(format!("plugins/{plugin_id}/assets/{name}"));
    }
    if let Some(name) = icon.strip_prefix("assets/") {
        return Some(format!("plugins/{plugin_id}/{name}"));
    }
    Some(format!("icons/{icon}.svg"))
}

/// 状态栏 tab 图标的共享串（`plugins/<slug>/<assets 相对路径>` 或宿主内置名）。
pub(crate) fn status_bar_icon_path(entry: &PluginEntry) -> Option<SharedString> {
    let sb = entry.manifest.status_bar.as_ref()?;
    Some(contrib_icon_path(&entry.manifest.id, &sb.icon))
}

/// topbar「菜单」条目图标路径（契约 v2.5，eb6）：与 status_bar 同一映射。
pub(crate) fn topbar_menu_icon_path(entry: &PluginEntry) -> Option<SharedString> {
    let tb = entry.manifest.topbar_menu.as_ref()?;
    Some(contrib_icon_path(&entry.manifest.id, &tb.icon))
}

impl Chat {
    /// 会话列表徽标（契约 v3）：命中该会话的**全部**徽标 → [(icon 资产路径, tone)]，
    /// 每插件一条（owner 唯一）。绘制归宿主：从右到左、间隔 10px 全画出（用户定夺
    /// 2026-10-28），不裁不并。
    pub(crate) fn plugin_badge(
        &mut self,
        session: &std::path::Path,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<(SharedString, String)> {
        let key = session.to_string_lossy().into_owned();
        let mut out: Vec<(SharedString, String)> = badges_for(&self.plugins.badges, session);
        const TTL: std::time::Duration = std::time::Duration::from_millis(1000);
        let n = self.plugins.plugins.len();
        for ix in 0..n {
            // 能力面（8tb.2）：徽标是每行渲染结果，载体无关——JS 未导出 /
            // wasm 未实现都回 None（负缓存 TTL 内不再问）
            let (pkg, capable) = {
                let e = &self.plugins.plugins[ix];
                (e.manifest.pkg.clone(), !e.disabled)
            };
            if !capable {
                continue;
            }
            if let Some(c) = self
                .plugins
                .row_cache
                .iter()
                .find(|c| c.pkg == pkg && c.session_key == key && c.at.elapsed() < TTL)
            {
                if let Some((icon, tone)) = &c.badge {
                    let id = self.plugins.plugins[ix].manifest.id.clone();
                    out.push((contrib_icon_path(&id, icon), tone.clone()));
                }
                continue;
            }
            if !self.plugin_ready(ix, cx) {
                continue;
            }
            let env = serde_json::json!({ "session": { "key": key } }).to_string();
            let (r, _) = {
                let entry = &mut self.plugins.plugins[ix];
                entry.hook_session_row(&env)
            };
            let badge = match r {
                Ok(Some(json)) => {
                    let v: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
                    let icon = v
                        .get("icon")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let tone = v
                        .get("tone")
                        .and_then(Value::as_str)
                        .unwrap_or("accent")
                        .to_string();
                    eprintln!(
                        "[pf-plugin] session_list_render 命中 pkg={} session={} icon={}",
                        self.plugins.plugins[ix].manifest.pkg, key, icon
                    );
                    if icon.is_empty() {
                        None
                    } else {
                        let id = self.plugins.plugins[ix].manifest.id.clone();
                        out.push((contrib_icon_path(&id, &icon), tone.clone()));
                        Some((icon, tone))
                    }
                }
                _ => None,
            };
            self.plugins.row_cache.push(RowCache {
                pkg,
                session_key: key.clone(),
                badge,
                at: std::time::Instant::now(),
            });
        }
        out
    }
}

/// 命中会话的全部徽标（纯函数，测试锚点）：过滤 + icon 映射，**保序**
/// （声明序 = 绘制从右到左，首条最右）。
pub(crate) fn badges_for(
    badges: &[PluginBadge],
    session: &std::path::Path,
) -> Vec<(SharedString, String)> {
    badges
        .iter()
        .filter(|b| {
            crate::services::workspace::same_path(std::path::Path::new(&b.session), session)
        })
        .map(|b| (contrib_icon_path(&b.owner, &b.icon), b.tone.clone()))
        .collect()
}

/// 贡献项图标三来源映射：宿主内置名 / `assets:<名>` / `assets/` 相对路径。
fn contrib_icon_path(id: &str, icon: &str) -> SharedString {
    // `assets/` 是目录结构的一段，**不能剥**——曾剥掉导致
    // plugins/<slug>/bolt.svg NotFound（图标空白，还进负缓存）
    if icon.starts_with("assets/") {
        SharedString::from(format!("plugins/{id}/{icon}"))
    } else if let Some(name) = icon.strip_prefix("assets:") {
        SharedString::from(format!("plugins/{id}/assets/{name}"))
    } else {
        SharedString::from(format!("icons/{icon}.svg"))
    }
}

pub(crate) use panel::{cell_menu_layer, plugin_panel_view, plugin_popup_layer};

/// 会话功能栏插件 icon（契约 v3 fn_bar）：每插件一颗 icon 钮（tone 着色、
/// tip 悬浮），点击 = fn_bar.command 回环（env 带当前会话键）。
pub(crate) fn fn_bar_plugin_elements(
    chat: &mut Chat,
    cx: &mut gpui::Context<Chat>,
) -> Vec<gpui::AnyElement> {
    // 当前会话键：**以标签页的会话路径为准**（= 用户看到的那个会话）。
    // 不能用「活跃 runtime 的 state.session_file」兜底：新建/刚切过去的会话还没
    // 起自己的 pi 进程时，runtime 池里的可能是上一个会话 → 点 icon 绑到旧会话
    // （真机：切到新会话扫码，绑定仍落在旧会话）。runtime 状态只做最后兜底。
    let session_key = {
        let tab = chat.pool.active_key.to_string();
        if tab.ends_with(".jsonl") {
            tab
        } else {
            chat.pool
                .runtimes
                .get(&chat.pool.active_key)
                .and_then(|rt| {
                    rt.read(cx)
                        .state
                        .as_ref()
                        .and_then(|s| s.session_file.clone().or_else(|| s.session_id.clone()))
                })
                .unwrap_or_default()
        }
    };
    let chromes = chat.plugin_fn_bar_chrome(&session_key, cx);
    let weak = cx.entity().downgrade();
    chromes
        .into_iter()
        .map(|(ix, icon_path, tone, tip)| {
            let t = crate::appearance::theme::theme();
            // 002：绿=已绑定。kitchen 等主题 accent 是红，fn_bar 的 accent
            // 固定映射主题绿（appearance::UNREAD，与旧版微信 icon 同色）
            let color = if tone == "accent" {
                crate::appearance::theme::UNREAD
            } else {
                t.text_muted
            };
            let weak = weak.clone();
            let key = session_key.clone();
            let mut el = div()
                .id(SharedString::from(format!("pf-fnbar-{ix}")))
                .flex()
                .items_center()
                .justify_center()
                .w(px(32.))
                .h(px(32.))
                .mr(px(20.)) // 按钮右侧留 20px（不压缩 icon，用户定夺）
                .rounded(px(6.))
                .cursor_pointer()
                .hover(move |s| s.bg(rgb(t.bg_hover)));
            if !tip.is_empty() {
                el = el.with_tip_text(tip);
            }
            el.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                let _ = weak.update(cx, |c, cx| {
                    c.plugin_fn_bar_click(ix, &key, window, cx);
                    cx.notify();
                });
            })
            .child(
                gpui::svg()
                    .path(icon_path)
                    .text_color(rgb(color))
                    .size(px(24.)), // fn 槽 icon = XL 档 24（用户定夺 2026-10-28，002「大样式」）
            )
            .into_any_element()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_badge_multi_owner_same_session_all_kept() {
        // 多插件同会话：全部保留、保序（从右到左 = 声明序），不裁不并（用户定夺 2026-10-28）
        let ses = "C:/x/a.jsonl";
        let badges = vec![
            PluginBadge {
                owner: "pf-wxbot".into(),
                session: ses.into(),
                icon: "assets:wx.svg".into(),
                tone: "accent".into(),
            },
            PluginBadge {
                owner: "other".into(),
                session: ses.into(),
                icon: "assets:o.svg".into(),
                tone: "faint".into(),
            },
            PluginBadge {
                owner: "third".into(),
                session: "C:/x/b.jsonl".into(),
                icon: "assets:t.svg".into(),
                tone: "accent".into(),
            },
        ];
        let hit = badges_for(&badges, std::path::Path::new(ses));
        assert_eq!(hit.len(), 2);
        assert_eq!(
            hit[0],
            ("plugins/pf-wxbot/assets/wx.svg".into(), "accent".into())
        );
        assert_eq!(
            hit[1],
            ("plugins/other/assets/o.svg".into(), "faint".into())
        );
        // 路径大小写变体也命中（same_path 归一化）
        assert_eq!(
            badges_for(&badges, std::path::Path::new("c:/X/A.jsonl")).len(),
            2
        );
        // 空列表 → 空
        assert!(badges_for(&[], std::path::Path::new(ses)).is_empty());
    }

    #[allow(dead_code)]
    fn __anchor() {}

    fn write_manifest(dir: &Path, body: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join("pf-plugin.toml");
        std::fs::write(&p, body).unwrap();
        p
    }

    const GOOD: &str = r#"
schema = 3
id = "quick-panel"
title = { zh = "快捷面板", en = "Quick Panel" }
version = "0.1.0"

[[contributes.status_bar]]
icon = "assets/bolt.svg"
title = { zh = "快捷" }

[[contributes.file_menu]]
id = "set-quick"
title = { zh = "设为快捷方式", en = "Pin Quick" }
icon = "pin"

[[contributes.panels]]
id = "quick"
title = { zh = "快捷" }
entry = "main.js"
"#;

    #[test]
    fn manifest_parse_and_validate() {
        let tmp = std::env::temp_dir().join(format!("pf-manifest-test-{}", std::process::id()));
        let dir = tmp.join("quick-panel");
        write_manifest(&dir, GOOD);
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();

        let m = parse_manifest(&dir).expect("合法清单应通过");
        assert_eq!(m.id, "quick-panel");
        assert_eq!(m.title_zh, "快捷面板");
        assert_eq!(m.title_en.as_deref(), Some("Quick Panel"));
        assert_eq!(m.status_bar.as_ref().unwrap().icon, "assets/bolt.svg");
        assert_eq!(m.file_menu.len(), 1);
        assert_eq!(m.file_menu[0].id, "set-quick");
        assert_eq!(m.panels[0].entry, "main.js");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 契约 v2.5 topbar_menu 解析（eb6）：条目入清单；重复声明拒绝
    #[test]
    fn manifest_topbar_menu_parse() {
        let tmp = std::env::temp_dir().join(format!("pf-topbar-test-{}", std::process::id()));
        let body = format!(
            "{GOOD}\n[[contributes.topbar_menu]]\ntitle = {{ zh = \"微信通话\" }}\nicon = \"assets/wx.svg\"\n"
        );
        let dir = tmp.join("quick-panel");
        write_manifest(&dir, &body);
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/wx.svg"), "<svg/>").unwrap();
        std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
        let m = parse_manifest(&dir).expect("topbar_menu 清单应通过");
        let tb = m.topbar_menu.as_ref().expect("条目应入清单");
        assert_eq!(tb.title_zh, "微信通话");
        assert_eq!(tb.icon, "assets/wx.svg");
        // 图标三来源映射（与 status_bar 共用 contrib_icon_path）
        assert_eq!(
            topbar_menu_icon_path(&plugin_entries(vec![m]).remove(0)).map(|s| s.to_string()),
            Some("plugins/quick-panel/assets/wx.svg".to_string())
        );
        // 重复段 = 拒绝
        let dir2 = tmp.join("dup");
        write_manifest(
            &dir2,
            &format!(
                "{body}\n[[contributes.topbar_menu]]\ntitle = {{ zh = \"第二条\" }}\nicon = \"bolt\"\n"
            ),
        );
        std::fs::write(dir2.join("main.js"), "export function render(){{}}").unwrap();
        assert!(parse_manifest(&dir2).is_err(), "topbar_menu 只能声明一个");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 契约 v2.4 sessions 解析：认领前缀声明段必须入清单（漏段曾把整份
    /// 清单拒在扫描关——wechat-bridge「未知段 [[contributes.sessions]]」）
    #[test]
    fn manifest_sessions_parse() {
        let tmp = std::env::temp_dir().join(format!("pf-sessions-test-{}", std::process::id()));
        let body = format!("{GOOD}\n[[contributes.sessions]]\nprefix = \"pf-wxbot-\"\n");
        let dir = tmp.join("quick-panel");
        write_manifest(&dir, &body);
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
        let m = parse_manifest(&dir).expect("含 sessions 段的清单必须通过");
        assert_eq!(m.sessions.len(), 1);
        assert_eq!(m.sessions[0].prefix, "pf-wxbot-");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 契约 v2.4 dock 解析：tab/panel/缺省/非法值四路
    /// 认领会话名：前缀 + 15 位时间戳（002 §5 口径，与 wx 线同构）
    #[test]
    fn claim_session_name_format() {
        let n = claim_session_name("mybot-");
        assert!(n.starts_with("mybot-"), "{n}");
        let ts = n.trim_start_matches("mybot-");
        assert_eq!(ts.len(), 15, "YYMMDDHHMMSS + 3位毫秒 = 15 位：{n}");
        assert!(ts.chars().all(|c| c.is_ascii_digit()), "{n}");
        // 唯一性由「前缀 + 毫秒」联合保证；同毫秒重名由认领槽替换语义吸收
        //（新建即换绑，旧名随换绑退役），不做 flaky 的跨调用断言
    }

    #[test]
    fn manifest_panel_dock_parse() {
        let tmp = std::env::temp_dir().join(format!("pf-dock-test-{}", std::process::id()));
        // tab 形态
        let dir = tmp.join("quick-panel");
        write_manifest(
            &dir,
            &GOOD.replace("entry = \"main.js\"", "entry = \"main.js\"\ndock = \"tab\""),
        );
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
        let mut m = parse_manifest(&dir).expect("tab 清单应通过");
        assert_eq!(m.panels[0].dock, PluginDock::Tab);
        m.pkg = "quick-panel".into(); // pkg 是扫描期回填字段，单测手工补
        assert_eq!(m.tab_page(), Some(("quick-panel", "quick")));

        // 缺省 = panel（老插件零迁移）
        let dir = tmp.join("quick-panel");
        write_manifest(&dir, GOOD);
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        let m = parse_manifest(&dir).expect("缺省清单应通过");
        assert_eq!(m.panels[0].dock, PluginDock::Panel);
        assert_eq!(m.tab_page(), None);

        // 非法值拒装（封闭词表）
        let dir = tmp.join("quick-panel");
        write_manifest(
            &dir,
            &GOOD.replace(
                "entry = \"main.js\"",
                "entry = \"main.js\"\ndock = \"sidebar\"",
            ),
        );
        let e = parse_manifest(&dir).unwrap_err();
        assert!(e.contains("dock"), "错误应提及 dock：{e}");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 契约 v2.4 pi_extensions 解析：路径清单进 manifest（Tier 2 门在 new_draft）
    #[test]
    fn manifest_pi_extensions_parse() {
        let tmp = std::env::temp_dir().join(format!("pf-piext-test-{}", std::process::id()));
        let body = GOOD.replace(
            "entry = \"main.js\"",
            "entry = \"main.js\"\n\n[[contributes.pi_extensions]]\npath = \"pi/bridge.js\"",
        );
        let dir = tmp.join("quick-panel");
        write_manifest(&dir, &body);
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
        let m = parse_manifest(&dir).expect("pi_extensions 清单应通过");
        assert_eq!(m.pi_extensions, vec!["pi/bridge.js".to_string()]);
        // 未声明 → 空
        let dir = tmp.join("quick-panel");
        write_manifest(&dir, GOOD);
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        let m = parse_manifest(&dir).expect("无 pi_extensions 应通过");
        assert!(m.pi_extensions.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 契约 v3 fn_bar / popups 解析：入清单 + command 必填 + size 封闭词表
    #[test]
    fn manifest_fn_bar_and_popups_parse() {
        let tmp = std::env::temp_dir().join(format!("pf-v3-test-{}", std::process::id()));
        let body = format!(
            "{GOOD}
[[contributes.fn_bar]]
command = \"bind\"

[[contributes.popups]]
id = \"qr\"
title = {{ zh = \"绑定微信\" }}
size = \"MD\"
entry = \"main.js\"
"
        );
        let dir = tmp.join("quick-panel");
        write_manifest(&dir, &body);
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
        let m = parse_manifest(&dir).expect("v3 清单应通过");
        assert_eq!(m.fn_bar.as_ref().unwrap().command, "bind");
        assert_eq!(m.popups.len(), 1);
        assert_eq!(m.popups[0].id, "qr");
        assert_eq!(m.popups[0].size, "MD");
        // fn_bar 缺 command = 拒
        let dir = tmp.join("bad-cmd");
        write_manifest(
            &dir,
            &body.replace(
                "command = \"bind\"
",
                "",
            ),
        );
        assert!(parse_manifest(&dir).is_err());
        // popup size 封闭词表
        let dir = tmp.join("bad-size");
        write_manifest(&dir, &body.replace("size = \"MD\"", "size = \"XL\""));
        assert!(parse_manifest(&dir).is_err());
        // fn_bar / popups 也是进驻面：仅这两段的包不是纯库包
        let dir = tmp.join("pf-wxbot");
        write_manifest(
            &dir,
            "schema = 3
id = \"pf-wxbot\"
title = { zh = \"x\" }
version = \"0.1.0\"

[[contributes.fn_bar]]
command = \"bind\"

[[contributes.popups]]
id = \"qr\"
title = { zh = \"弹窗\" }
entry = \"main.js\"
",
        );
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        let m = parse_manifest(&dir).expect("仅 fn_bar+popups 的包应通过");
        assert!(m.fn_bar.is_some() && m.popups.len() == 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn manifest_rejects() {
        let tmp = std::env::temp_dir().join(format!("pf-manifest-bad-{}", std::process::id()));
        // id ≠ 目录名
        let dir = tmp.join("other-name");
        write_manifest(
            &dir,
            &GOOD.replace("id = \"quick-panel\"", "id = \"quick-panel-x\""),
        );
        assert!(parse_manifest(&dir).is_err());
        // schema ≠ 支持的契约大版本（v3 起拒旧版；不降级运行）
        let dir = tmp.join("quick-panel");
        write_manifest(&dir, &GOOD.replace("schema = 3", "schema = 2"));
        assert!(parse_manifest(&dir).is_err());
        // 纯库包（无 contributes）
        let dir = tmp.join("quick-panel");
        write_manifest(
            &dir,
            "schema = 3\nid = \"quick-panel\"\ntitle = { zh = \"x\" }\nversion = \"0.1.0\"\n",
        );
        let e = parse_manifest(&dir).unwrap_err();
        assert!(e.contains("纯库"));
        // entry 不存在
        let dir = tmp.join("quick-panel");
        write_manifest(&dir, GOOD);
        assert!(parse_manifest(&dir).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn manifest_tolerates_inline_comments() {
        // 02 §2 示例清单自带行内注释；引号内的 # 是文案不是注释
        let tmp = std::env::temp_dir().join(format!("pf-manifest-cmt-{}", std::process::id()));
        let dir = tmp.join("quick-panel");
        write_manifest(
            &dir,
            "schema = 3  # 顶部注释
id = \"quick-panel\"   
title = { zh = \"快捷#面板\" } # 行内
version = \"0.1.0\"

[[contributes.panels]] # 段后注释
id = \"quick\"
title = { zh = \"快捷\" }
entry = \"main.js\" # 入口
",
        );
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        let m = parse_manifest(&dir).expect("带行内注释的清单应通过");
        assert_eq!(m.title_zh, "快捷#面板"); // 引号内 # 保留
        assert_eq!(m.panels[0].entry, "main.js");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn manifest_inner_package_parse() {
        // 加密包（inner_package）：无 entry 面板声明跳过文件校验；tab icon
        // 走宿主内置名（无 assets 文件也可过）——发版脚本 dist.mjs 的产物形态
        let tmp = std::env::temp_dir().join(format!("pf-inner-test-{}", std::process::id()));
        let dir = tmp.join("quick-panel");
        write_manifest(
            &dir,
            r#"
schema = 3
id = "quick-panel"
title = { zh = "快捷面板", en = "Quick Panel" }
version = "0.1.3"

wasm = "plugin.wasm"

[[contributes.status_bar]]
icon = "plug"
title = { zh = "快捷面板" }

[[contributes.panels]]
id = "quick-panel"
title = { zh = "快捷面板" }
"#,
        );
        std::fs::write(dir.join("plugin.wasm"), b"\0wasm-fake").unwrap();

        let m = parse_manifest(&dir).expect("加密包清单应通过（无 entry 文件）");
        assert_eq!(m.wasm.as_deref(), Some("plugin.wasm"));
        assert_eq!(m.status_bar.as_ref().unwrap().icon, "plug");
        assert_eq!(m.panels.len(), 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 两档布局都必须给出**同一个顶层包目录名**——停用过滤与「台账 ↔ 运行态」
    /// 对账都靠它；下钻出来的版本目录名只是装载位置，不是身份。
    #[test]
    fn scan_candidates_keeps_top_pkg() {
        let tmp = std::env::temp_dir().join(format!("pf-scan-pkg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        // market 安装链布局：plugins/<pkg>/<version>/pf-plugin.toml
        write_manifest(
            &tmp.join("@scope__quick/0.1.0"),
            &GOOD.replace("id = \"quick-panel\"", "id = \"quick\""),
        );
        // dev 手工平铺布局：plugins/<pkg>/pf-plugin.toml
        write_manifest(&tmp.join("flat"), &GOOD);

        let mut got: Vec<(String, String)> = scan_candidates(&tmp)
            .into_iter()
            .map(|(pkg, dir)| {
                (
                    pkg,
                    dir.strip_prefix(&tmp)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                )
            })
            .collect();
        got.sort();
        assert_eq!(
            got,
            vec![
                (
                    "@scope__quick".to_string(),
                    "@scope__quick/0.1.0".to_string()
                ),
                ("flat".to_string(), "flat".to_string()),
            ]
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
// 临时探针（勿入库）：直接跑 scan_manifests 看装载产物
