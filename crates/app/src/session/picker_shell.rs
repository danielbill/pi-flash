//! 勾选式 picker 公共骨架（090 重构收口）：MCP / 扩展 / 模型三个 picker
//! 共用的锚点定位、卡壳、限高滚动列表与空态行。
//!
//! 各 picker 的行渲染、开关业务语义（勾选即落盘 / 会话清单对齐）不入本
//! 文件——这里只有「长什么样、摆在哪里」。

use gpui::{
    AnyElement, IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px, rgb,
};

use crate::appearance::theme::Theme;
use crate::shell::types::PillAnchor;

/// 行高（10 行限高 = `ROW_H × 10`，三 picker 一致）。
pub(crate) const ROW_H: f32 = 30.;

/// 弹出菜单统一内边距（用户定稿 10px）：行 / 标题 / 空态的水平与上下
/// 留白唯一取值，扩展 / MCP / 模型 picker 与工具·思考胶囊菜单共用。
pub(crate) const PAD: f32 = 10.;

// ---------------------------------------------------------------------------
// 锚点定位
// ---------------------------------------------------------------------------

/// 弹出卡片的统一定位结果（唯一摆放算法 [`pill_anchor_edges`] 的产出）：
/// 水平恒为按钮居中；纵向二选一——`top` = 向下弹（按钮在视口上半），
/// `bottom` = 向上弹（按钮在视口下半，composer 常态）。
pub(crate) struct PillCardPos {
    pub left: gpui::Pixels,
    pub top: Option<gpui::Pixels>,
    pub bottom: Option<gpui::Pixels>,
}

/// 胶囊锚点 → 面板 (left + 纵向边)：**统一定位唯一落点**（2026-11-07 半屏
/// 定向收口）——按钮水平居中、间距 5px（不遮按钮）、右缘出视口贴边（留
/// 8px）；纵向按按钮所在半屏定向：按钮顶缘在视口上半 → 菜单向下弹（顶缘
/// 距按钮底缘 5px），否则向上弹（底缘距按钮顶缘 5px）。无锚点（自动化
/// 入口）= 右下兜底 64/24。inputpanel 三 picker / 工具·思考胶囊菜单 /
/// 设置·子代理页五菜单全走这一份，不许再各自搓定位。
pub(crate) fn pill_anchor_edges(
    anchor: Option<PillAnchor>,
    vp: gpui::Size<gpui::Pixels>,
    panel_w: gpui::Pixels,
) -> PillCardPos {
    let gap = px(5.);
    let mut left = match anchor {
        Some(a) => a.center_x - panel_w / 2.,
        None => vp.width - panel_w - px(24.),
    };
    if left + panel_w > vp.width - px(8.) {
        left = vp.width - panel_w - px(8.);
    }
    let left = left.max(px(8.));
    match anchor {
        Some(a) if a.top <= vp.height / 2. => PillCardPos {
            left,
            top: Some(a.bottom + gap),
            bottom: None,
        },
        Some(a) => PillCardPos {
            left,
            top: None,
            bottom: Some(vp.height - a.top + gap),
        },
        None => PillCardPos {
            left,
            top: None,
            bottom: Some(px(64.)),
        },
    }
}

// ---------------------------------------------------------------------------
// 卡壳与列表
// ---------------------------------------------------------------------------

/// picker 卡壳：absolute 定位 + 圆角描边卡（纵向边由 [`PillCardPos`] 决定
/// 向上/向下弹）。调用方继续挂键盘钩子（模型 picker 的 Enter/↑/↓）并包
/// `overlay::stop_click`、塞 header/列表。
pub(crate) fn picker_card(pos: PillCardPos, panel_w: gpui::Pixels, t: &'static Theme) -> gpui::Div {
    let base = div()
        .absolute()
        .left(pos.left)
        .w(panel_w)
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg))
        .shadow_lg()
        .overflow_hidden()
        .flex()
        .flex_col();
    match pos.top {
        Some(top) => base.top(top),
        None => base.bottom(pos.bottom.unwrap_or(px(64.))),
    }
}

/// 限高：n 行 × [`ROW_H`] 封顶 10 行；空列表给 60（≈两行空态的节奏）。
pub(crate) fn capped_list_h(n_rows: usize) -> f32 {
    if n_rows == 0 {
        60.
    } else {
        (n_rows as f32 * ROW_H).min(ROW_H * 10.)
    }
}

/// 10 行限高滚动列表容器：内容不到按内容高（不出滚动条），超出才截到
/// [`capped_list_h`] 并接调用方跨帧复用的滚动句柄。
pub(crate) fn capped_list(
    id: &'static str,
    scroll: &gpui::ScrollHandle,
    list_h: f32,
    rows: Vec<AnyElement>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .pb(px(PAD))
        .h(px(list_h))
        .overflow_y_scroll()
        .track_scroll(scroll)
        .flex()
        .flex_col()
        .children(rows)
}

/// 空态行（PAD/ui11/text_dim）。
pub(crate) fn empty_row(text: SharedString) -> AnyElement {
    let t = crate::appearance::theme::theme();
    div()
        .px(px(PAD))
        .py(px(PAD))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text_dim))
        .child(text)
        .into_any_element()
}
