//! Pi 扩展市场——工具栏（排序下拉 / 搜索框 / 未安装·已安装分段）与
//! 摘要行右端「全部更新」共用的右列宽量法。

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, MouseButton, Styled};

use super::render::{PAGE_LEFT, PAGE_RIGHT, TOOLBAR_PB, TOOLBAR_PT};
use super::{Chat, ExtFilter, ExtSort, LIST_ID};
use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::ui::tokens::{radius, space};

/// 工具栏：排序（最多下载/最近更新/名称）+ 搜索框 + 未安装/已安装 + 全部更新(n)。
pub(super) fn toolbar(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    side_w: f32,
) -> AnyElement {
    let st = &chat.aux.market.pi_ext;
    let mut bar = gpui::div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(gpui::px(space::SP4))
        .pl(gpui::px(PAGE_LEFT))
        .pr(gpui::px(PAGE_RIGHT))
        .pt(gpui::px(TOOLBAR_PT))
        .pb(gpui::px(TOOLBAR_PB));

    // 排序下拉（最左）：最多下载（服务端序）/ 最近更新 / 名称（本页客户端排序）
    if let Some(guard) = st.sort_guard.clone() {
        let sort = st.sort;
        let open = st.sort_open;
        let label = match sort {
            ExtSort::Downloads => tr("market.sort_downloads").to_string(),
            ExtSort::Updated => tr("market.sort_updated").to_string(),
            ExtSort::Name => tr("market.sort_name").to_string(),
        };
        let weak_t = weak.clone();
        let weak_d = weak.clone();
        let trigger = gpui::div()
            .id("pi-ext-sort-trigger")
            .flex()
            .items_center()
            .gap(gpui::px(space::SP2))
            .h(gpui::px(crate::ui::tokens::btn::MD_H))
            .px(gpui::px(crate::ui::tokens::btn::MD_PX))
            .rounded(gpui::px(radius::CTRL))
            .border_1()
            .border_color(gpui::rgb(t.border))
            .text_size(crate::appearance::ui_size(12.))
            .text_color(gpui::rgb(t.text_muted))
            .hover(|s| s.text_color(gpui::rgb(t.text)))
            .child(label)
            .child(crate::ui::icon(
                if open { "chevron-up" } else { "chevron-down" },
                12.,
                t.text_soft,
            ))
            .into_any_element();
        bar = bar.child(crate::ui::dropdown(
            "pi-ext-sort-dd",
            &guard,
            open,
            move |_, cx| {
                let _ = weak_t.update(cx, |c, cx| {
                    let st = &mut c.aux.market.pi_ext;
                    st.sort_open = !st.sort_open;
                    cx.notify();
                });
            },
            move |_, cx| {
                let _ = weak_d.update(cx, |c, cx| {
                    let st = &mut c.aux.market.pi_ext;
                    if st.sort_open {
                        st.sort_open = false;
                        cx.notify();
                    }
                });
            },
            trigger,
            move || {
                let mut menu = gpui::div()
                    .w(gpui::px(160.))
                    .py(gpui::px(space::SP2))
                    .bg(gpui::rgb(t.bg))
                    .border_1()
                    .border_color(gpui::rgb(t.border))
                    .rounded(gpui::px(radius::CARD))
                    .shadow_lg()
                    .overflow_hidden();
                /// 勾位宽度：选中打勾 / 未选中留白，文字不左右跳。
                const ICON_GUTTER: f32 = 12.;
                let icon_gutter = ICON_GUTTER;
                for (sopt, sid, slabel) in [
                    (
                        ExtSort::Downloads,
                        "pi-ext-sort-dl",
                        tr("market.sort_downloads").to_string(),
                    ),
                    (
                        ExtSort::Updated,
                        "pi-ext-sort-up",
                        tr("market.sort_updated").to_string(),
                    ),
                    (
                        ExtSort::Name,
                        "pi-ext-sort-nm",
                        tr("market.sort_name").to_string(),
                    ),
                ] {
                    let active = sopt == sort;
                    let weak_i = weak.clone();
                    menu = menu.child(
                        gpui::div()
                            .id(sid)
                            .h(gpui::px(28.))
                            .px(gpui::px(space::SP4))
                            .flex()
                            .items_center()
                            .gap(gpui::px(space::SP3))
                            .text_size(crate::appearance::ui_size(12.))
                            .cursor_pointer()
                            .rounded(gpui::px(radius::CTRL))
                            .when(active, |d| {
                                d.bg(gpui::rgba(crate::appearance::theme::rgba_a(t.accent, 0.10)))
                                    .text_color(gpui::rgb(t.accent))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                            })
                            .when(!active, |d| d.text_color(gpui::rgb(t.text_dim)))
                            .hover(|s| s.bg(gpui::rgb(t.bg_hover)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = weak_i.update(cx, |c, cx| {
                                    let st = &mut c.aux.market.pi_ext;
                                    st.sort = sopt;
                                    st.sort_open = false;
                                    crate::ui::vlist::reset_scroll(LIST_ID);
                                    cx.notify();
                                });
                            })
                            .child(
                                gpui::div()
                                    .w(gpui::px(icon_gutter))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .children(
                                        active.then(|| crate::ui::icon("check", 12., t.accent)),
                                    ),
                            )
                            .child(slabel),
                    );
                }
                menu.into_any_element()
            },
        ));
    }

    // 搜索框在排序之后（原「搜索」按钮已删：回车即搜，用户定稿）
    if let Some(input) = st.input.clone() {
        bar = bar.child(gpui::div().w(gpui::px(340.)).flex_shrink_0().child(input));
    }

    bar = bar.child(gpui::div().flex_1());

    // 未安装 / 已安装（单选切换，0 网络）
    let mut seg = gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(space::SP1))
        .p(gpui::px(space::SP1))
        .h(gpui::px(crate::ui::tokens::btn::MD_H))
        // 列宽由 side_col_w 定：与摘要行「全部更新」同宽（竖向左右缘对齐）
        .w(gpui::px(side_w))
        .flex_shrink_0()
        .rounded(gpui::px(radius::CARD))
        .border_1()
        .border_color(gpui::rgb(t.border))
        .bg(gpui::rgb(t.bg_panel));
    for (filter, id, label) in [
        (
            ExtFilter::NotInstalled,
            "pi-ext-f-none",
            tr("market.filter.not_installed"),
        ),
        (
            ExtFilter::Installed,
            "pi-ext-f-inst",
            tr("market.filter.installed"),
        ),
    ] {
        let active = st.filter == filter;
        let weak_f = weak.clone();
        seg = seg.child(
            gpui::div()
                .id(id)
                .h(gpui::px(24.))
                .px(gpui::px(crate::ui::tokens::btn::SM_PX))
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(gpui::px(radius::CTRL))
                .text_size(crate::appearance::ui_size(11.))
                // 选中段 = 白底「浮动拇指」（白底 + 1px 细边 + 微阴影，光主题
                // 里纯阴影几乎看不见）；未选中段 hover 淤底。两边都要有可见
                // 的反差，否则整条就是个静态标签条。
                .when(active, |d| {
                    d.bg(gpui::rgb(t.bg))
                        .shadow_sm()
                        .border_1()
                        .border_color(gpui::rgba(crate::appearance::theme::rgba_a(t.border, 0.6)))
                        .text_color(gpui::rgb(t.text))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .when(!active, |d| {
                    d.text_color(gpui::rgb(t.text_muted))
                        .hover(|s| s.bg(gpui::rgb(t.bg_hover)).text_color(gpui::rgb(t.text)))
                })
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = weak_f.update(cx, |c, cx| c.pi_ext_set_filter(filter, cx));
                })
                .child(label),
        );
    }
    // 未安装/已安装 切换「安装 switch」靠右（其后无其他元素）。
    // 「全部更新」已移到**摘要行右端**（用户定稿）。
    bar = bar.child(seg);
    bar.into_any_element()
}

/// 右侧共列宽：工具栏「未安装 | 已安装」分段控件与摘要行「全部更新」共用。
///
/// = 最长标签（SEMIBOLD 实测，随界面字号/语言缩放）+ 分段钮左右内距 +
/// 分段壳的描边·内衬·段间缝。不写死字面量，换语言不会截字。
pub(super) fn side_col_w(window: &mut gpui::Window) -> f32 {
    let size = crate::appearance::ui_size(11.);
    let font = gpui::Font {
        family: crate::appearance::panel_font().family.into(),
        features: gpui::FontFeatures::default(),
        fallbacks: None,
        // 选中段是 SEMIBOLD —— 按它量宽，选哪个段都不会溢出
        weight: gpui::FontWeight::SEMIBOLD,
        style: gpui::FontStyle::Normal,
    };
    let measure = |text: &str| -> f32 {
        let run = gpui::TextRun {
            len: text.len(),
            font: font.clone(),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        f32::from(
            window
                .text_system()
                .layout_line(text, size, &[run], None)
                .width,
        )
    };
    let label =
        measure(tr("market.filter.not_installed")).max(measure(tr("market.filter.installed")));
    let seg_btn = label + crate::ui::tokens::btn::SM_PX * 2.;
    seg_btn * 2. + space::SP1 * 3. + 2.
}
