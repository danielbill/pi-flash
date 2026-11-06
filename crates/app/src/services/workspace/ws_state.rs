//! Workspace 运行态（runtime state 持久化）：last-open 会话记忆 / 声音·语言
//! 偏好 / 全量 purge / 窗口与 dock 布局（UiState）/ 项目钉扎。落盘一律经
//! store（唯一 IO 层）。自 workspace.rs 拆出（090 重构）。itlq.9 超规拆分后
//! tab 带持久化在 `tabs_state`、文件树状态在 `tree_restore`，测试外移
//! `tests`。

use super::*;
use std::path::Path;

use serde_json::Value;

mod tabs_state;
mod tree_restore;

#[cfg(test)]
mod tests;

pub(crate) use tabs_state::*;
pub(crate) use tree_restore::*;

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
    /// 034 会话区底部状态栏显示开关（composer 功能栏 MCP 右侧 switch）。
    /// None = 旧档案缺字段，按默认**开**处理。
    pub session_status_bar: Option<bool>,
    /// 034 状态带**逐键静音**（用户偏好，产品默认全显示）：不渲染的 statusKey
    /// 列表。数据面照旧收全量（ext_status），只是画的时候跳过；当前没有逐键
    /// 的 UI 入口（后续可加"隐藏这一行"），先由配置承载。
    pub ext_status_muted: Vec<String>,
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
            session_status_bar: None,
            ext_status_muted: Vec::new(),
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
        session_status_bar: v.get("session_status_bar").and_then(|b| b.as_bool()),
        ext_status_muted: v
            .get("ext_status_muted")
            .and_then(|c| c.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// 034 状态带当前是否显示（缺字段 = 默认开）。
pub fn session_status_bar_on() -> bool {
    ui_state().session_status_bar.unwrap_or(true)
}

/// 034 状态带逐键静音清单（含该 key 就不渲染那一项）。
pub fn ext_status_muted_keys() -> Vec<String> {
    ui_state().ext_status_muted
}

/// 034 状态带逐键静音落盘。（静音入口后续做"隐藏这一行"时接上；当前先由
/// workspace.json 的 `__ui.ext_status_muted` 直接承载）
#[allow(dead_code)]
pub fn set_ext_status_muted_keys(keys: Vec<String>) {
    let mut st = ui_state();
    st.ext_status_muted = keys;
    save_ui_state(&st);
}

/// 034 状态带显示开关落盘（单字段读-改-写，走同一 `__ui` 通道）。
pub fn set_session_status_bar(on: bool) {
    let mut st = ui_state();
    st.session_status_bar = Some(on);
    save_ui_state(&st);
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
            "session_status_bar": s.session_status_bar,
            "ext_status_muted": s.ext_status_muted,
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
