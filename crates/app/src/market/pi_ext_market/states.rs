//! Pi 扩展市场——非列表异常态：离线黄条 / 错误红条、加载骨架、空态。

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, Styled};

use super::render::{PAGE_LEFT, PAGE_RIGHT, ROW_PAD_R, THUMB_H, THUMB_W, card_metrics};
use super::{Chat, ExtFilter};
use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::ui::tokens::{radius, space};

/// 离线黄条 / 错误红条（有则渲染）。
pub(super) fn banner(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> Option<AnyElement> {
    let st = &chat.aux.market.pi_ext;
    let shell = || {
        gpui::div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(gpui::px(space::SP3))
            .ml(gpui::px(PAGE_LEFT))
            .mr(gpui::px(PAGE_RIGHT))
            .mt(gpui::px(space::SP2))
            .px(gpui::px(space::SP4))
            .py(gpui::px(space::SP2))
            .rounded(gpui::px(radius::CTRL))
            .text_size(crate::appearance::ui_size(11.))
    };

    if let Some(err) = &st.error {
        let weak_retry = weak.clone();
        return Some(
            shell()
                .bg(gpui::rgba(crate::appearance::theme::danger_wash(t)))
                .text_color(gpui::rgb(t.danger))
                .child(crate::ui::icon("info", 12., t.danger))
                .child(crate::i18n::tf("market.load_failed", &[("e", err.clone())]))
                .child(gpui::div().flex_1())
                .child(crate::ui::button(
                    "pi-ext-retry",
                    tr("market.retry"),
                    crate::ui::BtnSize::Md,
                    crate::ui::BtnVariant::Secondary,
                    false,
                    move |_, _, cx| {
                        let _ = weak_retry.update(cx, |c, cx| c.pi_ext_retry(cx));
                    },
                ))
                .into_any_element(),
        );
    }

    if st.offline {
        let weak_retry = weak.clone();
        return Some(
            shell()
                .bg(gpui::rgb(t.bg_panel))
                .text_color(gpui::rgb(t.text_muted))
                .child(crate::ui::icon("info", 12., t.text_muted))
                .child(crate::i18n::tf(
                    "market.offline",
                    &[("n", st.entries.len().to_string())],
                ))
                .child(gpui::div().flex_1())
                .child(crate::ui::button(
                    "pi-ext-retry-2",
                    tr("market.retry"),
                    crate::ui::BtnSize::Md,
                    crate::ui::BtnVariant::Secondary,
                    false,
                    move |_, _, cx| {
                        let _ = weak_retry.update(cx, |c, cx| c.pi_ext_retry(cx));
                    },
                ))
                .into_any_element(),
        );
    }
    None
}

/// 加载中骨架（设计稿「加载中（骨架 + 分批上屏）」态）：
/// 第一批清单还没上屏时，列表**先把位置占住**（行高/卡片尺寸与真卡一致），
/// 不让页面看起来是空的、也不让首批到达时跳一下。
pub(super) fn skeleton_rows(t: &'static Theme) -> AnyElement {
    let bar = |w: f32, h: f32| {
        gpui::div()
            .w(gpui::px(w))
            .h(gpui::px(h))
            .rounded(gpui::px(radius::XS))
            .bg(gpui::rgb(t.bg_panel))
    };
    let mut col = gpui::div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .overflow_hidden();
    for _ in 0..6 {
        col = col.child(
            gpui::div()
                .w_full()
                .pr(gpui::px(ROW_PAD_R))
                .mt(crate::appearance::ui_size(space::SP4))
                .child(
                    gpui::div()
                        .w_full()
                        .h(gpui::px(card_metrics::card_h()))
                        .flex()
                        .items_start()
                        .gap(gpui::px(space::SP7))
                        .px(gpui::px(space::SP7))
                        .py(gpui::px(space::SP5))
                        .rounded(gpui::px(radius::MODAL))
                        .border_1()
                        .border_color(gpui::rgb(t.border))
                        .child(
                            bar(THUMB_W, THUMB_H)
                                .rounded(gpui::px(radius::CTRL))
                                .flex_shrink_0(),
                        )
                        .child(
                            gpui::div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(gpui::px(space::SP3))
                                .child(bar(220., 12.))
                                .child(bar(520., 10.))
                                .child(bar(380., 10.))
                                .child(bar(160., 10.)),
                        ),
                ),
        );
    }
    col.into_any_element()
}

/// 空态：加载中 → 转圈；否则空态文案。
pub(super) fn empty_area(chat: &Chat, t: &'static Theme) -> AnyElement {
    let st = &chat.aux.market.pi_ext;
    let mut col = gpui::div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(gpui::px(space::SP3))
        .text_size(crate::appearance::ui_size(12.))
        .text_color(gpui::rgb(t.text_dim));
    if st.loading {
        col = col
            .child(crate::ui::spinner(16., t.text_dim))
            .child(tr("market.loading").to_string());
    } else if st.filter == ExtFilter::Installed {
        col = col
            .child(crate::ui::icon("plug", 22., t.text_faint))
            .child(tr("market.empty_installed").to_string());
    } else {
        col = col
            .child(crate::ui::icon("search", 22., t.text_faint))
            .child(tr("market.empty").to_string());
    }
    col.into_any_element()
}
