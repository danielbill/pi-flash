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

// ---------------------------------------------------------------------------
// 锚点定位
// ---------------------------------------------------------------------------

/// 胶囊锚点 → 面板 (bottom, left)：统一定位（按钮水平居中 + 5px 间距），
/// 右缘出视口贴边（留 8px），无锚点（自动化入口）= 右下兜底 64/24。
/// 三份逐字符复制的算法的唯一落点。
pub(crate) fn pill_anchor_edges(
    anchor: Option<PillAnchor>,
    vp: gpui::Size<gpui::Pixels>,
    panel_w: gpui::Pixels,
) -> (gpui::Pixels, gpui::Pixels) {
    let gap = px(5.);
    match anchor {
        Some(a) => {
            let bottom = (vp.height - a.top + gap).max(px(8.));
            let mut left = a.center_x - panel_w / 2.;
            if left + panel_w > vp.width - px(8.) {
                left = vp.width - panel_w - px(8.);
            }
            (bottom, left.max(px(8.)))
        }
        None => (px(64.), vp.width - panel_w - px(24.)),
    }
}

// ---------------------------------------------------------------------------
// 卡壳与列表
// ---------------------------------------------------------------------------

/// picker 卡壳：absolute 定位 + 圆角描边卡。调用方继续挂键盘钩子
/// （模型 picker 的 Enter/↑/↓）并包 `overlay::stop_click`、塞 header/列表。
pub(crate) fn picker_card(
    bottom: gpui::Pixels,
    left: gpui::Pixels,
    panel_w: gpui::Pixels,
    t: &'static Theme,
) -> gpui::Div {
    div()
        .absolute()
        .bottom(bottom)
        .left(left)
        .w(panel_w)
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg))
        .shadow_lg()
        .overflow_hidden()
        .flex()
        .flex_col()
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
        .pb(px(6.))
        .h(px(list_h))
        .overflow_y_scroll()
        .track_scroll(scroll)
        .flex()
        .flex_col()
        .children(rows)
}

/// 空态行（px10/py12/ui11/text_dim）。
pub(crate) fn empty_row(text: SharedString) -> AnyElement {
    let t = crate::appearance::theme::theme();
    div()
        .px(px(10.))
        .py(px(12.))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text_dim))
        .child(text)
        .into_any_element()
}
