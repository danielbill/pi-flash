//! 行渲染残件：状态点色与时长文案（二级菜单 2026-11-07 用户定夺删除——
//! 原本住在这里的 panel_row/统计段/状态标记随浮层一起退役；点色供导航轮
//! 卡第三行，时长供会话流 subagent 工具卡）。

use super::*;

/// 第三行状态点色（会话运行结果青红点）：运行 accent、完成 NOTICE、
/// 异常/中止/轮次用尽 danger。
pub(super) fn row_dot_color(row: &CatalogRow) -> u32 {
    let t = T();
    if row.in_flight() {
        t.accent
    } else if row.status_kind() == StatusKind::Done {
        crate::appearance::theme::NOTICE
    } else {
        t.danger
    }
}

/// 秒 → 紧凑时长（pi-web 口径）：22s / 1m42s。
pub(crate) fn duration_text(secs: i64) -> String {
    if secs < 0 {
        return "—".into();
    }
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m{}s", secs / 60, secs % 60)
    }
}
