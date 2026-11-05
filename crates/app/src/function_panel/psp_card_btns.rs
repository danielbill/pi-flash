//! psp 详情卡共用原语（v54）：信息行 / 动作栏 icon 钮 / 取消·确认两键。
//! 会话右键详情卡与项目右键详情卡共用（psp_hover_card / psp_project_card）。
//! 自 psp_overlays.rs 切出（itlq.12 超规拆分，纯结构搬移）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::ui::{WithTip, icon};

pub(crate) fn info_row(
    icon_name: &'static str,
    text: SharedString,
    t: &'static Theme,
) -> impl gpui::IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .mt(px(7.5))
        .text_size(crate::appearance::ui_size(12.5))
        .text_color(rgb(t.text))
        .child(icon(icon_name, 15., t.text_muted))
        .child(text)
}

/// 项目卡动作栏 icon 钮：26×26 盒 + 标签栏项目行同款图标大档（LG）；
/// hover = 盒底 bg_hover + 图标着色（del 悬停变 danger 红，同项目行
/// 垃圾桶语义；new 悬停变常字色）。点击 = 应用动作（收卡/确认态切换
/// 由动作自带）。
pub(crate) fn card_icon_btn(
    weak: &gpui::WeakEntity<Chat>,
    id: &'static str,
    icon_name: &'static str,
    tip: &'static str,
    hover_color: u32,
    apply: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let t = T();
    let weak = weak.clone();
    div()
        .id(id)
        .w(px(26.))
        .h(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .group(id)
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .with_tip(tip)
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |c, cx| {
                apply(c, cx);
            });
        })
        .child(
            gpui::svg()
                .path(SharedString::from(format!("icons/{icon_name}.svg")))
                .text_color(rgb(t.text_dim))
                .group_hover(id, move |s| s.text_color(rgb(hover_color)))
                .size(px(crate::ui::tokens::icon::LG))
                .into_any_element(),
        )
}

pub(crate) fn ghost_btn(
    id: &'static str,
    label: &str,
    danger: bool,
    handler: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl gpui::IntoElement {
    crate::ui::button(
        id,
        label.to_string(),
        crate::ui::BtnSize::Sm,
        if danger {
            crate::ui::BtnVariant::Danger
        } else {
            crate::ui::BtnVariant::Secondary
        },
        false,
        handler,
    )
}

pub(crate) fn solid_danger_btn(
    id: &'static str,
    label: &str,
    handler: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl gpui::IntoElement {
    // §5.1 唯一按钮表：实心红不存在，破坏性主行动 = Danger 描边款
    crate::ui::button(
        id,
        label.to_string(),
        crate::ui::BtnSize::Sm,
        crate::ui::BtnVariant::Danger,
        false,
        handler,
    )
}
