//! Workspace 运行态（runtime state 持久化）：last-open 会话记忆 / 声音·语言
//! 偏好 / 全量 purge / per-project tab strip / 窗口与 dock 布局（UiState）/
//! 项目钉扎 / 文件树展开状态。落盘一律经 store（唯一 IO 层）。
//! 自 workspace.rs 拆出（090 重构）。

use super::*;
use std::path::Path;
use std::path::PathBuf;

use serde_json::Value;

/// Window bounds / maximized flag (schema landed in phase A, consumed by
/// the phase-D shell).
const WS_WINDOW_KEY: &str = "__window";
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
    Some(TabsState { tabs, active, chat })
}

/// 读取全局 tab 带存档。
pub fn tabs_state_for() -> Option<TabsState> {
    memory().get(WS_TABS_KEY).and_then(tabs_state_from_value)
}

/// 纯内核：存档 → JSON 值（空带 = None，调用方据此移除槽位）。
fn tabs_state_to_value(state: &TabsState) -> Option<Value> {
    // 空带通常 = 移除槽位（启动落默认 chat）；但空带也可能只是插件功能页
    // 暂存态——tab 带本就为空，若这里返回 None，重启就
    // 「蹿回桌面会话对话页」（2026-10-25 真机事故）
    if state.tabs.is_empty() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::workspace::load_map_from;
    use std::path::PathBuf;
    use std::sync::Mutex;

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

        // 空带 = None（写侧据此移除槽位）
        assert!(tabs_state_to_value(&TabsState::default()).is_none());

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
