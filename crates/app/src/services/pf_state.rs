//! 001-S4 运行期状态投影：Chat 实体仍是唯一真源，关键变动经这里同步进
//! `hub.state()` 快照；读点经 [`dock`] 拿 Arc 快照（O(1)、不持锁）。state
//! 不落盘、不投影、可重建——进程退出即失。dock 键口径 = "sessions" /
//! "git" / "plugin:<ix>"（比 DockPanel::as_str 持久化串更细：区分插件下标，
//! 状态栏/面板的 active 判断需要）。

/// dock 面板变动同步（各赋值点调用；漏发 = 读点看到旧值，故集中在此）
pub fn publish_dock(panel: crate::DockPanel) {
    pf_context::hub()
        .state
        .update(|s| s.dock = Some(dock_key(panel)));
}

/// 当前 dock 面板键（读点用；`None` = 尚未发布，读点回落缺省面板）。
pub fn dock() -> Option<String> {
    pf_context::hub().state.get().dock.clone()
}

/// dock 键比较（读点糖：`dock_is("git")` / `dock_is(&format!("plugin:{ix}"))`）。
pub fn dock_is(key: &str) -> bool {
    dock().as_deref() == Some(key)
}

/// 窗口尺寸快照同步（退出序列 shutdown_for_close 发布）。
pub fn publish_window(w: f64, h: f64, maximized: bool) {
    pf_context::hub()
        .state
        .update(|s| s.window = Some(pf_context::state::WindowSnapshot { w, h, maximized }));
}

/// 当前会话键同步（启动期 boot 复位点；会话切换发布点在 session/pool——
/// 该域文件另有进行中改动，S4+ 补）。
pub fn publish_active_session(key: &str) {
    pf_context::hub()
        .state
        .update(|s| s.active_session = Some(key.to_string()));
}

/// DockPanel → 状态键（与 `as_str` 持久化串的区别：插件态带下标）。
pub(crate) fn dock_key(panel: crate::DockPanel) -> String {
    match panel {
        crate::DockPanel::Default => "sessions".into(),
        crate::DockPanel::Git => "git".into(),
        crate::DockPanel::Plugin(ix) => format!("plugin:{ix}"),
    }
}
