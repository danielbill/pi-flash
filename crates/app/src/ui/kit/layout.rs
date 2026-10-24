//! 页面骨架：两栏（`sidebar_shell` / `sidebar_list` / `sidebar_item` /
//! `group_header`）与详情区（`detail_shell`），列宽 `LIST_W = 400`。见 §10.2。

use gpui::prelude::*;
use gpui::{AnyElement, FontWeight, SharedString, div, px, rgb};

use crate::appearance::theme::theme as T;

/// 设置页左列表列宽（Providers / 扩展 / MCP 清单 / 技能）：四页统一 400
/// （082：350→400，扩展页行内要容「↑ 可更新」箭头；四页同步加宽），
/// 基准 = 扩展页（2026-10-09 用户定稿「全部和扩展的列表宽度对齐」）。
pub(crate) const LIST_W: f32 = 400.;

/// ConfigSidebar 基座（240px 列，bg_panel + 右边框）。
pub(crate) fn sidebar_shell(id: &'static str) -> gpui::Stateful<gpui::Div> {
    let t = T();
    // 不刷底色：列表区与详情区同底（2026-10-10 用户定稿），只留右分隔线
    div()
        .id(id)
        .w(px(240.))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(rgb(t.border))
}

/// ConfigSidebarList：滚动区 + 底部 list-action 槽位由调用方拼。
pub(crate) fn sidebar_list() -> gpui::Stateful<gpui::Div> {
    div()
        .id("cfg-sb-list")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(6.))
        .pt(px(8.))
}

/// ConfigSidebarItem：30px 行（激活 bg_selected + semibold）。返回基座，
/// 调用方 `.child(...)` 拼内容并用 [`sidebar_item_on`] 挂点击。
pub(crate) fn sidebar_item(id: impl Into<SharedString>, active: bool) -> gpui::Stateful<gpui::Div> {
    let t = T();
    div()
        .id(id.into())
        .h(px(30.))
        .px(px(8.))
        .mb(px(1.))
        .rounded(px(5.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(crate::appearance::ui_size(12.))
        .cursor_pointer()
        .bg(rgb(if active { t.bg_selected } else { t.bg_panel }))
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .text_color(rgb(if active { t.text } else { t.text_muted }))
        .hover(|s| s.bg(rgb(t.bg_hover)))
}

/// ConfigSidebarGroupLabel：10px 大写组标题，右侧 aside（计数+开关）。
pub(crate) fn group_header(label: &str, aside: Option<AnyElement>) -> AnyElement {
    let t = T();
    div()
        .flex()
        .items_center()
        .px(px(8.))
        .pt(px(8.))
        .pb(px(3.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(crate::appearance::ui_size(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(label.to_string())),
        )
        .children(aside)
        .into_any_element()
}

/// ConfigDetail 基座：flex-1 滚动 + 20px 内边距。
pub(crate) fn detail_shell(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .p(px(20.))
        .text_size(crate::appearance::ui_size(12.))
        .flex()
        .flex_col()
        .gap_4()
}
