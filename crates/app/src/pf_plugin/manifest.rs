//! PF 插件清单（02 §2；手写极简解析，不引 toml 库——设计定案）：
//! [`PluginManifest`] + 8 贡献点 + TomlVal/parse_manifest + 启动扫描
//! （scan_manifests/scan_candidates）+ 插件资产注册表（PLUGIN_ASSETS/DIRS）。
//! 090 重构自 mod.rs 拆出；解析/扫描 tests 随迁。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// 清单（02 §2；手写极简解析，不引 toml 库——设计定案）
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
pub(crate) fn parse_manifest(dir: &Path) -> Result<PluginManifest, String> {
    let path = dir.join("pf-plugin.toml");
    let src =
        std::fs::read_to_string(&path).map_err(|e| format!("{} 不可读: {e}", path.display()))?;

    let mut m = PluginManifest {
        dir: dir.to_path_buf(),
        pkg_dir: dir.to_path_buf(), // 平铺默认；版本布局在目录名对账处上钻
        pkg: String::new(),         // 扫描期回填（顶层包目录名）
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
        service: Vec::new(),
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
                "contributes.service" => {
                    m.service.push(ServiceContrib {
                        id: String::new(),
                        scope: String::new(),
                        entry: String::new(),
                    });
                    "service".to_string()
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
                if got != SUPPORTED_MANIFEST_SCHEMA && got != 3 && got != 4 {
                    return Err(format!(
                        "pf-plugin.toml：清单 schema {got} 不被本版 PF 支持（支持 {SUPPORTED_MANIFEST_SCHEMA}，兼容 3/4）"
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
            ("pi_extensions", "path") => {
                if let Some(last) = m.pi_extensions.last_mut() {
                    *last = val.as_str("path", n)?.to_string();
                }
            }
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
            ("service", "id") => {
                m.service.last_mut().unwrap().id = val.as_str("id", n)?.to_string();
            }
            ("service", "scope") => {
                m.service.last_mut().unwrap().scope = val.as_str("scope", n)?.to_string();
            }
            ("service", "entry") => {
                m.service.last_mut().unwrap().entry = val.as_str("entry", n)?.to_string();
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
        && let Some(parent) = dir.parent()
        && let Some(p) = parent.file_name().and_then(|s| s.to_str())
    {
        base = p; // 版本目录 → 上钻到插件目录
        m.pkg_dir = parent.to_path_buf(); // 用户数据挂顶层包目录（data_dir）
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
    // 契约 v4：service 校验（id kebab / scope 封闭 / entry 非空）
    for sc in &m.service {
        let ok = !sc.id.is_empty()
            && sc
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !sc.id.starts_with('-')
            && !sc.id.ends_with('-');
        if !ok {
            return Err(format!(
                "pf-plugin.toml：service.id「{}」须 kebab-case",
                sc.id
            ));
        }
        if sc.scope != "session" {
            return Err(format!(
                "pf-plugin.toml：service.scope 只认 \"session\"，得到 {:?}",
                sc.scope
            ));
        }
        if sc.entry.is_empty() {
            return Err("pf-plugin.toml：service.entry 不能为空".into());
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
        // title 可空：空 = 不画标题栏（旧版弹窗无标题栏形态，用户 2026-10-28 定夺）
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
/// 扫描结果：可装载清单 + 台账里登记却装不上的「失效包」（090-10 迁移体验）。
pub(crate) struct ScanOutcome {
    pub manifests: Vec<PluginManifest>,
    pub stale: Vec<StalePlugin>,
}

/// 台账启用、磁盘有清单、但本版 PF 拒载的包。用户可见信号的唯一数据源
/// （启动 toast + 市场头部通知行 + 已安装页标签）；此前只进 stderr，release
/// 下用户看到的就是插件凭空消失。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StalePlugin {
    /// 台账包名（`@scope/name`）——市场 / 已安装页按它对账
    pub name: String,
    /// 清单 version（peek 行扫；缺则取版本目录名）
    pub version: String,
    /// 清单 title.zh（peek；缺则回落短包名）
    pub title_zh: String,
    pub kind: StaleKind,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StaleKind {
    /// 清单 schema 不在本版支持集 → 「需更新插件」（本轮 = 旧 schema 2 包）
    SchemaUnsupported { got: u32 },
    /// 其他清单错误 → 「包损坏」（原因原文）
    Invalid(String),
}

/// **装载白名单 = 台账**（唯一安装真相）：只有台账里登记且未停用的包才装载，
/// 磁盘上多出来的目录（手拷 / 卸载残留 / 历史野包）一律不装载——否则就会
/// 「市场说未安装、左面板却挂着它」（090-4 v5b 踩过）。
///
/// 薄包装：只要清单。**render 路径也在调它**（已安装页标题回落），所以这里
/// 不能有副作用；收集失效包走 [`scan_manifests_full`]。
pub(crate) fn scan_manifests() -> Vec<PluginManifest> {
    scan_manifests_full().manifests
}

/// 带失效包收集的扫描（启动链 / 单包重载用）。
pub(crate) fn scan_manifests_full() -> ScanOutcome {
    let Some(root) = pi_link::paths::plugins_dir() else {
        return ScanOutcome {
            manifests: Vec::new(),
            stale: Vec::new(),
        };
    };
    scan_root(&root, &pi_link::pf_plugins::load_ledger())
}

/// 扫描本体（显式 root + 台账，单测直接喂）。
pub(crate) fn scan_root(root: &Path, ledger: &[pi_link::pf_plugins::LedgerEntry]) -> ScanOutcome {
    // 目录别名（`@scope__name` 与平铺 `name` 都认）→ (台账包名, 台账版本)；只含未停用的
    let enabled: std::collections::HashMap<String, (String, String)> = ledger
        .iter()
        .filter(|l| !l.disabled)
        .flat_map(|l| {
            pi_link::pf_plugins::dir_aliases(&l.name)
                .into_iter()
                .map(move |a| (a, (l.name.clone(), l.version.clone())))
        })
        .collect();
    let mut out = ScanOutcome {
        manifests: Vec::new(),
        stale: Vec::new(),
    };
    let mut candidates = scan_candidates(root);
    candidates.sort_by(|a, b| a.1.cmp(&b.1));
    for (pkg, dir) in candidates {
        // 准入/停用判定必须看**顶层包目录名**：版本层目录名是版本号，拿它比
        // 包名永远比不中（停用过滤失效 → 重启后照旧装载，090-4 v5 踩过）
        let Some((ledger_name, ledger_version)) = enabled.get(&pkg) else {
            continue; // 未安装（台账无此包）或已停用，不装载不绘制
        };
        // 版本层布局**只认台账版本那一个目录**（台账 = 唯一安装真相）：更新后
        // 旧版本目录若还没清掉（被占用 / 上版 PF 没清），两版清单都合法时会
        // 双装载——双 tab、双右键项，且 register_plugin_dir 按字符串序 last-wins
        // （0.1.10 < 0.1.9）。平铺布局 dir == <pkg>，无版本可比，照旧。
        if dir != root.join(&pkg)
            && dir.file_name().and_then(|n| n.to_str()) != Some(ledger_version.as_str())
        {
            continue; // 非台账版本的残留目录：不装载、不算失效（prune 会清）
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
                out.manifests.push(m);
            }
            Err(e) => {
                // 拒载原因进 stale 走 toast/市场提示；stderr 只在 debug 构建打
                if cfg!(debug_assertions) {
                    eprintln!("[pf-plugin] 跳过 {}：{e}", dir.display());
                }
                out.stale.push(stale_of(ledger_name, &pkg, &dir, e));
            }
        }
    }
    out
}

/// 拒载清单 → 失效包条目：schema 不支持单列（可操作 = 去更新），其余归损坏。
fn stale_of(ledger_name: &str, pkg: &str, dir: &Path, err: String) -> StalePlugin {
    let head = peek_manifest_head(dir);
    let kind = match head.schema {
        Some(got) if got != SUPPORTED_MANIFEST_SCHEMA && got != 3 && got != 4 => {
            StaleKind::SchemaUnsupported { got }
        }
        _ => StaleKind::Invalid(err),
    };
    let version = head.version.unwrap_or_else(|| {
        dir.file_name()
            .and_then(|n| n.to_str())
            .filter(|n| n.starts_with(|c: char| c.is_ascii_digit()))
            .unwrap_or("")
            .to_string()
    });
    let title_zh = head
        .title_zh
        .unwrap_or_else(|| pkg.rsplit("__").next().unwrap_or(pkg).to_string());
    StalePlugin {
        name: ledger_name.to_string(),
        version,
        title_zh,
        kind,
    }
}

/// 清单头三字段的容错行扫（`schema` / `version` / `title.zh`），只看第一个
/// `[` 段之前的顶层行。给失效包取元数据用——此时 `parse_manifest` 已经拒了，
/// 不能再依赖它；也**不**做校验，拿到什么算什么。
#[derive(Default)]
struct ManifestHead {
    schema: Option<u32>,
    version: Option<String>,
    title_zh: Option<String>,
}

fn peek_manifest_head(dir: &Path) -> ManifestHead {
    let mut head = ManifestHead::default();
    let Ok(src) = std::fs::read_to_string(dir.join("pf-plugin.toml")) else {
        return head;
    };
    for line in src.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            break;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "schema" => head.schema = v.split('#').next().unwrap_or("").trim().parse().ok(),
            "version" => head.version = quoted(v),
            "title" => {
                // title = { zh = "快捷面板", en = "Quick Panel" }
                head.title_zh = v
                    .split_once("zh")
                    .and_then(|(_, rest)| rest.split_once('='))
                    .and_then(|(_, rest)| quoted(rest.trim()));
            }
            _ => {}
        }
    }
    head
}

/// 取行首的 `"..."` 字面量内容。
fn quoted(v: &str) -> Option<String> {
    let rest = v.strip_prefix('"')?;
    rest.split_once('"').map(|(s, _)| s.to_string())
}

/// 按包名直读清单标题（已装列表用）：停用插件被 scan 过滤、没有清单条目，
/// 显示名不能因此跌回裸包名——盘上清单还在，title 直读即可。
pub(crate) fn manifest_title_for(pkg: &str) -> Option<String> {
    for alias in pi_link::pf_plugins::dir_aliases(pkg) {
        let dir = pi_link::paths::plugins_dir()?.join(&alias);
        if dir.join("pf-plugin.toml").is_file() {
            if let Ok(m) = parse_manifest(&dir) {
                return Some(m.title_zh);
            }
        }
    }
    None
}

/// 已装插件清单声明的认领前缀全集（契约 v2.4 `[[contributes.sessions]]`）。
/// 启动恢复的**影子会话铁闸**数据源：直接读磁盘清单（后台线程可调），
/// 不看运行期台账——停用/未登记的包也算：它的认领线残留会话同样不该
/// 被桌面恢复接走。PF 不内置任何插件身份，前缀全部来自插件自声明。
pub(crate) fn declared_claim_prefixes() -> Vec<String> {
    let Some(root) = pi_link::paths::plugins_dir() else {
        return Vec::new();
    };
    scan_candidates(&root)
        .iter()
        .filter_map(|(_, dir)| parse_manifest(dir).ok())
        .flat_map(|m| m.sessions.into_iter().map(|s| s.prefix))
        .collect()
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
        if cfg!(debug_assertions) {
            eprintln!("[pf-plugin] asset {rel} -> {} bytes", b.len());
        }
    } else {
        // 未找到不缓存：扫描登记晚于首帧渲染时，下帧重查即中
        if cfg!(debug_assertions) {
            eprintln!("[pf-plugin] asset {rel} -> 未找到");
        }
        return None;
    }
    if let Ok(mut c) = cache.lock() {
        c.insert(rel.to_string(), bytes.clone());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    // 徽标/图标映射纯函数住在 entry.rs（crate::pf_plugin 顶层 re-export）
    use crate::pf_plugin::*;

    #[test]
    fn plugin_badge_multi_owner_same_session_all_kept() {
        // 多插件同会话：全部保留、保序（从右到左 = 声明序），不裁不并
        let ses = "C:/x/a.jsonl";
        let badges = vec![
            PluginBadge {
                owner: "pf-demo".into(),
                session: ses.into(),
                icon: "assets:x.svg".into(),
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
            ("plugins/pf-demo/assets/x.svg".into(), "accent".into())
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
schema = 4
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
            "{GOOD}\n[[contributes.topbar_menu]]\ntitle = {{ zh = \"面板标题\" }}\nicon = \"assets/top.svg\"\n"
        );
        let dir = tmp.join("quick-panel");
        write_manifest(&dir, &body);
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/top.svg"), "<svg/>").unwrap();
        std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
        let m = parse_manifest(&dir).expect("topbar_menu 清单应通过");
        let tb = m.topbar_menu.as_ref().expect("条目应入清单");
        assert_eq!(tb.title_zh, "面板标题");
        assert_eq!(tb.icon, "assets/top.svg");
        // 图标三来源映射（与 status_bar 共用 contrib_icon_path）
        assert_eq!(
            topbar_menu_icon_path(&plugin_entries(vec![m]).remove(0)).map(|s| s.to_string()),
            Some("plugins/quick-panel/assets/top.svg".to_string())
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
    /// 清单拒在扫描关——插件包「未知段 [[contributes.sessions]]」事故）
    #[test]
    fn manifest_sessions_parse() {
        let tmp = std::env::temp_dir().join(format!("pf-sessions-test-{}", std::process::id()));
        let body = format!("{GOOD}\n[[contributes.sessions]]\nprefix = \"pf-demo-\"\n");
        let dir = tmp.join("quick-panel");
        write_manifest(&dir, &body);
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
        let m = parse_manifest(&dir).expect("含 sessions 段的清单必须通过");
        assert_eq!(m.sessions.len(), 1);
        assert_eq!(m.sessions[0].prefix, "pf-demo-");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 契约 v2.4 dock 解析：tab/panel/缺省/非法值四路
    /// 认领会话名：前缀 + 15 位时间戳（§5 口径，各插件认领线同构）
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

    /// 契约 v4 service 解析：入清单 + scope 封闭；pi_extensions 已移除（声明即错）
    #[test]
    fn manifest_service_parse() {
        let tmp = std::env::temp_dir().join(format!("pf-svc-test-{}", std::process::id()));
        let body = GOOD.replace(
            "entry = \"main.js\"",
            "entry = \"main.js\"

[[contributes.service]]
id = \"demo-svc\"
scope = \"session\"
entry = \"pi/index.mjs\"",
        );
        let dir = tmp.join("quick-panel");
        write_manifest(&dir, &body);
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
        let m = parse_manifest(&dir).expect("service 清单应通过");
        assert_eq!(m.service.len(), 1);
        assert_eq!(m.service[0].id, "demo-svc");
        assert_eq!(m.service[0].scope, "session");
        assert_eq!(m.service[0].entry, "pi/index.mjs");
        // scope 封闭（本版仅 session）
        let bad = GOOD.replace(
            "entry = \"main.js\"",
            "entry = \"main.js\"

[[contributes.service]]
id = \"demo-svc\"
scope = \"global\"
entry = \"pi/index.mjs\"",
        );
        write_manifest(&dir, &bad);
        let e = parse_manifest(&dir).expect_err("scope=global 应拒");
        assert!(e.contains("session"), "错误应提及 session：{e}");
        // pi_extensions 段已恢复（90-6 revert：schema 3/4 双收）→ 合法且入清单
        let legacy = GOOD.replace(
            "entry = \"main.js\"",
            "entry = \"main.js\"

[[contributes.pi_extensions]]
path = \"pi/index.mjs\"",
        );
        write_manifest(&dir, &legacy);
        let m = parse_manifest(&dir).expect("pi_extensions 段已恢复，应通过");
        assert_eq!(m.pi_extensions, vec!["pi/index.mjs".to_string()]);
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
title = {{ zh = \"绑定面板\" }}
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
        let dir = tmp.join("pf-demo");
        write_manifest(
            &dir,
            "schema = 4
id = \"pf-demo\"
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
        write_manifest(&dir, &GOOD.replace("schema = 4", "schema = 2"));
        assert!(parse_manifest(&dir).is_err());
        // 纯库包（无 contributes）
        let dir = tmp.join("quick-panel");
        write_manifest(
            &dir,
            "schema = 4\nid = \"quick-panel\"\ntitle = { zh = \"x\" }\nversion = \"0.1.0\"\n",
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
            "schema = 4  # 顶部注释
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
schema = 4
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

    /// 把 GOOD 清单连同它引用的入口/资产一起落到目录（parse 校验文件存在）。
    fn write_good_pkg(dir: &Path, id: &str) {
        write_manifest(
            dir,
            &GOOD.replace("id = \"quick-panel\"", &format!("id = \"{id}\"")),
        );
        std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
    }

    fn ledger_entry(name: &str, version: &str, disabled: bool) -> pi_link::pf_plugins::LedgerEntry {
        pi_link::pf_plugins::LedgerEntry {
            name: name.into(),
            version: version.into(),
            integrity: String::new(),
            installed_at: String::new(),
            source: "market".into(),
            disabled,
        }
    }

    /// 090-10 迁移体验：用户数据挂**顶层包目录**。版本布局 pkg_dir = 父目录，
    /// 平铺布局 pkg_dir = 清单目录；两档 data_dir 都落 `<pkg>/data`——版本换代
    /// 时 data 路径不变，这是「更新不丢数据」的全部依据。
    #[test]
    fn pkg_dir_follows_layout() {
        let tmp = std::env::temp_dir().join(format!("pf-pkgdir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        // market 版本布局
        let ver = tmp.join("@scope__quick-panel").join("0.1.5");
        write_good_pkg(&ver, "quick-panel");
        let m = parse_manifest(&ver).expect("版本布局清单合法");
        assert_eq!(m.dir, ver, "wasm/assets 仍在版本目录");
        assert_eq!(m.pkg_dir, tmp.join("@scope__quick-panel"));
        assert_eq!(m.data_dir(), tmp.join("@scope__quick-panel").join("data"));
        // dev 平铺布局
        let flat = tmp.join("quick-panel");
        write_good_pkg(&flat, "quick-panel");
        let m = parse_manifest(&flat).expect("平铺清单合法");
        assert_eq!(m.pkg_dir, flat);
        assert_eq!(m.data_dir(), flat.join("data"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 090-10 迁移体验：台账启用、清单 schema 2 被本版拒 → 进 stale（kind /
    /// version / title 都从 peek 来，不依赖 parse）；合法包进 manifests 不进
    /// stale；台账没有的包两边都不进（野包不装载也不提示）。
    #[test]
    fn scan_root_collects_stale_for_rejected_ledger_pkgs() {
        let tmp = std::env::temp_dir().join(format!("pf-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        // ① 旧 schema 2 包（本机 0.1.4 现状）
        write_manifest(
            &tmp.join("@pi-flash-plugins__quick-panel").join("0.1.4"),
            r#"
schema = 2
id = "quick-panel"
title = { zh = "快捷面板", en = "Quick Panel" }
version = "0.1.4"
wasm = "plugin.wasm"
"#,
        );
        // ② 合法包
        write_good_pkg(&tmp.join("@s__other").join("1.0.0"), "other");
        // ③ 野包：磁盘有、台账无
        write_manifest(
            &tmp.join("@s__wild").join("0.0.1"),
            "schema = 2\nid = \"wild\"\n",
        );

        let ledger = vec![
            ledger_entry("@pi-flash-plugins/quick-panel", "0.1.4", false),
            ledger_entry("@s/other", "1.0.0", false),
        ];
        let out = scan_root(&tmp, &ledger);

        assert_eq!(out.manifests.len(), 1, "只有合法包装载");
        assert_eq!(out.manifests[0].id, "other");
        assert_eq!(
            out.stale,
            vec![StalePlugin {
                name: "@pi-flash-plugins/quick-panel".into(),
                version: "0.1.4".into(),
                title_zh: "快捷面板".into(),
                kind: StaleKind::SchemaUnsupported { got: 2 },
            }]
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 停用的包不装载也**不**进 stale——用户主动关的，不该被催更新。
    #[test]
    fn scan_root_skips_disabled_even_if_rejected() {
        let tmp = std::env::temp_dir().join(format!("pf-stale-dis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        write_manifest(
            &tmp.join("@s__old").join("0.1.0"),
            "schema = 2\nid = \"old\"\ntitle = { zh = \"旧\" }\nversion = \"0.1.0\"\n",
        );
        let out = scan_root(&tmp, &[ledger_entry("@s/old", "0.1.0", true)]);
        assert!(out.manifests.is_empty());
        assert!(out.stale.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 非 schema 类清单错误归 Invalid（原因原文），version 缺失回落版本目录名。
    #[test]
    fn scan_root_marks_other_parse_errors_invalid() {
        let tmp = std::env::temp_dir().join(format!("pf-stale-inv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        // schema 合法但 id 与目录名不符 → parse 拒；没有 version 行
        write_manifest(
            &tmp.join("@s__mismatch").join("2.0.0"),
            "schema = 5\nid = \"someone-else\"\ntitle = { zh = \"错位\" }\n",
        );
        let out = scan_root(&tmp, &[ledger_entry("@s/mismatch", "2.0.0", false)]);
        assert_eq!(out.stale.len(), 1);
        let s = &out.stale[0];
        assert_eq!(s.version, "2.0.0", "version 缺失 → 版本目录名");
        assert_eq!(s.title_zh, "错位");
        assert!(
            matches!(&s.kind, StaleKind::Invalid(r) if r.contains("目录名")),
            "{:?}",
            s.kind
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// mda8：版本层布局只认台账版本目录。更新后旧版本目录残留（被占用没删掉）
    /// 且两版清单都合法时，不能双装载；残留目录也不算失效（不催更新）。
    #[test]
    fn scan_root_loads_only_ledger_version() {
        let tmp = std::env::temp_dir().join(format!("pf-onever-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg = tmp.join("@s__quick-panel");
        write_good_pkg(&pkg.join("0.1.5"), "quick-panel");
        write_good_pkg(&pkg.join("0.1.6"), "quick-panel");
        // 字符串序陷阱：0.1.10 < 0.1.9，但台账说 0.1.6 → 只它
        write_good_pkg(&pkg.join("0.1.10"), "quick-panel");

        let out = scan_root(&tmp, &[ledger_entry("@s/quick-panel", "0.1.6", false)]);
        assert_eq!(out.manifests.len(), 1, "三个合法版本目录只装台账那一个");
        assert_eq!(out.manifests[0].dir, pkg.join("0.1.6"));
        assert!(out.stale.is_empty(), "残留目录不算失效");

        // 台账版本目录不存在（异常残局）→ 什么都不装、也不拿别的版本顶上
        let out = scan_root(&tmp, &[ledger_entry("@s/quick-panel", "9.9.9", false)]);
        assert!(out.manifests.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
