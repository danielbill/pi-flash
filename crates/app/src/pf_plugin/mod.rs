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

mod panel;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

use gpui::SharedString;
use pf_plugin_host::js::{JsAction, JsHost, JsPlugin};
use pf_plugin_host::{PluginAction, PluginInstance};

use crate::Chat;

// ---------------------------------------------------------------------------
// 清单（02 §2；手写极简解析，不引 toml 库——设计定案）
// ---------------------------------------------------------------------------

/// 本版 PF 支持的清单契约大版本（与插件仓 schema/plugin-contract.json 的
/// `manifest_schema` 对齐；加/删原语时一并升）
pub(crate) const SUPPORTED_MANIFEST_SCHEMA: u32 = 2;

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
    pub file_menu: Vec<FileMenuContrib>,
    pub panels: Vec<PanelContrib>,
    /// 内层 wasm 插件（契约 inner_package）：`wasm = "plugin.wasm"` 声明即启用；
    /// 装载 = pf_plugin_host::PluginInstance（init 即验签门，090-5 L3）
    pub wasm: Option<String>,
    /// 分发态 JS **字节码**（`plugin.qbc`，由 `qjsc` 从同一份 main.js 编出）：
    /// 与源码包同一引擎、同一 ctx、同一渲染路径，落盘无明文源码。
    pub bytecode: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct StatusBarContrib {
    /// 三来源之一：宿主内置名 / `assets:<名>` / `assets/` 相对路径
    pub icon: String,
    pub title_zh: String,
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
        file_menu: Vec::new(),
        panels: Vec::new(),
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
                "contributes.panels" => {
                    m.panels.push(PanelContrib {
                        id: String::new(),
                        title_zh: String::new(),
                        entry: "main.js".into(),
                    });
                    "panels".to_string()
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
        && let Some(p) = dir.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str())
    {
        base = p; // 版本目录 → 上钻到插件目录
    }
    let expected = base.rsplit("__").next().unwrap_or(base);
    if m.id != expected {
        return Err(format!(
            "pf-plugin.toml：id「{}」≠ 目录名「{base}」",
            m.id
        ));
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
    {
        return Err("纯库包（三项都没申请），不加载".into());
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

/// Chat 上的插件簇（与 aux/ws 等并列的最小簇）。
#[derive(Default)]
pub(crate) struct PluginUi {
    pub plugins: Vec<PluginEntry>,
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
            self.set_status(
                format!("插件 {id} 拒载：需要官方构建的 PF"),
                cx,
            );
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
            eprintln!("[pf-plugin] render/onPanelOpen 期间的动作被忽略: {a:?}");
        }
        match result {
            Ok(json) => {
                let entry = &mut self.plugins.plugins[ix];
                let v: serde_json::Value =
                    serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                // v2：render 直接返回组件树（根节点），没有 { body, rows } 外壳
                entry.body = Some(v);
                entry.tree_loaded = true;
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
                JsAction::RevealInExplorer(p) => {
                    crate::services::shell_open::reveal_in_explorer(Path::new(&p))
                }
                JsAction::Toast(m) => self.set_status(m, cx),
                JsAction::OpenUrl(u) => crate::services::shell_open::open_url(&u),
                JsAction::OpenFolder(p) => {
                    crate::services::shell_open::open_in_explorer(std::path::Path::new(&p))
                }
                JsAction::NewSession => self.new_session(cx),
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
        _ix: usize,
        actions: Vec<PluginAction>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        for a in actions {
            match a {
                PluginAction::Toast(m) => self.set_status(m, cx),
                PluginAction::OpenFile(p) => self.open_file_tab(PathBuf::from(&p), cx),
                PluginAction::RunScript(p) => {
                    let path = PathBuf::from(&p);
                    match crate::services::script_exec::resolve(&path, self.ws.cwd.as_deref()) {
                        Some(plan) => self.open_script_terminal(path, plan, window, cx),
                        None => self.set_status(format!("无法执行：{p}"), cx),
                    }
                }
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
    // 安装尾声钩子：wasm → cwasm 预编译压在安装那一次做完（pi-link 不依赖
    // wasmtime，故由 app 注入）。纯函数指针，零开销。
    pi_link::pf_plugins::install_prepare_hook(install_prepare);
    cx.spawn(async move |this, cx| {
        let manifests = cx
            .background_executor()
            .spawn(async move { scan_manifests() })
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

/// `measure-text`：App 级文本系统逐字符 advance 求和（无 window 依赖；
/// kerning 不计入，插件排版够准）。字体未注册 → 0.0（兑底是插件的责任，
/// 与 fs 缺失同款）。
fn wasm_measure_text(content: &str, size: f32) -> f32 {
    let Some(ts) = TEXT_SYSTEM.with(|c| c.borrow().clone()) else {
        return 0.0;
    };
    let font = gpui::Font {
        family: crate::appearance::panel_font().family.into(),
        features: gpui::FontFeatures::default(),
        fallbacks: None,
        weight: gpui::FontWeight::NORMAL,
        style: gpui::FontStyle::Normal,
    };
    let id = ts.resolve_font(&font);
    let size = gpui::px(size);
    let mut w = 0.0f32;
    for ch in content.chars() {
        if ch == '\n' {
            continue;
        }
        if let Ok(a) = ts.advance(id, size, ch) {
            w += f32::from(a.width);
        }
    }
    w
}

/// wasm 宿主钩子懒注入（首次 wasm 装载前，主线程）：文本系统 + 快照读口。
static WASM_HOOKS: OnceLock<()> = OnceLock::new();
fn ensure_wasm_hooks(cx: &gpui::App) {
    if WASM_HOOKS.set(()).is_err() {
        return;
    }
    TEXT_SYSTEM.with(|c| *c.borrow_mut() = Some(cx.text_system().clone()));
    pf_plugin_host::install_host_hooks(quick_pin_snap, wasm_measure_text);
    // 路径派生能力：与 JS 插件 ctx.fileIcon / ctx.isExecutive 同一套服务
    pf_plugin_host::install_path_hooks(
        |p| crate::services::file_icons::get_icon(Path::new(p)).to_string(),
        |p| crate::services::script_exec::exec_kind(Path::new(p)).is_some(),
    );
}

/// 内层 wasm 插件验签门（090-5 L3）——**只许后台线程调用**。
///
/// init 即验签门；实例即验即弃（运行实例是懒装载的另一份，见
/// plugin_wasm_ready）。返回**拒载包 id**：结论只进 stderr + 回传主线程置
/// disabled，不向用户播报任何正版校验提示。
///
/// 速度：装载走**插件目录内的**编译产物缓存（`<插件目录>/.cache/<sha256>.cwasm`）——
/// 安装时已预编译（见 [`install_prepare`]），故这里恒命中；之后每次 =
/// mmap 反序列化 + 实例化 + init（毫秒级）。旧的 `load()` 每次启动重编
/// （4.6 MB 组件 7.2 s debug）。
fn verify_wasm_plugins(manifests: Vec<PluginManifest>) -> Vec<String> {
    // 证明材料钩子是进程级（pf-plugin-host 里从 thread_local 改成全局），
    // 后台线程 init 时插件回调 get_host_proof 同样取得到材料
    pf_plugin_host::install_proof_hook(host_proof_material);
    let mut rejected = Vec::new();
    for m in manifests {
        let id = m.id.clone();
        let wasm_path = m.dir.join(m.wasm.as_deref().unwrap_or("plugin.wasm"));
        let cache = pi_link::pf_plugins::plugin_cache_for(&m.dir);
        let t0 = std::time::Instant::now();
        let loaded = pf_plugin_host::PluginInstance::load_cached(&wasm_path, &cache);
        let load_elapsed = t0.elapsed();
        let t1 = std::time::Instant::now();
        match loaded {
            Ok(mut inst) => match inst.init(&id) {
                Ok(()) => eprintln!(
                    "[pf-plugin] {id} 验签通过（装载 {load_elapsed:?} / init {:?}，后台线程）",
                    t1.elapsed()
                ),
                Err(e) => {
                    eprintln!("[pf-plugin] {id} 拒载：{e}");
                    rejected.push(id);
                }
            },
            Err(e) => {
                eprintln!("[pf-plugin] {id} 装载失败：{e}");
                rejected.push(id);
            }
        }
    }
    rejected
}

/// 安装尾声钩子实现：wasm 包在**安装时**就把 cwasm 编译产物做好，落进
/// `<插件目录>/.cache/`。此后启动、开面板一律 mmap 命中（毫秒级）。
///
/// 非 wasm 包（无 `wasm =` 声明）= 无需预编译；清单解析失败也不在此判死
/// （安装链路已校验过，这里只负责把重活搬完）。
fn install_prepare(plugin_dir: &Path) -> Result<(), String> {
    let Ok(m) = parse_manifest(plugin_dir) else {
        return Ok(());
    };
    let Some(wasm) = m.wasm.as_deref() else {
        return Ok(());
    };
    let wasm_path = plugin_dir.join(wasm);
    let cache = pi_link::pf_plugins::plugin_cache_for(plugin_dir);
    let t0 = std::time::Instant::now();
    match pf_plugin_host::PluginInstance::precompile_to_cache(&wasm_path, &cache) {
        Ok(_) => {
            eprintln!("[pf-plugin] 安装预编译 {} 完成（{:?}）", m.id, t0.elapsed());
            Ok(())
        }
        Err(e) => Err(format!("预编译失败: {e}")),
    }
}

/// 旧版安装预热（编译产物不在插件目录的历史包）：后台补编一次。
/// 新安装已由 [`install_prepare`] 在安装时做完，这里通常全部命中、立刻返回。
pub(crate) fn spawn_precompile_installed() {
    std::thread::spawn(|| {
        for m in scan_manifests().into_iter().filter(|m| m.wasm.is_some()) {
            let wasm = m.dir.join(m.wasm.as_deref().unwrap_or("plugin.wasm"));
            let cache = pi_link::pf_plugins::plugin_cache_for(&m.dir);
            let t0 = std::time::Instant::now();
            match pf_plugin_host::PluginInstance::precompile_to_cache(&wasm, &cache) {
                Ok(true) => {
                    eprintln!("[pf-plugin] 预编译 {} 完成（{:?}）", m.id, t0.elapsed())
                }
                Ok(false) => {} // 已命中
                Err(e) => eprintln!("[pf-plugin] 预编译 {} 失败：{e}", m.id),
            }
        }
    });
}

/// 本机构建是否带官方宿主证明（build.rs 注入；占位符 = 非官方）。
fn has_official_proof() -> bool {
    option_env!("PF_HOST_PROOF").is_some_and(|p| !p.is_empty() && p != "PROOF_PLACEHOLDER")
}

/// 官方构建证明材料（build.rs 把 PF_BUILD_PROOF/KID 烙成二进制常量，见
/// crates/app/build.rs；未设置的构建 = 占位符 → init 验签必败 → 拒载）。
fn host_proof_material() -> String {
    let proof = option_env!("PF_HOST_PROOF").unwrap_or("PROOF_PLACEHOLDER");
    let kid = option_env!("PF_HOST_PROOF_KID").unwrap_or("");
    serde_json::json!({ "kid": kid, "proof": proof }).to_string()
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
    let icon = sb.icon.as_str();
    // `assets/` 是目录结构的一段，**不能剥**——曾剥掉导致
    // plugins/<slug>/bolt.svg NotFound（图标空白，还进负缓存）
    if icon.starts_with("assets/") {
        Some(SharedString::from(format!(
            "plugins/{}/{}",
            entry.manifest.id, icon
        )))
    } else if let Some(name) = icon.strip_prefix("assets:") {
        Some(SharedString::from(format!(
            "plugins/{}/assets/{}",
            entry.manifest.id, name
        )))
    } else {
        Some(SharedString::from(format!("icons/{icon}.svg")))
    }
}

pub(crate) use panel::{cell_menu_layer, plugin_panel_view};

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &Path, body: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join("pf-plugin.toml");
        std::fs::write(&p, body).unwrap();
        p
    }

    const GOOD: &str = r#"
schema = 2
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
        // schema ≠ 支持的契约大版本
        let dir = tmp.join("quick-panel");
        write_manifest(&dir, &GOOD.replace("schema = 2", "schema = 3"));
        assert!(parse_manifest(&dir).is_err());
        // 纯库包（无 contributes）
        let dir = tmp.join("quick-panel");
        write_manifest(
            &dir,
            "schema = 2\nid = \"quick-panel\"\ntitle = { zh = \"x\" }\nversion = \"0.1.0\"\n",
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
            "schema = 2  # 顶部注释
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
schema = 2
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
