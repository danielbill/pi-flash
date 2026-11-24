//! 页面按钮（`Btn` 变体 + Chat 回调形态）。
//!
//! 只做变体/尺寸映射，视觉全在 [`crate::ui::button`]（§5.1 唯一按钮表）。
//! 设置页口径：`small` = MD(28)，默认 = LG(32)。
//!
//! 另含 §5.2 行尾图标动作钮 [`icon_btn`]（2026-11-22 自 046 task_act_btn
//! 收编，IB_SM 槽）。

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, MouseButton, ParentElement, SharedString,
    Styled, div, px, rgb,
};

use crate::Chat;
use crate::appearance::theme::theme;
use crate::ui::icon;
use crate::ui::tokens::{ib, icon as icon_tok, radius};
use crate::ui::tooltip::WithTip;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Btn {
    Primary,
    Secondary,
    Danger,
}

/// ConfigButton：variant × size，点击回调直接拿到 Chat（动作都在 Chat 上）。
/// 尺寸/配色委托 `ui::button`（§5.1 唯一按钮表）：small→MD(28)，default→LG(32)。
pub(crate) fn config_button(
    id: impl Into<SharedString>,
    weak: &gpui::WeakEntity<Chat>,
    label: &str,
    variant: Btn,
    small: bool,
    disabled: bool,
    on_click: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    let size = if small {
        crate::ui::BtnSize::Md
    } else {
        crate::ui::BtnSize::Lg
    };
    let variant = match variant {
        Btn::Primary => crate::ui::BtnVariant::Primary,
        Btn::Secondary => crate::ui::BtnVariant::Secondary,
        Btn::Danger => crate::ui::BtnVariant::Danger,
    };
    let weak = weak.clone();
    crate::ui::button(
        id,
        label.to_string(),
        size,
        variant,
        disabled,
        move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| on_click(c, cx));
        },
    )
}

/// §5.2 IB_SM 22 行尾图标动作钮（方形无边框、圆角 INLINE、hover bg_hover、
/// 禁用 opacity 0.4）：046 任务行 `task_act_btn`（§10.8 登记）同槽收编至此
/// （2026-11-22），列表行 hover 显影动作钮统一入口。内图标走 icon::MD 16
/// （046 行内实测口径，与规范表 IB_SM=14 的差异待规范回表）；tip 走 i18n
/// 键。点击回调拿 Chat（[`config_button`] 同形态）；mouse_down 就地截断，
/// 防行选中/兄弟 hitbox 叠压（gpui 内层先外层后派发）。
pub(crate) fn icon_btn(
    id: impl Into<SharedString>,
    weak: &gpui::WeakEntity<Chat>,
    icon_name: &'static str,
    tip_key: &'static str,
    locked: bool,
    on_click: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    icon_btn_lg(id, weak, icon_name, tip_key, locked, icon_tok::MD, on_click)
}

/// LG 18 档变体（046-T2 任务行尾动作钮四钮统一升档，2026-11-24 用户令：
/// MD 在行尾尺度偏小；钮盒仍 22 = ib::SM）。同类钮要新档从本函数扩，不
/// 要手搓。
pub(crate) fn icon_btn_lg(
    id: impl Into<SharedString>,
    weak: &gpui::WeakEntity<Chat>,
    icon_name: &'static str,
    tip_key: &'static str,
    locked: bool,
    icon_px: f32,
    on_click: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    let t = theme();
    let weak = weak.clone();
    div()
        .id(id.into())
        .size(px(ib::SM))
        .rounded(px(radius::INLINE))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when(locked, |d| d.opacity(0.4))
        .when(!locked, |d| d.hover(|s| s.bg(rgb(t.bg_hover))))
        .with_tip(tip_key)
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            if locked {
                return;
            }
            let _ = weak.update(cx, |c, cx| on_click(c, cx));
        })
        .child(icon(icon_name, icon_px, t.text_muted))
        .into_any_element()
}
