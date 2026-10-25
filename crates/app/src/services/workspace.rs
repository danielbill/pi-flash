//! Workspace state + app settings persistence.
//!
//! Three-layer config boundary (ARCHITECTURE.md): pi's own `settings.json`
//! (models/credentials/tools — pi's domain) / `pi-flash-app-settings.json`
//! (app appearance: theme, icon theme, fonts, plus lang/sound preferences,
//! 006 界面设置) / `pi-flash-workspace.json` (runtime state: last-open
//! session per workspace, globally-last workspace, window/dock layout,
//! per-project tab strip `__tabs`（启动恢复 tab 带）).
//!
//! IO rules: every write is atomic (tmp + rename via pi_link::config),
//! each file is read once per process (in-process cache), and saves that
//! would write identical content are skipped.

use std::path::Path;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

/// Key holding the globally-last active workspace (startup target).
const WS_LAST_KEY: &str = "__last";

/// Persisted language preference (legacy home: workspace memory file; the
/// canonical home is now app_settings.json, loads fall back to this key).
const WS_LANG_KEY: &str = "__lang";

/// Persisted sound preference (legacy home, same migration as lang).
const WS_SOUND_KEY: &str = "__sound";

/// 内容区 tab 带存档（`__tabs`）：一个项目关停/离开时的自由 tab 清单 +
/// 激活位 + 前台视图。启动恢复（startup::spawn_boot_restore）按 cwd 读回。
const WS_TABS_KEY: &str = "__tabs";

/// tab 带存档条目上限（防手滑开几十个终端后重启 = 几十个 conpty 进程，
/// 启动铁律不许）。超限截尾（先保打开序靠前的）。
pub const TABS_SAVE_CAP: usize = 32;

/// Window bounds / maximized flag (schema landed in phase A, consumed by
/// the phase-D shell).
const WS_WINDOW_KEY: &str = "__window";

// ---------------------------------------------------------------------------
// core: path-injected map IO (tests use these directly; no cache)
// ---------------------------------------------------------------------------

fn load_map_from(path: &Path) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    let Ok(raw) = std::fs::read_to_string(path) else {
        return out;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return out;
    };
    if let Some(obj) = parsed.as_object() {
        for (k, v) in obj {
            // `__last` stays as-is; workspace keys get normalized once.
            if k == WS_LAST_KEY {
                out.insert(k.clone(), v.clone());
            } else {
                out.insert(ws_key(k), v.clone());
            }
        }
    }
    out
}

/// Plain object load without workspace-key normalization (app_settings.json
/// keys are setting names, not paths).
fn load_plain_map_from(path: &Path) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    let Ok(raw) = std::fs::read_to_string(path) else {
        return out;
    };
    if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&raw) {
        out = obj;
    }
    out
}

/// Atomic map write (tmp + rename through pi_link::config::write_json) with
/// a no-op skip when the serialized content is unchanged.
fn save_map_to(path: &Path, map: &serde_json::Map<String, Value>) -> bool {
    let Ok(new_raw) = serde_json::to_string_pretty(map) else {
        return false;
    };
    if let Ok(current) = std::fs::read_to_string(path) {
        if current == new_raw {
            return false;
        }
    }
    // 自有目录可能还没建（首启 / 迁移后）——写前确保存在
    let _ = pi_link::paths::ensure_dir();
    let value = Value::Object(map.clone());
    pi_link::config::write_json(path, &value).is_ok()
}

/// Normalized workspace key: Path components joined with "\\", so
/// "D:/a/b" and "D:\a\b" share one memory slot (Windows path parity).
fn ws_key(cwd: &str) -> String {
    // String-level normalization: "/" -> "\\", strip trailing separator,
    // case-fold (Windows paths are case-insensitive).
    let mut k = cwd.replace('/', "\\");
    while k.ends_with('\\') {
        k.pop();
    }
    k.to_ascii_lowercase()
}

// ---------------------------------------------------------------------------
// workspace memory (read once per process, cached)
// ---------------------------------------------------------------------------

fn memory_path() -> Option<PathBuf> {
    // pi-flash 自有目录（010-启动.md §4）：不再往 ~/.pi/agent 写
    pi_link::paths::workspace_file()
}

/// Shared in-process cache for the workspace memory map (one static for
/// both read and write paths — startup used to hit the file four times:
/// lang, sound, last workspace, last open).
fn with_memory_cache<T>(f: impl FnOnce(&mut Option<serde_json::Map<String, Value>>) -> T) -> T {
    static CACHE: Mutex<Option<serde_json::Map<String, Value>>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

fn memory() -> serde_json::Map<String, Value> {
    with_memory_cache(|cache| {
        if cache.is_none() {
            let map = memory_path().map(|p| load_map_from(&p)).unwrap_or_default();
            *cache = Some(map);
        }
        cache.as_ref().expect("cache just filled").clone()
    })
}

/// Update cache + atomic file write. Returns true when the file changed.
fn set_memory(map: &serde_json::Map<String, Value>) -> bool {
    with_memory_cache(|cache| *cache = Some(map.clone()));
    match memory_path() {
        Some(p) => save_map_to(&p, map),
        None => false,
    }
}

/// Workspace-key-aware path equality.
pub fn same_ws(a: &str, b: &str) -> bool {
    ws_key(a) == ws_key(b)
}

/// Public ws-key conversion (psp collapsed-group persistence).
pub fn same_ws_key(cwd: &str) -> String {
    ws_key(cwd)
}

/// Path equality via the same string-level normalization as same_ws
/// (Windows Path::components is not reusable as a key).
pub fn same_path(a: &Path, b: &Path) -> bool {
    same_ws(&a.to_string_lossy(), &b.to_string_lossy())
}

/// 项目行/组名：目录名，无尾件（根盘等）回落全路径。
pub fn project_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// `path` 是否在 `root` 之下（含 root 本身）：与 same_path 同一套字符串级
/// 规范化（分隔符统一为 `\`、去尾分隔符、大小写折叠），所以 Windows 上
/// `d:/a\b` 与 `D:\A` 的层级关系判得对。023 用它决定「这个目录是否已被
/// workspace 的递归 watch 覆盖」，从而决定要不要给 cwd 外文件单挂监听。
pub fn is_under(path: &Path, root: &Path) -> bool {
    let p = ws_key(&path.to_string_lossy());
    let r = ws_key(&root.to_string_lossy());
    p == r || p.starts_with(&format!("{r}\\"))
}

/// 磁盘改动指纹 (mtime, len)（023 外部改动检测基准）：打开/保存/确认时
/// 记录，fs 泵信号到达时对比。mtime 不支持（罕见 FS）返回 None = 永不误报。
pub fn file_sig(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let md = std::fs::metadata(path).ok()?;
    Some((md.modified().ok()?, md.len()))
}

/// 应用自有目录（载荷目录 / npm 包根，见 `pi_link::paths::is_app_install_path`）
/// 的 cwd 不进工作区记忆：0.1.x 更新交接 spawn 不设 cwd，新进程继承旧载荷
/// 目录曾被记成 `__last`，之后每次启动都被恢复成 payload 项目。
fn app_owned_cwd(cwd: &str) -> bool {
    pi_link::paths::is_app_install_path(Path::new(cwd))
}

/// Remember `session_path` as the last open session for `cwd` (per-project
/// memory; switch_project resumes from here).
pub fn set_last_open(cwd: &str, session_path: &str) {
    if app_owned_cwd(cwd) {
        return;
    }
    let mut map = memory();
    // 010 重设计：启动恢复对象是「最后工作的会话」（recents 全局最新），
    // 全局 __last 工作区键不再参与启动，也不再写入
    let key = ws_key(cwd);
    map.insert(key, Value::String(session_path.into()));
    set_memory(&map);
}

pub fn load_sound_pref() -> bool {
    app_settings().sound.unwrap_or_else(|| {
        memory()
            .get(WS_SOUND_KEY)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    })
}

pub fn save_sound_pref(on: bool) {
    let mut s = app_settings();
    s.sound = Some(on);
    save_app_settings(&s);
}

/// Agent-run finished notification sound —— PF 原创暖木琴双音
/// （见 [`crate::services::sound`]）；非 Windows 平台为 no-op。
pub fn play_notify_sound() {
    crate::services::sound::play_notify_sound();
}

pub fn load_lang_pref() -> Option<usize> {
    app_settings().lang.or_else(|| {
        memory()
            .get(WS_LANG_KEY)
            .and_then(|v| v.as_u64())
            .map(|v| v.min(2) as usize)
    })
}

pub fn save_lang_pref(ix: usize) {
    let mut s = app_settings();
    s.lang = Some(ix);
    save_app_settings(&s);
}

/// 010 重设计：全局「最后工作区」记忆（__last / set_last_workspace /
/// get_last_workspace）已整体退役——启动恢复对象是最后会话，其项目随之
/// 成为 cwd，工作区不再是独立恢复目标。

/// Remember that `cwd` was left on a blank new session.
pub fn clear_last_open(cwd: &str) {
    let mut map = memory();
    map.insert(ws_key(cwd), Value::String(String::new()));
    set_memory(&map);
}

/// The remembered session path for `cwd`, if any.
pub fn get_last_open(cwd: &str) -> Option<String> {
    memory()
        .get(&ws_key(cwd))?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 启动清洗（幂等）：工作区记忆里落在当前进程安装根内的存量键——`__last`
/// （启动恢复目标被钉死在载荷目录 = 每次启动都落回 payload 的根因）与载荷
/// 目录名下的每项目记忆——删键落盘。`__` 前缀的非路径键（布局/UI 状态）
/// 不动。返回清除键数（0 = 干净 / 无从判定）。
pub fn purge_app_install_memory() -> usize {
    let mut map = memory();
    let removed =
        purge_app_paths_in_map(&mut map, pi_link::paths::current_install_root().as_deref());
    if removed > 0 {
        set_memory(&map);
    }
    removed
}

/// 纯内核（root 显式注入便于测试）：只改传入 map，**绝不碰全局缓存/落盘**
/// ——落盘职责归调用方。此前 removed>0 时顺手 set_memory，purge 测试的
/// 夹具 map 经此整表写穿真实 workspace.json（pins/布局/各项目记忆全冲掉，
/// 裸跑 `cargo test -p app` 每次复发，2026-10-18 实锤）。
fn purge_app_paths_in_map(map: &mut serde_json::Map<String, Value>, root: Option<&Path>) -> usize {
    use pi_link::paths::is_app_install_path_at;
    let Some(root) = root else { return 0 };
    let before = map.len();
    map.retain(|k, v| {
        if k == WS_LAST_KEY {
            // __last 的值是 ws_key 形式的路径字符串
            v.as_str()
                .map(|s| !is_app_install_path_at(Path::new(s), root))
                .unwrap_or(true)
        } else if k.starts_with("__") {
            true // __dock/__window/__ui/__pins：布局与 UI 状态，非路径键
        } else {
            !is_app_install_path_at(Path::new(k), root)
        }
    });
    before - map.len()
}

// ---------------------------------------------------------------------------
// 内容区 tab 带持久化（`__tabs`：{ tabs, active, chat }，单槽全局）
//
// 启动恢复的第二条腿（会话之外）：文件/终端 tab 跨重启复原。终端进程不
// 保活（PTY 随进程亡），恢复 = 按 cwd 重开；Changelog/市场/搜索页属瞬态
// 页不持久化。写入时机 = tab 带变更点（开关/激活/拖位）+ 关窗，内容相同
// 自动跳过写盘。
//
// **单槽全局**（2026-11 修订，首版 per-project 是踩错的坑）：内存里
// panel_tabs 本就是全局态（切项目不清带），按项目分槽会存出多份漂移拷贝
// ——关 tab 只清当前项目槽，重启落在另一个项目就从那个槽复活（实测
// pf-stats.mjs 三槽打地鼠）。与 `__window` 布局同语义：窗口级状态存一份。
// ---------------------------------------------------------------------------

/// 一条持久化的 tab（pi-flash-weia）。
#[derive(Debug, Clone, PartialEq)]
pub struct SavedTab {
    /// "file" | "term"
    pub kind: String,
    /// file = 文件绝对路径；term = 终端 cwd（重开时进程已亡，按 cwd 重生）
    pub path: String,
}

/// 一个项目的 tab 带存档。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TabsState {
    pub tabs: Vec<SavedTab>,
    /// 关停时自由区激活 tab 下标（None = 未激活/越界，恢复时落默认）
    pub active: Option<usize>,
    /// 关停时内容区前台是会话 tab（false = 前台是激活 tab 的视图）
    pub chat: bool,
    /// 关停时【微信通话】**标签展开着**（002：标签跨重启复原。用户点 ×
    /// 关掉就存成关着，重启不会被顶回来；开着就原样还回来）。
    pub wx_open: bool,
    /// 关停时内容区前台是**微信内容**（功能页 ContentView::Wx，或 Chat
    /// 视图里的微信线会话）。优先级：wx_front > chat > 激活 tab 视图。
    pub wx_front: bool,
}

/// 纯内核：从 JSON 值读 tab 带存档（无记录/旧版 per-project 嵌套形态 =
/// None——旧形态无顶层 `tabs` 字段，首次落盘即被新形态覆盖自迁移）。
fn tabs_state_from_value(v: &Value) -> Option<TabsState> {
    let tabs: Vec<SavedTab> = v
        .get("tabs")?
        .as_array()?
        .iter()
        .filter_map(|e| {
            let kind = e.get("k")?.as_str()?.to_string();
            let path = e.get("p")?.as_str()?.to_string();
            match kind.as_str() {
                "file" | "term" if !path.is_empty() => Some(SavedTab { kind, path }),
                _ => None,
            }
        })
        .take(TABS_SAVE_CAP)
        .collect();
    let active = v
        .get("active")
        .and_then(|a| a.as_u64())
        .map(|a| a as usize)
        .filter(|a| *a < tabs.len());
    let chat = v.get("chat").and_then(|b| b.as_bool()).unwrap_or(true);
    // 旧档兼容：单字段 `wx` 时代只有「前台是微信页」一种状态（标签必然
    // 开着），读成 wx_front=true 且 wx_open=true 即等价
    let legacy_wx = v.get("wx").and_then(|b| b.as_bool()).unwrap_or(false);
    let wx_front = v
        .get("wx_front")
        .and_then(|b| b.as_bool())
        .unwrap_or(legacy_wx);
    let wx_open = v
        .get("wx_open")
        .and_then(|b| b.as_bool())
        .unwrap_or(wx_front);
    Some(TabsState {
        tabs,
        active,
        chat,
        wx_open,
        wx_front,
    })
}

/// 读取全局 tab 带存档。
pub fn tabs_state_for() -> Option<TabsState> {
    memory().get(WS_TABS_KEY).and_then(tabs_state_from_value)
}

/// 纯内核：存档 → JSON 值（空带 = None，调用方据此移除槽位）。
fn tabs_state_to_value(state: &TabsState) -> Option<Value> {
    // 空带通常 = 移除槽位（启动落默认 chat）；唯独微信线要单独能存活
    // ——002 §6 清会话后前台停在[设置会话]页，tab 带本就为空，若这里
    // 返回 None，重启就「蹿回桌面会话对话页」（2026-10-25 真机事故）
    if state.tabs.is_empty() && !state.wx_open && !state.wx_front {
        return None;
    }
    let tabs: Vec<Value> = state
        .tabs
        .iter()
        .take(TABS_SAVE_CAP)
        .map(|t| serde_json::json!({ "k": t.kind, "p": t.path }))
        .collect();
    Some(serde_json::json!({
        "tabs": tabs,
        "active": state.active,
        "chat": state.chat,
        "wx_open": state.wx_open,
        "wx_front": state.wx_front,
    }))
}

/// 清空 tab 带存档槽（「启动恢复」关 = 存档一律不存：关着就什么都不恢复）。
pub fn clear_tabs_state() {
    let mut map = memory();
    if map.remove(WS_TABS_KEY).is_some() {
        set_memory(&map);
    }
}

/// 写全局 tab 带存档；空带 = 移除槽位。
pub fn save_tabs_state(state: &TabsState) {
    let mut map = memory();
    match tabs_state_to_value(state) {
        Some(v) => map.insert(WS_TABS_KEY.to_string(), v),
        None => map.remove(WS_TABS_KEY),
    };
    set_memory(&map);
}

// ---------------------------------------------------------------------------
// window / dock layout state (schema landed in phase A, consumed phase D)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct WindowState {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub maximized: bool,
}

/// v54 shell layout state (psp width/modes/panes + collapsed project groups),
/// one JSON blob under `__ui` in the workspace memory file.
#[derive(Debug, Clone, PartialEq)]
pub struct UiState {
    /// active statusbar panel: "sessions" | "files" | "git"
    pub panel: String,
    /// psp dock width in px (300–500)
    pub slp_w: f32,
    /// panels + statusbar hidden (Obsidian-style collapse)
    pub panes_hidden: bool,
    /// "grouped" | "flat"
    pub list_mode: String,
    /// "time" | "manual"
    pub sort_mode: String,
    /// collapsed project groups (workspace keys)
    pub collapsed: Vec<String>,
    /// 021 文件树操作栏「切换显示变更」开关（只列 git 变更文件）
    pub tree_changes_only: bool,
    /// 020-1 整合面板：会话子面板高度占比（0..1）；None = 默认 50/50
    pub dock_split: Option<f32>,
    /// 020 项目用户排序（psp 项目组 ws_key 有序表，拖拽落位产物）；空 =
    /// 无记录（列表用首建活动序）
    pub proj_order: Vec<String>,
    /// 020-1 dock 最大化：「none」|「files」|「sessions」——文件树/会话列表
    /// 最大化后关闭窗口，重启要还在该子面板最大化态（用户报障 2026-10-17）
    pub dock_max: String,
    /// 会话置顶（psp 会话标签 pin）：绝对路径有序表，**越后置顶越靠前**
    /// （index 0 = 最近置顶）。置顶会话哪怕跌出加载时间窗口也要可见，
    /// 所以这份清单还参与启动装载并集（startup::spawn_boot_restore）。
    pub session_pins: Vec<String>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            panel: "sessions".into(),
            slp_w: 340.,
            panes_hidden: false,
            list_mode: "grouped".into(),
            sort_mode: "time".into(),
            collapsed: Vec::new(),
            tree_changes_only: false,
            dock_max: "none".into(),
            dock_split: None,
            proj_order: Vec::new(),
            session_pins: Vec::new(),
        }
    }
}

const WS_UI_KEY: &str = "__ui";

pub fn ui_state() -> UiState {
    let d = UiState::default();
    let m = memory();
    let Some(v) = m.get(WS_UI_KEY) else {
        return d;
    };
    UiState {
        panel: v
            .get("panel")
            .and_then(|p| p.as_str())
            .unwrap_or(&d.panel)
            .to_string(),
        slp_w: v
            .get("slp_w")
            .and_then(|w| w.as_f64())
            .map(|w| (w as f32).clamp(300., 500.))
            .unwrap_or(d.slp_w),
        panes_hidden: v
            .get("panes_hidden")
            .and_then(|b| b.as_bool())
            .unwrap_or(false),
        list_mode: v
            .get("list_mode")
            .and_then(|s| s.as_str())
            .filter(|s| *s == "flat")
            .unwrap_or(&d.list_mode)
            .to_string(),
        sort_mode: v
            .get("sort_mode")
            .and_then(|s| s.as_str())
            .filter(|s| *s == "manual")
            .unwrap_or(&d.sort_mode)
            .to_string(),
        collapsed: v
            .get("collapsed")
            .and_then(|c| c.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        tree_changes_only: v
            .get("tree_changes_only")
            .and_then(|b| b.as_bool())
            .unwrap_or(false),
        dock_split: v
            .get("dock_split")
            .and_then(|r| r.as_f64())
            .map(|r| (r as f32).clamp(0.1, 0.9)),
        proj_order: v
            .get("proj_order")
            .and_then(|c| c.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        dock_max: v
            .get("dock_max")
            .and_then(|s| s.as_str())
            .filter(|s| matches!(*s, "files" | "sessions"))
            .unwrap_or("none")
            .to_string(),
        session_pins: v
            .get("session_pins")
            .and_then(|c| c.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// dock 最大化态 ↔ layout 两标志的**唯一映射**（两端共用，防漂移）。
///
/// 语义陷阱：`sess_min` / `files_min` 表示该面板**最小化**（收缩成控制栏），
/// 所以「文件树最大化」= `sess_min`。持久化字段 `dock_max` 反过来按
/// 「哪个面板最大化」取名，更直观。
pub fn dock_max_from_min(sess_min: bool, files_min: bool) -> &'static str {
    if sess_min {
        "files"
    } else if files_min {
        "sessions"
    } else {
        "none"
    }
}

/// [`dock_max_from_min`] 的反向：`dock_max` → (sess_min, files_min)。
pub fn min_from_dock_max(dock_max: &str) -> (bool, bool) {
    match dock_max {
        "files" => (true, false),
        "sessions" => (false, true),
        _ => (false, false),
    }
}

pub fn save_ui_state(s: &UiState) {
    let mut map = memory();
    map.insert(
        WS_UI_KEY.to_string(),
        serde_json::json!({
            "panel": s.panel, "slp_w": s.slp_w, "panes_hidden": s.panes_hidden,
            "list_mode": s.list_mode, "sort_mode": s.sort_mode, "collapsed": s.collapsed,
            "tree_changes_only": s.tree_changes_only,
            "dock_split": s.dock_split,
            "proj_order": s.proj_order,
            "dock_max": s.dock_max,
            "session_pins": s.session_pins,
        }),
    );
    set_memory(&map);
}

/// 021 Pin 收藏夹（`__pins`：{ ws_key: [绝对路径…] }）——文件树目录/文件
/// 收藏，per-project 各一份，跨重启保留。
const WS_PINS_KEY: &str = "__pins";

/// 当前项目的 Pin 收藏清单（存储序；展示序由 UI 按名称字母排）。
pub fn pins_for(cwd: &str) -> Vec<String> {
    memory()
        .get(WS_PINS_KEY)
        .and_then(|v| v.get(&ws_key(cwd)))
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// 整体改写当前项目的 Pin 收藏（pin/unpin/改名跟随/删除清理共用一条写路）；
/// 空清单 = 移除该项目的槽位。
pub fn save_pins_for(cwd: &str, pins: &[String]) {
    let mut map = memory();
    let key = ws_key(cwd);
    let entry = map
        .entry(WS_PINS_KEY.to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(serde_json::Map::new());
    }
    if let Some(obj) = entry.as_object_mut() {
        if pins.is_empty() {
            obj.remove(&key);
        } else {
            obj.insert(
                key,
                Value::Array(pins.iter().map(|p| Value::String(p.clone())).collect()),
            );
        }
    }
    set_memory(&map);
}

// ---------------------------------------------------------------------------
// 文件树状态（`__tree`：{ ws_key: { expanded: [相对路径…], pin: bool } }）
//
// 对齐 Zed：project panel 的展开始终是 **per-worktree 内存态**（
// `expanded_dir_ids: HashMap<WorktreeId, Vec<ProjectEntryId>>`，entry id 每次
// 重建都变，天然不跨会话），切项目不丢、打开项目默认展开根。本仓在此之上
// 加一层磁盘持久化（Zed 不做），让「我展开了 src/crates」跨重启还在。
//
// 存**相对项目根**的路径（"" = 根）：项目搬迁 / 盘符大小写变化时不废，体积也小。
// ---------------------------------------------------------------------------

/// 每项目展开条目上限（防御大仓库无限增长；超出丢弃最深项）。
pub const TREE_EXPANDED_CAP: usize = 300;

/// `__tree`：文件树状态（展开集合 + Pin 收藏夹视图开关）。
const WS_TREE_KEY: &str = "__tree";

/// 一个项目的文件树状态。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TreeState {
    /// 展开目录（相对项目根；`""` = 根）
    pub expanded: Vec<String>,
    /// 关闭前停在 Pin 收藏夹视图（重启恢复该视图）
    pub pin: bool,
}

/// 纯内核：从 map 读一个项目的文件树状态（无记录 = None）。
fn tree_state_from(map: &serde_json::Map<String, Value>, cwd: &str) -> Option<TreeState> {
    let v = map.get(WS_TREE_KEY)?.get(&ws_key(cwd))?.clone();
    let expanded = v
        .get("expanded")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let pin = v.get("pin").and_then(|b| b.as_bool()).unwrap_or(false);
    Some(TreeState { expanded, pin })
}

/// 读取一个项目的文件树状态（无记录 = None，调用方落默认「只展开根」）。
pub fn tree_state_for(cwd: &str) -> Option<TreeState> {
    tree_state_from(&memory(), cwd)
}

/// 纯内核：把一个项目的文件树状态写进传入 map（默认态 = 移除槽位）。
/// **绝不碰全局缓存/落盘**——落盘职责归调用方（测试用内核，不写任何盘）。
fn tree_state_into_map(map: &mut serde_json::Map<String, Value>, cwd: &str, state: &TreeState) {
    let key = ws_key(cwd);
    let entry = map
        .entry(WS_TREE_KEY.to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(serde_json::Map::new());
    }
    let is_default = !state.pin && state.expanded.iter().all(|p| p.is_empty());
    if let Some(obj) = entry.as_object_mut() {
        if is_default {
            obj.remove(&key);
        } else {
            obj.insert(
                key,
                serde_json::json!({
                    "expanded": state.expanded,
                    "pin": state.pin,
                }),
            );
        }
    }
}

/// 整体改写一个项目的文件树状态；完全默认（只展开根且不在 Pin 视图）= 移除槽位。
pub fn save_tree_state(cwd: &str, state: &TreeState) {
    let mut map = memory();
    tree_state_into_map(&mut map, cwd, state);
    set_memory(&map);
}

/// 绝对展开集合 → 存储形态：相对项目根（`""` = 根）、项目外路径丢弃、
/// 稳定排序后截断 [`TREE_EXPANDED_CAP`]（先保根，再按深度/字母）。
pub fn tree_rel_paths(cwd: &Path, expanded: &std::collections::HashSet<PathBuf>) -> Vec<String> {
    let mut rel: Vec<String> = expanded
        .iter()
        .filter(|p| is_under(p, cwd))
        .map(|p| {
            p.strip_prefix(cwd)
                .map(|r| r.to_string_lossy().to_string())
                .unwrap_or_default()
        })
        .collect();
    rel.sort_by(|a, b| {
        let depth = |s: &str| s.matches(['\\', '/']).count();
        depth(a).cmp(&depth(b)).then_with(|| a.cmp(b))
    });
    rel.dedup();
    // 根（空串）优先保留
    rel.sort_by_key(|s| !s.is_empty());
    rel.truncate(TREE_EXPANDED_CAP);
    rel
}

/// 存储形态 → 绝对展开集合：解析到 cwd 下，**丢弃项目外 / 已不存在**的条目
/// （磁盘目录已删、旧版绝对路径残留），恒含根，超上限截断。
pub fn tree_abs_paths(cwd: &Path, rel: &[String]) -> std::collections::HashSet<PathBuf> {
    let mut out: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    out.insert(cwd.to_path_buf());
    for r in rel.iter().take(TREE_EXPANDED_CAP) {
        if r.is_empty() {
            continue;
        }
        // 非法条目直接丢：绝对路径 / 含 `..`（`cwd.join("..")` 的字符串
        // 前缀仍是 cwd，单靠 is_under 拦不住）
        let rel_path = Path::new(r);
        if rel_path.is_absolute()
            || rel_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            continue;
        }
        let p = cwd.join(r);
        if p.is_dir() && is_under(&p, cwd) {
            out.insert(p);
        }
    }
    out
}

pub fn get_window_state() -> Option<WindowState> {
    let map = memory();
    let v = map.get(WS_WINDOW_KEY)?;
    Some(WindowState {
        x: v.get("x")?.as_f64()?,
        y: v.get("y")?.as_f64()?,
        w: v.get("w")?.as_f64()?,
        h: v.get("h")?.as_f64()?,
        maximized: v.get("max").and_then(|m| m.as_bool()).unwrap_or(false),
    })
}

pub fn save_window_state(s: &WindowState) {
    let mut map = memory();
    map.insert(
        WS_WINDOW_KEY.to_string(),
        serde_json::json!({
            "x": s.x, "y": s.y, "w": s.w, "h": s.h, "max": s.maximized,
        }),
    );
    set_memory(&map);
}

// ---------------------------------------------------------------------------
// app settings (006 界面设置): theme / icon theme / fonts + lang / sound
// ---------------------------------------------------------------------------

/// One font slot of the appearance settings (006): family + pt size.
/// `None` fields mean "unset" so defaults stay code-side.
#[derive(Debug, Clone, PartialEq)]
pub struct FontSpec {
    pub family: String,
    pub size: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AppSettings {
    /// theme id ("mist" / "rose" / "one-light" / ... 006 built-ins)
    pub theme: Option<String>,
    /// icon theme id (built-in set is pi-web's own icons)
    pub icon_theme: Option<String>,
    /// chat/session font (≈ zed UI font)
    pub session_font: Option<FontSpec>,
    /// function-panel font
    pub panel_font: Option<FontSpec>,
    /// markdown preview font
    pub markdown_font: Option<FontSpec>,
    /// language index (i18n::TABLE rows)
    pub lang: Option<usize>,
    /// notification sound on/off
    pub sound: Option<bool>,
    /// v60: startup load time window in days (7/14/30) — sessions whose
    /// last activity falls inside the window form the psp list
    pub load_window_days: Option<u64>,
    /// v54 其他页: restore last workspace + session on startup
    pub restore: Option<bool>,
    /// 界面页: 展示思考（关（默认）=新思考块收成一行；开=默认展开全文）
    pub show_thinking: Option<bool>,
    /// 界面页: 文件树 git 标识（023 默认关——清爽目录树；开 = M/A/D/R/U/C
    /// 徽标 + 目录变更点）
    pub git_markers: Option<bool>,
    /// 其他页: 各项目默认显示的会话数量（psp 初始页大小，3-10，默认 10）
    pub session_display_count: Option<u64>,
    /// 其他页: 启动自动折叠阈值（小时）——项目内会话最后活动时间全部早于
    /// 阈值即折叠。档位 24/72/168（1/3/7 天），默认 168（7 天）
    pub auto_collapse_hours: Option<u64>,
    /// @ 文件索引缓存有效期（秒，1-3600，默认 60）
    pub at_index_ttl_secs: Option<u64>,
    /// @ 文件索引缓存的最多项目数（每 cwd 一份，1-100，默认 10）
    pub at_index_max_projects: Option<u64>,
    /// 042：默认模型（设置-模型页下列表五角星）。PF 自有默认——不写 pi 的
    /// settings.json（系统 pi / pi-web 的地盘），spawn 时以 `--model` 落到
    /// 新会话。两键齐才算设置了默认。
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
    /// 042：被【移除】的内置商（不再出现在设置页合并列表；重新配置 key 时
    /// 自动除名）。空数组与缺键同义。
    pub removed_providers: Option<Vec<String>>,
    /// 081 其他页: 自动更新（默认 on——off 时只提示新版本号 ↑ 不下载）
    pub auto_update: Option<bool>,
    /// 052 其他页: 会话自动清理天数——最后活动超出该天数的非置顶会话
    /// 启动时移入回收站。默认 0 = 关闭（2026-10-18 用户定夺），设置后钳制 [0,100]。
    pub auto_clean_days: Option<u64>,
    /// 界面页: Markdown 所见即所得（024 Live Preview 装饰；默认关——
    /// 已知回车崩溃与标题行距问题未修，用户显式开启才挂装饰）
    pub md_live_preview: Option<bool>,
    /// 界面页: 部分操作隐藏（dock 两操作栏不常用 icon 常态透明、hover
    /// 操作栏才显影；默认 on）
    pub autohide_icons: Option<bool>,
    /// 界面页: 会话内容宽度 px（消息列 / composer 胶囊 / 新会话页内容带 /
    /// ask 问卷卡共用的单一内容列宽，pi-web `--chat-content-max-width`
    /// parity；缺键 = [`CHAT_WIDTH_DEFAULT`]）
    pub chat_content_width: Option<u64>,
    /// 090 排队护栏: 运行中发送方式——"queue"（默认）= 扣 PF 本地队列、
    /// AgentSettled 才逐条发出；"steer" = prompt+steer 立即插进当前轮。
    /// 非法值/缺键都落 queue。
    pub send_mode: Option<String>,
}

fn app_settings_path() -> Option<PathBuf> {
    // pi-flash 自有目录（010-启动.md §4）
    pi_link::paths::app_settings_file()
}

fn font_spec_from(v: &Value) -> Option<FontSpec> {
    Some(FontSpec {
        family: v.get("family")?.as_str()?.to_string(),
        size: v.get("size")?.as_f64()? as f32,
    })
}

fn font_spec_to(spec: &FontSpec) -> Value {
    serde_json::json!({ "family": spec.family, "size": spec.size })
}

/// One shared cache for app settings (single static for reads and writes).
fn with_app_settings_cache<T>(f: impl FnOnce(&mut Option<AppSettings>) -> T) -> T {
    static CACHE: Mutex<Option<AppSettings>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

/// Load app_settings.json; reads each file once per process. Absent
/// lang/sound fall back to their legacy workspace-memory keys.
pub fn app_settings() -> AppSettings {
    with_app_settings_cache(|cache| {
        if let Some(s) = cache.as_ref() {
            return s.clone();
        }
        let s = match app_settings_path() {
            Some(p) => {
                let map = load_plain_map_from(&p);
                AppSettings {
                    theme: map
                        .get("theme")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    icon_theme: map
                        .get("icon_theme")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    session_font: map.get("session_font").and_then(font_spec_from),
                    panel_font: map.get("panel_font").and_then(font_spec_from),
                    markdown_font: map.get("markdown_font").and_then(font_spec_from),
                    lang: map
                        .get("lang")
                        .and_then(|v| v.as_u64())
                        .map(|v| v.min(2) as usize),
                    sound: map.get("sound").and_then(|v| v.as_bool()),
                    load_window_days: map.get("load_window_days").and_then(|v| v.as_u64()),
                    restore: map.get("startup_restore").and_then(|v| v.as_bool()),
                    show_thinking: map.get("show_thinking").and_then(|v| v.as_bool()),
                    git_markers: map.get("git_markers").and_then(|v| v.as_bool()),
                    session_display_count: map
                        .get("session_display_count")
                        .and_then(|v| v.as_u64()),
                    auto_collapse_hours: map.get("auto_collapse_hours").and_then(|v| v.as_u64()),
                    at_index_ttl_secs: map.get("at_index_ttl_secs").and_then(|v| v.as_u64()),
                    at_index_max_projects: map
                        .get("at_index_max_projects")
                        .and_then(|v| v.as_u64()),
                    default_provider: map
                        .get("default_provider")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    default_model: map
                        .get("default_model")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    removed_providers: map.get("removed_providers").and_then(|v| {
                        Some(
                            v.as_array()?
                                .iter()
                                .filter_map(|e| e.as_str().map(str::to_string))
                                .collect::<Vec<_>>(),
                        )
                    }),
                    auto_update: map.get("auto_update").and_then(|v| v.as_bool()),
                    auto_clean_days: map
                        .get("auto_clean_days")
                        .and_then(|v| v.as_u64())
                        .map(|v| v.min(100)),
                    md_live_preview: map.get("md_live_preview").and_then(|v| v.as_bool()),
                    autohide_icons: map.get("autohide_icons").and_then(|v| v.as_bool()),
                    chat_content_width: map.get("chat_content_width").and_then(|v| v.as_u64()),
                    send_mode: map
                        .get("send_mode")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                }
            }
            None => AppSettings::default(),
        };
        *cache = Some(s.clone());
        s
    })
}

/// Atomic save + cache update; skips the write when nothing changed.
pub fn save_app_settings(s: &AppSettings) {
    with_app_settings_cache(|cache| *cache = Some(s.clone()));
    let Some(path) = app_settings_path() else {
        return;
    };
    let mut obj = serde_json::Map::new();
    if let Some(v) = &s.theme {
        obj.insert("theme".into(), Value::String(v.clone()));
    }
    if let Some(v) = &s.icon_theme {
        obj.insert("icon_theme".into(), Value::String(v.clone()));
    }
    if let Some(v) = &s.session_font {
        obj.insert("session_font".into(), font_spec_to(v));
    }
    if let Some(v) = &s.panel_font {
        obj.insert("panel_font".into(), font_spec_to(v));
    }
    if let Some(v) = &s.markdown_font {
        obj.insert("markdown_font".into(), font_spec_to(v));
    }
    if let Some(v) = s.lang {
        obj.insert("lang".into(), Value::Number((v as u64).into()));
    }
    if let Some(v) = s.sound {
        obj.insert("sound".into(), Value::Bool(v));
    }
    if let Some(v) = s.load_window_days {
        obj.insert("load_window_days".into(), Value::Number(v.into()));
    }
    if let Some(v) = s.restore {
        obj.insert("startup_restore".into(), Value::Bool(v));
    }
    if let Some(v) = s.show_thinking {
        obj.insert("show_thinking".into(), Value::Bool(v));
    }
    if let Some(v) = s.git_markers {
        obj.insert("git_markers".into(), Value::Bool(v));
    }
    if let Some(v) = s.session_display_count {
        obj.insert("session_display_count".into(), Value::Number(v.into()));
    }
    if let Some(v) = s.auto_collapse_hours {
        obj.insert("auto_collapse_hours".into(), Value::Number(v.into()));
    }
    if let Some(v) = s.at_index_ttl_secs {
        obj.insert("at_index_ttl_secs".into(), Value::Number(v.into()));
    }
    if let Some(v) = s.at_index_max_projects {
        obj.insert("at_index_max_projects".into(), Value::Number(v.into()));
    }
    if let Some(v) = &s.default_provider {
        obj.insert("default_provider".into(), Value::String(v.clone()));
    }
    if let Some(v) = &s.default_model {
        obj.insert("default_model".into(), Value::String(v.clone()));
    }
    match &s.removed_providers {
        Some(v) if !v.is_empty() => {
            obj.insert(
                "removed_providers".into(),
                Value::Array(v.iter().map(|e| Value::String(e.clone())).collect()),
            );
        }
        _ => {}
    }
    if let Some(v) = s.auto_update {
        obj.insert("auto_update".into(), Value::Bool(v));
    }
    if let Some(v) = s.auto_clean_days {
        obj.insert("auto_clean_days".into(), Value::Number(v.into()));
    }
    if let Some(v) = s.md_live_preview {
        obj.insert("md_live_preview".into(), Value::Bool(v));
    }
    if let Some(v) = s.autohide_icons {
        obj.insert("autohide_icons".into(), Value::Bool(v));
    }
    if let Some(v) = s.chat_content_width {
        obj.insert("chat_content_width".into(), Value::Number(v.into()));
    }
    if let Some(v) = &s.send_mode {
        obj.insert("send_mode".into(), Value::String(v.clone()));
    }
    save_map_to(&path, &obj);
}

/// 042：默认模型读取（两键齐生效；键不存在 = 未设置默认）。
pub fn default_model_pref() -> Option<(String, String)> {
    let s = app_settings();
    Some((s.default_provider?, s.default_model?))
}

/// 090 排队护栏：运行中发送方式（设置-其他）。返回 "queue"（默认，含缺键
/// /非法值）或 "steer"。
pub fn send_mode_pref() -> &'static str {
    match app_settings().send_mode.as_deref() {
        Some("steer") => "steer",
        _ => "queue",
    }
}

/// 090：发送方式写入（设置页两档分段按钮）。
pub fn save_send_mode(mode: &str) {
    let mut s = app_settings();
    s.send_mode = Some(mode.to_string());
    save_app_settings(&s);
}

/// 042：默认模型写入（设置页五角星 / inputpanel 星标共用；None = 取消）。
/// 落盘即生效——新会话 spawn 读最新值。
/// 042：【移除】的内置商清单（合并列表过滤用）。
pub fn removed_providers() -> Vec<String> {
    app_settings().removed_providers.unwrap_or_default()
}

/// 042：整体改写移除清单（空切片 = 清空/全恢复）。
pub fn set_removed_providers(list: &[String]) {
    let mut s = app_settings();
    s.removed_providers = Some(list.to_vec()).filter(|v| !v.is_empty());
    save_app_settings(&s);
}

pub fn set_default_model_pref(pref: Option<(String, String)>) {
    let mut s = app_settings();
    match pref {
        Some((p, m)) => {
            s.default_provider = Some(p);
            s.default_model = Some(m);
        }
        None => {
            s.default_provider = None;
            s.default_model = None;
        }
    }
    save_app_settings(&s);
}

/// Startup load time window in days (settings-其他「加载时间窗口」档位
/// 7/14/30, default 7) — the recents list filters on it.
pub fn load_window_days() -> u64 {
    match app_settings().load_window_days.unwrap_or(7) {
        14 => 14,
        30 => 30,
        _ => 7,
    }
}

/// Sessions whose tail is read into memory at startup (fixed, not a
/// setting — the load window is the user-facing knob).
pub const TAIL_PRELOAD: usize = 10;

/// v54 其他: startup restore toggle (default off).
/// 启动恢复开关：on = 落最近活动项目并**直接加载**该项目最后会话；
/// off = 落同一项目但停在新会话页（项目记忆与开关无关，恒生效）。
/// 默认 on（「启动恢复最后会话」是默认体验；显式关过的用户保留其设置）。
pub fn startup_restore() -> bool {
    app_settings().restore.unwrap_or(true)
}

/// 其他页: 各项目默认显示的会话数量（psp 初始页大小，3-10，默认 10）。
pub fn session_display_count() -> usize {
    app_settings()
        .session_display_count
        .unwrap_or(10)
        .clamp(3, 10) as usize
}

/// 其他页: 启动自动折叠阈值（小时；档位 24/72/168 = 1/3/7 天，默认 168）。
/// 启动装载项目集后，项目内会话最后活动时间全部早于该阈值 → 该项目组折叠。
pub fn auto_collapse_hours() -> u64 {
    match app_settings().auto_collapse_hours.unwrap_or(168) {
        24 => 24,
        72 => 72,
        _ => 168,
    }
}

/// @ 文件索引缓存有效期（秒；默认 60，clamp 1-3600）。
pub fn at_index_ttl_secs() -> u64 {
    app_settings()
        .at_index_ttl_secs
        .unwrap_or(60)
        .clamp(1, 3600)
}

/// @ 文件索引缓存的最多项目数（每 cwd 一份；默认 10，clamp 1-100）。
pub fn at_index_max_projects() -> usize {
    app_settings()
        .at_index_max_projects
        .unwrap_or(10)
        .clamp(1, 100) as usize
}

/// 界面页: 展示思考（关（默认）=新思考块收成一行；开=默认展开全文）。
pub fn show_thinking() -> bool {
    app_settings().show_thinking.unwrap_or(false)
}

/// 界面页: 部分操作隐藏（dock 会话控制栏「列表切换」与文件树操作栏五颗
/// 功能 icon 常态透明，hover 各自操作栏才显影；默认 on）。渲染期每帧读，
/// `app_settings()` 走进程内缓存，开销可忽略。
pub fn autohide_icons() -> bool {
    app_settings().autohide_icons.unwrap_or(true)
}

/// 开发旗标快照（dev_flags::KNOWN 全键，进程内首读一次）。渲染期每帧调用
/// 安全；文件改动重启（重新 cargo run）生效。
fn dev_flag_snapshot() -> &'static std::collections::HashMap<String, bool> {
    static SNAPSHOT: OnceLock<std::collections::HashMap<String, bool>> = OnceLock::new();
    SNAPSHOT.get_or_init(|| {
        pi_link::dev_flags::KNOWN
            .iter()
            .map(|key| (key.to_string(), pi_link::dev_flags::enabled(key)))
            .collect()
    })
}

/// 开发旗标查询（键 = 模块名，见 pi-link `dev_flags::KNOWN`；缺键 = false）。
pub fn dev_flag_enabled(key: &str) -> bool {
    dev_flag_snapshot().get(key).copied().unwrap_or(false)
}

/// subagent 功能总开关（开发旗标，默认关；dev-flags.json /
/// PI_FLASH_SUBAGENTS 控制）。
pub fn subagents_enabled() -> bool {
    dev_flag_enabled("subagents")
}

/// 040-1 Pi 扩展市场入口开关（开发旗标，**默认关 = 发版不可见**）。
/// 关态 = topbar 「菜单」三横钮不渲染、`open_pi_ext_market` 直接返回，
/// 任何入口（菜单/自动化）都摸不到市场页。
pub fn pi_ext_market_enabled() -> bool {
    dev_flag_enabled("pi_ext_market")
}

/// 060 微信通信模块总开关（开发旗标，**默认关 = 发版不可见**）。关态 =
/// 三横菜单【微信通信】项与 topbar 微信线 tab 不渲染、`open_wx` 直接
/// 返回、入站泵不挂（服务状态机根本不被问到——模块整死）。发版排除
/// 语义同 040-1 市场：dev 环境用 dev-flags.json `"wx": true` 或
/// `PI_FLASH_WX=1` 打开。
pub fn wx_enabled() -> bool {
    dev_flag_enabled("wx")
}

/// topbar「菜单」三横钮是否渲染：**恒显**（2026-10-23 用户禁令：不许删菜单 icon）。
/// 菜单是多入口的壳，里面 090-4「Pi-Flash 插件」是主线常驻项，按钮不能跟着
/// 别的页面的 dev-flag 一起消失；各市场项自己的显隐照旧由 menu.rs 管。
pub fn market_menu_enabled() -> bool {
    true
}

/// 文件树 git 标识开关（设置-界面；默认关 = 清爽目录树，023 定案）。
pub fn git_markers() -> bool {
    app_settings().git_markers.unwrap_or(false)
}

/// 081 其他页: 自动更新开关（默认 on）。
pub fn auto_update() -> bool {
    app_settings().auto_update.unwrap_or(true)
}

/// 052 其他页: 会话自动清理天数（默认 0 = 关闭——2026-10-18 用户定夺：自动删
/// 会话太凶，显式设置后才生效；读入口统一钳制 [0,100]，存量脏值也在
/// 这里被拦住）。
pub fn auto_clean_days() -> u64 {
    app_settings().auto_clean_days.unwrap_or(0).min(100)
}

/// 界面页: Markdown 所见即所得（024 Live Preview 装饰；默认关 = md 源码态
/// 走普通编辑器路径。已知回车崩溃与标题行距问题未修，显式开启才挂装饰）。
pub fn md_live_preview() -> bool {
    app_settings().md_live_preview.unwrap_or(false)
}

// ---- 会话内容宽度（pi-web `--chat-content-max-width` parity）------------
//
// 单一内容列宽：消息列、composer 胶囊、新会话页内容带、ask 问卷卡全部读
// 这里（此前各自硬编码 920），改一处即整列同宽、不会再错位。
// 阈值取法（2026-10-29 用户定案：1200 封顶、步进 20）：
// - 默认 920 = 本仓历史观感（pi-web 默认 820，我们不跟随改默认值）；
// - 最小 560 = 阅列底线（默认字号 15px 下约 37 汉字/行，用户气泡 85% 宽
//   约 30 字），再窄正文与代码块就开始一句一折行；pi-web 的 min 直接等于
//   默认（只许加宽），我们放开收窄——PF 左 dock 吃宽，收窄是真实需求；
// - 最大 1200 = 用户定案封顶（用户否决了按行字数/按窗口推导的方案：列宽
//   写的是 `w_full().max_w(cap)`，cap 只是封顶，与屏幕分辨率无关，小屏自动
//   取可用宽）；
// - 步进 20 = 用户定案（560→1200 共 32 档，一次拖动就能落位，存盘值整齐）。
pub const CHAT_WIDTH_MIN: u32 = 560;
pub const CHAT_WIDTH_DEFAULT: u32 = 920;
pub const CHAT_WIDTH_MAX: u32 = 1200;
/// 滑块步进（20px，用户定案；min/default/max 均为 20 的整数倍）
pub const CHAT_WIDTH_STEP: u32 = 20;

/// 会话内容宽度（px，已钳制；渲染期多次读取，只读缓存）。
pub fn chat_content_width() -> f32 {
    clamp_chat_width(
        app_settings()
            .chat_content_width
            .unwrap_or(CHAT_WIDTH_DEFAULT as u64),
    ) as f32
}

fn clamp_chat_width(v: u64) -> u32 {
    (v as u32).clamp(CHAT_WIDTH_MIN, CHAT_WIDTH_MAX)
}

/// 拖动中的实时预览：只改内存缓存不落盘（每帧一个 MouseMove，逐帧写
/// JSON 没必要），松手再 [`save_app_settings_now`] 落一次。
pub fn preview_chat_content_width(width: u32) {
    with_app_settings_cache(|cache| {
        let mut s = (*cache).clone().unwrap_or_default();
        s.chat_content_width = Some(clamp_chat_width(width as u64) as u64);
        *cache = Some(s);
    });
}

/// 把内存缓存（拖拽预览后的值）落盘。
pub fn save_app_settings_now() {
    save_app_settings(&app_settings());
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpfile(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pi-flash-ws-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn purge_app_memory_drops_poisoned_keys_only() {
        let root = Path::new("D:/softwares/pi-flash");
        let mut map = serde_json::Map::new();
        map.insert(
            WS_LAST_KEY.into(),
            Value::String("d:\\softwares\\pi-flash\\payload".into()),
        );
        map.insert(
            "d:\\softwares\\pi-flash\\payload".into(),
            Value::String("sess.jsonl".into()),
        );
        map.insert(
            "d:\\ai_workspace\\pi_work".into(),
            Value::String("keep.jsonl".into()),
        );
        map.insert("__window".into(), serde_json::json!({"max": true}));
        assert_eq!(purge_app_paths_in_map(&mut map, Some(root)), 2);
        assert!(!map.contains_key(WS_LAST_KEY), "脏 __last 删键");
        assert!(!map.contains_key("d:\\softwares\\pi-flash\\payload"));
        assert_eq!(map.get("d:\\ai_workspace\\pi_work").unwrap(), "keep.jsonl");
        assert!(map.contains_key("__window"), "非路径键不动");
        assert_eq!(purge_app_paths_in_map(&mut map, None), 0);
        assert_eq!(purge_app_paths_in_map(&mut map, Some(root)), 0, "幂等");
    }

    #[test]
    fn map_io_roundtrip_and_noop_skip() {
        let path = tmpfile("ws.json");
        let mut map = serde_json::Map::new();
        map.insert("k".into(), Value::String("v".into()));
        assert!(save_map_to(&path, &map));
        // identical content -> skipped write
        assert!(!save_map_to(&path, &map));
        let loaded = load_map_from(&path);
        assert_eq!(loaded.get("k").unwrap(), "v");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn atomic_write_leaves_no_tmp() {
        let path = tmpfile("atomic.json");
        let mut map = serde_json::Map::new();
        map.insert("a".into(), Value::Bool(true));
        save_map_to(&path, &map);
        assert!(path.is_file());
        assert!(
            !path.with_extension("json.tmp").exists(),
            "tmp renamed away"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn window_dock_state_roundtrip_via_map() {
        let path = tmpfile("layout.json");
        let mut map = serde_json::Map::new();
        map.insert(
            WS_WINDOW_KEY.into(),
            serde_json::json!({"x": 10.0, "y": 20.0, "w": 1180.0, "h": 760.0, "max": true}),
        );
        map.insert(
            "__ui".into(),
            serde_json::json!({"panel": "git", "slp_w": 300.0, "panes_hidden": false,
                                "list_mode": "grouped", "sort_mode": "time", "collapsed": []}),
        );
        save_map_to(&path, &map);
        let loaded = load_map_from(&path);
        let w = loaded.get(WS_WINDOW_KEY).unwrap();
        assert_eq!(w.get("max").unwrap(), &Value::Bool(true));
        assert_eq!(w.get("w").unwrap(), &serde_json::json!(1180.0));
        let d = loaded.get("__ui").unwrap();
        assert_eq!(d.get("panel").unwrap(), "git");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// 文件树状态：相对路径往返 + 失效 prune。
    #[test]
    fn tree_state_rel_abs_roundtrip_and_prune() {
        let base = tmpfile("tree").parent().unwrap().to_path_buf();
        let cwd = base.join("proj");
        std::fs::create_dir_all(cwd.join("src").join("deep")).unwrap();
        std::fs::create_dir_all(cwd.join("docs")).unwrap();

        let expanded: std::collections::HashSet<PathBuf> = [
            cwd.clone(),
            cwd.join("src"),
            cwd.join("src").join("deep"),
            cwd.join("docs"),
            base.join("other"),
        ]
        .into_iter()
        .collect();
        let rel = tree_rel_paths(&cwd, &expanded);
        assert!(rel.iter().any(|p| p.is_empty()), "根存空串");
        assert!(rel.iter().any(|p| p == "src"), "相对路径");
        assert!(!rel.iter().any(|p| p.contains("other")), "项目外丢弃");

        let back = tree_abs_paths(&cwd, &rel);
        assert!(back.contains(&cwd) && back.contains(&cwd.join("src")));
        assert!(back.contains(&cwd.join("src").join("deep")));

        std::fs::remove_dir_all(cwd.join("docs")).unwrap();
        let pruned = tree_abs_paths(
            &cwd,
            &["docs".to_string(), "src".to_string(), "..".to_string()],
        );
        assert_eq!(pruned.len(), 2, "只剩根与 src");
        assert!(pruned.contains(&cwd));
        assert!(!pruned.contains(&cwd.join("docs")));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 默认态（只展开根、不在 Pin 视图）不落槽位；有状态才写。走纯内核，
    /// 不经全局 save_tree_state —— 裸跑测试不写任何盘。
    #[test]
    fn tree_state_default_is_not_stored() {
        let base = tmpfile("tree-store").parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();
        let cwd = base.join("proj");
        std::fs::create_dir_all(&cwd).unwrap();
        let cwd_s = cwd.to_string_lossy().to_string();

        let mut map = serde_json::Map::new();
        tree_state_into_map(
            &mut map,
            &cwd_s,
            &TreeState {
                expanded: vec![String::new()],
                pin: false,
            },
        );
        assert!(tree_state_from(&map, &cwd_s).is_none(), "默认态不占槽位");

        tree_state_into_map(
            &mut map,
            &cwd_s,
            &TreeState {
                expanded: vec![String::new(), "src".into()],
                pin: true,
            },
        );
        let st = tree_state_from(&map, &cwd_s).expect("有内容应可读回");
        assert_eq!(st.expanded, vec![String::new(), "src".to_string()]);
        assert!(st.pin);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// dock 最大化映射：`*_min` = 最小化，所以「文件树最大化」= sess_min。
    #[test]
    fn dock_max_mapping_roundtrips() {
        for (sess_min, files_min) in [(false, false), (true, false), (false, true)] {
            let dm = dock_max_from_min(sess_min, files_min);
            assert_eq!(min_from_dock_max(dm), (sess_min, files_min), "dm={dm}");
        }
        assert_eq!(dock_max_from_min(true, false), "files", "文件树最大化");
        assert_eq!(dock_max_from_min(false, true), "sessions", "会话列表最大化");
        assert_eq!(min_from_dock_max("garbage"), (false, false), "坏值回落常态");
    }

    #[test]
    fn is_under_normalizes_separators_and_case() {
        // cwd 内（正/反斜杠、大小写都要判对）
        assert!(is_under(Path::new("D:\\proj\\src"), Path::new("D:\\proj")));
        assert!(is_under(Path::new("d:/proj/src"), Path::new("D:\\PROJ")));
        assert!(is_under(Path::new("D:\\proj"), Path::new("D:\\proj\\")));
        // 前缀像但不是子目录（proj2 不能被 proj 覆盖）
        assert!(!is_under(Path::new("D:\\proj2"), Path::new("D:\\proj")));
        // 工作区外（另一个盘/目录）
        assert!(!is_under(
            Path::new("D:\\my_obsidian\\vault"),
            Path::new("D:\\ai_workspace")
        ));
    }

    #[test]
    fn app_settings_roundtrip() {
        let path = tmpfile("app-settings.json");
        let mut obj = serde_json::Map::new();
        obj.insert("theme".into(), Value::String("mist".into()));
        obj.insert(
            "session_font".into(),
            serde_json::json!({"family": "Inter", "size": 14.5}),
        );
        obj.insert("lang".into(), Value::Number(1u64.into()));
        save_map_to(&path, &obj);
        let map = load_map_from(&path);
        let spec = font_spec_from(map.get("session_font").unwrap()).unwrap();
        assert_eq!(spec.family, "Inter");
        assert_eq!(spec.size, 14.5);
        assert_eq!(map.get("theme").unwrap(), "mist");
        assert_eq!(
            map.get("lang").and_then(|v| v.as_u64()),
            Some(1),
            "lang/sound live in app_settings.json, not the workspace map"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// tab 带存档（单槽全局）：纯内核往返 + 空带删槽位 + 越界 active
    /// 过滤 + 旧版 per-project 嵌套形态不识别。
    #[test]
    fn tabs_state_roundtrip_and_prune() {
        let state = TabsState {
            tabs: vec![
                SavedTab {
                    kind: "file".into(),
                    path: "D:\\a\\b.rs".into(),
                },
                SavedTab {
                    kind: "term".into(),
                    path: "D:\\a".into(),
                },
            ],
            active: Some(1),
            chat: false,
            wx_open: false,
            wx_front: false,
        };
        let v = tabs_state_to_value(&state).unwrap();
        assert_eq!(tabs_state_from_value(&v).unwrap(), state);

        // 越界 active 被读侧滤掉
        let v = tabs_state_to_value(&TabsState {
            active: Some(9),
            ..state.clone()
        })
        .unwrap();
        assert_eq!(
            tabs_state_from_value(&v).unwrap().active,
            None,
            "越界下标不采信"
        );

        // 空带 = None（写侧据此移除槽位）；微信线例外（可单独存活）
        assert!(tabs_state_to_value(&TabsState::default()).is_none());
        let wx_page_only = TabsState {
            tabs: vec![],
            active: None,
            chat: false,
            wx_open: true,
            wx_front: true,
        };
        assert!(tabs_state_to_value(&wx_page_only).is_some());

        // 旧档（单字段 `wx`）读成「标签开 + 前台是微信页」
        let legacy = serde_json::json!({ "tabs": [], "active": null, "chat": false, "wx": true });
        let back = tabs_state_from_value(&legacy).unwrap();
        assert!(back.wx_open && back.wx_front, "旧 wx 档兼容");

        // 标签开着但前台是桌面会话：tab 存活、前台不抢
        let wx_bg = TabsState {
            tabs: vec![],
            active: None,
            chat: true,
            wx_open: true,
            wx_front: false,
        };
        let v = tabs_state_to_value(&wx_bg).unwrap();
        assert_eq!(tabs_state_from_value(&v).unwrap(), wx_bg);

        // 非法条目读侧丢弃（kind 不识 / 空 path）
        let v = serde_json::json!({ "tabs": [
            {"k": "file", "p": "D:\\x.rs"},
            {"k": "market", "p": ""},
            {"k": "term", "p": ""},
        ], "active": 0, "chat": true });
        let back = tabs_state_from_value(&v).unwrap();
        assert_eq!(back.tabs.len(), 1, "非法条目丢弃");

        // 旧版 per-project 嵌套形态（无顶层 tabs）= 不识别
        let legacy = serde_json::json!({ "d:\\a": { "tabs": [
            {"k": "file", "p": "D:\\x.rs"}], "active": 0, "chat": true }});
        assert!(tabs_state_from_value(&legacy).is_none(), "旧形态不采信");
    }
}
