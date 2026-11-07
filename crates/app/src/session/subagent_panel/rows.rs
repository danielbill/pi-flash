//! 行渲染残件：状态点色（二级菜单 2026-11-07 用户定夺删除——原本住在这里
//! 的 panel_row/统计段/状态标记随浮层一起退役；点色供导航轮卡第三行）。

use super::*;

/// 第三行状态点色（2026-11-07 定夺：**对齐会话列表状态槽口径**，不自造）：
/// 运行 accent、完成 NOTICE 青蓝、中断/轮次用尽 WARN 橙（= 列表
/// turn_warnings 同源）、异常 danger 红。
pub(super) fn row_dot_color(row: &CatalogRow) -> u32 {
    let t = T();
    match row.status_kind() {
        StatusKind::Running => t.accent,
        StatusKind::Done => crate::appearance::theme::NOTICE,
        StatusKind::Aborted | StatusKind::MaxTurns => crate::appearance::theme::WARN,
        StatusKind::Error => t.danger,
    }
}
