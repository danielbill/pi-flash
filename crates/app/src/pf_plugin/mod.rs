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

use crate::Chat;

// ---------------------------------------------------------------------------
// 清单（02 §2；手写极简解析，不引 toml 库——设计定案）
// ---------------------------------------------------------------------------

/// 本版 PF 支持的清单契约大版本（与插件仓 schema/plugin-contract.json 的
/// `manifest_schema` 对齐；加/删原语时一并升）
pub(crate) const SUPPORTED_MANIFEST_SCHEMA: u32 = 2;

#[derive(Debug, Clone)]
pub(crate) struct PluginManifest {
    /// 安装目录 = `<数据目录>/plugins/<id>/`
    pub dir: PathBuf,
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
        id: String::new(),
        title_zh: String::new(),
        title_en: None,
        version: String::new(),
        status_bar: None,
        file_menu: Vec::new(),
        panels: Vec::new(),
        wasm: None,
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
    let dir_name = dir.file_name().and_then(|s| s.to_str()).unwrap_or_default();
    if m.id != dir_name {
        return Err(format!(
            "pf-plugin.toml：id「{}」≠ 目录名「{dir_name}」",
            m.id
        ));
    }
    if m.title_zh.is_empty() {
        return Err("pf-plugin.toml：缺 title.zh（U7）".into());
    }
    if m.version.is_empty() {
        return Err("pf-plugin.toml：缺 version".into());
    }
    if m.status_bar.is_none() && m.file_menu.is_empty() && m.panels.is_empty() {
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
        if m.wasm.is_none() && !dir.join(&p.entry).is_file() {
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
/// 非 Send 的 JsPlugin，条目装配必须在主线程做）；坏清单 eprintln 跳过；
/// 台账标了停用的目录整个跳过（不装载不绘制，090-4 v5）。
pub(crate) fn scan_manifests() -> Vec<PluginManifest> {
    let Some(root) = pi_link::paths::plugins_dir() else {
        return Vec::new();
    };
    let disabled: std::collections::HashSet<String> = pi_link::pf_plugins::load_ledger()
        .into_iter()
        .filter(|l| l.disabled)
        .map(|l| pi_link::pf_plugins::dir_name(&l.name))
        .collect();
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&root) else {
        return out;
    };
    let mut dirs: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for dir in dirs {
        if !dir.is_dir() || !dir.join("pf-plugin.toml").is_file() {
            continue;
        }
        if dir
            .file_name()
            .is_some_and(|f| disabled.contains(f.to_string_lossy().as_ref()))
        {
            continue; // 停用（台账 disabled 位）
        }
        match parse_manifest(&dir) {
            Ok(m) => {
                // 内层 wasm 插件无 contributes 也合法（验签在 wasm 内，090-5 L3）
                if m.wasm.is_none()
                    && m.status_bar.is_none()
                    && m.file_menu.is_empty()
                    && m.panels.is_empty()
                {
                    continue; // 纯库包
                }
                out.push(m);
            }
            Err(e) => eprintln!("[pf-plugin] 跳过 {}：{e}", dir.display()),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 插件资产（状态栏 icon）：gpui AssetSource 走磁盘，内存缓存
// ---------------------------------------------------------------------------

static PLUGIN_ASSETS: OnceLock<Mutex<HashMap<String, Option<Vec<u8>>>>> = OnceLock::new();

/// `rel` = `<slug>/<assets 相对路径>`（assets.rs 的 `plugins/` 前缀段）。
pub(crate) fn plugin_asset(rel: &str) -> Option<Vec<u8>> {
    let cache = PLUGIN_ASSETS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(hit) = cache.lock() {
        if let Some(hit) = hit.get(rel) {
            return hit.clone();
        }
    }
    let bytes = pi_link::paths::plugins_dir().and_then(|d| {
        let full = d.join(rel);
        let r = std::fs::read(&full);
        eprintln!(
            "[pf-plugin] asset {rel} -> {} bytes ({:?})",
            r.as_ref().map(|b| b.len()).unwrap_or(0),
            r.as_ref().err()
        );
        r.ok()
    });
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
        self.layout.dock_panel = crate::DockPanel::Plugin(ix);
        self.plugin_refresh(ix, cx);
    }

    /// 懒加载就绪：runtime 已建或现建；禁用/失败返回 false（已 toast）。
    fn plugin_ready(&mut self, ix: usize, cx: &mut gpui::Context<Self>) -> bool {
        let Some(entry) = self.plugins.plugins.get_mut(ix) else {
            return false;
        };
        if entry.disabled {
            return false;
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
        let entry_path = entry.manifest.dir.join(&entry_rel);
        let id = entry.manifest.id.clone();
        match JsPlugin::load(&entry_path, &dir_str, host.clone()) {
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

    /// onPanelOpen + render → rows 缓存（02 §4 懒加载链）。
    fn plugin_refresh(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        if !self.plugin_ready(ix, cx) {
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

    /// 插件异常隔离（02 §4）：捕获 → toast → 会话级禁用，不影响宿主。
    fn plugin_broken(&mut self, ix: usize, err: String, cx: &mut gpui::Context<Self>) {
        let Some(entry) = self.plugins.plugins.get_mut(ix) else {
            return;
        };
        entry.disabled = true;
        entry.js = None;
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
/// ② 内层 wasm 插件验签门（090-5 L3）：组件编译是秒级成本（4.6 MB 组件实测
///    ~2s），且**揭幕后**才发起——编译/实例化/init 全在后台线程，结果回主
///    线程只写状态栏。旧实现把 load+init 放在主线程 `update` 里直接跑，
///    UI 卡死数秒（本次修复）。
pub(crate) fn spawn_plugin_scan(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        let manifests = cx
            .background_executor()
            .spawn(async move { scan_manifests() })
            .await;
        let (wasm, js): (Vec<_>, Vec<_>) = manifests.into_iter().partition(|m| m.wasm.is_some());
        // ① JS 插件先落位：不等 wasm 验签（否则状态栏 tab 被秒级编译拖死）
        let _ = this.update(cx, |c, cx| c.plugins_set(plugin_entries(js), cx));
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
        let msgs = cx
            .background_executor()
            .spawn(async move { verify_wasm_plugins(wasm) })
            .await;
        let _ = this.update(cx, |c, cx| {
            for msg in msgs {
                c.set_status(msg, cx);
            }
            cx.notify();
        });
    })
    .detach();
}

/// 内层 wasm 插件验签门（090-5 L3）——**只许后台线程调用**。
/// init 即验签门，实例即验即弃（wasm 渲染面是后续切片），结论由调用方经
/// `set_status` 面向用户（失败不静默，02 §7.2）。
fn verify_wasm_plugins(manifests: Vec<PluginManifest>) -> Vec<String> {
    // 证明材料钩子是进程级（pf-plugin-host 里从 thread_local 改成全局），
    // 后台线程 init 时插件回调 get_host_proof 同样取得到材料
    pf_plugin_host::install_proof_hook(host_proof_material);
    let mut msgs = Vec::new();
    for m in manifests {
        let id = m.id.clone();
        let title = pick_lang(&m.title_zh, &m.title_en);
        let wasm_path = m.dir.join(m.wasm.as_deref().unwrap_or("plugin.wasm"));
        let t0 = std::time::Instant::now();
        let msg = match pf_plugin_host::PluginInstance::load(&wasm_path) {
            Ok(mut inst) => match inst.init(&id) {
                Ok(()) => format!("插件 {title}：官方验证通过，已激活"),
                Err(e) => format!("插件 {title} 拒载：{e}"),
            },
            Err(e) => format!("插件 {title} 装载失败：{e}"),
        };
        eprintln!("[pf-plugin] {msg}（验签 {:?}，后台线程）", t0.elapsed());
        msgs.push(msg);
    }
    msgs
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
}
