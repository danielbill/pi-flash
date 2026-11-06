//! 内容区 tab 带持久化（`__tabs`：{ tabs, active, chat }，单槽全局）。
//! 自 ws_state.rs 切出（itlq.9 超规拆分，纯结构搬移）。

use super::*;
use serde_json::Value;

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
pub(crate) fn tabs_state_from_value(v: &Value) -> Option<TabsState> {
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
pub(crate) fn tabs_state_to_value(state: &TabsState) -> Option<Value> {
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
