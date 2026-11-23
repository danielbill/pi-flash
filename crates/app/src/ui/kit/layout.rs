//! 页面骨架：两栏（`sidebar_shell` / `sidebar_list` / `sidebar_item` /
//! `group_header`）与详情区（`detail_shell`），列宽 `LIST_W = 450`。见 §10.2。

use gpui::prelude::*;
use gpui::{AnyElement, FontWeight, SharedString, div, px, rgb, rgba};

use crate::appearance::theme::theme as T;

/// 设置页左列表列宽（Providers / 扩展 / MCP 清单 / 技能 / 子代理）：五页统一 450
/// （082：350→400；2026-11-08：400→450，子代理页并入同宽，五页同步加宽），
/// 基准 = 扩展页（2026-10-09 用户定稿「全部和扩展的列表宽度对齐」）。
pub(crate) const LIST_W: f32 = 450.;

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
/// pb 13（2026-11-22 用户令：标题栏↔列表条目距离 +10，原 pb 3；设置页
/// 五列表与模型/插件选择弹层组头同源共享）。
pub(crate) fn group_header(label: &str, aside: Option<AnyElement>) -> AnyElement {
    let t = T();
    div()
        .flex()
        .items_center()
        .px(px(8.))
        .pt(px(8.))
        .pb(px(13.))
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

/// 列表行分隔线（2026-11-23 用户令）：1px hairline **左右内缩对齐行内容
/// 留白**，不满铺顶两头；色 = `border_alpha(ROW_ALPHA=0x66)`，**比卡片
/// 边框细且淡**（2026-11-23 二令）。绝对定位 overlay 不占布局（divider
/// 高亮线同款手法，border 参与布局会推挤行内容）。挂行容器（容器须
/// `.relative()`），首行不挂。模型列表 / 任务日志等行式列表共用，禁就
/// 地手画 border_t。
pub(crate) fn list_row_divider(left: f32, right: f32) -> AnyElement {
    let t = T();
    div()
        .absolute()
        .top_0()
        .left(px(left))
        .right(px(right))
        .h(px(1.))
        .bg(rgba(crate::appearance::theme::border_alpha(
            t,
            crate::ui::tokens::divider::ROW_ALPHA,
        )))
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
